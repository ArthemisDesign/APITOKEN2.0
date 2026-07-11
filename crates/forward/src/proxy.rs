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

use crate::meter::{BillCtx, MeterCtx, TeeMeter};
use crate::state::AppState;
use crate::upstream::{limits_from_headers, Limits};
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

/// Клиентский ключ из запроса (x-api-key либо Bearer). Публично — используется и в `server`
/// для эндпоинта `/balance`.
pub fn client_key(headers: &HeaderMap) -> Option<String> {
    if let Some(v) = headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        return Some(v.trim().to_string());
    }
    let a = headers.get("authorization").and_then(|v| v.to_str().ok())?;
    a.strip_prefix("Bearer ").or_else(|| a.strip_prefix("bearer "))
        .map(|s| s.trim().to_string())
}

/// Админ-доступ (env-ключи `CLAUDE_API_KEYS` или localhost, если ключи не заданы). Без биллинга.
pub fn authed(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> bool {
    if app.cfg.api_keys.is_empty() {
        return peer.ip().is_loopback();
    }
    match client_key(headers) {
        Some(k) => app.cfg.api_keys.iter().any(|x| x == &k),
        None => false,
    }
}

/// Результат авторизации запроса.
enum Authz {
    /// Админ (env-ключ/localhost) — без тарификации.
    Admin,
    /// Ключ клиента с балансом — тарифицируем ответ и списываем.
    Metered { key: String, mult_bp: i64 },
    /// Ключ есть, но баланс ≤ 0.
    PaymentRequired,
    /// Ключ неизвестен/заблокирован.
    Unauthorized,
}

/// Приоритет: сначала биллинг-ключ (из таблицы api_keys), иначе — админ (env/localhost).
fn authorize(app: &AppState, headers: &HeaderMap, peer: &SocketAddr) -> Authz {
    if let (Some(billing), Some(k)) = (&app.billing, client_key(headers)) {
        if let Some(row) = billing.get(&k) {
            if row.status != "active" {
                return Authz::Unauthorized;
            }
            if row.balance_nano <= 0 {
                return Authz::PaymentRequired;
            }
            return Authz::Metered { key: k, mult_bp: row.mult_bp };
        }
    }
    if authed(app, headers, peer) { Authz::Admin } else { Authz::Unauthorized }
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
/// Первый system-блок уже несёт Claude-Code-идентичность? (billing-header/identity — как шлёт
/// САМ Claude Code). Тогда повторно инжектить не надо — иначе получим двойную identity.
fn is_cc_marker(text: &str) -> bool {
    text.starts_with("x-anthropic-billing-header:")
        || text.starts_with("You are Claude Code")
        || text.starts_with("You are a Claude agent")
}

fn inject_identity(v: &mut Value, identity: &str) -> bool {
    let obj = match v.as_object_mut() { Some(o) => o, None => return false };
    if !obj.contains_key("messages") { return false; } // не messages-запрос — не трогаем
    match obj.get("system").cloned() {
        None => { obj.insert("system".into(), serde_json::json!([{"type":"text","text":identity}])); }
        Some(Value::String(s)) => {
            if is_cc_marker(&s) { return false; }       // клиент прислал identity строкой — не дублируем
            obj.insert("system".into(),
                serde_json::json!([{"type":"text","text":identity},{"type":"text","text":s}]));
        }
        Some(Value::Array(mut arr)) => {
            let first_cc = arr.first()
                .and_then(|b| b.get("text")).and_then(|t| t.as_str())
                .map(is_cc_marker).unwrap_or(false);
            if first_cc { return false; }               // уже Claude-Code-запрос (напр. сам Claude Code)
            arr.insert(0, serde_json::json!({"type":"text","text":identity}));
            obj.insert("system".into(), Value::Array(arr));
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
    let authz = authorize(&app, &parts.headers, &peer);
    match authz {
        Authz::Unauthorized =>
            return err_response(StatusCode::UNAUTHORIZED, "authentication_error", "invalid api key"),
        Authz::PaymentRequired =>
            return err_response(StatusCode::PAYMENT_REQUIRED, "invalid_request_error",
                                "insufficient balance — top up your key"),
        Authz::Admin | Authz::Metered { .. } => {}
    }
    let method: Method = parts.method.clone();
    let pq = parts.uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
    let url = format!("{}{}", app.cfg.upstream.trim_end_matches('/'), pq);

    let raw = match axum::body::to_bytes(body, BODY_LIMIT).await {
        Ok(b) => b,
        Err(_) => return err_response(StatusCode::BAD_REQUEST, "invalid_request_error", "body read error"),
    };

    // тело: один парс — вытаскиваем модель (для тарификации) и инжектим identity
    // (иначе токен подписки не пустят на /v1/messages).
    let mut body_bytes = raw.to_vec();
    let mut model = String::new();
    if let Ok(mut v) = serde_json::from_slice::<Value>(&raw) {
        model = v.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        if app.cfg.inject_identity && inject_identity(&mut v, &app.cfg.identity) {
            if let Ok(b) = serde_json::to_vec(&v) { body_bytes = b; }
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
            Err(e) => {
                app.pool.mark_cooling(&sub.email, 10); // битый прокси → cooling + −1 in-flight
                last = err_response(StatusCode::BAD_GATEWAY, "api_error", &format!("proxy: {e}"));
                continue;
            }
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

        // ПАССИВНЫЙ сбор лимитов из боевого ответа: свежий util/reset без лишних запросов
        // (обновляет polled_ts → активный поллер сам перестаёт трогать «живые» подписки).
        let lim = limits_from_headers(resp.headers());
        if lim.has_util() {
            app.pool.set_util(&sub.email, lim.util5h, lim.util7d, lim.status.clone(),
                              lim.reset5h, lim.reset7d);
        }

        let rotate = code == 429 || code == 401 || code == 403 || code == 408
            || code == 409 || code == 425 || st.is_server_error();
        if rotate {
            let secs = if code == 429 {
                cool_secs_429(&resp, &lim, pool::now(), app.cfg.cool_secs)
            } else { 10 };
            app.pool.mark_cooling(&sub.email, secs);
            eprintln!("↻ ротация: {} вернул {} — cooling {}s", sub.email, code, secs);
            last = err_response(st, "overloaded_error", &format!("upstream {code} via {}", sub.email));
            continue;
        }

        // успех или клиентская ошибка запроса (одинакова на любой подписке) → отдаём как есть
        app.pool.mark_ok(&sub.email);
        // на УСПЕХЕ всегда меряем ответ: расход подписки → калибровка пула; для метерного
        // ключа дополнительно списываем с баланса. 4xx/ошибки не меряем (реального расхода нет).
        let meter = if st.is_success() {
            let bill = match &authz {
                Authz::Metered { key, mult_bp } => app.billing.clone().map(|billing| BillCtx {
                    billing, key: key.clone(), mult_bp: *mult_bp,
                }),
                _ => None,
            };
            Some(MeterCtx {
                pool: app.pool.clone(),
                email: sub.email.clone(),
                model: model.clone(),
                is_sse: is_event_stream(&resp),
                bill,
            })
        } else {
            None
        };
        return stream_back(st, resp, meter);
    }
    last
}

/// Ответ — SSE-стрим? (по content-type). Определяет способ парсинга usage.
fn is_event_stream(resp: &reqwest::Response) -> bool {
    resp.headers().get("content-type")
        .and_then(|v| v.to_str().ok())
        .map(|c| c.contains("text/event-stream"))
        .unwrap_or(false)
}

/// Явный заголовок `Retry-After` (самый авторитетный хинт — Anthropic сам говорит, когда можно).
fn retry_after_header(resp: &reqwest::Response) -> Option<i64> {
    let v = resp.headers().get("retry-after")?.to_str().ok()?;
    v.trim().parse::<i64>().ok().map(|s| s.max(1))
}

/// Секунды до сброса ОКНА-виновника: если недельное окно почти выбрано (util7d≥0.98) — студим
/// до reset7d (это могут быть дни, и это ПРАВИЛЬНО — иначе ретраим впустую), иначе до reset5h.
fn window_cool(lim: &Limits, now: i64) -> Option<i64> {
    let fut = |t: Option<i64>| t.filter(|x| *x > now).map(|x| (x - now).max(1));
    let (r5, r7) = (fut(lim.reset5h), fut(lim.reset7d));
    if lim.util7d.unwrap_or(0.0) >= 0.98 { r7.or(r5) } else { r5.or(r7) }
}

/// Сколько студить подписку при 429: Retry-After → окно-виновник → дефолт.
fn cool_secs_429(resp: &reqwest::Response, lim: &Limits, now: i64, default: i64) -> i64 {
    retry_after_header(resp).or_else(|| window_cool(lim, now)).unwrap_or(default)
}

/// Отдать ответ апстрима клиенту байт-в-байт (стримом — работает и для SSE).
/// Если задан `meter` — оборачиваем тело в tee-метеринг: клиент получает те же байты,
/// а на завершении стрима списываем стоимость с ключа (тело клиенту НЕ задерживается).
fn stream_back(st: StatusCode, resp: reqwest::Response, meter: Option<MeterCtx>) -> Response {
    let mut builder = Response::builder().status(st);
    for (name, value) in resp.headers().iter() {
        if !skip_resp_header(name.as_str()) {
            builder = builder.header(name.as_str(), value.as_bytes());
        }
    }
    let stream = resp.bytes_stream().map(|chunk| {
        chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
    });
    let body = match meter {
        Some(ctx) => Body::from_stream(TeeMeter::new(Box::pin(stream), ctx)),
        None => Body::from_stream(stream),
    };
    builder.body(body).unwrap_or_else(|_| {
        err_response(StatusCode::INTERNAL_SERVER_ERROR, "api_error", "response build error")
    })
}
