//! Байт-в-байт прокси native lanes в stable origins плоскостей.
//!
//! Инварианты (docs/engine/UNIFIED_ROUTER.md):
//! - никакой трансляции: метод, путь+query, заголовки (кроме hop-by-hop и router-owned capabilities) и тело
//!   идут в плоскость без изменений; ответ (включая SSE и native errors)
//!   возвращается без буферизации;
//! - native lanes не ретраятся; universal fallback может повторить запрос
//!   только после точного `not_started` или доказанного ConnectionRefused
//!   (docs/engine/ROUTING_FENCING.md §3.3);
//! - disconnect клиента транзитивно рвёт соединение к плоскости: hyper роняет
//!   тело ответа → роняется reqwest-стрим → соединение router→плоскость
//!   закрывается → TeeMeter плоскости дренирует до authoritative usage
//!   (инвариант 4). Поэтому здесь нет detached-тасков вокруг тела ответа.
//! - `x-apitoken-execution-state` (авторитетная семантика исполнения,
//!   docs/engine/ROUTING_FENCING.md §3) — контракт между движком и его клиентом;
//!   с каждого публичного ответа заголовок снимается; universal fallback
//!   проверяет его до снятия, но клиент внутренний сигнал не видит.

use std::error::Error as StdError;
use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, Request, Response, StatusCode};
use reqwest::Client;

use crate::error::{self, Lane};
use crate::metrics::RouterMetrics;

const BALANCE_HEADER_TIMEOUT: Duration = Duration::from_secs(2);

// Hop-by-hop заголовки (RFC 9110 §7.6.1) плюс `host` (reqwest выставляет по
// URL апстрима). `content-length` не трогаем: reqwest сам заменит его на
// chunked для потокового тела.
const HOP_BY_HOP: [HeaderName; 8] = [
    HeaderName::from_static("connection"),
    HeaderName::from_static("keep-alive"),
    HeaderName::from_static("proxy-authenticate"),
    HeaderName::from_static("proxy-authorization"),
    HeaderName::from_static("te"),
    HeaderName::from_static("trailer"),
    HeaderName::from_static("transfer-encoding"),
    HeaderName::from_static("upgrade"),
];

/// Заголовки с credentials, которые каталог пересылает в плоскости verbatim
/// (auth passthrough, инвариант 1: биллинг и авторизация остаются в плоскости).
pub const AUTH_HEADERS: [HeaderName; 3] = [
    HeaderName::from_static("x-api-key"),
    HeaderName::from_static("x-goog-api-key"),
    HeaderName::from_static("authorization"),
];

/// Public compatibility escape hatch for harnesses that can add headers but cannot add arbitrary
/// JSON body properties. `routing.rs` consumes it on executable GPT universal requests; every
/// proxy path removes it before contacting a provider plane.
pub const SERVICE_TIER_HEADER: HeaderName = HeaderName::from_static("x-apitoken-service-tier");

/// Авторитетная семантика исполнения (docs/engine/ROUTING_FENCING.md §3): выставляется
/// движком только на отказах без исполнения (`not_started`). С транзитных ответов router
/// заголовок снимает — за его условия отвечает только сам движок.
const EXECUTION_STATE_HEADER: HeaderName = HeaderName::from_static("x-apitoken-execution-state");
const EXECUTION_STATE_NOT_STARTED: &[u8] = b"not_started";
pub const EXECUTION_GROUP_HEADER: HeaderName =
    HeaderName::from_static("x-apitoken-execution-group");
pub const EXECUTION_ATTEMPT_HEADER: HeaderName = HeaderName::from_static("x-apitoken-attempt");

pub struct ExecutionAttemptHeaders {
    pub group_id: String,
    pub attempt: usize,
}

/// Единственные два доказательства, разрешающие следующий universal attempt.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RetryReason {
    NotStarted,
    ConnectionRefused,
}

impl RetryReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotStarted => "not_started",
            Self::ConnectionRefused => "connect_refused",
        }
    }
}

/// Результат одной попытки до передачи публичных байтов клиенту. Ответ уже
/// очищен от внутреннего execution-state заголовка.
pub struct ProxyAttempt {
    pub response: Response<Body>,
    pub retry_reason: Option<RetryReason>,
}

/// Копия заголовков запроса без hop-by-hop. Токены, перечисленные в значении
/// `connection`, тоже вырезаются.
pub fn strip_hop_by_hop(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(headers.len());
    let mut connection_tokens: Vec<HeaderName> = Vec::new();
    if let Some(value) = headers.get("connection").and_then(|v| v.to_str().ok()) {
        connection_tokens = value
            .split(',')
            .filter_map(|t| HeaderName::try_from(t.trim()).ok())
            .collect();
    }
    for (name, value) in headers.iter() {
        if name == "host" || HOP_BY_HOP.contains(name) || connection_tokens.contains(name) {
            continue;
        }
        out.append(name.clone(), value.clone());
    }
    out
}

/// Проксирование одного запроса в `origin` (stable balancer плоскости).
/// Никакого router-owned таймаута ни на response headers, ни на response body: non-stream
/// generation legitimately returns headers only after the provider finishes, while SSE streams
/// can live for minutes. Connect is bounded by the shared client; after that the caller's
/// disconnect and the provider plane own the lifetime.
pub async fn proxy_attempt(
    client: &Client,
    origin: &str,
    target_lane: Lane,
    error_lane: Lane,
    req: Request<Body>,
    execution: Option<&ExecutionAttemptHeaders>,
    metrics: &RouterMetrics,
) -> ProxyAttempt {
    proxy_attempt_with_optional_header_timeout(
        client,
        origin,
        target_lane,
        error_lane,
        req,
        execution,
        None,
        metrics,
    )
    .await
}

async fn proxy_attempt_with_optional_header_timeout(
    client: &Client,
    origin: &str,
    target_lane: Lane,
    error_lane: Lane,
    req: Request<Body>,
    execution: Option<&ExecutionAttemptHeaders>,
    response_header_timeout: Option<Duration>,
    metrics: &RouterMetrics,
) -> ProxyAttempt {
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{origin}{path_query}");
    let method = req.method().clone();
    let mut headers = strip_hop_by_hop(req.headers());
    // These headers are a router-owned capability. Client copies are always erased, including on
    // native and universal single-attempt lanes; only the explicit fallback engine can add them.
    headers.remove(&EXECUTION_GROUP_HEADER);
    headers.remove(&EXECUTION_ATTEMPT_HEADER);
    headers.remove(&SERVICE_TIER_HEADER);
    if let Some(execution) = execution {
        headers.insert(
            EXECUTION_GROUP_HEADER,
            execution
                .group_id
                .parse()
                .expect("router-generated UUIDv4 is a valid header value"),
        );
        headers.insert(
            EXECUTION_ATTEMPT_HEADER,
            execution
                .attempt
                .to_string()
                .parse()
                .expect("positive attempt is a valid header value"),
        );
    }
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());

    let send = client
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send();
    let upstream = match response_header_timeout {
        Some(limit) => match tokio::time::timeout(limit, send).await {
            Ok(result) => result,
            Err(_) => {
                metrics.response_header_timeout(target_lane);
                eprintln!(
                    "router: {target_lane:?} lane upstream transport failed class=response_header_timeout"
                );
                return ProxyAttempt {
                    response: error::upstream_unavailable(
                        error_lane,
                        "The provider plane did not return response headers in time.",
                    ),
                    retry_reason: None,
                };
            }
        },
        None => send.await,
    };

    match upstream {
        Ok(res) => {
            let status = res.status();
            let not_started = {
                let mut values = res.headers().get_all(&EXECUTION_STATE_HEADER).iter();
                matches!(
                    (values.next(), values.next()),
                    (Some(value), None) if value.as_bytes() == EXECUTION_STATE_NOT_STARTED
                )
            };
            // 429 — capacity отказ, а не исправимая клиентом ошибка. Остальные
            // 4xx (особенно 401/402) никогда не должны обходиться другой моделью.
            let retry_reason = (!status.is_success()
                && not_started
                && (!status.is_client_error() || status == StatusCode::TOO_MANY_REQUESTS))
                .then_some(RetryReason::NotStarted);
            let mut builder = Response::builder().status(res.status());
            let filtered = strip_hop_by_hop(res.headers());
            for (name, value) in filtered.iter() {
                // Авторитетная семантика исполнения — не транзитный контракт: снимаем
                // (см. EXECUTION_STATE_HEADER).
                if name == EXECUTION_STATE_HEADER {
                    continue;
                }
                builder = builder.header(name, value);
            }
            // Тело отдаётся стримом по мере поступления: ни полного чтения, ни
            // копии в память. Ошибка апстрима посреди тела обрывает стрим —
            // клиент видит усечённый ответ и обрыв соединения, как и при прямом
            // соединении с плоскостью.
            let response = match builder.body(Body::from_stream(res.bytes_stream())) {
                Ok(response) => response,
                Err(_) => {
                    return ProxyAttempt {
                        response: error::upstream_unavailable(
                            error_lane,
                            "failed to build upstream response",
                        ),
                        retry_reason: None,
                    }
                }
            };
            ProxyAttempt {
                response,
                retry_reason,
            }
        }
        Err(e) => {
            // Сюда попадают только отказы ДО получения заголовков ответа
            // (connect refused, connect timeout, сброс до ответа): запрос мог
            // не дойти до плоскости. Только exact io::ErrorKind::ConnectionRefused
            // доказывает, что TCP-соединение не установилось; is_connect() также
            // включает timeout/DNS/ambiguous failures и потому недостаточен.
            let retry_reason =
                error_chain_has_connection_refused(&e).then_some(RetryReason::ConnectionRefused);
            let class = retry_reason.map_or("ambiguous", RetryReason::as_str);
            // Display reqwest::Error содержит URL (и потенциальный secret query),
            // поэтому логируем только bounded classification.
            eprintln!("router: {target_lane:?} lane upstream transport failed class={class}");
            ProxyAttempt {
                response: error::upstream_unavailable(
                    error_lane,
                    "The provider plane is temporarily unavailable.",
                ),
                retry_reason,
            }
        }
    }
}

/// Проксирование native lane: ровно одна попытка независимо от transport/
/// execution-state результата.
pub async fn proxy_request(
    client: &Client,
    origin: &str,
    lane: Lane,
    req: Request<Body>,
    metrics: &RouterMetrics,
) -> Response<Body> {
    proxy_attempt(client, origin, lane, lane, req, None, metrics)
        .await
        .response
}

/// `/balance` is a read-only shared-authority surface. It is safe to continue after transport or
/// 5xx because no execution or reservation can start; 401 and every non-5xx response are terminal.
/// The first successful provider response keeps its body and end-to-end headers unchanged.
pub async fn proxy_balance(
    client: &Client,
    origins: [(&str, Lane); 3],
    req: Request<Body>,
    metrics: &RouterMetrics,
) -> Response<Body> {
    let (mut parts, _body) = req.into_parts();
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    let mut last = None;
    for (index, (origin, lane)) in origins.into_iter().enumerate() {
        let mut request = Request::builder()
            .method(parts.method.clone())
            .uri(parts.uri.clone())
            .version(parts.version)
            .body(Body::empty())
            .expect("validated balance request parts");
        *request.headers_mut() = parts.headers.clone();
        let attempt = proxy_attempt_with_optional_header_timeout(
            client,
            origin,
            lane,
            Lane::Anthropic,
            request,
            None,
            Some(BALANCE_HEADER_TIMEOUT),
            metrics,
        )
        .await;
        let status = attempt.response.status();
        if status == StatusCode::UNAUTHORIZED || !status.is_server_error() {
            return attempt.response;
        }
        last = Some(attempt.response);
        if let Some((_, next_lane)) = origins.get(index + 1) {
            metrics.balance_failover(lane, *next_lane);
        }
    }
    last.unwrap_or_else(|| {
        error::upstream_unavailable(
            Lane::Anthropic,
            "The balance authority is temporarily unavailable.",
        )
    })
}

fn error_chain_has_connection_refused(error: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(source) = current {
        if source
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::ConnectionRefused)
        {
            return true;
        }
        current = source.source();
    }
    false
}

/// Выбор заголовков авторизации из входящего запроса для фонового запроса
/// каталога. Передаются verbatim, без добавления и удаления.
pub fn auth_passthrough(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(headers.len().min(AUTH_HEADERS.len() * 2));
    for name in AUTH_HEADERS {
        for value in headers.get_all(&name) {
            out.append(&name, value.clone());
        }
    }
    out
}

/// Ответ роутов /health, /live — router-local liveness без авторизации
/// (инвариант 3: никогда не конъюнкция health плоскостей).
pub async fn health() -> (StatusCode, axum::Json<serde_json::Value>) {
    (StatusCode::OK, axum::Json(serde_json::json!({"ok": true})))
}

/// Ответ роута /ready. Процесс stateless и не имеет зависимостей readiness:
/// слушающий сокет — уже готовность. Деградация плоскостей видна в каталоге
/// (заголовок x-apitoken-catalog-degraded), а не в этом ответе.
pub async fn ready() -> (StatusCode, axum::Json<serde_json::Value>) {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({"ready": true})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn strip_removes_hop_by_hop_host_and_connection_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("router.apitoken.sale"));
        headers.insert("connection", HeaderValue::from_static("keep-alive, x-bye"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("x-bye", HeaderValue::from_static("1"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("x-api-key", HeaderValue::from_static("sk-pool-secret"));
        headers.insert(
            "anthropic-beta",
            HeaderValue::from_static("messages-2023-12-15"),
        );
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        let stripped = strip_hop_by_hop(&headers);
        assert!(stripped.get("host").is_none());
        assert!(stripped.get("connection").is_none());
        assert!(stripped.get("keep-alive").is_none());
        assert!(stripped.get("x-bye").is_none());
        assert!(stripped.get("transfer-encoding").is_none());
        assert_eq!(stripped.get("x-api-key").unwrap(), "sk-pool-secret");
        assert_eq!(
            stripped.get("anthropic-beta").unwrap(),
            "messages-2023-12-15"
        );
        assert_eq!(stripped.get("anthropic-version").unwrap(), "2023-06-01");
    }

    #[test]
    fn native_proxy_preserves_affinity_and_exact_gemini_evidence_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-session-id", HeaderValue::from_static("sticky-client-session"));
        headers.insert(
            "x-apitoken-calibration-profile",
            HeaderValue::from_static("gemini_oauth_000001"),
        );
        headers.insert(
            "x-apitoken-calibration-request-id",
            HeaderValue::from_static("123e4567-e89b-42d3-a456-426614174000"),
        );

        let stripped = strip_hop_by_hop(&headers);
        assert_eq!(stripped.get("x-session-id").unwrap(), "sticky-client-session");
        assert_eq!(
            stripped.get("x-apitoken-calibration-profile").unwrap(),
            "gemini_oauth_000001"
        );
        assert_eq!(
            stripped
                .get("x-apitoken-calibration-request-id")
                .unwrap(),
            "123e4567-e89b-42d3-a456-426614174000"
        );
    }

    #[test]
    fn auth_passthrough_keeps_only_credential_headers() {
        let mut headers = HeaderMap::new();
        headers.append("x-api-key", HeaderValue::from_static("sk-pool-a"));
        headers.append("x-api-key", HeaderValue::from_static("sk-pool-a-rotating"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("sk-pool-b"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer sk-pool-c"),
        );
        headers.insert("user-agent", HeaderValue::from_static("test"));
        headers.insert("anthropic-beta", HeaderValue::from_static("b"));

        let auth = auth_passthrough(&headers);
        assert_eq!(auth.len(), 4);
        let x_api_keys: Vec<_> = auth
            .get_all("x-api-key")
            .iter()
            .map(|value| value.to_str().unwrap())
            .collect();
        assert_eq!(x_api_keys, ["sk-pool-a", "sk-pool-a-rotating"]);
        assert_eq!(auth.get("x-goog-api-key").unwrap(), "sk-pool-b");
        assert_eq!(auth.get("authorization").unwrap(), "Bearer sk-pool-c");
        assert!(auth.get("user-agent").is_none());
    }

    #[test]
    fn retry_transport_classifier_is_exact() {
        let refused = std::io::Error::from(std::io::ErrorKind::ConnectionRefused);
        assert!(error_chain_has_connection_refused(&refused));

        for kind in [
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::ConnectionReset,
            std::io::ErrorKind::NotConnected,
            std::io::ErrorKind::NetworkUnreachable,
        ] {
            let error = std::io::Error::from(kind);
            assert!(!error_chain_has_connection_refused(&error), "{kind:?}");
        }
    }

    #[tokio::test]
    async fn bounded_balance_header_deadline_is_terminal_and_never_enables_retry() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = stream.read(&mut request).await.unwrap();
            std::future::pending::<()>().await;
        });
        let request = Request::builder()
            .method("POST")
            .uri("/v1/messages")
            .body(Body::from("{}"))
            .unwrap();
        let result = proxy_attempt_with_optional_header_timeout(
            &Client::new(),
            &format!("http://{address}"),
            Lane::Anthropic,
            Lane::Anthropic,
            request,
            None,
            Some(Duration::from_millis(50)),
            &RouterMetrics::new(),
        )
        .await;
        server.abort();

        assert_eq!(result.response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(result.retry_reason, None);
    }

    #[tokio::test]
    async fn every_data_plane_waits_for_legitimately_delayed_non_stream_headers() {
        for (lane, path) in [
            (Lane::Anthropic, "/v1/messages"),
            (Lane::OpenAi, "/v1/chat/completions"),
            (Lane::Gemini, "/v1beta/models/gemini-test:generateContent"),
        ] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = [0_u8; 1024];
                let _ = stream.read(&mut request).await.unwrap();
                tokio::time::sleep(Duration::from_millis(75)).await;
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")
                    .await
                    .unwrap();
            });
            let request = Request::builder()
                .method("POST")
                .uri(path)
                .body(Body::from("{}"))
                .unwrap();
            let result = tokio::time::timeout(
                Duration::from_millis(250),
                proxy_attempt(
                    &Client::new(),
                    &format!("http://{address}"),
                    lane,
                    lane,
                    request,
                    None,
                    &RouterMetrics::new(),
                ),
            )
            .await
            .expect("data-plane proxy must outlive the old synthetic header deadline");
            server.await.unwrap();

            assert_eq!(result.response.status(), StatusCode::OK, "{lane:?}");
            assert_eq!(result.retry_reason, None, "{lane:?}");
        }
    }

    #[test]
    fn lane_from_path_covers_public_contract() {
        assert_eq!(Lane::from_path("/v1/messages"), Some(Lane::Anthropic));
        assert_eq!(
            Lane::from_path("/v1/messages/count_tokens"),
            Some(Lane::Anthropic)
        );
        assert_eq!(Lane::from_path("/balance"), Some(Lane::Anthropic));
        assert_eq!(Lane::from_path("/v1/responses"), Some(Lane::OpenAi));
        assert_eq!(
            Lane::from_path("/v1/responses/resp_1/input_items"),
            Some(Lane::OpenAi)
        );
        assert_eq!(Lane::from_path("/v1/chat/completions"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1/models"), Some(Lane::OpenAi));
        assert_eq!(
            Lane::from_path("/v1/models/anthropic/claude-x"),
            Some(Lane::OpenAi)
        );
        assert_eq!(Lane::from_path("/v1beta/models"), Some(Lane::Gemini));
        assert_eq!(
            Lane::from_path("/v1beta/models/gemini-2.5-pro:generateContent"),
            Some(Lane::Gemini)
        );
        assert_eq!(Lane::from_path("/health"), None);
        assert_eq!(Lane::from_path("/unknown"), None);
    }
}
