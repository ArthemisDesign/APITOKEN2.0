//! Shared model routing and serial fallback for universal Chat, Responses and
//! Messages surfaces (docs/engine/ROUTING_FENCING.md phase 6.2).
//!
//! Requests without `models`, `provider`, `preset/*` or the router-owned Fast header preserve the
//! historical behavior and exact body bytes. Advanced plans expand reviewed presets, resolve one
//! aggregate catalog snapshot, apply deterministic provider preferences and call the engine-owned
//! account-policy preflight before attempt 1. Router-only fields are then removed and `model` is
//! replaced for each serial attempt. A next attempt is allowed only by `proxy::RetryReason`'s
//! fail-closed transport/execution proof.

use std::collections::HashSet;
use std::sync::Arc;

use axum::body::{to_bytes, Body, Bytes};
use axum::extract::Request;
use axum::http::{request::Parts, HeaderMap};
use axum::response::Response;

use crate::catalog::{self, NS_ANTHROPIC, NS_GOOGLE, NS_OPENAI};
use crate::error::{self, Lane};
use crate::policy::{
    self, PolicyCandidate, PreflightError, ProviderNamespace, ProviderPreferences, SortMode,
};
use crate::{presets, proxy, AppState};

const BODY_LIMIT: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

    fn policy_unavailable(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::policy_unavailable(),
            Self::Messages => error::messages_policy_unavailable(),
        }
    }

    fn policy_restricted(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::policy_restricted(),
            Self::Messages => error::messages_policy_restricted(),
        }
    }
}

#[derive(Clone, Debug)]
struct ResolvedAttempt {
    /// Value inserted into the per-attempt request body.
    body_model_value: String,
    /// Public catalog identity used in bounded logs and duplicate detection.
    catalog_id: String,
    /// Provider namespace and canonical native ID sent to engine policy authority.
    provider: ProviderNamespace,
    canonical_model_id: String,
    lane: Lane,
}

#[derive(Clone, Debug)]
struct ExpandedCandidate {
    body_model_value: String,
    preset_id: Option<&'static str>,
}

/// Shared universal handler. The response body is never buffered; only the
/// already-required 32 MiB request body is materialized.
pub async fn proxy_universal(state: Arc<AppState>, req: Request, surface: Surface) -> Response {
    let (mut parts, body) = req.into_parts();
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
    let fast_header =
        match take_fast_service_tier_header(&mut parts.headers, surface, parts.uri.path()) {
            Ok(present) => present,
            Err(response) => return response,
        };

    let has_models = value.get("models").is_some();
    let has_provider = value.get("provider").is_some();
    let has_preset = presets::is_preset_syntax(&model);
    if !has_models && !has_provider && !has_preset {
        return proxy_single(
            &state,
            parts,
            bytes,
            &mut value,
            &model,
            surface,
            fast_header,
        )
        .await;
    }

    // Fail before catalog/auth/network work when rollout is disabled.
    if !state.cfg.fallback_enabled {
        let (message, param) = if has_models {
            ("Parameter `models` is disabled on this router.", "models")
        } else if has_provider {
            (
                "Parameter `provider` is disabled on this router.",
                "provider",
            )
        } else {
            ("Routing presets are disabled on this router.", "model")
        };
        return surface.invalid(message, Some(param));
    }

    let preferences = match value.get("provider") {
        Some(provider) => match ProviderPreferences::parse(provider) {
            Ok(preferences) => preferences,
            Err(()) => {
                return surface.invalid(
                    "Parameter `provider` contains invalid routing preferences.",
                    Some("provider"),
                )
            }
        },
        None => ProviderPreferences::default(),
    };

    let mut requested = vec![model];
    if let Some(models_value) = value.get("models") {
        let fallback_models = match models_value.as_array() {
            Some(models) if !models.is_empty() => models,
            _ => {
                return surface.invalid(
                    "Parameter `models` must be a non-empty array of model IDs.",
                    Some("models"),
                );
            }
        };
        requested.reserve(fallback_models.len());
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

    let mut expanded = Vec::new();
    let mut referenced_presets = Vec::new();
    for requested_model in requested {
        if presets::is_preset_syntax(&requested_model) {
            let Some(preset) = presets::find(&requested_model) else {
                return surface.invalid(
                    &format!("Unknown routing preset: `{requested_model}`."),
                    Some("model"),
                );
            };
            referenced_presets.push(preset.id());
            expanded.extend(preset.models().iter().cloned().map(|body_model_value| {
                ExpandedCandidate {
                    body_model_value,
                    preset_id: Some(preset.id()),
                }
            }));
        } else {
            expanded.push(ExpandedCandidate {
                body_model_value: requested_model,
                preset_id: None,
            });
        }
    }
    if expanded.len() > policy::MAX_CANDIDATES {
        return surface.invalid(
            "The expanded routing chain must contain at most 32 models.",
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

    let mut canonical_seen = HashSet::with_capacity(expanded.len());
    let mut preset_live = vec![false; referenced_presets.len()];
    let mut attempts = Vec::with_capacity(expanded.len());
    for candidate in expanded {
        let Some((namespace, entry)) = catalog::find(&entries, &candidate.body_model_value) else {
            if candidate.preset_id.is_some() {
                continue;
            }
            return surface.invalid(
                &format!(
                    "Unknown model in routing chain: `{}`.",
                    candidate.body_model_value
                ),
                Some("models"),
            );
        };
        if let Some(preset_id) = candidate.preset_id {
            if let Some(index) = referenced_presets
                .iter()
                .position(|referenced| *referenced == preset_id)
            {
                preset_live[index] = true;
            }
        }
        if !canonical_seen.insert(entry.id.clone()) {
            return surface.invalid(
                "The routing chain must not contain duplicate models.",
                Some("models"),
            );
        }
        let Some(provider) = provider_for_namespace(namespace) else {
            return surface.invalid(
                &format!(
                    "Unknown model in routing chain: `{}`.",
                    candidate.body_model_value
                ),
                Some("models"),
            );
        };
        attempts.push(ResolvedAttempt {
            body_model_value: candidate.body_model_value,
            catalog_id: entry.id.clone(),
            provider,
            canonical_model_id: entry.alias.clone(),
            lane: provider.lane(),
        });
    }
    if preset_live.iter().any(|live| !live) {
        return surface.catalog_unavailable();
    }

    attempts.retain(|attempt| preferences.allows(attempt.provider));
    if attempts.is_empty() {
        return surface.invalid(
            "Provider preferences removed every model from the routing chain.",
            Some("provider"),
        );
    }
    attempts.sort_by_key(|attempt| preferences.order_rank(attempt.provider));
    if let Some(sort) = preferences.sort() {
        if attempts
            .iter()
            .any(|attempt| presets::ranks(&attempt.catalog_id).is_none())
        {
            return surface.invalid(
                "A model in the routing chain has no reviewed rank for the requested sort.",
                Some("provider"),
            );
        }
        attempts.sort_by_key(|attempt| {
            let (price, latency) =
                presets::ranks(&attempt.catalog_id).expect("ranks validated above");
            match sort {
                SortMode::Price => price,
                SortMode::Latency => latency,
            }
        });
    }
    if !preferences.allow_fallbacks() {
        attempts.truncate(1);
    }

    let policy_candidates: Vec<_> = attempts
        .iter()
        .map(|attempt| PolicyCandidate {
            id: &attempt.catalog_id,
            provider: attempt.provider,
            canonical_model_id: &attempt.canonical_model_id,
        })
        .collect();
    let allowed = match policy::preflight(
        &state.client,
        &crate::origins(&state),
        &auth,
        &policy_candidates,
    )
    .await
    {
        Ok(allowed) => allowed,
        Err(PreflightError::Unauthorized) => return surface.auth_rejected(),
        Err(PreflightError::Unavailable) => return surface.policy_unavailable(),
        Err(PreflightError::Restricted) => return surface.policy_restricted(),
    };
    let allowed: HashSet<_> = allowed.iter().map(String::as_str).collect();
    attempts.retain(|attempt| allowed.contains(attempt.catalog_id.as_str()));

    if fast_header {
        if attempts.iter().any(|attempt| attempt.lane != Lane::OpenAi) {
            return surface.invalid(
                "Header `x-apitoken-service-tier` is supported only for GPT models.",
                Some(proxy::SERVICE_TIER_HEADER.as_str()),
            );
        }
        if let Err(response) = apply_fast_service_tier(&mut value, surface) {
            return response;
        }
    }

    let Some(object) = value.as_object_mut() else {
        // A valid top-level `model` can only exist on an object; kept as a
        // fail-closed guard if serde_json behavior changes.
        return surface.invalid("Invalid JSON object in request body.", None);
    };
    object.remove("models");
    object.remove("provider");
    let surface_label = surface.label(parts.uri.path());
    let attempt_count = attempts.len();
    let attempt_lanes: Vec<_> = attempts.iter().map(|attempt| attempt.lane).collect();
    let group_id = if attempt_count > 1 {
        match fresh_execution_group_id() {
            Ok(group_id) => Some(group_id),
            Err(()) => return surface.catalog_unavailable(),
        }
    } else {
        None
    };
    for (index, attempt) in attempts.into_iter().enumerate() {
        value
            .as_object_mut()
            .expect("validated request object")
            .insert(
                "model".to_string(),
                serde_json::Value::String(attempt.body_model_value),
            );
        let attempt_bytes = match serde_json::to_vec(&value) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return surface.invalid("Invalid JSON in request body.", None),
        };
        let origin = origin_for_lane(&state, attempt.lane);
        let request = request_from_parts(&parts, attempt_bytes);
        let execution = group_id
            .as_ref()
            .map(|group_id| proxy::ExecutionAttemptHeaders {
                group_id: group_id.clone(),
                attempt: index + 1,
            });
        let result = proxy::proxy_attempt(
            &state.client,
            origin,
            attempt.lane,
            surface.error_lane(),
            request,
            execution.as_ref(),
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
        if let Some(reason) = retry {
            state
                .metrics
                .fallback(attempt.lane, attempt_lanes[index + 1], reason);
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
    value: &mut serde_json::Value,
    model: &str,
    surface: Surface,
    fast_header: bool,
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
    let bytes = if fast_header {
        if lane != Lane::OpenAi {
            return surface.invalid(
                "Header `x-apitoken-service-tier` is supported only for GPT models.",
                Some(proxy::SERVICE_TIER_HEADER.as_str()),
            );
        }
        if let Err(response) = apply_fast_service_tier(value, surface) {
            return response;
        }
        match serde_json::to_vec(value) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return surface.invalid("Invalid JSON in request body.", None),
        }
    } else {
        bytes
    };
    let origin = origin_for_lane(state, lane);
    let request = if fast_header {
        request_from_parts(&parts, bytes)
    } else {
        Request::from_parts(parts, Body::from(bytes))
    };
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

fn take_fast_service_tier_header(
    headers: &mut HeaderMap,
    surface: Surface,
    path: &str,
) -> Result<bool, Response> {
    let (present, valid) = {
        let mut values = headers.get_all(&proxy::SERVICE_TIER_HEADER).iter();
        match (values.next(), values.next()) {
            (None, _) => (false, false),
            (Some(value), None) => (true, matches!(value.as_bytes(), b"fast" | b"priority")),
            (Some(_), Some(_)) => (true, false),
        }
    };
    if !present {
        return Ok(false);
    }
    headers.remove(&proxy::SERVICE_TIER_HEADER);
    if !valid {
        return Err(surface.invalid(
            "Header `x-apitoken-service-tier` must occur once with value `fast` or `priority`.",
            Some(proxy::SERVICE_TIER_HEADER.as_str()),
        ));
    }
    if path == "/v1/messages/count_tokens" {
        return Err(surface.invalid(
            "Header `x-apitoken-service-tier` is not supported for token counting.",
            Some(proxy::SERVICE_TIER_HEADER.as_str()),
        ));
    }
    Ok(true)
}

fn apply_fast_service_tier(
    value: &mut serde_json::Value,
    surface: Surface,
) -> Result<(), Response> {
    let Some(object) = value.as_object_mut() else {
        return Err(surface.invalid("Invalid JSON object in request body.", None));
    };
    if let Some(service_tier) = object.get("service_tier") {
        if !matches!(service_tier.as_str(), Some("fast" | "priority")) {
            return Err(surface.invalid(
                "Header `x-apitoken-service-tier` conflicts with body parameter `service_tier`.",
                Some("service_tier"),
            ));
        }
    }
    if surface == Surface::Messages {
        if let Some(speed) = object.get("speed") {
            if speed.as_str() != Some("fast") {
                return Err(surface.invalid(
                    "Header `x-apitoken-service-tier` conflicts with body parameter `speed`.",
                    None,
                ));
            }
        }
    }
    object.insert(
        "service_tier".to_string(),
        serde_json::Value::String("priority".to_string()),
    );
    Ok(())
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
    provider_for_namespace(namespace).map(ProviderNamespace::lane)
}

fn provider_for_namespace(namespace: &str) -> Option<ProviderNamespace> {
    match namespace {
        NS_ANTHROPIC => Some(ProviderNamespace::Anthropic),
        NS_OPENAI => Some(ProviderNamespace::OpenAi),
        NS_GOOGLE => Some(ProviderNamespace::Google),
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
    use super::*;
    use axum::http::{HeaderValue, StatusCode};

    #[test]
    fn attempt_log_model_is_single_line_and_bounded() {
        let id = format!("openai/gpt-5.6\nsecret:{}", "x".repeat(200));
        let logged = bounded_log_id(&id);
        assert!(!logged.contains('\n'));
        assert!(logged.len() <= 128);
        assert!(logged.starts_with("openai/gpt-5.6?secret?"));
    }

    #[test]
    fn fast_header_accepts_exact_aliases_and_is_consumed() {
        for tier in ["fast", "priority"] {
            let mut headers = HeaderMap::new();
            headers.insert(
                proxy::SERVICE_TIER_HEADER,
                HeaderValue::from_str(tier).unwrap(),
            );
            assert!(take_fast_service_tier_header(
                &mut headers,
                Surface::Chat,
                "/v1/chat/completions"
            )
            .unwrap());
            assert!(headers.get(&proxy::SERVICE_TIER_HEADER).is_none());
        }
    }

    #[test]
    fn fast_header_rejects_invalid_duplicate_and_counting_uses() {
        let mut invalid = HeaderMap::new();
        invalid.insert(
            proxy::SERVICE_TIER_HEADER,
            HeaderValue::from_static("economy"),
        );
        let response =
            take_fast_service_tier_header(&mut invalid, Surface::Responses, "/v1/responses")
                .unwrap_err();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let mut duplicate = HeaderMap::new();
        duplicate.append(proxy::SERVICE_TIER_HEADER, HeaderValue::from_static("fast"));
        duplicate.append(
            proxy::SERVICE_TIER_HEADER,
            HeaderValue::from_static("priority"),
        );
        assert!(take_fast_service_tier_header(
            &mut duplicate,
            Surface::Chat,
            "/v1/chat/completions"
        )
        .is_err());

        let mut counting = HeaderMap::new();
        counting.insert(proxy::SERVICE_TIER_HEADER, HeaderValue::from_static("fast"));
        assert!(take_fast_service_tier_header(
            &mut counting,
            Surface::Messages,
            "/v1/messages/count_tokens"
        )
        .is_err());
    }

    #[test]
    fn fast_header_normalizes_equivalent_body_values_and_rejects_conflicts() {
        for mut value in [
            serde_json::json!({"model": "openai/gpt-5.6"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "fast"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "priority"}),
        ] {
            apply_fast_service_tier(&mut value, Surface::Responses).unwrap();
            assert_eq!(value["service_tier"], "priority");
        }

        for mut value in [
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "default"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": null}),
        ] {
            let response = apply_fast_service_tier(&mut value, Surface::Chat).unwrap_err();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        let mut messages = serde_json::json!({
            "model": "openai/gpt-5.6",
            "speed": "fast"
        });
        apply_fast_service_tier(&mut messages, Surface::Messages).unwrap();
        assert_eq!(messages["service_tier"], "priority");

        messages["speed"] = serde_json::Value::String("standard".to_string());
        assert!(apply_fast_service_tier(&mut messages, Surface::Messages).is_err());
    }
}
