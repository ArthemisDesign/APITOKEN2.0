use super::*;
use axum::body::{to_bytes, Body, Bytes};
use axum::http::{Request as AxumRequest, Response as AxumResponse, StatusCode};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_stream::wrappers::ReceiverStream;

// ---------- тестовая инфраструктура: mock-плоскости и запуск router'а ----------

#[derive(Clone, Debug)]
struct Recorded {
    method: String,
    path: String,
    x_api_key: Option<String>,
    anthropic_beta: Option<String>,
    anthropic_version: Option<String>,
    x_apitoken_client: Option<String>,
    execution_group: Option<String>,
    execution_attempt: Option<String>,
    logical_request_ids: Vec<String>,
    service_tier_header: Option<String>,
    host: Option<String>,
}

type SharedLog = Arc<StdMutex<Vec<Recorded>>>;

fn oversized_pending_body() -> reqwest::Body {
    // Keep the upload pending after headers. The router rejects the declared length before
    // reading, so the test observes the 413 deterministically instead of racing a 64 MiB
    // client write against the server closing the HTTP/1 connection.
    reqwest::Body::wrap_stream(futures_util::stream::pending::<Result<Bytes, std::io::Error>>())
}

fn record_of(req: &AxumRequest<Body>) -> Recorded {
    let header = |name: &str| {
        req.headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
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
        x_apitoken_client: header("x-apitoken-client"),
        execution_group: header("x-apitoken-execution-group"),
        execution_attempt: header("x-apitoken-attempt"),
        logical_request_ids: req
            .headers()
            .get_all("x-apitoken-logical-request-id")
            .iter()
            .map(|value| value.to_str().unwrap().to_string())
            .collect(),
        service_tier_header: header("x-apitoken-service-tier"),
        host: header("host"),
    }
}

fn assert_canonical_uuid_v4(value: &str) {
    let bytes = value.as_bytes();
    assert_eq!(bytes.len(), 36);
    assert_eq!(
        (bytes[8], bytes[13], bytes[18], bytes[23]),
        (b'-', b'-', b'-', b'-')
    );
    assert_eq!(bytes[14], b'4');
    assert!(matches!(bytes[19], b'8' | b'9' | b'a' | b'b'));
    assert!(bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) || matches!(byte, b'0'..=b'9' | b'a'..=b'f')
    }));
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
    // Не освобождаем случайный ephemeral port: параллельный тест может немедленно занять его,
    // и запрос, который должен получить ConnectionRefused, попадёт в чужую mock-плоскость.
    // TCP port 0 нельзя назначить listener'у, поэтому он остаётся детерминированным отказом
    // без гонки за повторное использование порта на macOS и production Linux.
    "http://127.0.0.1:0".to_string()
}

fn make_router(anthropic: &str, openai: &str, gemini: &str, ttl: Duration) -> Router {
    make_router_with(
        anthropic,
        openai,
        gemini,
        ttl,
        false,
        build_client().unwrap(),
    )
}

fn make_fallback_router(anthropic: &str, openai: &str, gemini: &str, ttl: Duration) -> Router {
    make_router_with(
        anthropic,
        openai,
        gemini,
        ttl,
        true,
        build_client().unwrap(),
    )
}

fn private_spool_root() -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "router-spool-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

fn make_router_with(
    anthropic: &str,
    openai: &str,
    gemini: &str,
    ttl: Duration,
    fallback_enabled: bool,
    client: Client,
) -> Router {
    let spool_root = private_spool_root();
    let body_storage = bounded_body::Budget::new(
        api_limits::current::ROUTER_SPOOL_BUDGET,
        api_limits::ByteLimit::from_bytes(api_limits::MIB),
    )
    .unwrap();
    let body_memory = bounded_body::Budget::new(
        api_limits::current::ROUTER_MEMORY_BUDGET,
        api_limits::ByteLimit::from_bytes(api_limits::MIB),
    )
    .unwrap();
    let body_spool = bounded_body::PrivateSpoolFactory::new(&spool_root).unwrap();
    app(Arc::new(AppState {
        cfg: Config {
            host: "127.0.0.1".into(),
            port: 0,
            anthropic_origin: anthropic.into(),
            // Dead by default: these fixtures count requests against the mock planes, and pointing
            // the KIMI producer at the Anthropic mock would add one hit per aggregate to every
            // such count. An absent optional plane is a supported state, so this stays honest.
            kimi_origin: "http://127.0.0.1:1".into(),
            openai_origin: openai.into(),
            gemini_origin: gemini.into(),
            fallback_enabled,
            body_limits: api_limits::current::ROUTER,
            body_idle_secs: api_limits::current::ROUTER_BODY_IDLE_SECS,
            body_max_secs: api_limits::current::ROUTER_BODY_MAX_SECS,
            body_spool_root: spool_root,
        },
        client,
        catalog: Catalog::with_ttl(ttl),
        metrics: Arc::new(RouterMetrics::new()),
        body_storage,
        body_memory,
        body_spool,
    }))
}

/// Echo-плоскость: возвращает тело запроса байт-в-байт и логирует заголовки.
async fn echo_plane() -> (String, SharedLog) {
    let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
    let state = log.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let log = state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            let recorded = record_of(&req);
            let reflected_logical_id = recorded.logical_request_ids.first().cloned();
            log.lock().unwrap().push(recorded);
            let bytes = to_bytes(req.into_body(), 16 * 1024 * 1024).await.unwrap();
            let mut response = AxumResponse::builder()
                .status(StatusCode::OK)
                .header("content-type", "application/octet-stream")
                .header("x-plane-marker", "echo");
            if let Some(logical_id) = reflected_logical_id {
                response = response.header("x-apitoken-logical-request-id", logical_id);
            }
            response.body(Body::from(bytes)).unwrap()
        }
    }));
    (spawn(router).await, log)
}

const ANTHROPIC_MODELS: &str = r#"{"data":[
    {"type":"model","id":"claude-opus-4-8","created_at":"2026-05-28T00:00:00Z","display_name":"Claude Opus 4.8","max_input_tokens":1000000,"max_tokens":128000,
     "capabilities":{"image_input":{"supported":true},"structured_outputs":{"supported":true},"thinking":{"supported":true},"effort":{"supported":true,"low":{"supported":true},"medium":{"supported":true},"high":{"supported":true},"xhigh":{"supported":true},"max":{"supported":true}}}},
    {"type":"model","id":"claude-haiku-4-5","created_at":"2025-10-15T00:00:00Z","display_name":"Claude Haiku 4.5","max_input_tokens":200000,"max_tokens":64000,
     "capabilities":{"image_input":{"supported":true},"structured_outputs":{"supported":true},"thinking":{"supported":true},"effort":{"supported":false,"low":{"supported":false},"medium":{"supported":false},"high":{"supported":false},"xhigh":{"supported":false},"max":{"supported":false}}}}
],"has_more":false,"first_id":"claude-opus-4-8","last_id":"claude-haiku-4-5"}"#;

const OPENAI_MODELS: &str = r#"{"object":"list","data":[
    {"id":"gpt-5.6","object":"model","created":1783555200,"owned_by":"apitoken","apitoken":{"limits":{"context":400000,"input":272000,"output":128000},"capabilities":{"reasoning_efforts":["none","low","medium","high","xhigh","max"],"service_tiers":["standard","priority"],"input_modalities":["text","image"],"output_modalities":["text"],"tool_calling":true,"structured_outputs":true,"streaming":true}}},
    {"id":"gpt-5.5","object":"model","created":1783555200,"owned_by":"apitoken","apitoken":{"limits":{"output":128000},"capabilities":{"reasoning_efforts":["none","low","medium","high","xhigh"],"service_tiers":["standard"],"input_modalities":["text","image"],"output_modalities":["text"],"tool_calling":true,"structured_outputs":true,"streaming":true}}}
]}"#;

const GEMINI_MODELS: &str = r#"{"models":[
    {"name":"models/gemini-2.5-pro","created":1750118400,"displayName":"Gemini 2.5 Pro","supportedGenerationMethods":["generateContent"],"apitoken":{"limits":{"context":1048576,"input":1048576,"output":65536},"capabilities":{"reasoning_efforts":["low","medium","high"],"service_tiers":["standard"],"input_modalities":["text","image"],"output_modalities":["text"],"tool_calling":true,"structured_outputs":true,"streaming":true}}},
    {"name":"models/gemini-2.5-flash","created":1750118400,"displayName":"Gemini 2.5 Flash","apitoken":{"limits":{"context":1048576,"input":1048576,"output":65536},"capabilities":{"reasoning_efforts":["low","medium","high"],"service_tiers":["standard"],"input_modalities":["text","image"],"output_modalities":["text"],"tool_calling":true,"structured_outputs":true,"streaming":true}}}
]}"#;

const ANTHROPIC_ROUTING_MODELS: &str = r#"{"data":[
    {"type":"model","id":"claude-sonnet-5","created_at":"2026-06-30T00:00:00Z","display_name":"Claude Sonnet 5"},
    {"type":"model","id":"claude-opus-5","created_at":"2026-07-24T00:00:00Z","display_name":"Claude Opus 5"},
    {"type":"model","id":"claude-haiku-4-5-20251001","created_at":"2025-10-15T00:00:00Z","display_name":"Claude Haiku 4.5"}
]}"#;

const OPENAI_ROUTING_MODELS: &str = r#"{"object":"list","data":[
    {"id":"gpt-5.6-terra","object":"model","created":1783555200,"owned_by":"apitoken"},
    {"id":"gpt-5.6-sol","object":"model","created":1783555200,"owned_by":"apitoken"},
    {"id":"gpt-5.6-luna","object":"model","created":1783555200,"owned_by":"apitoken"}
]}"#;

const GEMINI_ROUTING_MODELS: &str = r#"{"models":[
    {"name":"models/gemini-3.6-flash","created":1784592000,"displayName":"Gemini 3.6 Flash"},
    {"name":"models/gemini-3.1-pro-preview","created":1771459200,"displayName":"Gemini 3.1 Pro Preview"},
    {"name":"models/gemini-3.1-flash-lite","created":1778112000,"displayName":"Gemini 3.1 Flash-Lite"}
]}"#;

async fn catalog_pricing_response(req: AxumRequest<Body>) -> AxumResponse<Body> {
    let credential = req
        .headers()
        .get("x-api-key")
        .or_else(|| req.headers().get(header::AUTHORIZATION))
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let rate = match credential {
        "key-a" | "Bearer key-a" => "1000000000",
        "key-b" | "Bearer key-b" => "2000000000",
        _ => "5000000000",
    };
    let body = to_bytes(req.into_body(), 1024 * 1024).await.unwrap();
    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let entries: Vec<_> = request["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| {
            let id = candidate["id"].as_str().unwrap();
            let provider = candidate["provider_id"].as_str().unwrap();
            let card = serde_json::json!({
                "input": rate,
                "output": rate,
                "cache_read": rate,
                "cache_write": rate,
            });
            serde_json::json!({
                "id": id,
                "standard": card,
                "priority": (provider == "openai").then_some(card),
            })
        })
        .collect();
    AxumResponse::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "unit": "nano_usd_per_million_tokens",
                "mode": "legacy",
                "entries": entries,
            }))
            .unwrap(),
        ))
        .unwrap()
}

fn authenticated_response() -> AxumResponse<Body> {
    AxumResponse::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(r#"{"schema_version":1,"authenticated":true}"#))
        .unwrap()
}

/// Каталог-плоскость: отдаёт fixture на свой catalog path, логирует запрос.
/// `mode`: "ok" — fixture, "fail" — всегда 500, "auth" — всегда 401.
async fn catalog_plane(
    body: &'static str,
    path: &'static str,
    mode: &'static str,
) -> (String, SharedLog) {
    let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
    let state = log.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let log = state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return if mode == "auth" {
                    AxumResponse::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::from("{}"))
                        .unwrap()
                } else {
                    authenticated_response()
                };
            }
            let recorded = record_of(&req);
            log.lock().unwrap().push(recorded);
            if req.uri().path() == "/internal/router/catalog/pricing" {
                return match mode {
                    "ok" => catalog_pricing_response(req).await,
                    "pricing-auth" => AxumResponse::builder()
                        .status(StatusCode::UNAUTHORIZED)
                        .body(Body::from("{}"))
                        .unwrap(),
                    _ => AxumResponse::builder()
                        .status(StatusCode::SERVICE_UNAVAILABLE)
                        .body(Body::from("{}"))
                        .unwrap(),
                };
            }
            let status = match mode {
                "ok" | "pricing-fail" | "pricing-auth" if req.uri().path() == path => {
                    StatusCode::OK
                }
                "auth" => StatusCode::UNAUTHORIZED,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            AxumResponse::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Body::from(if status == StatusCode::OK {
                    body
                } else {
                    "{}"
                }))
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

type SharedBodies = Arc<StdMutex<Vec<Vec<u8>>>>;

async fn unrestricted_policy_response(req: AxumRequest<Body>) -> AxumResponse<Body> {
    let body = to_bytes(req.into_body(), 64 * 1024).await.unwrap();
    let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let allowed: Vec<_> = request["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|candidate| candidate["id"].as_str().unwrap())
        .collect();
    AxumResponse::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "mode": "unrestricted",
                "allowed": allowed,
            }))
            .unwrap(),
        ))
        .unwrap()
}

#[derive(Clone, Copy)]
enum PolicyReply {
    Unrestricted,
    Fixed(StatusCode, &'static str),
}

async fn policy_attempt_plane(
    catalog_body: &'static str,
    catalog_path: &'static str,
    policy_reply: PolicyReply,
    attempt_status: StatusCode,
    execution_state: Option<&'static str>,
) -> (String, SharedBodies, SharedLog) {
    let bodies: SharedBodies = Arc::new(StdMutex::new(Vec::new()));
    let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
    let body_state = bodies.clone();
    let log_state = log.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let bodies = body_state.clone();
        let log = log_state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                log.lock().unwrap().push(record_of(&req));
                return authenticated_response();
            }
            log.lock().unwrap().push(record_of(&req));
            if req.uri().path() == catalog_path {
                return AxumResponse::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(catalog_body))
                    .unwrap();
            }
            if req.uri().path() == "/internal/router/catalog/pricing" {
                return catalog_pricing_response(req).await;
            }
            if req.uri().path() == "/internal/router/policy/preflight" {
                return match policy_reply {
                    PolicyReply::Unrestricted => unrestricted_policy_response(req).await,
                    PolicyReply::Fixed(status, body) => {
                        let _ = to_bytes(req.into_body(), 64 * 1024).await;
                        AxumResponse::builder()
                            .status(status)
                            .header("content-type", "application/json")
                            .body(Body::from(body))
                            .unwrap()
                    }
                };
            }
            let body = to_bytes(req.into_body(), 64 * 1024 * 1024).await.unwrap();
            bodies.lock().unwrap().push(body.to_vec());
            let mut builder = AxumResponse::builder()
                .status(attempt_status)
                .header("content-type", "application/json");
            if let Some(value) = execution_state {
                builder = builder.header("x-apitoken-execution-state", value);
            }
            builder.body(Body::from(body)).unwrap()
        }
    }));
    (spawn(router).await, bodies, log)
}

/// Plane с живым каталогом и программируемым ответом universal attempt.
/// В body-log попадают только billable POST attempts; catalog/служебные GET туда не пишутся.
async fn attempt_plane(
    catalog_body: &'static str,
    catalog_path: &'static str,
    attempt_status: StatusCode,
    execution_state: Option<&'static str>,
    hang_attempt: bool,
) -> (String, SharedBodies) {
    let bodies: SharedBodies = Arc::new(StdMutex::new(Vec::new()));
    let state = bodies.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let bodies = state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            if req.uri().path() == catalog_path {
                return AxumResponse::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(catalog_body))
                    .unwrap();
            }
            if req.uri().path() == "/internal/router/policy/preflight" {
                return unrestricted_policy_response(req).await;
            }
            let is_billable_attempt = req.method() == axum::http::Method::POST;
            let body = to_bytes(req.into_body(), 64 * 1024 * 1024).await.unwrap();
            if is_billable_attempt {
                bodies.lock().unwrap().push(body.to_vec());
            }
            if hang_attempt {
                return std::future::pending::<AxumResponse<Body>>().await;
            }
            let mut builder = AxumResponse::builder()
                .status(attempt_status)
                .header("content-type", "application/json")
                .header("x-attempt-plane", catalog_path);
            if let Some(value) = execution_state {
                builder = builder.header("x-apitoken-execution-state", value);
            }
            builder.body(Body::from(body)).unwrap()
        }
    }));
    (spawn(router).await, bodies)
}

async fn identity_attempt_plane(
    catalog_body: &'static str,
    catalog_path: &'static str,
    attempt_status: StatusCode,
    execution_state: Option<&'static str>,
) -> (String, SharedLog) {
    let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
    let state = log.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let log = state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            if req.uri().path() == catalog_path {
                return AxumResponse::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "application/json")
                    .body(Body::from(catalog_body))
                    .unwrap();
            }
            if req.uri().path() == "/internal/router/policy/preflight" {
                return unrestricted_policy_response(req).await;
            }
            let recorded = record_of(&req);
            let reflected_logical_id = recorded.logical_request_ids.first().cloned();
            log.lock().unwrap().push(recorded);
            let mut builder = AxumResponse::builder()
                .status(attempt_status)
                .header("content-type", "application/json");
            if let Some(logical_id) = reflected_logical_id {
                builder = builder.header("x-apitoken-logical-request-id", logical_id);
            }
            if let Some(value) = execution_state {
                builder = builder.header("x-apitoken-execution-state", value);
            }
            builder.body(Body::from("{}")).unwrap()
        }
    }));
    (spawn(router).await, log)
}

/// Отдаёт ровно один catalog response с `Connection: close`, затем
/// закрывает listener. Следующая попытка к тому же origin получает
/// доказуемый TCP ConnectionRefused.
async fn one_shot_catalog_origin(body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut received = Vec::new();
            let mut chunk = [0_u8; 1024];
            while !received.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut chunk).await.unwrap();
                if count == 0 {
                    return;
                }
                received.extend_from_slice(&chunk[..count]);
            }
            let is_auth = received
                .windows(b" /internal/router/auth/preflight ".len())
                .any(|window| window == b" /internal/router/auth/preflight ");
            let response_body = if is_auth {
                r#"{"schema_version":1,"authenticated":true}"#
            } else {
                body
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_body.len(),
                response_body,
            );
            stream.write_all(response.as_bytes()).await.unwrap();
            stream.shutdown().await.unwrap();
            if !is_auth {
                drop(listener);
                return;
            }
        }
    });
    format!("http://{addr}")
}

// ---------- native lanes ----------

#[tokio::test]
async fn native_lane_passes_body_headers_and_response_verbatim() {
    let (origin, log) = echo_plane().await;
    let router = spawn(make_router(
        &origin,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    // Namespaced `anthropic/*` модель: dispatch (этап 5.1) обязан сохранить
    // байт-идентичный passthrough native lane, каталог не опрашивается.
    let payload =
        r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"stream":true,"messages":[]}"#;
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{router}/v1/messages"))
        .header("x-api-key", "sk-pool-secret")
        .header("anthropic-beta", "messages-2023-12-15")
        .header("anthropic-version", "2023-06-01")
        .header("x-apitoken-client", "opencode/1.2.3")
        .header("x-apitoken-execution-group", "client-spoof")
        .header("x-apitoken-attempt", "99")
        .header("x-apitoken-logical-request-id", "not-a-uuid")
        .header(
            "x-apitoken-logical-request-id",
            "123e4567-e89b-42d3-a456-426614174000",
        )
        .header("content-type", "application/json")
        .body(payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("x-plane-marker").unwrap(), "echo");
    assert!(response
        .headers()
        .get("x-apitoken-logical-request-id")
        .is_none());
    assert_eq!(response.text().await.unwrap(), payload);

    let recorded = log.lock().unwrap().pop().expect("plane saw a request");
    assert_eq!(recorded.method, "POST");
    assert_eq!(recorded.path, "/v1/messages");
    assert_eq!(recorded.x_api_key.as_deref(), Some("sk-pool-secret"));
    assert_eq!(
        recorded.anthropic_beta.as_deref(),
        Some("messages-2023-12-15")
    );
    assert_eq!(recorded.anthropic_version.as_deref(), Some("2023-06-01"));
    assert_eq!(
        recorded.x_apitoken_client.as_deref(),
        Some("opencode/1.2.3")
    );
    assert!(recorded.execution_group.is_none());
    assert!(recorded.execution_attempt.is_none());
    assert_eq!(recorded.logical_request_ids.len(), 1);
    assert_canonical_uuid_v4(&recorded.logical_request_ids[0]);
    assert_ne!(recorded.logical_request_ids[0], "not-a-uuid");
    assert_ne!(
        recorded.logical_request_ids[0],
        "123e4567-e89b-42d3-a456-426614174000"
    );
    // Host переписывается на адрес плоскости, а не прокидывается клиентский.
    assert!(recorded.host.as_deref().unwrap().starts_with("127.0.0.1:"));
}

#[tokio::test]
async fn universal_single_requests_get_distinct_private_logical_ids() {
    let (openai, log) = echo_plane().await;
    let router = spawn(make_router(
        "http://127.0.0.1:1",
        &openai,
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for _ in 0..2 {
        let response = client
            .post(format!("{router}/v1/responses"))
            .header("x-apitoken-logical-request-id", "spoofed")
            .body(r#"{"model":"openai/gpt-5.6","input":"hi"}"#)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response
            .headers()
            .get("x-apitoken-logical-request-id")
            .is_none());
    }

    let recorded = log.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    for request in &recorded {
        assert_eq!(request.logical_request_ids.len(), 1);
        assert_canonical_uuid_v4(&request.logical_request_ids[0]);
        assert_ne!(request.logical_request_ids[0], "spoofed");
        assert!(request.execution_group.is_none());
        assert!(request.execution_attempt.is_none());
    }
    assert_ne!(
        recorded[0].logical_request_ids[0],
        recorded[1].logical_request_ids[0]
    );
}

#[tokio::test]
async fn fast_header_normalizes_all_gpt_surfaces_and_is_stripped() {
    let (openai, log) = echo_plane().await;
    let router = spawn(make_router(
        "http://127.0.0.1:1",
        &openai,
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for (path, body) in [
        (
            "/v1/chat/completions",
            serde_json::json!({
                "model": "openai/gpt-5.6",
                "messages": [{"role": "user", "content": "hi"}]
            }),
        ),
        (
            "/v1/responses",
            serde_json::json!({
                "model": "openai/gpt-5.6", "input": "hi", "service_tier": "fast"
            }),
        ),
        (
            "/v1/messages",
            serde_json::json!({
                "model": "openai/gpt-5.6", "max_tokens": 32,
                "messages": [{"role": "user", "content": "hi"}], "speed": "fast"
            }),
        ),
    ] {
        let response = client
            .post(format!("{router}{path}"))
            .header("x-apitoken-service-tier", "fast")
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let forwarded: serde_json::Value = response.json().await.unwrap();
        assert_eq!(forwarded["service_tier"], "priority", "{path}");
    }

    let recorded = log.lock().unwrap().clone();
    assert_eq!(recorded.len(), 3);
    assert!(recorded
        .iter()
        .all(|request| request.service_tier_header.is_none()));
}

#[tokio::test]
async fn camel_service_tier_alias_normalizes_chat_and_responses_before_plane() {
    let (openai, log) = echo_plane().await;
    let router = spawn(make_router(
        "http://127.0.0.1:1",
        &openai,
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for (path, body) in [
        (
            "/v1/chat/completions",
            serde_json::json!({
                "model": "openai/gpt-5.6",
                "messages": [{"role": "user", "content": "hi"}],
                "serviceTier": "priority"
            }),
        ),
        (
            "/v1/responses",
            serde_json::json!({
                "model": "openai/gpt-5.6",
                "input": "hi",
                "serviceTier": "fast",
                "service_tier": "priority"
            }),
        ),
    ] {
        let response = client
            .post(format!("{router}{path}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let forwarded: serde_json::Value = response.json().await.unwrap();
        assert_eq!(forwarded["service_tier"], "priority", "{path}");
        assert!(forwarded.get("serviceTier").is_none(), "{path}");
    }

    assert_eq!(log.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn camel_service_tier_alias_rejects_invalid_conflicting_and_non_gpt_requests() {
    let (anthropic, log_a) = echo_plane().await;
    let (openai, log_o) = echo_plane().await;
    let (gemini, log_g) = echo_plane().await;
    let router = spawn(make_router(&anthropic, &openai, &gemini, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for (path, body) in [
        (
            "/v1/chat/completions",
            serde_json::json!({
                "model": "openai/gpt-5.6", "messages": [], "serviceTier": "default"
            }),
        ),
        (
            "/v1/responses",
            serde_json::json!({
                "model": "openai/gpt-5.6", "input": "hi",
                "serviceTier": "priority", "service_tier": "default"
            }),
        ),
        (
            "/v1/chat/completions",
            serde_json::json!({
                "model": "anthropic/claude-opus-4-8", "messages": [],
                "serviceTier": "priority"
            }),
        ),
        (
            "/v1/messages",
            serde_json::json!({
                "model": "openai/gpt-5.6", "max_tokens": 32, "messages": [],
                "serviceTier": "priority"
            }),
        ),
        (
            "/v1/messages/count_tokens",
            serde_json::json!({
                "model": "openai/gpt-5.6", "messages": [],
                "serviceTier": "priority"
            }),
        ),
    ] {
        let response = client
            .post(format!("{router}{path}"))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }

    assert!(log_a.lock().unwrap().is_empty());
    assert!(log_o.lock().unwrap().is_empty());
    assert!(log_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fast_header_rejects_conflicts_non_gpt_and_token_count_before_plane_call() {
    let (anthropic, log_a) = echo_plane().await;
    let (openai, log_o) = echo_plane().await;
    let (gemini, log_g) = echo_plane().await;
    let router = spawn(make_router(&anthropic, &openai, &gemini, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for (path, tier, body) in [
        (
            "/v1/chat/completions",
            "economy",
            serde_json::json!({
                "model": "openai/gpt-5.6", "messages": []
            }),
        ),
        (
            "/v1/responses",
            "fast",
            serde_json::json!({
                "model": "openai/gpt-5.6", "input": "hi", "service_tier": "default"
            }),
        ),
        (
            "/v1/messages",
            "fast",
            serde_json::json!({
                "model": "anthropic/claude-opus-4-8", "max_tokens": 32, "messages": []
            }),
        ),
        (
            "/v1/messages/count_tokens",
            "fast",
            serde_json::json!({
                "model": "openai/gpt-5.6", "messages": []
            }),
        ),
    ] {
        let response = client
            .post(format!("{router}{path}"))
            .header("x-apitoken-service-tier", tier)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }

    assert!(log_a.lock().unwrap().is_empty());
    assert!(log_o.lock().unwrap().is_empty());
    assert!(log_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn native_lane_strips_execution_state_header_from_transit_responses() {
    // Плоскость пометила отказ авторитетной семантикой исполнения. Router снимает
    // заголовок с транзита (за его условия отвечает только сам движок), а статус,
    // тело и остальные заголовки доезжают до клиента без изменений.
    let plane = spawn(
        Router::new().fallback(any(|req: AxumRequest<Body>| async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            AxumResponse::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("content-type", "application/json")
                .header("x-apitoken-execution-state", "not_started")
                .header("x-plane-marker", "refused")
                .body(Body::from("{}"))
                .unwrap()
        })),
    )
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(response
        .headers()
        .get("x-apitoken-execution-state")
        .is_none());
    assert_eq!(response.headers().get("x-plane-marker").unwrap(), "refused");
    assert_eq!(response.text().await.unwrap(), "{}");
}

#[tokio::test]
async fn native_lane_passes_query_string_verbatim() {
    let (origin, log) = echo_plane().await;
    let router = spawn(make_router(
        &origin,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    reqwest::Client::new()
        .post(format!("{router}/v1/messages?beta=true"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(
        log.lock().unwrap().pop().unwrap().path,
        "/v1/messages?beta=true"
    );
}

#[tokio::test]
async fn sse_stream_first_chunk_is_not_buffered() {
    // SSE-плоскость: первый чанк сразу, второй через 700 мс. Если router
    // буферизует, первый чанк клиент увидит только после полного ответа.
    let plane = spawn(
        Router::new().fallback(any(|req: AxumRequest<Body>| async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
            tokio::spawn(async move {
                let _ = tx
                    .send(Ok(Bytes::from("event: message_start\ndata: {}\n\n")))
                    .await;
                tokio::time::sleep(Duration::from_millis(700)).await;
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
            });
            AxumResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        })),
    )
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    let started = std::time::Instant::now();
    let mut response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(
            r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"stream":true,"messages":[]}"#,
        )
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
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    let mut response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(
            r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"stream":true,"messages":[]}"#,
        )
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
async fn native_wrapper_routes_inject_logical_identity_only_for_executable_proxy_calls() {
    let (anthropic, log_a) = echo_plane().await;
    let (openai, log_o) = echo_plane().await;
    let (gemini, log_g) = echo_plane().await;
    let router = spawn(make_router(&anthropic, &openai, &gemini, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for (method, path) in [
        ("POST", "/v1beta/models/gemini-2.5-pro:generateContent"),
        (
            "POST",
            "/v1beta/models/gemini-2.5-pro:streamGenerateContent?alt=sse",
        ),
        ("GET", "/v1beta/models"),
        ("OPTIONS", "/v1beta/models"),
        ("POST", "/upload/v1beta/files?uploadType=resumable"),
    ] {
        let response = client
            .request(method.parse().unwrap(), format!("{router}{path}"))
            .send()
            .await
            .unwrap();
        assert!(response
            .headers()
            .get("x-apitoken-logical-request-id")
            .is_none());
    }
    // /v1/responses — universal lane с dispatch по model (этап 4.1):
    // валидный namespaced model обязателен; openai/* остаётся на своей
    // плоскости. Stored endpoints dispatch не используют (решение 5).
    client
        .post(format!("{router}/v1/responses"))
        .body(r#"{"model":"openai/gpt-5.6","input":"hi"}"#)
        .send()
        .await
        .unwrap();
    for (method, path) in [
        ("POST", "/v1/responses/input_tokens"),
        ("GET", "/v1/responses/resp_42"),
        ("DELETE", "/v1/responses/resp_42"),
        ("GET", "/v1/responses/resp_42/input_items"),
        ("POST", "/v1/images/generations"),
        ("POST", "/v1/images/edits"),
    ] {
        let response = client
            .request(method.parse().unwrap(), format!("{router}{path}"))
            .send()
            .await
            .unwrap();
        assert!(response
            .headers()
            .get("x-apitoken-logical-request-id")
            .is_none());
    }
    // /v1/chat/completions — universal lane: до плоскости доходит запрос
    // с валидным namespaced model (этап 3.1, chat::proxy_chat).
    client
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"openai/gpt-5.6","messages":[]}"#)
        .send()
        .await
        .unwrap();
    client
        .get(format!("{router}/balance"))
        .send()
        .await
        .unwrap();

    let gemini_requests = log_g.lock().unwrap().clone();
    assert_eq!(gemini_requests.len(), 5);
    for request in &gemini_requests {
        assert_eq!(request.logical_request_ids.len(), 1, "{}", request.path);
        assert_canonical_uuid_v4(&request.logical_request_ids[0]);
    }
    let openai_requests = log_o.lock().unwrap().clone();
    assert_eq!(openai_requests.len(), 8);
    for request in &openai_requests {
        assert_eq!(request.logical_request_ids.len(), 1, "{}", request.path);
        assert_canonical_uuid_v4(&request.logical_request_ids[0]);
    }
    let distinct: std::collections::HashSet<_> = gemini_requests
        .iter()
        .chain(openai_requests.iter())
        .map(|request| request.logical_request_ids[0].as_str())
        .collect();
    assert_eq!(distinct.len(), 13);
    let anthropic_paths: Vec<String> = log_a
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.path.clone())
        .collect();
    assert_eq!(anthropic_paths, ["/balance"]);
    assert!(log_a.lock().unwrap()[0].logical_request_ids.is_empty());
    assert_eq!(
        gemini_requests.last().unwrap().path,
        "/upload/v1beta/files?uploadType=resumable"
    );
    assert!(openai_requests
        .iter()
        .all(|request| !request.path.starts_with("/upload/")));
}

#[tokio::test]
async fn balance_fails_over_from_anthropic_5xx_and_preserves_openai_response() {
    let first_calls = Arc::new(AtomicUsize::new(0));
    let state = first_calls.clone();
    let anthropic = spawn(Router::new().fallback(any(move || {
        let state = state.clone();
        async move {
            state.fetch_add(1, Ordering::SeqCst);
            AxumResponse::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .header("x-first-plane", "failed")
                .body(Body::from("anthropic unavailable"))
                .unwrap()
        }
    })))
    .await;
    let (openai, openai_log) = echo_plane().await;
    let router = spawn(make_router(
        &anthropic,
        &openai,
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;

    let response = reqwest::Client::new()
        .get(format!("{router}/balance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("x-plane-marker").unwrap(), "echo");
    assert!(response.headers().get("x-first-plane").is_none());
    assert_eq!(response.bytes().await.unwrap(), Bytes::new());
    assert_eq!(first_calls.load(Ordering::SeqCst), 1);
    assert_eq!(openai_log.lock().unwrap()[0].path, "/balance");
}

#[tokio::test]
async fn balance_unauthorized_is_terminal_and_never_reaches_another_plane() {
    let anthropic = spawn(Router::new().fallback(any(|| async {
        AxumResponse::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("content-type", "application/json")
            .body(Body::from(r#"{"error":{"type":"authentication_error"}}"#))
            .unwrap()
    })))
    .await;
    let later_calls = Arc::new(AtomicUsize::new(0));
    let later_state = later_calls.clone();
    let openai = spawn(Router::new().fallback(any(move || {
        let later_calls = later_state.clone();
        async move {
            later_calls.fetch_add(1, Ordering::SeqCst);
            AxumResponse::new(Body::from("unexpected"))
        }
    })))
    .await;
    let router = spawn(make_router(
        &anthropic,
        &openai,
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;

    let response = reqwest::Client::new()
        .get(format!("{router}/balance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(later_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn balance_and_router_preflights_strip_spoofed_logical_identity_without_injection() {
    let (anthropic, _, log_a) = policy_attempt_plane(
        ANTHROPIC_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (openai, _, log_o) = policy_attempt_plane(
        OPENAI_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (gemini, _, log_g) = policy_attempt_plane(
        GEMINI_ROUTING_MODELS,
        "/v1beta/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let router = spawn(make_fallback_router(
        &anthropic,
        &openai,
        &gemini,
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    let balance = client
        .get(format!("{router}/balance"))
        .header("x-apitoken-logical-request-id", "spoofed-balance")
        .send()
        .await
        .unwrap();
    assert_eq!(balance.status(), StatusCode::OK);

    let response = client
        .post(format!("{router}/v1/responses"))
        .header("x-apitoken-logical-request-id", "spoofed-universal")
        .body(r#"{"model":"anthropic/claude-sonnet-5","models":["openai/gpt-5.6-terra"],"input":"hi"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let logs = [log_a, log_o, log_g];
    let recorded: Vec<_> = logs
        .iter()
        .flat_map(|log| log.lock().unwrap().clone())
        .collect();
    for request in recorded.iter().filter(|request| {
        request.path == "/balance"
            || request.path == "/internal/router/auth/preflight"
            || request.path == "/internal/router/policy/preflight"
    }) {
        assert!(
            request.logical_request_ids.is_empty(),
            "{} unexpectedly received {:?}",
            request.path,
            request.logical_request_ids
        );
    }
    assert!(recorded
        .iter()
        .any(|request| request.path == "/internal/router/policy/preflight"));
    let executable: Vec<_> = recorded
        .iter()
        .filter(|request| request.path == "/v1/responses")
        .collect();
    assert_eq!(executable.len(), 1);
    assert_eq!(executable[0].logical_request_ids.len(), 1);
    assert_canonical_uuid_v4(&executable[0].logical_request_ids[0]);
}

#[tokio::test]
async fn unavailable_auth_authority_fails_before_universal_body_dispatch() {
    let dead = dead_origin().await;
    let router = spawn(make_router(
        &dead,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["type"], "api_error");
}

// ---------- universal serial fallback (routing phase 6.2) ----------

#[tokio::test]
async fn fallback_flag_off_rejects_models_and_provider_before_any_plane() {
    let (a, log_a) = echo_plane().await;
    let (o, log_o) = echo_plane().await;
    let (g, log_g) = echo_plane().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;

    let client = reqwest::Client::new();
    for (body, param) in [
        (
            r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"input":"hi"}"#,
            "models",
        ),
        (
            r#"{"model":"anthropic/claude-opus-4-8","provider":{"only":["anthropic"]},"input":"hi"}"#,
            "provider",
        ),
    ] {
        let response = client
            .post(format!("{router}/v1/responses"))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["param"], param, "{body}");
    }
    assert!(log_a.lock().unwrap().is_empty());
    assert!(log_o.lock().unwrap().is_empty());
    assert!(log_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn signed_not_started_falls_back_and_rewrites_each_attempt_body() {
    let (a, bodies_a) = attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        StatusCode::SERVICE_UNAVAILABLE,
        Some("not_started"),
        false,
    )
    .await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, bodies_g) =
        attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .header("x-api-key", "sk-pool-secret")
        .body(
            r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"messages":[{"role":"user","content":"hi"}]}"#,
        )
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("x-apitoken-execution-state")
        .is_none());
    let final_body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(final_body["model"], "openai/gpt-5.6");
    assert!(final_body.get("models").is_none());

    let first: serde_json::Value = serde_json::from_slice(&bodies_a.lock().unwrap()[0]).unwrap();
    let second: serde_json::Value = serde_json::from_slice(&bodies_o.lock().unwrap()[0]).unwrap();
    assert_eq!(first["model"], "anthropic/claude-opus-4-8");
    assert_eq!(second["model"], "openai/gpt-5.6");
    assert!(first.get("models").is_none());
    assert!(second.get("models").is_none());
    assert!(bodies_g.lock().unwrap().is_empty());

    let metrics = reqwest::get(format!("{router}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains(
        "claude_router_fallback_total{from_namespace=\"anthropic\",to_namespace=\"openai\",reason=\"not_started\"} 1"
    ));
    assert_eq!(
        metrics
            .lines()
            .filter(|line| line.starts_with("claude_router_fallback_total{"))
            .count(),
        18
    );
}

#[tokio::test]
async fn fallback_owns_one_uuid_group_and_monotonic_attempt_headers() {
    let (a, log_a) = identity_attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        StatusCode::SERVICE_UNAVAILABLE,
        Some("not_started"),
    )
    .await;
    let (o, log_o) =
        identity_attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None).await;
    let (g, _) =
        identity_attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None).await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/responses"))
        .header("x-apitoken-execution-group", "client-controlled")
        .header("x-apitoken-attempt", "9000")
        .header("x-apitoken-client", "claude_code/2.1.220")
        .body(r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"input":"hi"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response
        .headers()
        .get("x-apitoken-logical-request-id")
        .is_none());

    let first = log_a.lock().unwrap().first().cloned().unwrap();
    let second = log_o.lock().unwrap().first().cloned().unwrap();
    let group = first.execution_group.as_deref().unwrap();
    let bytes = group.as_bytes();
    assert_eq!(bytes.len(), 36);
    assert_eq!(
        (bytes[8], bytes[13], bytes[18], bytes[23]),
        (b'-', b'-', b'-', b'-')
    );
    assert_eq!(bytes[14], b'4');
    assert!(matches!(bytes[19], b'8' | b'9' | b'a' | b'b'));
    assert!(bytes.iter().enumerate().all(|(index, byte)| {
        matches!(index, 8 | 13 | 18 | 23) || matches!(byte, b'0'..=b'9' | b'a'..=b'f')
    }));
    assert_ne!(group, "client-controlled");
    assert_eq!(second.execution_group.as_deref(), Some(group));
    assert_eq!(first.execution_attempt.as_deref(), Some("1"));
    assert_eq!(second.execution_attempt.as_deref(), Some("2"));
    assert_eq!(
        first.x_apitoken_client.as_deref(),
        Some("claude_code/2.1.220")
    );
    assert_eq!(second.x_apitoken_client, first.x_apitoken_client);
    assert_eq!(first.logical_request_ids.len(), 1);
    assert_canonical_uuid_v4(&first.logical_request_ids[0]);
    assert_eq!(second.logical_request_ids, first.logical_request_ids);
}

#[tokio::test]
async fn fallback_matrix_stops_on_ambiguous_5xx_and_client_4xx() {
    for (status, signal) in [
        (StatusCode::SERVICE_UNAVAILABLE, None),
        (StatusCode::SERVICE_UNAVAILABLE, Some("NOT_STARTED")),
        (StatusCode::PAYMENT_REQUIRED, Some("not_started")),
        (StatusCode::BAD_REQUEST, Some("not_started")),
    ] {
        let (a, bodies_a) =
            attempt_plane(ANTHROPIC_MODELS, "/v1/models", status, signal, false).await;
        let (o, bodies_o) =
            attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
        let (g, _) =
            attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
        let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
        let response = reqwest::Client::new()
            .post(format!("{router}/v1/messages"))
            .body(
                r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"max_tokens":16,"messages":[]}"#,
            )
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            status,
            "status={status} signal={signal:?}"
        );
        assert!(response
            .headers()
            .get("x-apitoken-execution-state")
            .is_none());
        assert_eq!(bodies_a.lock().unwrap().len(), 1);
        assert!(bodies_o.lock().unwrap().is_empty());
        let metrics = reqwest::get(format!("{router}/metrics"))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(metrics.contains(
            "claude_router_fallback_total{from_namespace=\"anthropic\",to_namespace=\"openai\",reason=\"not_started\"} 0"
        ));
    }
}

#[tokio::test]
async fn signed_429_is_a_retryable_capacity_refusal_for_count_tokens() {
    let (a, bodies_a) = attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        StatusCode::TOO_MANY_REQUESTS,
        Some("not_started"),
        false,
    )
    .await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, _) = attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages/count_tokens"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(bodies_a.lock().unwrap().len(), 1);
    assert_eq!(bodies_o.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn proven_connection_refused_retries_but_timeout_does_not() {
    // Catalog validates the Anthropic ID, then that one-shot origin closes
    // completely before the first attempt. TCP cannot have carried a body.
    let a = one_shot_catalog_origin(ANTHROPIC_MODELS).await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, _) = attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let payload =
        r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"messages":[]}"#;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(payload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(bodies_o.lock().unwrap().len(), 1);
    let metrics = reqwest::get(format!("{router}/metrics"))
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(metrics.contains(
        "claude_router_fallback_total{from_namespace=\"anthropic\",to_namespace=\"openai\",reason=\"connect_refused\"} 1"
    ));

    // A deterministic timeout after TCP connect but before response headers
    // is ambiguous: the plane has received the body, so no second attempt.
    let (a, bodies_a) =
        attempt_plane(ANTHROPIC_MODELS, "/v1/models", StatusCode::OK, None, true).await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, _) = attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    let timeout_client = Client::builder()
        .timeout(Duration::from_millis(150))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap();
    let router = spawn(make_router_with(
        &a,
        &o,
        &g,
        Duration::ZERO,
        true,
        timeout_client,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(payload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    assert_eq!(bodies_a.lock().unwrap().len(), 1);
    assert!(bodies_o.lock().unwrap().is_empty());
}

#[tokio::test]
async fn fallback_chain_validation_is_lane_shaped_and_preflighted() {
    let (a, bodies_a) =
        attempt_plane(ANTHROPIC_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, bodies_g) =
        attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    // This test exercises validation, not stale-catalog refresh. Keep one stable aggregate so
    // host-wide parallel test load cannot turn repeated zero-TTL fetches into a timing probe.
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::from_secs(60))).await;
    let client = reqwest::Client::new();

    for models in [
        "[]",
        "{}",
        r#"[""]"#,
        r#"[42]"#,
        r#"["anthropic/claude-opus-4-8"]"#,
    ] {
        let response = client
            .post(format!("{router}/v1/chat/completions"))
            .body(format!(
                r#"{{"model":"anthropic/claude-opus-4-8","models":{models},"messages":[]}}"#
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "models={models}"
        );
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["type"], "invalid_request_error");
        assert_eq!(json["error"]["param"], "models");
    }

    // Alias + namespaced ID of the same catalog entry is also a duplicate.
    for models in [
        r#"["anthropic/claude-opus-4-8"]"#,
        r#"["cohere/command-x"]"#,
    ] {
        let response = client
            .post(format!("{router}/v1/messages"))
            .body(format!(
                r#"{{"model":"claude-opus-4-8","models":{models},"max_tokens":16,"messages":[]}}"#
            ))
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "models={models}"
        );
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["type"], "error");
        assert_eq!(json["error"]["type"], "invalid_request_error");
    }

    // Validation completes before the first billable attempt.
    assert!(bodies_a.lock().unwrap().is_empty());
    assert!(bodies_o.lock().unwrap().is_empty());
    assert!(bodies_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn policy_preflight_fails_over_mixed_versions_but_never_executes_without_a_valid_reply() {
    for first_reply in [
        PolicyReply::Fixed(StatusCode::NOT_FOUND, "{}"),
        PolicyReply::Fixed(StatusCode::INTERNAL_SERVER_ERROR, "{}"),
        PolicyReply::Fixed(StatusCode::OK, r#"{"schema_version":1,"mode":"strict"}"#),
    ] {
        let (a, bodies_a, _) = policy_attempt_plane(
            ANTHROPIC_MODELS,
            "/v1/models",
            first_reply,
            StatusCode::OK,
            None,
        )
        .await;
        let (o, bodies_o, log_o) = policy_attempt_plane(
            OPENAI_MODELS,
            "/v1/models",
            PolicyReply::Unrestricted,
            StatusCode::OK,
            None,
        )
        .await;
        let (g, bodies_g, _) = policy_attempt_plane(
            GEMINI_MODELS,
            "/v1beta/models",
            PolicyReply::Fixed(StatusCode::NOT_FOUND, "{}"),
            StatusCode::OK,
            None,
        )
        .await;
        let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
        let response = reqwest::Client::new()
            .post(format!("{router}/v1/responses"))
            .body(
                r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"input":"hi"}"#,
            )
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(bodies_a.lock().unwrap().len(), 1);
        assert!(bodies_o.lock().unwrap().is_empty());
        assert!(bodies_g.lock().unwrap().is_empty());
        assert!(log_o
            .lock()
            .unwrap()
            .iter()
            .any(|request| { request.path == "/internal/router/policy/preflight" }));
    }

    let (a, bodies_a, _) = policy_attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        PolicyReply::Fixed(StatusCode::NOT_FOUND, "{}"),
        StatusCode::OK,
        None,
    )
    .await;
    let (o, bodies_o, _) = policy_attempt_plane(
        OPENAI_MODELS,
        "/v1/models",
        PolicyReply::Fixed(StatusCode::INTERNAL_SERVER_ERROR, "{}"),
        StatusCode::OK,
        None,
    )
    .await;
    let (g, bodies_g, _) = policy_attempt_plane(
        GEMINI_MODELS,
        "/v1beta/models",
        PolicyReply::Fixed(StatusCode::OK, "not-json"),
        StatusCode::OK,
        None,
    )
    .await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"max_tokens":16,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["type"], "api_error");
    assert!(bodies_a.lock().unwrap().is_empty());
    assert!(bodies_o.lock().unwrap().is_empty());
    assert!(bodies_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn policy_401_is_terminal_and_invalid_ordered_subsets_fail_closed() {
    let (a, bodies_a, _) = policy_attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        PolicyReply::Fixed(StatusCode::UNAUTHORIZED, "{}"),
        StatusCode::OK,
        None,
    )
    .await;
    let (o, bodies_o, log_o) = policy_attempt_plane(
        OPENAI_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (g, bodies_g, _) = policy_attempt_plane(
        GEMINI_MODELS,
        "/v1beta/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert!(!log_o
        .lock()
        .unwrap()
        .iter()
        .any(|request| { request.path == "/internal/router/policy/preflight" }));
    assert!(bodies_a.lock().unwrap().is_empty());
    assert!(bodies_o.lock().unwrap().is_empty());
    assert!(bodies_g.lock().unwrap().is_empty());

    for invalid in [
        r#"{"schema_version":1,"mode":"strict","allowed":["unknown/model"]}"#,
        r#"{"schema_version":1,"mode":"strict","allowed":["anthropic/claude-opus-4-8","anthropic/claude-opus-4-8"]}"#,
        r#"{"schema_version":1,"mode":"strict","allowed":["openai/gpt-5.6","anthropic/claude-opus-4-8"]}"#,
    ] {
        let (a, bodies_a, _) = policy_attempt_plane(
            ANTHROPIC_MODELS,
            "/v1/models",
            PolicyReply::Fixed(StatusCode::OK, invalid),
            StatusCode::OK,
            None,
        )
        .await;
        let (o, bodies_o, _) = policy_attempt_plane(
            OPENAI_MODELS,
            "/v1/models",
            PolicyReply::Fixed(StatusCode::NOT_FOUND, "{}"),
            StatusCode::OK,
            None,
        )
        .await;
        let (g, bodies_g, _) = policy_attempt_plane(
            GEMINI_MODELS,
            "/v1beta/models",
            PolicyReply::Fixed(StatusCode::NOT_FOUND, "{}"),
            StatusCode::OK,
            None,
        )
        .await;
        let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
        let response = reqwest::Client::new()
            .post(format!("{router}/v1/responses"))
            .body(
                r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"input":"hi"}"#,
            )
            .send()
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "{invalid}"
        );
        assert!(bodies_a.lock().unwrap().is_empty());
        assert!(bodies_o.lock().unwrap().is_empty());
        assert!(bodies_g.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn provider_filters_order_and_allow_fallbacks_are_deterministic() {
    let (a, bodies_a, _) = policy_attempt_plane(
        ANTHROPIC_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (o, bodies_o, _) = policy_attempt_plane(
        OPENAI_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (g, bodies_g, _) = policy_attempt_plane(
        GEMINI_ROUTING_MODELS,
        "/v1beta/models",
        PolicyReply::Unrestricted,
        StatusCode::SERVICE_UNAVAILABLE,
        Some("not_started"),
    )
    .await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"anthropic/claude-sonnet-5","models":["openai/gpt-5.6-terra","google/gemini-3.6-flash"],"provider":{"ignore":["anthropic"],"order":["google","openai"]},"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(bodies_a.lock().unwrap().is_empty());
    let google: serde_json::Value = serde_json::from_slice(&bodies_g.lock().unwrap()[0]).unwrap();
    let openai: serde_json::Value = serde_json::from_slice(&bodies_o.lock().unwrap()[0]).unwrap();
    assert_eq!(google["model"], "google/gemini-3.6-flash");
    assert_eq!(openai["model"], "openai/gpt-5.6-terra");
    assert!(google.get("provider").is_none());
    assert!(google.get("models").is_none());

    let (a, _, _) = policy_attempt_plane(
        ANTHROPIC_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (o, bodies_o, log_o) = policy_attempt_plane(
        OPENAI_ROUTING_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (g, bodies_g, _) = policy_attempt_plane(
        GEMINI_ROUTING_MODELS,
        "/v1beta/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/responses"))
        .body(r#"{"model":"openai/gpt-5.6-sol","models":["openai/gpt-5.6-luna","google/gemini-3.1-flash-lite"],"provider":{"allow_fallbacks":false},"input":"hi"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(bodies_g.lock().unwrap().is_empty());
    let body: serde_json::Value = serde_json::from_slice(&bodies_o.lock().unwrap()[0]).unwrap();
    assert_eq!(body["model"], "openai/gpt-5.6-sol");
    let execution = log_o
        .lock()
        .unwrap()
        .iter()
        .find(|request| request.path == "/v1/responses")
        .cloned()
        .unwrap();
    assert!(execution.execution_group.is_none());
}

#[tokio::test]
async fn provider_validation_and_chain_bounds_fail_before_execution() {
    let (a, bodies_a) =
        attempt_plane(ANTHROPIC_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (o, bodies_o) =
        attempt_plane(OPENAI_MODELS, "/v1/models", StatusCode::OK, None, false).await;
    let (g, bodies_g) =
        attempt_plane(GEMINI_MODELS, "/v1beta/models", StatusCode::OK, None, false).await;
    let router = spawn(make_fallback_router(&a, &o, &g, Duration::from_secs(60))).await;
    let client = reqwest::Client::new();

    for body in [
        r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"provider":{"sort":"price"},"input":"hi"}"#.to_string(),
        r#"{"model":"anthropic/claude-opus-4-8","models":["openai/gpt-5.6"],"provider":{"only":[]},"input":"hi"}"#.to_string(),
        r#"{"model":"anthropic/claude-opus-4-8","provider":{"only":["openai"],"ignore":["openai"]},"input":"hi"}"#.to_string(),
    ] {
        let response = client
            .post(format!("{router}/v1/responses"))
            .body(body)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    let overflow: Vec<_> = (0..32)
        .map(|index| format!("unknown/model-{index}"))
        .collect();
    let response = client
        .post(format!("{router}/v1/responses"))
        .json(&serde_json::json!({
            "model": "anthropic/claude-opus-4-8",
            "models": overflow,
            "input": "hi"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(bodies_a.lock().unwrap().is_empty());
    assert!(bodies_o.lock().unwrap().is_empty());
    assert!(bodies_g.lock().unwrap().is_empty());
}

#[tokio::test]
async fn policy_transport_failure_uses_another_authority_origin_then_fenced_fallback() {
    let openai = one_shot_catalog_origin(OPENAI_MODELS).await;
    let (anthropic, bodies_a, _) = policy_attempt_plane(
        ANTHROPIC_MODELS,
        "/v1/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let (gemini, _, _) = policy_attempt_plane(
        GEMINI_MODELS,
        "/v1beta/models",
        PolicyReply::Unrestricted,
        StatusCode::OK,
        None,
    )
    .await;
    let router = spawn(make_fallback_router(
        &anthropic,
        &openai,
        &gemini,
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"openai/gpt-5.6","models":["anthropic/claude-opus-4-8"],"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(bodies_a.lock().unwrap().len(), 1);
}

// ---------- universal chat dispatch (model-based routing, этап 3.1) ----------

/// Плоскость для chat-тестов: каталог на своём catalog path + echo всего
/// остального (тело запроса байт-в-байт, лог заголовков).
async fn chat_plane(body: &'static str, path: &'static str) -> (String, SharedLog) {
    let log: SharedLog = Arc::new(StdMutex::new(Vec::new()));
    let state = log.clone();
    let router = Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let log = state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            log.lock().unwrap().push(record_of(&req));
            if req.uri().path() == path {
                return AxumResponse::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap();
            }
            let bytes = to_bytes(req.into_body(), 64 * 1024 * 1024).await.unwrap();
            AxumResponse::builder()
                .header("content-type", "application/octet-stream")
                .body(Body::from(bytes))
                .unwrap()
        }
    }));
    (spawn(router).await, log)
}

#[tokio::test]
async fn chat_namespaced_models_route_by_prefix_without_catalog_fetch() {
    let (a, log_a) = echo_plane().await;
    let (o, log_o) = echo_plane().await;
    let (g, log_g) = echo_plane().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in [
        "anthropic/claude-opus-4-8",
        "openai/gpt-5.6",
        "google/gemini-2.5-pro",
    ] {
        let payload =
            format!(r#"{{"model":"{model}","messages":[{{"role":"user","content":"hi"}}]}}"#);
        let response = client
            .post(format!("{router}/v1/chat/completions"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        // Тело проксируется байт-в-байт: namespaced ID резолвит плоскость.
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let (la, lo, lg) = (
        log_a.lock().unwrap().clone(),
        log_o.lock().unwrap().clone(),
        log_g.lock().unwrap().clone(),
    );
    // Ровно один запрос на каждой плоскости: catalog fetch не выполнялся,
    // dispatch по префиксу каталог не опрашивает.
    assert_eq!(la.len(), 1);
    assert_eq!(lo.len(), 1);
    assert_eq!(lg.len(), 1);
    assert_eq!(la[0].path, "/v1/chat/completions");
    assert_eq!(lo[0].path, "/v1/chat/completions");
    assert_eq!(lg[0].path, "/v1/chat/completions");
    assert_eq!(la[0].x_api_key.as_deref(), Some("sk-pool-secret"));
}

#[tokio::test]
async fn chat_alias_routes_via_cached_catalog() {
    let (a, log_a) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, log_o) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, log_g) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in ["claude-opus-4-8", "gpt-5.6", "gemini-2.5-pro"] {
        let payload = format!(r#"{{"model":"{model}","messages":[]}}"#);
        let response = client
            .post(format!("{router}/v1/chat/completions"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let paths = |log: &SharedLog| {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
    };
    let (pa, po, pg) = (paths(&log_a), paths(&log_o), paths(&log_g));
    // Каждая плоскость увидела catalog fetch и ровно один chat-запрос.
    assert!(pa.contains(&"/v1/models?limit=1000".to_string()), "{pa:?}");
    assert!(po.contains(&"/v1/models".to_string()), "{po:?}");
    assert!(
        pg.contains(&"/v1beta/models?pageSize=1000".to_string()),
        "{pg:?}"
    );
    assert_eq!(
        pa.iter().filter(|p| *p == "/v1/chat/completions").count(),
        1,
        "{pa:?}"
    );
    assert_eq!(
        po.iter().filter(|p| *p == "/v1/chat/completions").count(),
        1,
        "{po:?}"
    );
    assert_eq!(
        pg.iter().filter(|p| *p == "/v1/chat/completions").count(),
        1,
        "{pg:?}"
    );
}

#[tokio::test]
async fn chat_unknown_model_is_openai_shaped_404() {
    let (a, _) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, _) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, _) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    // Неизвестный префикс и неизвестный alias оба уходят в alias-поиск и 404.
    for model in ["cohere/command-x", "gpt-9"] {
        let payload = format!(r#"{{"model":"{model}","messages":[]}}"#);
        let response = client
            .post(format!("{router}/v1/chat/completions"))
            .body(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{model}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["code"], "model_not_found", "{model}");
        assert_eq!(json["error"]["param"], "model", "{model}");
        assert!(
            json["error"]["message"].as_str().unwrap().contains(model),
            "{model}"
        );
    }
}

#[tokio::test]
async fn chat_invalid_json_and_missing_model_are_400_without_plane_call() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for body in ["not json{", "{}", r#"{"model":42}"#, r#"{"model":""}"#] {
        let response = client
            .post(format!("{router}/v1/chat/completions"))
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["type"], "invalid_request_error", "{body}");
        if body == "not json{" {
            assert!(json["error"]["param"].is_null(), "{body}");
        } else {
            assert_eq!(json["error"]["param"], "model", "{body}");
        }
    }
    // Невалидный запрос не должен дойти до плоскости.
    assert!(log_a.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unauthorized_client_is_rejected_while_large_body_stream_is_still_open() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempt_state = attempts.clone();
    let plane = spawn(Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let attempts = attempt_state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return AxumResponse::builder()
                    .status(StatusCode::UNAUTHORIZED)
                    .body(Body::from("{}"))
                    .unwrap();
            }
            attempts.fetch_add(1, Ordering::SeqCst);
            AxumResponse::builder()
                .status(StatusCode::OK)
                .body(Body::from("{}"))
                .unwrap()
        }
    })))
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:0",
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;
    let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
    body_tx
        .send(Ok(Bytes::from_static(
            b"{\"model\":\"anthropic/claude-opus-4-8\",",
        )))
        .await
        .unwrap();
    let response = tokio::time::timeout(
        Duration::from_millis(500),
        reqwest::Client::new()
            .post(format!("{router}/v1/chat/completions"))
            .body(reqwest::Body::wrap_stream(ReceiverStream::new(body_rx)))
            .send(),
    )
    .await
    .expect("auth rejection must not wait for the unfinished body")
    .unwrap();
    drop(body_tx);

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn universal_body_budget_fails_fast_without_becoming_an_execution_queue() {
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempt_state = attempts.clone();
    let plane = spawn(Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let attempts = attempt_state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            attempts.fetch_add(1, Ordering::SeqCst);
            AxumResponse::new(Body::from("unexpected execution"))
        }
    })))
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:0",
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;
    let held_body = || {
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        tx.try_send(Ok::<Bytes, std::io::Error>(Bytes::from(vec![
            b' ';
            64 * 1024
                * 1024
        ])))
        .unwrap();
        (reqwest::Body::wrap_stream(ReceiverStream::new(rx)), tx)
    };
    let mut held_senders = Vec::new();
    let mut held_tasks = Vec::new();
    // The 512 MiB budget admits eight maximal 64 MiB bodies before fail-fast overload.
    for _ in 0..8 {
        let (body, tx) = held_body();
        let held_router = router.clone();
        held_tasks.push(tokio::spawn(async move {
            reqwest::Client::new()
                .post(format!("{held_router}/v1/chat/completions"))
                .body(body)
                .send()
                .await
        }));
        held_senders.push(tx);
    }
    let client = reqwest::Client::new();
    for _ in 0..100 {
        let metrics = client
            .get(format!("{router}/metrics"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        if metrics.contains("claude_router_active_body_admission_units 512") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(attempts.load(Ordering::SeqCst), 0);

    let third = tokio::time::timeout(
        Duration::from_millis(500),
        client
            .post(format!("{router}/v1/chat/completions"))
            .body(r#"{"model":"anthropic/claude-opus-4-8","messages":[]}"#)
            .send(),
    )
    .await
    .expect("body admission overload must fail without queueing")
    .unwrap();
    held_senders.clear();
    for task in held_tasks {
        task.abort();
    }

    assert_eq!(third.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body: serde_json::Value = third.json().await.unwrap();
    assert_eq!(body["error"]["code"], "router_overloaded");
    assert_eq!(attempts.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn empty_chunked_slow_clients_reserve_one_unit_each_not_half_the_budget() {
    let (plane, _) = echo_plane().await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:0",
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;
    let slow_body = || {
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(1);
        (reqwest::Body::wrap_stream(ReceiverStream::new(rx)), tx)
    };
    let mut tasks = Vec::new();
    let mut senders = Vec::new();
    for _ in 0..2 {
        let (body, tx) = slow_body();
        let target = router.clone();
        tasks.push(tokio::spawn(async move {
            reqwest::Client::new()
                .post(format!("{target}/v1/chat/completions"))
                .body(body)
                .send()
                .await
        }));
        senders.push(tx);
    }
    let client = reqwest::Client::new();
    for _ in 0..100 {
        let metrics = client
            .get(format!("{router}/metrics"))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        if metrics.contains("claude_router_active_body_admission_units 2") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let response = tokio::time::timeout(
        Duration::from_secs(1),
        client
            .post(format!("{router}/v1/chat/completions"))
            .body(r#"{"model":"anthropic/claude-opus-4-8","messages":[]}"#)
            .send(),
    )
    .await
    .expect("two empty slow bodies must leave capacity for normal traffic")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    drop(senders);
    for task in tasks {
        task.abort();
    }
}

#[tokio::test]
async fn body_permit_is_not_retained_by_open_sse_responses() {
    type OpenStreams = Arc<StdMutex<Vec<tokio::sync::mpsc::Sender<Result<Bytes, std::io::Error>>>>>;
    let open_streams: OpenStreams = Arc::new(StdMutex::new(Vec::new()));
    let stream_state = open_streams.clone();
    let plane = spawn(Router::new().fallback(any(move |req: AxumRequest<Body>| {
        let open_streams = stream_state.clone();
        async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(2);
            tx.try_send(Ok(Bytes::from_static(b"data: open\n\n")))
                .unwrap();
            open_streams.lock().unwrap().push(tx);
            AxumResponse::builder()
                .status(StatusCode::OK)
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        }
    })))
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:0",
        "http://127.0.0.1:0",
        Duration::ZERO,
    ))
    .await;
    let mut responses = Vec::new();
    for _ in 0..3 {
        let request_body =
            reqwest::Body::wrap_stream(tokio_stream::iter(vec![Ok::<Bytes, std::io::Error>(
                Bytes::from_static(br#"{"model":"anthropic/claude-opus-4-8","stream":true}"#),
            )]));
        let response = tokio::time::timeout(
            Duration::from_millis(500),
            reqwest::Client::new()
                .post(format!("{router}/v1/chat/completions"))
                .body(request_body)
                .send(),
        )
        .await
        .expect("an open SSE response must not retain the request-body permit")
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    assert_eq!(open_streams.lock().unwrap().len(), 3);
    drop(responses);
    open_streams.lock().unwrap().clear();
}

#[tokio::test]
async fn chat_namespaced_dispatch_survives_dead_catalog_alias_is_503() {
    let (a, _) = echo_plane().await;
    let (d1, d2) = (dead_origin().await, dead_origin().await);
    let router = spawn(make_router(&a, &d1, &d2, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    // Namespaced dispatch каталог не опрашивает: мёртвые плоскости не мешают.
    let response = client
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Alias без живого каталога (и без кэша) — честный 503.
    let response = client
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"claude-opus-4-8","messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["code"], "catalog_unavailable");
}

#[tokio::test]
async fn chat_alias_with_auth_rejected_catalog_is_unified_401() {
    let (a, _) = catalog_plane("", "/v1/models", "auth").await;
    let (d1, d2) = (dead_origin().await, dead_origin().await);
    let router = spawn(make_router(&a, &d1, &d2, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"claude-opus-4-8","messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["code"], "invalid_api_key");
}

#[tokio::test]
async fn chat_oversized_body_is_413_and_never_reaches_plane() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .header(reqwest::header::CONTENT_LENGTH, 64 * 1024 * 1024 + 1)
        .body(oversized_pending_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["type"], "invalid_request_error");
    assert_eq!(json["error"]["code"], "payload_too_large");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("64 MiB"));
    assert!(log_a.lock().unwrap().is_empty());
}

#[tokio::test]
async fn chat_gzip_body_is_415_and_never_reaches_plane() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .header(reqwest::header::CONTENT_ENCODING, "gzip")
        .body(r#"{"model":"anthropic/claude-opus-4-8","messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["code"], "unsupported_content_encoding");
    assert!(log_a.lock().unwrap().is_empty());
}

#[tokio::test]
async fn chat_sse_response_first_chunk_is_not_buffered() {
    // Буферизуется только тело запроса; ответ стримится, как в native lanes.
    let plane = spawn(
        Router::new().fallback(any(|req: AxumRequest<Body>| async move {
            if req.uri().path() == "/internal/router/auth/preflight" {
                return authenticated_response();
            }
            let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(4);
            tokio::spawn(async move {
                let _ = tx.send(Ok(Bytes::from("data: {\"delta\":{}}\n\n"))).await;
                tokio::time::sleep(Duration::from_millis(700)).await;
                let _ = tx.send(Ok(Bytes::from("data: [DONE]\n\n"))).await;
            });
            AxumResponse::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(ReceiverStream::new(rx)))
                .unwrap()
        })),
    )
    .await;
    let router = spawn(make_router(
        &plane,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;

    let mut response = reqwest::Client::new()
        .post(format!("{router}/v1/chat/completions"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","stream":true,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_millis(400), response.chunk())
        .await
        .expect("first chat SSE chunk must arrive before the plane finishes")
        .unwrap()
        .expect("first chunk present");
    assert!(String::from_utf8_lossy(&first).contains("delta"));
}

// ---------- universal responses dispatch (model-based routing, этап 4.1) ----------

#[tokio::test]
async fn responses_namespaced_models_route_by_prefix_without_catalog_fetch() {
    let (a, log_a) = echo_plane().await;
    let (o, log_o) = echo_plane().await;
    let (g, log_g) = echo_plane().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in [
        "anthropic/claude-opus-4-8",
        "openai/gpt-5.6",
        "google/gemini-2.5-pro",
    ] {
        let payload = format!(r#"{{"model":"{model}","input":"hi"}}"#);
        let response = client
            .post(format!("{router}/v1/responses"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        // Тело проксируется байт-в-байт: namespaced ID резолвит плоскость.
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let (la, lo, lg) = (
        log_a.lock().unwrap().clone(),
        log_o.lock().unwrap().clone(),
        log_g.lock().unwrap().clone(),
    );
    // Ровно один запрос на каждой плоскости: catalog fetch не выполнялся,
    // dispatch по префиксу каталог не опрашивает.
    assert_eq!(la.len(), 1);
    assert_eq!(lo.len(), 1);
    assert_eq!(lg.len(), 1);
    assert_eq!(la[0].path, "/v1/responses");
    assert_eq!(lo[0].path, "/v1/responses");
    assert_eq!(lg[0].path, "/v1/responses");
    assert_eq!(la[0].x_api_key.as_deref(), Some("sk-pool-secret"));
}

#[tokio::test]
async fn responses_alias_routes_via_cached_catalog() {
    let (a, log_a) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, log_o) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, log_g) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in ["claude-opus-4-8", "gpt-5.6", "gemini-2.5-pro"] {
        let payload = format!(r#"{{"model":"{model}","input":"hi"}}"#);
        let response = client
            .post(format!("{router}/v1/responses"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let paths = |log: &SharedLog| {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
    };
    let (pa, po, pg) = (paths(&log_a), paths(&log_o), paths(&log_g));
    // Каждая плоскость увидела catalog fetch и ровно один responses-запрос.
    assert!(pa.contains(&"/v1/models?limit=1000".to_string()), "{pa:?}");
    assert!(po.contains(&"/v1/models".to_string()), "{po:?}");
    assert!(
        pg.contains(&"/v1beta/models?pageSize=1000".to_string()),
        "{pg:?}"
    );
    assert_eq!(
        pa.iter().filter(|p| *p == "/v1/responses").count(),
        1,
        "{pa:?}"
    );
    assert_eq!(
        po.iter().filter(|p| *p == "/v1/responses").count(),
        1,
        "{po:?}"
    );
    assert_eq!(
        pg.iter().filter(|p| *p == "/v1/responses").count(),
        1,
        "{pg:?}"
    );
}

#[tokio::test]
async fn responses_unknown_model_is_openai_shaped_404() {
    let (a, _) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, _) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, _) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    // Неизвестный префикс и неизвестный alias оба уходят в alias-поиск и 404.
    for model in ["cohere/command-x", "gpt-9"] {
        let payload = format!(r#"{{"model":"{model}","input":"hi"}}"#);
        let response = client
            .post(format!("{router}/v1/responses"))
            .body(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{model}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["code"], "model_not_found", "{model}");
        assert_eq!(json["error"]["param"], "model", "{model}");
        assert!(
            json["error"]["message"].as_str().unwrap().contains(model),
            "{model}"
        );
    }
}

#[tokio::test]
async fn responses_invalid_json_and_missing_model_are_400_without_plane_call() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for body in ["not json{", "{}", r#"{"model":42}"#, r#"{"model":""}"#] {
        let response = client
            .post(format!("{router}/v1/responses"))
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["error"]["type"], "invalid_request_error", "{body}");
        if body == "not json{" {
            assert!(json["error"]["param"].is_null(), "{body}");
        } else {
            assert_eq!(json["error"]["param"], "model", "{body}");
        }
    }
    // Невалидный запрос не должен дойти до плоскости.
    assert!(log_a.lock().unwrap().is_empty());
}

#[tokio::test]
async fn responses_oversized_body_is_413_and_never_reaches_plane() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/responses"))
        .header(reqwest::header::CONTENT_LENGTH, 64 * 1024 * 1024 + 1)
        .body(oversized_pending_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["type"], "invalid_request_error");
    assert_eq!(json["error"]["code"], "payload_too_large");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("64 MiB"));
    assert!(log_a.lock().unwrap().is_empty());
}

// ---------- universal messages dispatch (model-based routing, этап 5.1) ----------

#[tokio::test]
async fn messages_namespaced_models_route_by_prefix_without_catalog_fetch() {
    let (a, log_a) = echo_plane().await;
    let (o, log_o) = echo_plane().await;
    let (g, log_g) = echo_plane().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in [
        "anthropic/claude-opus-4-8",
        "openai/gpt-5.6",
        "google/gemini-2.5-pro",
    ] {
        let payload = format!(
            r#"{{"model":"{model}","max_tokens":64,"messages":[{{"role":"user","content":"hi"}}]}}"#
        );
        let response = client
            .post(format!("{router}/v1/messages"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        // Тело проксируется байт-в-байт: namespaced ID резолвит плоскость.
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let (la, lo, lg) = (
        log_a.lock().unwrap().clone(),
        log_o.lock().unwrap().clone(),
        log_g.lock().unwrap().clone(),
    );
    // Ровно один запрос на каждой плоскости: catalog fetch не выполнялся,
    // dispatch по префиксу каталог не опрашивает.
    assert_eq!(la.len(), 1);
    assert_eq!(lo.len(), 1);
    assert_eq!(lg.len(), 1);
    assert_eq!(la[0].path, "/v1/messages");
    assert_eq!(lo[0].path, "/v1/messages");
    assert_eq!(lg[0].path, "/v1/messages");
    assert_eq!(la[0].x_api_key.as_deref(), Some("sk-pool-secret"));
}

/// Регрессия контракта native lane: claude-alias через каталог обязан
/// остаться байт-идентичным passthrough на Anthropic-плоскость (как и
/// namespaced `anthropic/*` выше).
#[tokio::test]
async fn messages_alias_routes_via_cached_catalog() {
    let (a, log_a) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, log_o) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, log_g) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in ["claude-opus-4-8", "gpt-5.6", "gemini-2.5-pro"] {
        let payload = format!(r#"{{"model":"{model}","max_tokens":64,"messages":[]}}"#);
        let response = client
            .post(format!("{router}/v1/messages"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let paths = |log: &SharedLog| {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
    };
    let (pa, po, pg) = (paths(&log_a), paths(&log_o), paths(&log_g));
    // Каждая плоскость увидела catalog fetch и ровно один messages-запрос.
    assert!(pa.contains(&"/v1/models?limit=1000".to_string()), "{pa:?}");
    assert!(po.contains(&"/v1/models".to_string()), "{po:?}");
    assert!(
        pg.contains(&"/v1beta/models?pageSize=1000".to_string()),
        "{pg:?}"
    );
    assert_eq!(
        pa.iter().filter(|p| *p == "/v1/messages").count(),
        1,
        "{pa:?}"
    );
    assert_eq!(
        po.iter().filter(|p| *p == "/v1/messages").count(),
        1,
        "{po:?}"
    );
    assert_eq!(
        pg.iter().filter(|p| *p == "/v1/messages").count(),
        1,
        "{pg:?}"
    );
}

#[tokio::test]
async fn messages_bare_haiku_alias_rewrites_to_the_dated_native_id() {
    let (a, log_a) = chat_plane(ANTHROPIC_ROUTING_MODELS, "/v1/models").await;
    let (o, log_o) = chat_plane(OPENAI_ROUTING_MODELS, "/v1/models").await;
    let (g, log_g) = chat_plane(GEMINI_ROUTING_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{router}/v1/messages"))
        .header("x-api-key", "sk-pool-secret")
        .body(r#"{"model":"claude-haiku-4-5","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let echoed: serde_json::Value = response.json().await.unwrap();
    assert_eq!(echoed["model"], "claude-haiku-4-5-20251001");
    assert_eq!(echoed["max_tokens"], 64);

    let response = client
        .post(format!("{router}/v1/messages"))
        .header("x-api-key", "sk-pool-secret")
        .body(r#"{"model":"claude-haiku-4-5-20251001","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let echoed: serde_json::Value = response.json().await.unwrap();
    assert_eq!(echoed["model"], "claude-haiku-4-5-20251001");

    let (la, lo, lg) = (
        log_a.lock().unwrap().clone(),
        log_o.lock().unwrap().clone(),
        log_g.lock().unwrap().clone(),
    );
    let paths = |log: &SharedLog| {
        log.lock()
            .unwrap()
            .iter()
            .map(|r| r.path.clone())
            .collect::<Vec<_>>()
    };
    let (pa, po, pg) = (paths(&log_a), paths(&log_o), paths(&log_g));
    assert!(pa.contains(&"/v1/models?limit=1000".to_string()), "{pa:?}");
    assert!(po.contains(&"/v1/models".to_string()), "{po:?}");
    assert!(
        pg.contains(&"/v1beta/models?pageSize=1000".to_string()),
        "{pg:?}"
    );
    assert_eq!(
        pa.iter().filter(|p| *p == "/v1/messages").count(),
        2,
        "{pa:?}"
    );
    assert_eq!(
        po.iter().filter(|p| *p == "/v1/messages").count(),
        0,
        "{po:?}"
    );
    assert_eq!(
        pg.iter().filter(|p| *p == "/v1/messages").count(),
        0,
        "{pg:?}"
    );
}

#[tokio::test]
async fn messages_unknown_model_is_anthropic_shaped_404() {
    let (a, _) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, _) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, _) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    // Неизвестный префикс и неизвестный alias оба уходят в alias-поиск и 404.
    for model in ["cohere/command-x", "gpt-9"] {
        let payload = format!(r#"{{"model":"{model}","max_tokens":64,"messages":[]}}"#);
        let response = client
            .post(format!("{router}/v1/messages"))
            .body(payload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{model}");
        let json: serde_json::Value = response.json().await.unwrap();
        // Ошибки messages dispatch — Anthropic-конверт, не OpenAI.
        assert_eq!(json["type"], "error", "{model}");
        assert_eq!(json["error"]["type"], "not_found_error", "{model}");
        assert!(
            json["error"]["message"].as_str().unwrap().contains(model),
            "{model}"
        );
    }
}

#[tokio::test]
async fn messages_invalid_json_and_missing_model_are_anthropic_shaped_400() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let client = reqwest::Client::new();

    for body in ["not json{", "{}", r#"{"model":42}"#, r#"{"model":""}"#] {
        let response = client
            .post(format!("{router}/v1/messages"))
            .body(body.to_string())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
        let json: serde_json::Value = response.json().await.unwrap();
        assert_eq!(json["type"], "error", "{body}");
        assert_eq!(json["error"]["type"], "invalid_request_error", "{body}");
        assert!(
            json["error"]["message"].as_str().unwrap().contains("model") || body == "not json{",
            "{body}"
        );
    }
    // Невалидный запрос не должен дойти до плоскости.
    assert!(log_a.lock().unwrap().is_empty());
}

#[tokio::test]
async fn messages_namespaced_dispatch_survives_dead_catalog_alias_is_503() {
    let (a, _) = echo_plane().await;
    let (d1, d2) = (dead_origin().await, dead_origin().await);
    let router = spawn(make_router(&a, &d1, &d2, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    // Namespaced dispatch каталог не опрашивает: мёртвые плоскости не мешают.
    let response = client
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"anthropic/claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Alias без живого каталога (и без кэша) — честный 503 в Anthropic-конверте.
    let response = client
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "api_error");
}

#[tokio::test]
async fn messages_alias_with_auth_rejected_catalog_is_anthropic_shaped_401() {
    let (a, _) = catalog_plane("", "/v1/models", "auth").await;
    let (d1, d2) = (dead_origin().await, dead_origin().await);
    let router = spawn(make_router(&a, &d1, &d2, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .body(r#"{"model":"claude-opus-4-8","max_tokens":64,"messages":[]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "authentication_error");
}

#[tokio::test]
async fn messages_oversized_body_is_413_and_never_reaches_plane() {
    let (a, log_a) = echo_plane().await;
    let router = spawn(make_router(
        &a,
        "http://127.0.0.1:1",
        "http://127.0.0.1:2",
        Duration::ZERO,
    ))
    .await;
    let response = reqwest::Client::new()
        .post(format!("{router}/v1/messages"))
        .header(reqwest::header::CONTENT_LENGTH, 64 * 1024 * 1024 + 1)
        .body(oversized_pending_body())
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"]["type"], "invalid_request_error");
    assert!(json["error"]["message"]
        .as_str()
        .unwrap()
        .contains("64 MiB"));
    assert!(log_a.lock().unwrap().is_empty());
}

/// `/v1/messages/count_tokens` использует тот же namespace-dispatch, что Messages:
/// endpoint реализован на каждой плоскости, каталог для namespaced ID не нужен.
#[tokio::test]
async fn count_tokens_namespaced_models_route_by_prefix_without_catalog_fetch() {
    let (a, log_a) = echo_plane().await;
    let (o, log_o) = echo_plane().await;
    let (g, log_g) = echo_plane().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in [
        "anthropic/claude-opus-4-8",
        "openai/gpt-5.6",
        "google/gemini-2.5-pro",
    ] {
        let payload =
            format!(r#"{{"model":"{model}","messages":[{{"role":"user","content":"hi"}}]}}"#);
        let response = client
            .post(format!("{router}/v1/messages/count_tokens"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    for log in [&log_a, &log_o, &log_g] {
        let requests = log.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].path, "/v1/messages/count_tokens");
        assert_eq!(requests[0].x_api_key.as_deref(), Some("sk-pool-secret"));
    }
}

#[tokio::test]
async fn count_tokens_aliases_route_via_cached_catalog() {
    let (a, log_a) = chat_plane(ANTHROPIC_MODELS, "/v1/models").await;
    let (o, log_o) = chat_plane(OPENAI_MODELS, "/v1/models").await;
    let (g, log_g) = chat_plane(GEMINI_MODELS, "/v1beta/models").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    for model in ["claude-opus-4-8", "gpt-5.6", "gemini-2.5-pro"] {
        let payload = format!(r#"{{"model":"{model}","messages":[]}}"#);
        let response = client
            .post(format!("{router}/v1/messages/count_tokens"))
            .header("x-api-key", "sk-pool-secret")
            .body(payload.clone())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{model}");
        assert_eq!(response.text().await.unwrap(), payload, "{model}");
    }

    let paths = |log: &SharedLog| {
        log.lock()
            .unwrap()
            .iter()
            .map(|request| request.path.clone())
            .collect::<Vec<_>>()
    };
    let (pa, po, pg) = (paths(&log_a), paths(&log_o), paths(&log_g));
    assert!(pa.contains(&"/v1/models?limit=1000".to_string()), "{pa:?}");
    assert!(po.contains(&"/v1/models".to_string()), "{po:?}");
    assert!(
        pg.contains(&"/v1beta/models?pageSize=1000".to_string()),
        "{pg:?}"
    );
    for paths in [&pa, &po, &pg] {
        assert_eq!(
            paths
                .iter()
                .filter(|path| *path == "/v1/messages/count_tokens")
                .count(),
            1,
            "{paths:?}"
        );
    }
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
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    assert!(response.headers().get(catalog::DEGRADED_HEADER).is_none());
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["object"], "list");
    let ids: Vec<&str> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
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
    assert_eq!(json["data"][0]["created"], 1_779_926_400);
    assert_eq!(json["data"][2]["created"], 1_783_555_200);
    assert_eq!(json["data"][4]["created"], 1_750_118_400);
    assert_eq!(json["data"][0]["owned_by"], "anthropic");
    assert_eq!(
        json["data"][0]["reasoning_efforts"],
        serde_json::json!(["low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(json["data"][1]["reasoning_efforts"], serde_json::json!([]));
    assert_eq!(
        json["data"][2]["reasoning_efforts"],
        serde_json::json!(["none", "low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(
        json["data"][2]["service_tiers"],
        serde_json::json!(["standard", "priority"])
    );
    assert_eq!(
        json["data"][0]["service_tiers"],
        serde_json::json!(["standard"])
    );
    assert_eq!(
        json["data"][4]["service_tiers"],
        serde_json::json!(["standard"])
    );
    assert_eq!(
        json["data"][0]["apitoken"]["limits"],
        serde_json::json!({"context": 1_000_000, "input": 1_000_000, "output": 128_000})
    );
    assert_eq!(
        json["data"][2]["apitoken"]["capabilities"]["reasoning_efforts"],
        serde_json::json!(["none", "low", "medium", "high", "xhigh", "max"])
    );
    assert_eq!(
        json["data"][0]["apitoken"]["capabilities"]["input_modalities"],
        serde_json::json!(["text", "image"])
    );
    assert_eq!(
        json["data"][0]["apitoken"]["capabilities"]["reasoning"],
        true
    );
    assert_eq!(
        json["data"][2]["apitoken"]["capabilities"]["tool_calling"],
        true
    );
    assert_eq!(
        json["data"][4]["apitoken"]["limits"],
        serde_json::json!({"context": 1_048_576, "input": 1_048_576, "output": 65_536})
    );
    assert_eq!(
        json["data"][0]["apitoken"]["pricing"]["unit"],
        "nano_usd_per_million_tokens"
    );
    assert_eq!(
        json["data"][0]["apitoken"]["pricing"]["standard"]["input"],
        "5000000000"
    );
    assert!(json["data"][0]["apitoken"]["pricing"]["priority"].is_null());
    assert_eq!(
        json["data"][2]["apitoken"]["pricing"]["priority"]["output"],
        "5000000000"
    );

    // Auth passthrough: ключ клиента дошёл до плоскости каталога verbatim.
    let catalog_request = log_a
        .lock()
        .unwrap()
        .iter()
        .find(|request| request.method == "GET")
        .cloned()
        .expect("catalog fetch hit the plane");
    assert_eq!(catalog_request.path, "/v1/models?limit=1000");
    assert_eq!(catalog_request.x_api_key.as_deref(), Some("sk-pool-secret"));
}

#[tokio::test]
async fn catalog_returns_codex_native_overlay_after_plane_auth() {
    let (a, o, g, log_a, _, _) = three_catalog_planes().await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;

    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .header("x-api-key", "sk-pool-secret")
        .header("user-agent", "codex_cli_rs/0.146.0")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap(),
        serde_json::json!({"models": []})
    );

    let catalog_request = log_a
        .lock()
        .unwrap()
        .pop()
        .expect("Codex discovery must still authenticate through a provider plane");
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
    assert_eq!(
        response.headers().get(catalog::DEGRADED_HEADER).unwrap(),
        "google"
    );
    let json: serde_json::Value = response.json().await.unwrap();
    let ids: Vec<&str> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 4);
    assert!(!ids.iter().any(|id| id.starts_with("google/")));
}

#[tokio::test]
async fn catalog_treats_malformed_authoritative_metadata_as_a_degraded_plane() {
    const MALFORMED_OPENAI: &str = r#"{"object":"list","data":[
        {"id":"gpt-bad","created":1783555200,"apitoken":{"limits":{"context":0}}}
    ]}"#;
    let (a, _, g, _, _, _) = three_catalog_planes().await;
    let (o, _) = catalog_plane(MALFORMED_OPENAI, "/v1/models", "ok").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;

    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(catalog::DEGRADED_HEADER).unwrap(),
        "openai"
    );
    let json: serde_json::Value = response.json().await.unwrap();
    assert!(json["data"]
        .as_array()
        .unwrap()
        .iter()
        .all(|model| !model["id"].as_str().unwrap().starts_with("openai/")));

    const MISSING_CREATED_OPENAI: &str =
        r#"{"object":"list","data":[{"id":"gpt-undated","object":"model"}]}"#;
    let (o, _) = catalog_plane(MISSING_CREATED_OPENAI, "/v1/models", "ok").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(catalog::DEGRADED_HEADER).unwrap(),
        "openai"
    );
}

#[tokio::test]
async fn catalog_removes_cross_plane_alias_collisions_without_hiding_namespaced_models() {
    const ANTHROPIC_COLLISION: &str = r#"{"data":[{"id":"shared","created_at":"2026-01-01T00:00:00Z","display_name":"Anthropic Shared"}]}"#;
    const OPENAI_COLLISION: &str = r#"{"object":"list","data":[{"id":"shared","object":"model","created":1783555200,"owned_by":"apitoken"}]}"#;
    let (a, _) = catalog_plane(ANTHROPIC_COLLISION, "/v1/models", "ok").await;
    let (o, _) = catalog_plane(OPENAI_COLLISION, "/v1/models", "ok").await;
    let (g, _) = catalog_plane(GEMINI_MODELS, "/v1beta/models", "ok").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let client = reqwest::Client::new();

    let response = client
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let json: serde_json::Value = response.json().await.unwrap();
    let models = json["data"].as_array().unwrap();
    for id in ["anthropic/shared", "openai/shared"] {
        let model = models.iter().find(|model| model["id"] == id).unwrap();
        assert_eq!(model["aliases"], serde_json::json!([]));
    }

    let response = client
        .get(format!("{router}/v1/models/shared"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = client
        .post(format!("{router}/v1/chat/completions"))
        .header("content-type", "application/json")
        .body(r#"{"model":"shared","messages":[{"role":"user","content":"hi"}]}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    for id in ["anthropic/shared", "openai/shared"] {
        let response = client
            .get(format!("{router}/v1/models/{id}"))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{id}");
        assert_eq!(
            response.json::<serde_json::Value>().await.unwrap()["id"],
            id
        );
    }
}

#[tokio::test]
async fn catalog_cache_never_reuses_personalized_rates_between_keys() {
    let (a, o, g, log_a, log_o, log_g) = three_catalog_planes().await;
    let router = spawn(make_router(&a, &o, &g, Duration::from_secs(60))).await;
    let client = reqwest::Client::new();

    let first: serde_json::Value = client
        .get(format!("{router}/v1/models"))
        .header("x-api-key", "key-a")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let second: serde_json::Value = client
        .get(format!("{router}/v1/models"))
        .header("x-api-key", "key-b")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    assert_eq!(
        first["data"][0]["apitoken"]["pricing"]["standard"]["input"],
        "1000000000"
    );
    assert_eq!(
        second["data"][0]["apitoken"]["pricing"]["standard"]["input"],
        "2000000000"
    );
    let all_logs = [log_a, log_o, log_g];
    assert_eq!(
        all_logs
            .iter()
            .flat_map(|log| log.lock().unwrap().clone())
            .filter(|request| request.method == "GET")
            .count(),
        3,
        "the second key must reuse only the shared capability catalog"
    );
    let pricing_requests: Vec<_> = all_logs
        .iter()
        .flat_map(|log| log.lock().unwrap().clone())
        .filter(|request| request.path == "/internal/router/catalog/pricing")
        .collect();
    assert_eq!(pricing_requests.len(), 2);
    assert_eq!(pricing_requests[0].x_api_key.as_deref(), Some("key-a"));
    assert_eq!(pricing_requests[1].x_api_key.as_deref(), Some("key-b"));
}

#[tokio::test]
async fn catalog_pricing_is_fail_closed_and_authority_401_is_terminal() {
    let (a, _) = catalog_plane(ANTHROPIC_MODELS, "/v1/models", "pricing-fail").await;
    let (o, _) = catalog_plane(OPENAI_MODELS, "/v1/models", "pricing-fail").await;
    let (g, _) = catalog_plane(GEMINI_MODELS, "/v1beta/models", "pricing-fail").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .header("x-api-key", "key-a")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "private, no-store"
    );
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "pricing_unavailable"
    );

    let (a, log_a) = catalog_plane(ANTHROPIC_MODELS, "/v1/models", "pricing-auth").await;
    let (o, log_o) = catalog_plane(OPENAI_MODELS, "/v1/models", "ok").await;
    let (g, log_g) = catalog_plane(GEMINI_MODELS, "/v1beta/models", "ok").await;
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .header("x-api-key", "invalid")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "invalid_api_key"
    );
    assert_eq!(
        log_a
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.path == "/internal/router/catalog/pricing")
            .count(),
        1
    );
    for log in [log_o, log_g] {
        assert!(log
            .lock()
            .unwrap()
            .iter()
            .all(|request| request.path != "/internal/router/catalog/pricing"));
    }
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
    let response = client
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert!(response.headers().get(catalog::DEGRADED_HEADER).is_none());
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["data"]
            .as_array()
            .unwrap()
            .len(),
        6
    );

    // Плоскость упала: тот же каталог из last-good кэша + маркер деградации.
    flag.store(false, Ordering::SeqCst);
    let response = client
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(catalog::DEGRADED_HEADER).unwrap(),
        "google"
    );
    let json: serde_json::Value = response.json().await.unwrap();
    let ids: Vec<&str> = json["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&"google/gemini-2.5-pro"));
    assert!(log.lock().unwrap().len() >= 2);
}

#[tokio::test]
async fn catalog_is_503_when_no_plane_has_ever_answered() {
    let (d1, d2, d3) = (
        dead_origin().await,
        dead_origin().await,
        dead_origin().await,
    );
    let router = spawn(make_router(&d1, &d2, &d3, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["code"], "catalog_unavailable");
}

#[tokio::test]
async fn catalog_auth_rejection_becomes_unified_401() {
    let (a, _) = catalog_plane("", "/v1/models", "auth").await;
    let (o, g) = (dead_origin().await, dead_origin().await);
    let router = spawn(make_router(&a, &o, &g, Duration::ZERO)).await;
    let response = reqwest::Client::new()
        .get(format!("{router}/v1/models"))
        .send()
        .await
        .unwrap();
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
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(json["id"], "anthropic/claude-opus-4-8");
    assert_eq!(
        json["reasoning_efforts"],
        serde_json::json!(["low", "medium", "high", "xhigh", "max"])
    );

    let json: serde_json::Value = client
        .get(format!("{router}/v1/models/claude-opus-4-8"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(json["id"], "anthropic/claude-opus-4-8");

    let json: serde_json::Value = client
        .get(format!("{router}/v1/models/gpt-5.6"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(json["id"], "openai/gpt-5.6");
    assert_eq!(
        json["service_tiers"],
        serde_json::json!(["standard", "priority"])
    );

    let response = client
        .get(format!("{router}/v1/models/cohere/command-x"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "model_not_found"
    );

    let response = client
        .get(format!("{router}/v1/models/gpt-9"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ---------- router-local поверхности и ошибки ----------

#[tokio::test]
async fn health_and_ready_are_router_local_even_with_dead_planes() {
    let (d1, d2, d3) = (
        dead_origin().await,
        dead_origin().await,
        dead_origin().await,
    );
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

    let response = client
        .get(format!("{router}/v1/completions"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let json: serde_json::Value = response.json().await.unwrap();
    assert_eq!(json["error"]["code"], "unsupported_endpoint");

    let response = client
        .put(format!("{router}/v1/messages"))
        .body("{}")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["type"],
        "not_found_error"
    );

    let response = client
        .get(format!("{router}/v1/chat/completions"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(
        response.json::<serde_json::Value>().await.unwrap()["error"]["code"],
        "method_not_allowed"
    );
}
