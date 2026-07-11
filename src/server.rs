//! HTTP-роутер: наши управляющие эндпоинты + прозрачный форвардинг всего остального.
//!
//!   GET /health            — жив ли сервер (без авторизации)
//!   GET /pool              — статус пула (util/cooling, без секретов)
//!   *                      — форвардинг на api.anthropic.com (см. proxy::forward)

use crate::proxy::{authed, forward};
use crate::state::AppState;
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::get;
use axum::Router;
use serde_json::json;
use std::net::SocketAddr;

pub fn router(app: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/pool", get(pool_status))
        .fallback(forward)
        .with_state(app)
}

async fn health(State(app): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "ok": true,
        "subs": app.pool.len(),
        "upstream": app.cfg.upstream,
        "auth": !app.cfg.api_keys.is_empty(),
    }))
}

async fn pool_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !authed(&app, &headers, &peer) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "unauthorized"}))).into_response();
    }
    let now = crate::pool::now();
    let pool: Vec<_> = app.pool.snapshot().into_iter().map(|(s, l)| json!({
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
    Json(json!({"pool": pool, "cap": app.cfg.util_cap, "poller": app.cfg.poll})).into_response()
}
