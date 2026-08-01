//! Байт-в-байт прокси native lanes в stable origins плоскостей.
//!
//! Инварианты (docs/engine/UNIFIED_ROUTER.md):
//! - никакой трансляции: метод, путь+query, заголовки (кроме hop-by-hop) и тело
//!   идут в плоскость без изменений; ответ (включая SSE и native errors)
//!   возвращается без буферизации;
//! - никаких ретраев: повтор после отправки запроса создал бы второй billable
//!   request_id (инвариант 2). Отказ соединения честно превращается в 502;
//! - disconnect клиента транзитивно рвёт соединение к плоскости: hyper роняет
//!   тело ответа → роняется reqwest-стрим → соединение router→плоскость
//!   закрывается → TeeMeter плоскости дренирует до authoritative usage
//!   (инвариант 4). Поэтому здесь нет detached-тасков вокруг тела ответа.

use axum::body::Body;
use axum::http::{HeaderMap, HeaderName, Request, Response, StatusCode};
use reqwest::Client;

use crate::error::{self, Lane};

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
/// Никакого таймаута на весь запрос: SSE-стримы живут минуты. Единственный
/// таймаут — connect (2 с) на клиенте; дальше жизнью соединения управляет
/// клиентский disconnect и плоскость.
pub async fn proxy_request(
    client: &Client,
    origin: &str,
    lane: Lane,
    req: Request<Body>,
) -> Response<Body> {
    let path_query = req
        .uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{origin}{path_query}");
    let method = req.method().clone();
    let headers = strip_hop_by_hop(req.headers());
    let body = reqwest::Body::wrap_stream(req.into_body().into_data_stream());

    let upstream = client
        .request(method, &url)
        .headers(headers)
        .body(body)
        .send()
        .await;

    match upstream {
        Ok(res) => {
            let mut builder = Response::builder().status(res.status());
            let filtered = strip_hop_by_hop(res.headers());
            for (name, value) in filtered.iter() {
                builder = builder.header(name, value);
            }
            // Тело отдаётся стримом по мере поступления: ни полного чтения, ни
            // копии в память. Ошибка апстрима посреди тела обрывает стрим —
            // клиент видит усечённый ответ и обрыв соединения, как и при прямом
            // соединении с плоскостью.
            match builder.body(Body::from_stream(res.bytes_stream())) {
                Ok(response) => response,
                Err(_) => error::upstream_unavailable(lane, "failed to build upstream response"),
            }
        }
        Err(e) => {
            // Сюда попадают только отказы ДО получения заголовков ответа
            // (connect refused, connect timeout, сброс до ответа): запрос мог
            // не дойти до плоскости, но мы не знаем этого наверняка — поэтому
            // никакого автоматического повтора, только честная 502.
            eprintln!("router: {lane:?} lane upstream {origin} failed: {e}");
            error::upstream_unavailable(lane, "The provider plane is temporarily unavailable.")
        }
    }
}

/// Выбор заголовков авторизации из входящего запроса для фонового запроса
/// каталога. Передаются verbatim, без добавления и удаления.
pub fn auth_passthrough(headers: &HeaderMap) -> HeaderMap {
    let mut out = HeaderMap::with_capacity(AUTH_HEADERS.len());
    for name in AUTH_HEADERS {
        if let Some(value) = headers.get(&name) {
            out.append(name, value.clone());
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
    (StatusCode::OK, axum::Json(serde_json::json!({"ready": true})))
}


#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn strip_removes_hop_by_hop_host_and_connection_tokens() {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("router.apitoken.sale"));
        headers.insert("connection", HeaderValue::from_static("keep-alive, x-bye"));
        headers.insert("keep-alive", HeaderValue::from_static("timeout=5"));
        headers.insert("x-bye", HeaderValue::from_static("1"));
        headers.insert("transfer-encoding", HeaderValue::from_static("chunked"));
        headers.insert("x-api-key", HeaderValue::from_static("sk-pool-secret"));
        headers.insert("anthropic-beta", HeaderValue::from_static("messages-2023-12-15"));
        headers.insert("anthropic-version", HeaderValue::from_static("2023-06-01"));

        let stripped = strip_hop_by_hop(&headers);
        assert!(stripped.get("host").is_none());
        assert!(stripped.get("connection").is_none());
        assert!(stripped.get("keep-alive").is_none());
        assert!(stripped.get("x-bye").is_none());
        assert!(stripped.get("transfer-encoding").is_none());
        assert_eq!(stripped.get("x-api-key").unwrap(), "sk-pool-secret");
        assert_eq!(stripped.get("anthropic-beta").unwrap(), "messages-2023-12-15");
        assert_eq!(stripped.get("anthropic-version").unwrap(), "2023-06-01");
    }

    #[test]
    fn auth_passthrough_keeps_only_credential_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", HeaderValue::from_static("sk-pool-a"));
        headers.insert("x-goog-api-key", HeaderValue::from_static("sk-pool-b"));
        headers.insert("authorization", HeaderValue::from_static("Bearer sk-pool-c"));
        headers.insert("user-agent", HeaderValue::from_static("test"));
        headers.insert("anthropic-beta", HeaderValue::from_static("b"));

        let auth = auth_passthrough(&headers);
        assert_eq!(auth.len(), 3);
        assert_eq!(auth.get("x-api-key").unwrap(), "sk-pool-a");
        assert_eq!(auth.get("x-goog-api-key").unwrap(), "sk-pool-b");
        assert_eq!(auth.get("authorization").unwrap(), "Bearer sk-pool-c");
        assert!(auth.get("user-agent").is_none());
    }

    #[test]
    fn lane_from_path_covers_public_contract() {
        assert_eq!(Lane::from_path("/v1/messages"), Some(Lane::Anthropic));
        assert_eq!(Lane::from_path("/v1/messages/count_tokens"), Some(Lane::Anthropic));
        assert_eq!(Lane::from_path("/balance"), Some(Lane::Anthropic));
        assert_eq!(Lane::from_path("/v1/responses"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1/responses/resp_1/input_items"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1/chat/completions"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1/models"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1/models/anthropic/claude-x"), Some(Lane::OpenAi));
        assert_eq!(Lane::from_path("/v1beta/models"), Some(Lane::Gemini));
        assert_eq!(
            Lane::from_path("/v1beta/models/gemini-2.5-pro:generateContent"),
            Some(Lane::Gemini)
        );
        assert_eq!(Lane::from_path("/health"), None);
        assert_eq!(Lane::from_path("/unknown"), None);
    }
}
