//! Read-only credential preflight for the stateless unified router.
//!
//! Universal router surfaces must authenticate before materializing their bounded but large JSON
//! bodies. Every fixed provider runtime exposes the same loopback-only endpoint so the router can
//! fail over across mixed availability without importing engine crates or reserving money.

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forward::{authed, resolve_client_key, AppState};
use serde_json::json;
use std::net::SocketAddr;

const SCHEMA_VERSION: u64 = 1;

fn error(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    (
        status,
        Json(json!({"error": {"code": code, "message": message}})),
    )
        .into_response()
}

fn authenticated() -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "schema_version": SCHEMA_VERSION,
            "authenticated": true,
        })),
    )
        .into_response()
}

/// Authenticate the forwarded credential without reading a request body or mutating money.
pub(crate) async fn preflight(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if authed(&app, &headers, &peer) {
        return authenticated();
    }

    let Some(billing) = app.billing.as_ref() else {
        return error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid API credential.",
        );
    };
    match resolve_client_key(billing, &headers).await {
        Ok(Some(_)) => authenticated(),
        Ok(None) => error(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid API credential.",
        ),
        Err(_) => error(
            StatusCode::SERVICE_UNAVAILABLE,
            "auth_unavailable",
            "Authentication is temporarily unavailable.",
        ),
    }
}
