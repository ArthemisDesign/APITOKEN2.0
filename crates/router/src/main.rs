//! claude-router — единый stateless вход для всех provider-плоскостей
//! (этап 1b docs/engine/UNIFIED_ROUTER.md).
//!
//! Bounded context ВНЕ слоёв registry ← pool ← forward ← server: крейт не
//! импортирует их и общается с плоскостями только по HTTP через stable
//! loopback origins (8790/8792/8794). Router не резервирует и не списывает
//! деньги (инвариант 1), не ретраит ничего (инвариант 2), не имеет очередей,
//! semaphore и breaker (инвариант 3) и не буферизует SSE (инвариант 4).

mod catalog;
mod config;
mod error;
mod proxy;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Path, Request, State};
use axum::http::HeaderMap;
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use axum::Router;
use reqwest::Client;

use catalog::{Catalog, PlaneOrigins};
use config::Config;
use error::Lane;

/// Состояние процесса: HTTP-клиент и кэш каталога. Денег, ключей, очередей и
// лимитов здесь нет — всё это остаётся в плоскостях.
pub struct AppState {
    cfg: Config,
    client: Client,
    catalog: Catalog,
}

/// HTTP-клиент плоскостей. Только loopback HTTP: TLS не нужен. Redirect не
/// следуем — нативный ответ плоскости отдаётся клиенту как есть. Таймаут
/// только на connect: длительные SSE-стримы не должны умирать по общему
/// таймауту; их жизненным циклом управляет клиентский disconnect.
fn build_client() -> anyhow::Result<Client> {
    Ok(Client::builder()
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .pool_max_idle_per_host(64)
        .build()?)
}

/// Таблица маршрутизации публичного контракта (UNIFIED_ROUTER.md,
/// «Публичный контракт»). Форма пути выбирает плоскость; ключ и модель —
/// нет. Единственная собственная поверхность router'а — агрегированный
/// каталог /v1/models.
pub fn app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/health", get(proxy::health))
        .route("/live", get(proxy::health))
        .route("/ready", get(proxy::ready))
        .route("/balance", get(proxy_anthropic))
        .route("/v1/messages", post(proxy_anthropic))
        .route("/v1/messages/count_tokens", post(proxy_anthropic))
        .route("/v1/models", get(list_models))
        .route("/v1/models/{*id}", get(get_model))
        .route("/v1/responses", post(proxy_openai))
        .route("/v1/responses/input_tokens", post(proxy_openai))
        .route("/v1/responses/{id}", get(proxy_openai).delete(proxy_openai))
        .route("/v1/responses/{id}/input_items", get(proxy_openai))
        .route("/v1/chat/completions", post(proxy_openai))
        .route("/v1beta/{*rest}", any(proxy_gemini))
        .fallback(error_fallback)
        .method_not_allowed_fallback(method_not_allowed_fallback)
        .with_state(state)
}

async fn proxy_anthropic(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_request(&state.client, &state.cfg.anthropic_origin, Lane::Anthropic, req).await
}

async fn proxy_openai(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_request(&state.client, &state.cfg.openai_origin, Lane::OpenAi, req).await
}

async fn proxy_gemini(State(state): State<Arc<AppState>>, req: Request) -> Response {
    proxy::proxy_request(&state.client, &state.cfg.gemini_origin, Lane::Gemini, req).await
}

/// 404 вне контракта. OpenAI-совместимый конверт: универсальные клиенты —
/// основная аудитория путей, не совпадающих с native lanes.
async fn error_fallback() -> Response {
    error::unsupported_endpoint()
}

/// 405 в форме плоскости, выбранной по пути.
async fn method_not_allowed_fallback(req: Request) -> Response {
    error::method_not_allowed(req.uri().path())
}

fn origins(state: &AppState) -> PlaneOrigins<'_> {
    PlaneOrigins {
        anthropic: &state.cfg.anthropic_origin,
        openai: &state.cfg.openai_origin,
        gemini: &state.cfg.gemini_origin,
    }
}

/// `GET /v1/models` — единый каталог. Ответ OpenAI-совместим (`object: list`),
/// ID namespaced; `anthropic/claude-*` принимается discovery Claude Code
/// (он игнорирует ID вне префиксов claude/anthropic — см. документ).
async fn list_models(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let auth = proxy::auth_passthrough(&headers);
    let aggregate = state.catalog.aggregate(&state.client, &origins(&state), &auth).await;
    if aggregate.auth_rejected {
        return auth_rejected_response();
    }
    let entries = catalog::dedup(aggregate.entries);
    if entries.is_empty() {
        return error::catalog_unavailable();
    }
    let data: Vec<_> =
        entries.iter().map(|(ns, e)| e.to_json(ns)).collect();
    let mut response = axum::Json(serde_json::json!({"object": "list", "data": data})).into_response();
    if !aggregate.degraded.is_empty() {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(catalog::DEGRADED_HEADER),
            axum::http::HeaderValue::from_str(&aggregate.degraded.join(","))
                .expect("namespace list is header-safe"),
        );
    }
    response
}

/// `GET /v1/models/{id}` — namespaced ID или нативный alias.
async fn get_model(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    let auth = proxy::auth_passthrough(&headers);
    let aggregate = state.catalog.aggregate(&state.client, &origins(&state), &auth).await;
    if aggregate.auth_rejected {
        return auth_rejected_response();
    }
    let entries = catalog::dedup(aggregate.entries);
    match catalog::find(&entries, &id) {
        Some((ns, entry)) => {
            let mut response = axum::Json(entry.to_json(ns)).into_response();
            if !aggregate.degraded.is_empty() {
                response.headers_mut().insert(
                    axum::http::HeaderName::from_static(catalog::DEGRADED_HEADER),
                    axum::http::HeaderValue::from_str(&aggregate.degraded.join(","))
                        .expect("namespace list is header-safe"),
                );
            }
            response
        }
        None => {
            if entries.is_empty() {
                error::catalog_unavailable()
            } else {
                error::model_not_found(&id)
            }
        }
    }
}

/// Единый 401 каталога: ключ проверяет общий billing authority плоскостей,
/// поэтому отказ любой из них однозначен. Конверт OpenAI-совместим, как и
/// сам каталог.
fn auth_rejected_response() -> Response {
    (
        axum::http::StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({"error": {"message": "Invalid or missing API key.",
            "type": "invalid_request_error", "code": "invalid_api_key"}})),
    )
        .into_response()
}

async fn shutdown_signal() {
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {},
        _ = sigterm.recv() => {},
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = Config::from_env()?;
    let state = Arc::new(AppState { client: build_client()?, catalog: Catalog::new(), cfg });
    let addr: SocketAddr = format!("{}:{}", state.cfg.host, state.cfg.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    eprintln!(
        "claude-router listening on {addr} (anthropic={}, openai={}, gemini={})",
        state.cfg.anthropic_origin, state.cfg.openai_origin, state.cfg.gemini_origin
    );
    // Graceful shutdown: SIGTERM прекращает приём новых соединений; живые
    // SSE-стримы добиваются до TimeoutStopSec юнита (см.
    // systemd/claude-router.service). Blue-green реплики router'а — этап 6.
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{to_bytes, Body, Bytes};
    use axum::http::{Request as AxumRequest, Response as AxumResponse, StatusCode};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex as StdMutex};
    use tokio_stream::wrappers::ReceiverStream;

    // ---------- тестовая инфраструктура: mock-плоскости и запуск router'а ----------

    #[derive(Clone, Debug)]
    struct Recorded {
        method: String,
        path: String,
        x_api_key: Option<String>,
        anthropic_beta: Option<String>,
        anthropic_version: Option<String>,
        host: Option<String>,
    }

    type SharedLog = Arc<StdMutex<Vec<Recorded>>>;

    fn record_of(req: &AxumRequest<Body>) -> Recorded {
        let header = |name: &str| {
            req.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
        };
        Recorded {
            method: req.method().to_string(),
            path: req
                .uri()
                .path_and_query()
                .map(|pq| pq.as_str().to_string())
                .unwrap_or_else(|| "/".into()),
            x_api_key: header("x-api-key"),
            anthropic_beta: header("anthropic-beta"),
            anthropic_version: header("anthropic-version"),
            host: header("host"),
        }
    }

    /// Запускает axum-приложение на свободном loopback-порту, возвращает origin.
    async fn spawn(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    /// Origin, который гарантированно отказывает соединения.
    async fn dead_origin() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}")
    }

    fn make_router(anthropic: &str, openai: &str, gemini: &str, ttl: Duration) -> Router {
        app(Arc::new(AppState {
            cfg: Config {
                host: "127.0.0.1".into(),
                port: 0,
                anthropic_origin: anthropic.into(),
                openai_origin: openai.into(),
                gemini_origin: gemini.into(),
            },
            client: build_client().unwrap(),
            catalog: Catalog::with_ttl(ttl),
        }))
    }

    /// Echo-плоскость: возвращает тело запроса байт-в-байт и логирует заголовки.
    async fn echo_plane() -> (String, SharedLog) {
        let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
        let state = log.clone();
        let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
            let log = state.clone();
            async move {
                let recorded = record_of(&req);
                log.lock().unwrap().push(recorded);
                let bytes = to_bytes(req.into_body(), 16 * 1024 * 1024).await.unwrap();
                AxumResponse::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/octet-stream")
                    .header("x-plane-marker", "echo")
                    .body(Body::from(bytes))
                    .unwrap()
            }
        }));
        (spawn(router).await, log)
    }

    const ANTHROPIC_MODELS: &str = r#"{"data":[
        {"type":"model","id":"claude-opus-4-8","display_name":"Claude Opus 4.8","created_at":"2026-01-01T00:00:00Z"},
        {"type":"model","id":"claude-haiku-4-5","display_name":"Claude Haiku 4.5","created_at":"2026-01-02T00:00:00Z"}
    ],"has_more":false,"first_id":"claude-opus-4-8","last_id":"claude-haiku-4-5"}"#;

    const OPENAI_MODELS: &str = r#"{"object":"list","data":[
        {"id":"gpt-5.6","object":"model","created":0,"owned_by":"apitoken"},
        {"id":"gpt-5.5","object":"model","created":0,"owned_by":"apitoken"}
    ]}"#;

    const GEMINI_MODELS: &str = r#"{"models":[
        {"name":"models/gemini-2.5-pro","displayName":"Gemini 2.5 Pro","supportedGenerationMethods":["generateContent"]},
        {"name":"models/gemini-2.5-flash","displayName":"Gemini 2.5 Flash"}
    ]}"#;

    /// Каталог-плоскость: отдаёт fixture на свой catalog path, логирует запрос.
    /// `mode`: "ok" — fixture, "fail" — всегда 500, "auth" — всегда 401.
    async fn catalog_plane(body: &'static str, path: &'static str, mode: &'static str) -> (String, SharedLog) {
        let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
        let state = log.clone();
        let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
            let log = state.clone();
            async move {
                let recorded = record_of(&req);
                log.lock().unwrap().push(recorded);
                let status = match mode {
                    "ok" if req.uri().path() == path => StatusCode::OK,
                    "auth" => StatusCode::UNAUTHORIZED,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                AxumResponse::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(if status == StatusCode::OK { body } else { "{}" }))
                    .unwrap()
            }
        }));
        (spawn(router).await, log)
    }

    async fn three_catalog_planes() -> (String, String, String, SharedLog, SharedLog, SharedLog) {
        let (anthropic, log_a) = catalog_plane(ANTHROPIC_MODELS, "/v1/models", "ok").await;
        let (openai, log_o) = catalog_plane(OPENAI_MODELS, "/v1/models", "ok").await;
        let (gemini, log_g) = catalog_plane(GEMINI_MODELS, "/v1beta/models", "ok").await;
        (anthropic, openai, gemini, log_a, log_o, log_g)
    }

    // ---------- native lanes ----------

    #[tokio::test]
    async fn native_lane_passes_body_headers_and_response_verbatim() {
        let (origin, log) = echo_plane().await;
        let router = spawn(make_router(&origin, "http://127.0.0.1:1", "http://127.0.0.1:2", Duration::ZERO)).await;

        let payload = r#"{"model":"claude-opus-4-8","max_tokens":64,"stream":true,"messages":[]}"#;
        let client = reqwest::Client::new();
        let response = client
            .post(format!("{router}/v1/messages"))
            .header("x-api-key", "sk-pool-secret")
            .header("anthropic-beta", "messages-2023-12-15")
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .body(payload)
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("x-plane-marker").unwrap(), "echo");
        assert_eq!(response.text().await.unwrap(), payload);

        let recorded = log.lock().unwrap().pop().expect("plane saw a request");
        assert_eq!(recorded.method, "POST");
        assert_eq!(recorded.path, "/v1/messages");
        assert_eq!(recorded.x_api_key.as_deref(), Some("sk-pool-secret"));
        assert_eq!(recorded.anthropic_beta.as_deref(), Some("messages-2023-12-15"));
        assert_eq!(recorded.anthropic_version.as_deref(), Some("2023-06-01"));
        // Host переписывается на адрес плоскости, а не прокидывается клиентский.
        assert!(recorded.host.as_deref().unwrap().starts_with("127.0.0.1:"));
    }

    #[tokio::test]
    async fn native_lane_passes_query_string_verbatim() {
        let (origin, log) = echo_plane().await;
        let router = spawn(make_router(&origin, "http://127.0.0.1:1", "http://127.0.0.1:2", Duration::ZERO)).await;

        reqwest::Client::new()
            .post(format!("{router}/v1/messages?beta=true"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(log.lock().unwrap().pop().unwrap().path, "/v1/messages?beta=true");
    }

    #[tokio::test]
    async fn sse_stream_first_chunk_is_not_buffered() {
        // SSE-плоскость: первый чанк сразу, второй через 700 мс. Если router
        // буферизует, первый чанк клиент увидит только после полного ответа.
        let plane = spawn(Router::new().fallback(any(|| async {
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
            tokio::spawn(async move {
                let _ = tx.send(Ok(Bytes::from("event: message_start\ndata: {}\n\n"))).await;
                tokio::time::sleep(Duration::from_millis(700)).await;
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
            });
            AxumResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        })))
        .await;
        let router = spawn(make_router(&plane, "http://127.0.0.1:1", "http://127.0.0.1:2", Duration::ZERO)).await;

        let started = std::time::Instant::now();
        let mut response = reqwest::Client::new()
            .post(format!("{router}/v1/messages"))
            .body("{}")
            .send()
            .await
            .unwrap();
        let first = tokio::time::timeout(Duration::from_millis(400), response.chunk())
            .await
            .expect("first SSE chunk must arrive before the plane finishes")
            .unwrap()
            .expect("first chunk present");
        assert!(String::from_utf8_lossy(&first).contains("message_start"));
        assert!(started.elapsed() < Duration::from_millis(650));

        let second = response.chunk().await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&second).contains("[DONE]"));
    }

    #[tokio::test]
    async fn client_disconnect_tears_down_plane_connection() {
        // Плоскость шлёт чанки до обрыва; disconnect клиента обязан транзитивно
        // закрыть соединение router→плоскость (инвариант 4: TeeMeter drain).
        let (gone_tx, mut gone_rx) = tokio::sync::oneshot::channel::<()>();
        let gone = Arc::new(StdMutex::new(Some(gone_tx)));
        let plane = spawn(Router::new().fallback(any(move || {
            let gone = gone.clone();
            async move {
                let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
                tokio::spawn(async move {
                    loop {
                        let chunk = Ok(Bytes::from("data: tick\n\n"));
                        if tx.send(chunk).await.is_err() {
                            if let Some(signal) = gone.lock().unwrap().take() {
                                let _ = signal.send(());
                            }
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(20)).await;
                    }
                });
                AxumResponse::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(ReceiverStream::new(rx)))
                    .unwrap()
            }
        })))
        .await;
        let router = spawn(make_router(&plane, "http://127.0.0.1:1", "http://127.0.0.1:2", Duration::ZERO)).await;

        let mut response = reqwest::Client::new()
            .post(format!("{router}/v1/messages"))
            .body("{}")
            .send()
            .await
            .unwrap();
        let _first = response.chunk().await.unwrap().unwrap();
        drop(response); // клиент оборвал стрим

        tokio::time::timeout(Duration::from_secs(3), &mut gone_rx)
            .await
            .expect("plane must observe the transitive disconnect")
            .unwrap();
    }

    #[tokio::test]
    async fn gemini_and_stored_responses_paths_reach_their_planes() {
        let (anthropic, log_a) = echo_plane().await;
        let (openai, log_o) = echo_plane().await;
        let (gemini, log_g) = echo_plane().await;
        let router = spawn(make_router(&anthropic, &openai, &gemini, Duration::ZERO)).await;
        let client = reqwest::Client::new();

        for (method, path) in [
            ("POST", "/v1beta/models/gemini-2.5-pro:generateContent"),
            ("POST", "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse"),
            ("GET", "/v1beta/models"),
            ("OPTIONS", "/v1beta/models"),
        ] {
            client.request(method.parse().unwrap(), format!("{router}{path}")).send().await.unwrap();
        }
        for (method, path) in [
            ("POST", "/v1/responses"),
            ("POST", "/v1/responses/input_tokens"),
            ("GET", "/v1/responses/resp_42"),
            ("DELETE", "/v1/responses/resp_42"),
            ("GET", "/v1/responses/resp_42/input_items"),
            ("POST", "/v1/chat/completions"),
        ] {
            client.request(method.parse().unwrap(), format!("{router}{path}")).send().await.unwrap();
        }
        client.get(format!("{router}/balance")).send().await.unwrap();

        assert_eq!(log_g.lock().unwrap().len(), 4);
        assert_eq!(log_o.lock().unwrap().len(), 6);
        let anthropic_paths: Vec<String> =
            log_a.lock().unwrap().iter().map(|r| r.path.clone()).collect();
        assert_eq!(anthropic_paths, ["/balance"]);
    }

    #[tokio::test]
    async fn unreachable_plane_is_honest_502_without_retry() {
        let dead = dead_origin().await;
        let router = spawn(make_router(&dead, "http://127.0.0.1:1", "http://127.0.0.1:2", Duration::ZERO)).await;
        let response = reqwest::Client::new()
            .post(format!("{router}/v1/messages"))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["type"], "api_error");
    }

    // ---------- единый каталог ----------

    #[tokio::test]
    async fn catalog_merges_three_planes_with_namespaces_and_order() {
        let (a, o, g, log_a, _, _) = three_catalog_planes().await;
        let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;

        let response = reqwest::Client::new()
            .get(format!("{router}/v1/models"))
            .header("x-api-key", "sk-pool-secret")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().get(catalog::DEGRADED_HEADER).is_none());
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["object"], "list");
        let ids: Vec<&str> = json["data"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(
            ids,
            [
                "anthropic/claude-opus-4-8",
                "anthropic/claude-haiku-4-5",
                "openai/gpt-5.6",
                "openai/gpt-5.5",
                "google/gemini-2.5-pro",
                "google/gemini-2.5-flash"
            ]
        );
        assert_eq!(json["data"][0]["aliases"][0], "claude-opus-4-8");
        assert_eq!(json["data"][0]["name"], "Claude Opus 4.8");
        assert_eq!(json["data"][0]["owned_by"], "anthropic");

        // Auth passthrough: ключ клиента дошёл до плоскости каталога verbatim.
        let catalog_request = log_a.lock().unwrap().pop().expect("catalog fetch hit the plane");
        assert_eq!(catalog_request.path, "/v1/models?limit=1000");
        assert_eq!(catalog_request.x_api_key.as_deref(), Some("sk-pool-secret"));
    }

    #[tokio::test]
    async fn catalog_degrades_partially_when_one_plane_fails() {
        let (a, o, _, _, _, _) = three_catalog_planes().await;
        let (gemini, _) = catalog_plane("", "/v1beta/models", "fail").await;
        let router = spawn(make_router(&a, &o, &gemini, Duration::ZERO)).await;

        let response = reqwest::Client::new()
            .get(format!("{router}/v1/models"))
            .header("x-api-key", "sk-pool")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(catalog::DEGRADED_HEADER).unwrap(), "google");
        let json: serde_json::Value = response.json().await.unwrap();
        let ids: Vec<&str> = json["data"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert_eq!(ids.len(), 4);
        assert!(!ids.iter().any(|id| id.starts_with("google/")));
    }

    #[tokio::test]
    async fn catalog_serves_stale_cache_and_marks_degraded() {
        let flag = Arc::new(AtomicBool::new(true));
        let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
        let plane = {
            let flag = flag.clone();
            let log = log.clone();
            spawn(Router::new().fallback(any(move |req: AxumRequest<Body>| {
                let flag = flag.clone();
                let log = log.clone();
                async move {
                    log.lock().unwrap().push(record_of(&req));
                    if flag.load(Ordering::SeqCst) {
                        AxumResponse::builder()
                            .header("content-type", "application/json")
                            .body(Body::from(GEMINI_MODELS))
                            .unwrap()
                    } else {
                        AxumResponse::builder()
                            .status(StatusCode::INTERNAL_SERVER_ERROR)
                            .body(Body::from("{}"))
                            .unwrap()
                    }
                }
            })))
            .await
        };
        let (a, o, _, _, _, _) = three_catalog_planes().await;
        let router = spawn(make_router(&a, &o, &plane, Duration::ZERO)).await;
        let client = reqwest::Client::new();

        // Первый запрос: плоскость жива, кэш наполняется, degraded нет.
        let response = client.get(format!("{router}/v1/models")).send().await.unwrap();
        assert!(response.headers().get(catalog::DEGRADED_HEADER).is_none());
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["data"].as_array().unwrap().len(), 6);

        // Плоскость упала: тот же каталог из last-good кэша + маркер деградации.
        flag.store(false, Ordering::SeqCst);
        let response = client.get(format!("{router}/v1/models")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get(catalog::DEGRADED_HEADER).unwrap(), "google");
        let json: serde_json::Value = response.json().await.unwrap();
        let ids: Vec<&str> = json["data"].as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
        assert!(ids.contains(&"google/gemini-2.5-pro"));
        assert!(log.lock().unwrap().len() >= 2);
    }

    #[tokio::test]
    async fn catalog_is_503_when_no_plane_has_ever_answered() {
        let (d1, d2, d3) = (dead_origin().await, dead_origin().await, dead_origin().await);
        let router = spawn(make_router(&d1, &d2, &d3, Duration::ZERO)).await;
        let response = reqwest::Client::new().get(format!("{router}/v1/models")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["code"], "catalog_unavailable");
    }

    #[tokio::test]
    async fn catalog_auth_rejection_becomes_unified_401() {
        let (a, _) = catalog_plane("", "/v1/models", "auth").await;
        let (o, g) = (dead_origin().await, dead_origin().await);
        let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
        let response = reqwest::Client::new().get(format!("{router}/v1/models")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["code"], "invalid_api_key");
    }

    #[tokio::test]
    async fn get_model_resolves_namespaced_id_and_alias() {
        let (a, o, g, _, _, _) = three_catalog_planes().await;
        let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
        let client = reqwest::Client::new();

        let json: serde_json::Value = client
            .get(format!("{router}/v1/models/anthropic/claude-opus-4-8"))
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(json["id"], "anthropic/claude-opus-4-8");

        let json: serde_json::Value = client
            .get(format!("{router}/v1/models/claude-opus-4-8"))
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(json["id"], "anthropic/claude-opus-4-8");

        let json: serde_json::Value = client
            .get(format!("{router}/v1/models/gpt-5.6"))
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(json["id"], "openai/gpt-5.6");

        let response = client.get(format!("{router}/v1/models/cohere/command-x")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["error"]["code"], "model_not_found");

        let response = client.get(format!("{router}/v1/models/gpt-9")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    // ---------- router-local поверхности и ошибки ----------

    #[tokio::test]
    async fn health_and_ready_are_router_local_even_with_dead_planes() {
        let (d1, d2, d3) = (dead_origin().await, dead_origin().await, dead_origin().await);
        let router = spawn(make_router(&d1, &d2, &d3, Duration::ZERO)).await;
        let client = reqwest::Client::new();
        for path in ["/health", "/live", "/ready"] {
            let response = client.get(format!("{router}{path}")).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{path}");
        }
    }

    #[tokio::test]
    async fn unknown_path_is_404_and_wrong_method_is_lane_shaped_405() {
        let (a, o, g, _, _, _) = three_catalog_planes().await;
        let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
        let client = reqwest::Client::new();

        let response = client.get(format!("{router}/v1/completions")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["code"], "unsupported_endpoint");

        let response = client.put(format!("{router}/v1/messages")).body("{}").send().await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(response.json::<serde_json::Value>().await.unwrap()["error"]["type"], "not_found_error");

        let response = client.get(format!("{router}/v1/chat/completions")).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
            "method_not_allowed"
        );
    }
}
