//! Прозрачный форвардинг (Шаг B): для клиента — обычный api.anthropic.com.
//!
//! Клиент шлёт стандартный Anthropic-запрос (напр. Anthropic SDK с base_url=наш сервер и
//! api_key=наш ключ). Мы:
//!   1) авторизуем клиента по нашему ключу (x-api-key / Bearer);
//!   2) под капотом инжектим Claude Code identity в system + oauth-заголовки (иначе токен
//!      подписки не пускают на /v1/messages) — протокол для клиента при этом НЕ меняется;
//!   3) выбираем наименее загруженную подписку пула, шлём запрос с её Bearer через её прокси;
//!   4) при 429/5xx/протухшем токене — cooling и ротация на следующую подписку;
//!   5) ответ (включая SSE-стрим) отдаём клиенту байт-в-байт.

use crate::state::AppState;
use axum::body::Body;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use futures_util::StreamExt;
use serde_json::Value;
use std::collections::HashSet;
use std::net::SocketAddr;

const BODY_LIMIT: usize = 64 * 1024 * 1024;

// Заголовки клиента, которые НЕ пробрасываем апстриму (перезаписываем или служебные).
fn skip_req_header(name: &str) -> bool {
    matches!(name,
        "host" | "content-length" | "connection" | "authorization" | "x-api-key"
        | "anthropic-beta" | "anthropic-version" | "accept-encoding" | "transfer-encoding"
        | "upgrade" | "proxy-connection" | "proxy-authorization" | "keep-alive" | "te" | "trailer")
}
// Hop-by-hop заголовки апстрима, которые не отдаём клиенту (тело стримим чанками).
fn skip_resp_header(name: &str) -> bool {
    matches!(name,
        "connection" | "transfer-encoding" | "content-length" | "content-encoding"
        | "keep-alive" | "proxy-connection" | "upgrade" | "te" | "trailer")
}

fn client_key(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    let a = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

pub fn authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if app.cfg.api_keys.is_empty() {
        return peer.ip().is_loopback();
    }
    match client_key(headers) {
        Some(k) => app.cfg.api_keys.iter().any(|x| x == &k),
        None => false,
    }
}

/// Anthropic-подобная ошибка (чтобы SDK-клиент видел привычную форму).
fn err_response(code: StatusCode, kind: &str, msg: &str) -> Response {
    let body = serde_json::json!({"type": "error", "error": {"type": kind, "message": msg}});
    Response::builder()
        .status(code)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

/// Инжект Claude Code identity первым system-блоком (если его там ещё нет).
fn inject_identity(v: &mut Value, identity: &str) -> bool {
    let obj = match v.as_object_mut() { Some(o) => o, None => return false };
    if !obj.contains_key("messages") { return false; } // не messages-запрос — не трогаем
    match obj.get("system").cloned() {
        None => { obj.insert("system".into(), serde_json::json!([{"type":"text","text":identity}])); }
        Some(Value::String(s)) => {
            obj.insert("system".into(),
                serde_json::json!([{"type":"text","text":identity},{"type":"text","text":s}]));
        }
        Some(Value::Array(mut arr)) => {
            let first_ok = arr.first()
                .and_then(|b| b.get("text")).and_then(|t| t.as_str()) == Some(identity);
            if !first_ok {
                arr.insert(0, serde_json::json!({"type":"text","text":identity}));
                obj.insert("system".into(), Value::Array(arr));
            } else { return false; }
        }
        _ => return false,
    }
    true
}

/// Слить anthropic-beta клиента с нашим (гарантируем присутствие oauth-беты).
fn merge_beta(client_beta: Option<&str>, default_beta: &str) -> String {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for part in default_beta.split(',').chain(client_beta.unwrap_or("").split(',')) {
        let p = part.trim();
        if !p.is_empty() && seen.insert(p.to_string()) { out.push(p.to_string()); }
    }
    out.join(", ")
}

pub async fn forward(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: axum::extract::Request,
) -> Response {
    let (parts, body) = req.into_parts();
    if !authed(&app, &parts.headers, &peer) {
        return err_response(StatusCode::UNAUTHORIZED, "authentication_error", "invalid api key");
    }
    let method: Method = parts.method.clone();
    let pq = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let url = format!("{}{}", app.cfg.upstream.trim_end_matches('/'), pq);

    let raw = match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => return err_response(StatusCode::BAD_REQUEST, "invalid_request_error", "body read error"),
    };

    // тело: инжектим identity в JSON messages-запрос (иначе токен подписки не пустят)
    let mut body_bytes = raw.to_vec();
    if app.cfg.inject_identity {
        if let Ok(mut v) = serde_json::from_slice::<Value>(&raw) {
            if inject_identity(&mut v, &app.cfg.identity) {
                if let Ok(b) = serde_json::to_vec(&v) { body_bytes = b; }
            }
        }
    }

    let version = parts.headers.get("anthropic-version")
        .and_then(|v| v.to_str().ok()).unwrap_or(&app.cfg.anthropic_version).to_string();
    let beta = merge_beta(
        parts.headers.get("anthropic-beta").and_then(|v| v.to_str().ok()),
        &app.cfg.default_beta);

    let mut tried: HashSet<String> = HashSet::new();
    let mut last = err_response(StatusCode::SERVICE_UNAVAILABLE, "overloaded_error",
                               "no subscriptions available in pool");

    for _ in 0..app.cfg.max_tries.max(1) {
        let sub = match app.pool.pick(&tried, false) { Some(s) => s, None => break };
        tried.insert(sub.email.clone());
        app.pool.mark_used(&sub.email);

        let client = match app.clients.get(&sub.proxy) {
            Ok(c) => c,
            Err(e) => { last = err_response(StatusCode::BAD_GATEWAY, "api_error", &format!("proxy: {e}")); continue; }
        };

        let mut rb = client.request(method.clone(), &url)
            .header("authorization", format!("Bearer {}", sub.token))
            .header("anthropic-version", &version)
            .header("anthropic-beta", &beta);
        for (name, value) in parts.headers.iter() {
            let n = name.as_str();
            if !skip_req_header(n) { rb = rb.header(n, value.as_bytes()); }
        }
        rb = rb.body(body_bytes.clone());

        let resp = match rb.send().await {
            Ok(r) => r,
            Err(e) => {
                app.pool.mark_cooling(&sub.email, 15);
                last = err_response(StatusCode::BAD_GATEWAY, "api_error",
                                    &format!("upstream via {}: {e}", sub.email));
                continue;
            }
        };

        let st = resp.status();
        let code = st.as_u16();
        let rotate = code == 429 || code == 401 || code == 403 || code == 408
            || code == 409 || code == 425 || st.is_server_error();
        if rotate {
            let secs = retry_after(&resp).unwrap_or(app.cfg.cool_secs);
            app.pool.mark_cooling(&sub.email, if code == 429 { secs } else { 10 });
            eprintln!("↻ ротация: {} вернул {} — cooling {}s", sub.email, code, secs);
            last = err_response(st, "overloaded_error", &format!("upstream {code} via {}", sub.email));
            continue;
        }

        // успех или клиентская ошибка запроса (одинакова на любой подписке) → отдаём как есть
        app.pool.mark_ok(&sub.email);
        return stream_back(st, resp);
    }
    last
}

fn retry_after(resp: &reqwest::Response) -> Option<i64> {
    let h = resp.headers();
    if let Some(v) = h.get("retry-after").and_then(|v| v.to_str().ok()) {
        if let Ok(secs) = v.trim().parse::<i64>() { return Some(secs.max(1)); }
    }
    if let Some(v) = h.get("anthropic-ratelimit-unified-5h-reset").and_then(|v| v.to_str().ok()) {
        if let Ok(ts) = v.trim().parse::<f64>() {
            let d = ts as i64 - pool::now();
            if d > 0 { return Some(d); }
        }
    }
    None
}

/// Отдать ответ апстрима клиенту байт-в-байт (стримом — работает и для SSE).
fn stream_back(st: StatusCode, resp: reqwest::Response) -> Response {
    let mut builder = Response::builder().status(st);
    for (name, value) in resp.headers().iter() {
        if !skip_resp_header(name.as_str()) {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
    }
    let stream = resp.bytes_stream().map(|chunk| {
        chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });
    builder.body(Body::from_stream(stream)).unwrap_or_else(|_| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, "api_error", "response build error")
    })
}
