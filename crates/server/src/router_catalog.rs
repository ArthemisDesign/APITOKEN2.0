//! Model discovery for provider planes that have no client-facing catalogue of their own.
//!
//! The Anthropic plane's `GET /v1/models` is a byte-for-byte proxy of `api.anthropic.com`, and
//! that transparency is a load-bearing invariant of the public API — a client must not be able to
//! tell our fleet from Anthropic's. KIMI rides the same plane (it speaks Anthropic Messages), so
//! its aliases cannot be appended to that response without breaking the invariant for every
//! customer, including those who never asked for KIMI.
//!
//! So discovery moves to an internal producer instead, alongside `/internal/router/catalog/pricing`:
//! the unified router asks this endpoint what the KIMI plane serves, and the public surface is
//! untouched. The list is the published subscription aliases from `metering`, never the official
//! Open Platform ids — those are tariff keys the gateway refuses on the wire.
//!
//! The response carries only public catalogue facts. No account, credential, policy or profile
//! identity leaves the runtime, and nothing here writes money.

use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use forward::{authed, resolve_client_key, AppState};
use serde_json::{json, Value};
use std::net::SocketAddr;

const SCHEMA_VERSION: u64 = 1;

/// Reasoning efforts the KIMI gateway accepts and normalizes. `minimal` is deliberately absent:
/// `reasoning_effort` in the gateway rejects it, so advertising it would hand the router a value
/// that turns into a 400 on first use.
const REASONING_EFFORTS: [&str; 6] = ["none", "low", "medium", "high", "xhigh", "max"];

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({"error": {"code": "unauthorized", "message": "Invalid API credential."}})),
    )
        .into_response()
}

/// Publish the KIMI plane's model list to the unified router.
///
/// An empty list is the honest answer when the plane is disabled on this slot: the router then
/// simply has no `kimi/*` entries, rather than marking the namespace degraded and serving a stale
/// snapshot of models this process cannot route.
pub(crate) async fn kimi(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    if !authed(&app, &headers, &peer) {
        let Some(billing) = app.billing.as_ref() else {
            return unauthorized();
        };
        let now = pool::now();
        match resolve_client_key(billing, &headers).await {
            Ok(Some((_, auth))) if auth.active_at(now) => {}
            Ok(_) => return unauthorized(),
            Err(_) => {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({"error": {
                        "code": "catalog_unavailable",
                        "message": "Catalog discovery is temporarily unavailable.",
                    }})),
                )
                    .into_response()
            }
        }
    }

    let data: Vec<_> = if app.kimi.is_some() {
        metering::kimi_subscription_models()
            .iter()
            .map(|model| {
                json!({
                    "id": model.alias,
                    "display_name": display_name(model.alias),
                    "max_input_tokens": model.input_token_limit,
                    "reasoning_efforts": REASONING_EFFORTS,
                    // Thinking is on by default and switched off through `thinking.type`, so the
                    // capability is proven. Image input and structured outputs are not proven on
                    // the subscription endpoint; `null` keeps them unknown instead of promising or
                    // denying a capability we have not tested.
                    "reasoning": true,
                    "image_input": Value::Null,
                    "structured_outputs": Value::Null,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    (
        StatusCode::OK,
        Json(json!({"schema_version": SCHEMA_VERSION, "data": data})),
    )
        .into_response()
}

/// Operator-facing label. The provider publishes none for the subscription aliases, so the
/// catalogue would otherwise show a bare id in every client model picker.
fn display_name(alias: &str) -> &'static str {
    match alias {
        "kimi-for-coding" => "Kimi for Coding",
        "kimi-for-coding-highspeed" => "Kimi for Coding (High Speed)",
        "k3" => "Kimi K3 (1M)",
        "k3[1m]" => "Kimi K3 (1M)",
        "k3-256k" => "Kimi K3 (256K)",
        _ => "Kimi",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_published_alias_has_a_label_and_an_accepted_effort_set() {
        for model in metering::kimi_subscription_models() {
            assert_ne!(
                display_name(model.alias),
                "Kimi",
                "{} has no explicit label",
                model.alias
            );
        }
        // `minimal` is in the router's vocabulary but not in the gateway's; publishing it would
        // advertise a value that becomes `invalid_reasoning_effort` on first use.
        assert!(!REASONING_EFFORTS.contains(&"minimal"));
    }
}
