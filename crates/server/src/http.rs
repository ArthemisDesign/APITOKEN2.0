//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health   — жив ли сервер (без авторизации)
//!   GET /pool     — статус пула (util/cooling, без секретов)
//!   *             — форвардинг на api.anthropic.com (см. forward::forward)

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use forward::{authed, client_key, forward, AppState};
use serde_json::json;
use std::net::SocketAddr;

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/pool", get(pool_status))
        .route("/balance", get(balance))
        .fallback(forward)
        .with_state(app)
}

async fn health(State(app): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "subs": app.pool.len(),
        "upstream": app.cfg.upstream,
        "auth": !app.cfg.api_keys.is_empty(),
        "billing": app.billing.is_some(),
    }))
}

/// Баланс по своему ключу: клиент шлёт свой x-api-key/Bearer → видит остаток в USD.
async fn balance(State(app): State<AppState>, headers: HeaderMap) -> Response {
    let billing = match &app.billing {
        Some(b) => b,
        None => return (StatusCode::NOT_FOUND, Json(json!({"error": "billing disabled"}))).into_response(),
    };
    let key = match client_key(&headers) {
        Some(k) => k,
        None => return (StatusCode::UNAUTHORIZED, Json(json!({"error": "no api key"}))).into_response(),
    };
    match billing.get(&key) {
        Some(r) => Json(json!({
            "balance": metering::nano_to_usd_string(r.balance_nano as i128),
            "spent": metering::nano_to_usd_string(r.spent_nano as i128),
            "balance_nano": r.balance_nano,
            "spent_nano": r.spent_nano,
            "multiplier": r.mult_bp as f64 / 10000.0,
            "status": r.status,
        })).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "unknown key"}))).into_response(),
    }
}

async fn pool_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !authed(&app, &headers, &peer) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    let now = pool::now();
    let list: Vec<_> = app.pool.snapshot().into_iter().map(|(s, l)| json!({
        "email": s.email,
        "fleet": s.fleet,
        "proxy": !s.proxy.is_empty(),
        "util5h": l.util5h,
        "util7d": l.util7d,
        "status": l.status,
        "cooling": l.cooling_until > now,
        "cooling_left": (l.cooling_until - now).max(0),
        "last_used": l.last_used,
        "polled_ts": l.polled_ts,
    })).collect();
    Json(json!({"pool": list, "cap": app.cfg.util_cap, "poller": app.cfg.poll})).into_response()
}
