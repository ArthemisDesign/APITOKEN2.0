//! Shared model routing and serial fallback for universal Chat, Responses and
//! Messages surfaces (docs/engine/ROUTING_FENCING.md phase 6.2).
//!
//! Requests without `models` preserve the historical behavior and exact body
//! bytes. Explicit fallback chains are validated against one aggregate catalog
//! snapshot before the first attempt, then `models` is removed and `model` is
//! replaced for each serial attempt. A next attempt is allowed only by
//! `proxy::RetryReason`'s fail-closed transport/execution proof.

use std::collections::HashSet;
use std::sync::Arc;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::Request;
use axum::http::request::Parts;
use axum::response::Response;

use crate::catalog::{self, NS_ANTHROPIC, NS_GOOGLE, NS_OPENAI};
use crate::error::{self, Lane};
use crate::{proxy, AppState};

const BODY_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum Surface {
    Chat,
    Responses,
    Messages,
}

impl Surface {
    fn label(self, path: &str) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Messages if path == "/v1/messages/count_tokens" => "messages_count_tokens",
            Self::Messages => "messages",
        }
    }

    fn error_lane(self) -> Lane {
        match self {
            Self::Chat | Self::Responses => Lane::OpenAi,
            Self::Messages => Lane::Anthropic,
        }
    }

    fn invalid(self, message: &str, param: Option<&str>) -> Response {
        match self {
            Self::Chat | Self::Responses => error::invalid_chat_request(message, param),
            Self::Messages => error::invalid_messages_request(message),
        }
    }

    fn model_not_found(self, model: &str) -> Response {
        match self {
            Self::Chat | Self::Responses => error::model_not_found(model),
            Self::Messages => error::messages_model_not_found(model),
        }
    }

    fn catalog_unavailable(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::catalog_unavailable(),
            Self::Messages => error::messages_catalog_unavailable(),
        }
    }

    fn auth_rejected(self) -> Response {
        match self {
            Self::Chat | Self::Responses => crate::auth_rejected_response(),
            Self::Messages => error::messages_auth_rejected(),
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedAttempt {
    /// Value inserted into the per-attempt request body.
    requested_model: String,
    /// Public catalog identity used in bounded logs and duplicate detection.
    catalog_id: String,
    lane: Lane,
}

/// Shared universal handler. The response body is never buffered; only the
/// already-required 32 MiB request body is materialized.
pub async fn proxy_universal(state: Arc<AppState>, req: Request, surface: Surface) -> Response {
    let (parts, body) = req.into_parts();
    let bytes = match to_bytes(body, BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return surface.invalid("Request body exceeds the 32 MiB limit.", None);
        }
    };
    let mut value = match serde_json::from_slice::<serde_json::Value>(&bytes) {
        Ok(value) => value,
        Err(_) => return surface.invalid("Invalid JSON in request body.", None),
    };
    let model = match value.get("model").and_then(|model| model.as_str()) {
        Some(model) if !model.is_empty() => model.to_string(),
        _ => {
            return surface.invalid(
                "Missing or invalid required parameter: model.",
                Some("model"),
            );
        }
    };

    if value.get("models").is_none() {
        return proxy_single(&state, parts, bytes, &model, surface).await;
    }

    // Fail before catalog/auth/network work when rollout is disabled.
    if !state.cfg.fallback_enabled {
        return surface.invalid(
            "Parameter `models` is disabled on this router.",
            Some("models"),
        );
    }

    let fallback_models = match value.get("models").and_then(|models| models.as_array()) {
        Some(models) if !models.is_empty() => models,
        _ => {
            return surface.invalid(
                "Parameter `models` must be a non-empty array of model IDs.",
                Some("models"),
            );
        }
    };
    let mut requested = Vec::with_capacity(fallback_models.len() + 1);
    requested.push(model);
    for candidate in fallback_models {
        match candidate.as_str() {
            Some(candidate) if !candidate.trim().is_empty() => {
                requested.push(candidate.to_string());
            }
            _ => {
                return surface.invalid(
                    "Every entry in `models` must be a non-empty string.",
                    Some("models"),
                );
            }
        }
    }
    let mut raw_seen = HashSet::with_capacity(requested.len());
    if requested
        .iter()
        .any(|candidate| !raw_seen.insert(candidate.as_str()))
    {
        return surface.invalid(
            "The fallback chain must not contain duplicate models.",
            Some("models"),
        );
    }

    let auth = proxy::auth_passthrough(&parts.headers);
    let aggregate = state
        .catalog
        .aggregate(&state.client, &crate::origins(&state), &auth)
        .await;
    if aggregate.auth_rejected {
        return surface.auth_rejected();
    }
    let entries = catalog::dedup(aggregate.entries);
    if entries.is_empty() {
        return surface.catalog_unavailable();
    }

    let mut canonical_seen = HashSet::with_capacity(requested.len());
    let mut attempts = Vec::with_capacity(requested.len());
    for requested_model in requested {
        let Some((namespace, entry)) = catalog::find(&entries, &requested_model) else {
            return surface.invalid(
                &format!("Unknown model in fallback chain: `{requested_model}`."),
                Some("models"),
            );
        };
        if !canonical_seen.insert(entry.id.clone()) {
            return surface.invalid(
                "The fallback chain must not contain duplicate models.",
                Some("models"),
            );
        }
        let Some(lane) = lane_for_namespace(namespace) else {
            return surface.invalid(
                &format!("Unknown model in fallback chain: `{requested_model}`."),
                Some("models"),
            );
        };
        attempts.push(ResolvedAttempt {
            requested_model,
            catalog_id: entry.id.clone(),
            lane,
        });
    }

    let Some(object) = value.as_object_mut() else {
        // A valid top-level `model` can only exist on an object; kept as a
        // fail-closed guard if serde_json behavior changes.
        return surface.invalid("Invalid JSON object in request body.", None);
    };
    object.remove("models");
    let surface_label = surface.label(parts.uri.path());
    let attempt_count = attempts.len();
    let group_id = match fresh_execution_group_id() {
        Ok(group_id) => group_id,
        Err(()) => return surface.catalog_unavailable(),
    };
    for (index, attempt) in attempts.into_iter().enumerate() {
        value
            .as_object_mut()
            .expect("validated request object")
            .insert(
                "model".to_string(),
                serde_json::Value::String(attempt.requested_model),
            );
        let attempt_bytes = match serde_json::to_vec(&value) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return surface.invalid("Invalid JSON in request body.", None),
        };
        let origin = origin_for_lane(&state, attempt.lane);
        let request = request_from_parts(&parts, attempt_bytes);
        let execution = proxy::ExecutionAttemptHeaders {
            group_id: group_id.clone(),
            attempt: index + 1,
        };
        let result = proxy::proxy_attempt(
            &state.client,
            origin,
            attempt.lane,
            surface.error_lane(),
            request,
            Some(&execution),
        )
        .await;
        let status = result.response.status();
        let retry = result.retry_reason.filter(|_| index + 1 < attempt_count);
        let logged_model = bounded_log_id(&attempt.catalog_id);
        eprintln!(
            "router: fallback surface={surface_label} attempt={}/{} model={} lane={:?} status={} retry={}",
            index + 1,
            attempt_count,
            logged_model,
            attempt.lane,
            status.as_u16(),
            retry.map_or("none", proxy::RetryReason::as_str),
        );
        if retry.is_some() {
            continue;
        }
        return result.response;
    }

    // `models` is non-empty and `model` is mandatory, so the chain cannot be
    // empty. Keep a lane-shaped guard instead of panicking on malformed state.
    surface.invalid("The fallback chain is empty.", Some("models"))
}

async fn proxy_single(
    state: &AppState,
    parts: Parts,
    bytes: Bytes,
    model: &str,
    surface: Surface,
) -> Response {
    let lane = match catalog::namespace_lane(model) {
        Some(lane) => lane,
        None => {
            let auth = proxy::auth_passthrough(&parts.headers);
            let aggregate = state
                .catalog
                .aggregate(&state.client, &crate::origins(state), &auth)
                .await;
            if aggregate.auth_rejected {
                return surface.auth_rejected();
            }
            let entries = catalog::dedup(aggregate.entries);
            if entries.is_empty() {
                return surface.catalog_unavailable();
            }
            match catalog::find(&entries, model) {
                Some((namespace, _)) => match lane_for_namespace(namespace) {
                    Some(lane) => lane,
                    None => return surface.model_not_found(model),
                },
                None => return surface.model_not_found(model),
            }
        }
    };
    let origin = origin_for_lane(state, lane);
    let request = Request::from_parts(parts, Body::from(bytes));
    proxy::proxy_attempt(
        &state.client,
        origin,
        lane,
        surface.error_lane(),
        request,
        None,
    )
    .await
    .response
}

fn fresh_execution_group_id() -> Result<String, ()> {
    let mut bytes = [0_u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    ))
}

fn request_from_parts(parts: &Parts, body: Bytes) -> Request {
    let mut request = Request::builder()
        .method(parts.method.clone())
        .uri(parts.uri.clone())
        .version(parts.version)
        .body(Body::from(body))
        .expect("validated request parts");
    *request.headers_mut() = parts.headers.clone();
    // Rewriting `model` and removing `models` changes the byte length. Let
    // reqwest derive framing for the new body instead of forwarding the
    // client's now-stale Content-Length.
    request
        .headers_mut()
        .remove(axum::http::header::CONTENT_LENGTH);
    request
}

fn lane_for_namespace(namespace: &str) -> Option<Lane> {
    match namespace {
        NS_ANTHROPIC => Some(Lane::Anthropic),
        NS_OPENAI => Some(Lane::OpenAi),
        NS_GOOGLE => Some(Lane::Gemini),
        _ => None,
    }
}

fn origin_for_lane(state: &AppState, lane: Lane) -> &str {
    match lane {
        Lane::Anthropic => &state.cfg.anthropic_origin,
        Lane::OpenAi => &state.cfg.openai_origin,
        Lane::Gemini => &state.cfg.gemini_origin,
    }
}

/// Catalog IDs are public, but the plane response is still an external input.
/// Keep attempt logs single-line and bounded even if a malformed catalog emits
/// control characters or an oversized ID.
fn bounded_log_id(id: &str) -> String {
    id.chars()
        .take(128)
        .map(|character| match character {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '/' | '-' | '_' | '.' => character,
            _ => '?',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::bounded_log_id;

    #[test]
    fn attempt_log_model_is_single_line_and_bounded() {
        let id = format!("openai/gpt-5.6\nsecret:{}", "x".repeat(200));
        let logged = bounded_log_id(&id);
        assert!(!logged.contains('\n'));
        assert!(logged.len() <= 128);
        assert!(logged.starts_with("openai/gpt-5.6?secret?"));
    }
}
