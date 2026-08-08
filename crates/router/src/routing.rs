//! Shared model routing and serial fallback for universal Chat, Responses and
//! Messages surfaces (docs/engine/ROUTING_FENCING.md phase 6.2).
//!
//! Requests without `models`, `provider`, `preset/*` or router-owned Fast compatibility selectors
//! preserve the historical behavior and exact body bytes. Advanced plans expand reviewed presets,
//! resolve one aggregate catalog snapshot, apply deterministic provider preferences and call the
//! engine-owned account-policy preflight before attempt 1. Router-only fields are then removed and
//! `model` is replaced for each serial attempt. A next attempt is allowed only by
//! `proxy::RetryReason`'s fail-closed transport/execution proof.

use std::collections::HashSet;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::{Body, Bytes};
use axum::extract::Request;
use axum::http::{request::Parts, HeaderMap};
use axum::response::Response;
use futures_util::{stream, StreamExt};
use tokio::sync::OwnedSemaphorePermit;

use crate::auth::{self, AuthError};
use crate::catalog::{self, NS_ANTHROPIC, NS_GOOGLE, NS_KIMI, NS_OPENAI};
use crate::error::{self, Lane};
use crate::metrics::{AuthOutcome, PolicyFailure, RouterMetrics};
use crate::policy::{
    self, PolicyCandidate, PreflightError, ProviderNamespace, ProviderPreferences, SortMode,
};
use crate::{presets, proxy, AppState};

const BODY_LIMIT: usize = 32 * 1024 * 1024;
const BODY_READ_IDLE_TIMEOUT: Duration = Duration::from_secs(15);
const BODY_READ_MAX_TIMEOUT: Duration = Duration::from_secs(5 * 60);
pub const BODY_ADMISSION_UNIT_BYTES: usize = 1024 * 1024;
pub const BODY_ADMISSION_UNITS: usize = 64;
const MAX_BODY_ADMISSION_UNITS: u32 = (BODY_LIMIT / BODY_ADMISSION_UNIT_BYTES) as u32;

struct BodyAdmissionPermit {
    permit: OwnedSemaphorePermit,
    metrics: Arc<RouterMetrics>,
    units: u32,
}

impl BodyAdmissionPermit {
    fn new(permit: OwnedSemaphorePermit, metrics: Arc<RouterMetrics>, units: u32) -> Self {
        metrics.body_units_acquired(units);
        Self {
            permit,
            metrics,
            units,
        }
    }

    fn merge(&mut self, permit: OwnedSemaphorePermit, units: u32) {
        self.permit.merge(permit);
        self.units += units;
        self.metrics.body_units_acquired(units);
    }
}

impl Drop for BodyAdmissionPermit {
    fn drop(&mut self) {
        self.metrics.body_units_released(self.units);
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BodyReadError {
    TooLarge,
    Overloaded,
    Timeout,
    Transport,
}

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

    fn auth_unavailable(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::auth_unavailable(),
            Self::Messages => error::messages_auth_unavailable(),
        }
    }

    fn overloaded(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::body_admission_overloaded(),
            Self::Messages => error::messages_body_admission_overloaded(),
        }
    }

    fn body_timeout(self) -> Response {
        match self {
            Self::Chat | Self::Responses => error::body_read_timeout(),
            Self::Messages => error::messages_body_read_timeout(),
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
    let _request_guard = state.metrics.universal_request();
    let auth_headers = proxy::auth_passthrough(req.headers());
    let auth_started = Instant::now();
    let auth_result = auth::preflight(&state.client, &crate::origins(&state), &auth_headers).await;
    state.metrics.auth(
        match auth_result {
            Ok(()) => AuthOutcome::Success,
            Err(AuthError::Unauthorized) => AuthOutcome::Unauthorized,
            Err(AuthError::Unavailable) => AuthOutcome::Unavailable,
        },
        auth_started.elapsed(),
    );
    match auth_result {
        Ok(()) => {}
        Err(AuthError::Unauthorized) => return surface.auth_rejected(),
        Err(AuthError::Unavailable) => {
            elog::warn("router-auth", "auth preflight unavailable");
            return surface.auth_unavailable();
        }
    }
    let (mut parts, body) = req.into_parts();
    let (bytes, body_permit) = match read_body(&state, &parts.headers, body).await {
        Ok(result) => result,
        Err(BodyReadError::TooLarge) => {
            return surface.invalid("Request body exceeds the 32 MiB limit.", None)
        }
        Err(BodyReadError::Overloaded) => {
            elog::warn("router", "body admission overload");
            return surface.overloaded();
        }
        Err(BodyReadError::Timeout) => return surface.body_timeout(),
        Err(BodyReadError::Transport) => {
            return surface.invalid("Failed to read request body.", None)
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
    let fast_body_alias = has_fast_service_tier_alias(&value);

    let has_models = value.get("models").is_some();
    let has_provider = value.get("provider").is_some();
    let has_preset = presets::is_preset_syntax(&model);
    if !has_models && !has_provider && !has_preset {
        return proxy_single(
            &state,
            parts,
            bytes,
            value,
            &model,
            surface,
            fast_header,
            fast_body_alias,
            body_permit,
        )
        .await;
    }

    // Fail before catalog, policy, or billable network work when rollout is disabled. Early auth
    // has already run so an unauthenticated caller cannot use this parse path as a memory oracle.
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

    // Advanced routing serializes a fresh body for each attempt; the original raw allocation is no
    // longer needed after parsing. The weighted permit is transferred to attempt 1 below.
    drop(bytes);

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
        .aggregate(
            &state.client,
            &crate::origins(&state),
            &auth,
            &state.metrics,
        )
        .await;
    if aggregate.auth_rejected {
        return surface.auth_rejected();
    }
    let entries = catalog::dedup(aggregate.entries);
    if entries.is_empty() {
        elog::warn("router", "catalog empty; router degraded");
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
            canonical_model_id: entry.native_id.clone(),
            lane: provider.lane(),
        });
    }
    if preset_live.iter().any(|live| !live) {
        for (index, live) in preset_live.iter().enumerate() {
            if !*live {
                elog::warn(
                    "router",
                    format!(
                        "preset {} has no live catalog members",
                        referenced_presets[index]
                    ),
                );
                break;
            }
        }
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
    let allowed_result = policy::preflight(
        &state.client,
        &crate::origins(&state),
        &auth,
        &policy_candidates,
    )
    .await;
    let allowed = match allowed_result {
        Ok(allowed) => allowed,
        Err(PreflightError::Unauthorized) => {
            state.metrics.policy_failure(PolicyFailure::Unauthorized);
            return surface.auth_rejected();
        }
        Err(PreflightError::Unavailable) => {
            state.metrics.policy_failure(PolicyFailure::Unavailable);
            elog::warn("router", "policy preflight unavailable or restricted");
            return surface.policy_unavailable();
        }
        Err(PreflightError::Restricted) => {
            state.metrics.policy_failure(PolicyFailure::Restricted);
            return surface.policy_restricted();
        }
    };
    let allowed: HashSet<_> = allowed.iter().map(String::as_str).collect();
    attempts.retain(|attempt| allowed.contains(attempt.catalog_id.as_str()));

    if fast_header || fast_body_alias {
        if attempts.iter().any(|attempt| attempt.lane != Lane::OpenAi) {
            return surface.invalid(
                "Fast service-tier compatibility selectors are supported only for GPT models.",
                Some(if fast_header {
                    "x-apitoken-service-tier"
                } else {
                    "serviceTier"
                }),
            );
        }
        if let Err(response) = normalize_fast_service_tier(&mut value, surface, fast_header) {
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
    // Advanced routing must retain the parsed template to build a later attempt. Keep its
    // admission weight until a terminal set of response headers is selected; otherwise a slow
    // provider could leave large serde trees resident after their upload permit was released.
    let _body_permit = body_permit;
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
        let request = request_from_parts(&parts, attempt_bytes, None);
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
            &state.metrics,
        )
        .await;
        let status = result.response.status();
        let retry = result.retry_reason.filter(|_| index + 1 < attempt_count);
        let logged_model = bounded_log_id(&attempt.catalog_id);
        elog::info(
            "router",
            format!(
                "router: fallback surface={surface_label} attempt={}/{} model={} lane={:?} status={} retry={}",
                index + 1,
                attempt_count,
                logged_model,
                attempt.lane,
                status.as_u16(),
                retry.map_or("none", proxy::RetryReason::as_str),
            ),
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

/// A valid declared length reserves its complete weight before reading. Unknown/chunked requests
/// start at one unit and fail-fast acquire further units only when their observed bytes cross each
/// MiB boundary, so empty slow clients cannot pin half of the global budget each.
fn declared_body_admission_units(headers: &HeaderMap) -> Result<Option<u32>, BodyReadError> {
    let Some(length) = headers
        .get(axum::http::header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return Ok(None);
    };
    if length > BODY_LIMIT {
        return Err(BodyReadError::TooLarge);
    }
    Ok(Some(body_units_for_len(length)))
}

fn body_units_for_len(length: usize) -> u32 {
    length
        .saturating_add(BODY_ADMISSION_UNIT_BYTES - 1)
        .checked_div(BODY_ADMISSION_UNIT_BYTES)
        .unwrap_or(MAX_BODY_ADMISSION_UNITS as usize)
        .clamp(1, MAX_BODY_ADMISSION_UNITS as usize) as u32
}

async fn read_body(
    state: &AppState,
    headers: &HeaderMap,
    body: Body,
) -> Result<(Bytes, BodyAdmissionPermit), BodyReadError> {
    read_body_with_timeouts(
        state,
        headers,
        body,
        BODY_READ_IDLE_TIMEOUT,
        BODY_READ_MAX_TIMEOUT,
    )
    .await
}

async fn read_body_with_timeouts(
    state: &AppState,
    headers: &HeaderMap,
    body: Body,
    idle_timeout: Duration,
    max_timeout: Duration,
) -> Result<(Bytes, BodyAdmissionPermit), BodyReadError> {
    let declared_units = declared_body_admission_units(headers)?;
    let initial_units = declared_units.unwrap_or(1);
    let permit = state
        .body_admission
        .clone()
        .try_acquire_many_owned(initial_units)
        .map_err(|_| {
            state.metrics.body_admission_overload();
            BodyReadError::Overloaded
        })?;
    let mut admission = BodyAdmissionPermit::new(permit, state.metrics.clone(), initial_units);
    let admission_pool = state.body_admission.clone();
    let metrics = state.metrics.clone();

    let read = async move {
        let mut stream = body.into_data_stream();
        let started_at = tokio::time::Instant::now();
        let mut last_progress_at = started_at;
        let mut bytes = Vec::with_capacity(
            headers
                .get(axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0)
                .min(BODY_LIMIT),
        );
        loop {
            let now = tokio::time::Instant::now();
            let idle_remaining = idle_timeout
                .checked_sub(now.duration_since(last_progress_at))
                .ok_or(BodyReadError::Timeout)?;
            let max_remaining = max_timeout
                .checked_sub(now.duration_since(started_at))
                .ok_or(BodyReadError::Timeout)?;
            let next = tokio::time::timeout(idle_remaining.min(max_remaining), stream.next())
                .await
                .map_err(|_| BodyReadError::Timeout)?;
            let Some(chunk) = next else {
                break;
            };
            let chunk = chunk.map_err(|e| {
                elog::warn("router", format!("request body stream error: {e}"));
                BodyReadError::Transport
            })?;
            if chunk.is_empty() {
                continue;
            }
            last_progress_at = tokio::time::Instant::now();
            let next_len = bytes
                .len()
                .checked_add(chunk.len())
                .ok_or(BodyReadError::TooLarge)?;
            if next_len > BODY_LIMIT {
                return Err(BodyReadError::TooLarge);
            }
            let needed_units = body_units_for_len(next_len);
            if needed_units > admission.units {
                let additional = needed_units - admission.units;
                let permit = admission_pool
                    .clone()
                    .try_acquire_many_owned(additional)
                    .map_err(|_| {
                        metrics.body_admission_overload();
                        BodyReadError::Overloaded
                    })?;
                admission.merge(permit, additional);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((Bytes::from(bytes), admission))
    };

    match read.await {
        Err(BodyReadError::Timeout) => {
            state.metrics.body_read_timeout();
            Err(BodyReadError::Timeout)
        }
        result => result,
    }
}

fn upload_body(bytes: Bytes, permit: BodyAdmissionPermit) -> Body {
    // The permit remains in the unfold state after yielding the only chunk. Reqwest polls once more
    // for EOF only after it has consumed that chunk, so admission releases after upload rather than
    // after provider TTFT/response headers. Transport cancellation drops the state immediately.
    let stream = stream::unfold((Some(bytes), Some(permit)), |(bytes, permit)| async move {
        bytes.map(|bytes| (Ok::<Bytes, Infallible>(bytes), (None, permit)))
    });
    Body::from_stream(stream)
}

async fn proxy_single(
    state: &AppState,
    parts: Parts,
    bytes: Bytes,
    mut value: serde_json::Value,
    model: &str,
    surface: Surface,
    fast_header: bool,
    fast_body_alias: bool,
    body_permit: BodyAdmissionPermit,
) -> Response {
    let mut rewrite_native_model = None;
    let lane = match catalog::namespace_lane(model) {
        Some(lane) => lane,
        None => {
            let auth = proxy::auth_passthrough(&parts.headers);
            let aggregate = state
                .catalog
                .aggregate(&state.client, &crate::origins(state), &auth, &state.metrics)
                .await;
            if aggregate.auth_rejected {
                return surface.auth_rejected();
            }
            let entries = catalog::dedup(aggregate.entries);
            if entries.is_empty() {
                elog::warn("router", "catalog empty; router degraded");
                return surface.catalog_unavailable();
            }
            match catalog::find(&entries, model) {
                Some((namespace, entry)) => {
                    if entry.native_id != model {
                        rewrite_native_model = Some(entry.native_id.clone());
                    }
                    match lane_for_namespace(namespace) {
                        Some(lane) => lane,
                        None => return surface.model_not_found(model),
                    }
                }
                None => return surface.model_not_found(model),
            }
        }
    };
    let fast_compat = fast_header || fast_body_alias;
    let bytes = if fast_compat {
        if lane != Lane::OpenAi {
            return surface.invalid(
                "Fast service-tier compatibility selectors are supported only for GPT models.",
                Some(if fast_header {
                    "x-apitoken-service-tier"
                } else {
                    "serviceTier"
                }),
            );
        }
        if let Err(response) = normalize_fast_service_tier(&mut value, surface, fast_header) {
            return response;
        }
        match serde_json::to_vec(&value) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return surface.invalid("Invalid JSON in request body.", None),
        }
    } else if let Some(native_model) = &rewrite_native_model {
        value
            .as_object_mut()
            .expect("validated request object")
            .insert(
                "model".to_string(),
                serde_json::Value::String(native_model.clone()),
            );
        match serde_json::to_vec(&value) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return surface.invalid("Invalid JSON in request body.", None),
        }
    } else {
        bytes
    };
    // The model and lane are now resolved and any rewritten body has been serialized. Drop the
    // potentially large serde tree before transferring the only remaining body allocation and
    // its admission permit into the outbound upload stream.
    drop(value);
    let origin = origin_for_lane(state, lane);
    let request = if fast_compat || rewrite_native_model.is_some() {
        request_from_parts(&parts, bytes, Some(body_permit))
    } else {
        Request::from_parts(parts, upload_body(bytes, body_permit))
    };
    proxy::proxy_attempt(
        &state.client,
        origin,
        lane,
        surface.error_lane(),
        request,
        None,
        &state.metrics,
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

fn has_fast_service_tier_alias(value: &serde_json::Value) -> bool {
    value
        .as_object()
        .is_some_and(|object| object.contains_key("serviceTier"))
}

fn normalize_fast_service_tier(
    value: &mut serde_json::Value,
    surface: Surface,
    header_present: bool,
) -> Result<(), Response> {
    let Some(object) = value.as_object_mut() else {
        return Err(surface.invalid("Invalid JSON object in request body.", None));
    };
    let alias_present = object.contains_key("serviceTier");
    if !header_present && !alias_present {
        return Ok(());
    }
    if alias_present {
        if surface == Surface::Messages {
            return Err(surface.invalid(
                "Body parameter `serviceTier` is supported only on OpenAI-compatible Chat and Responses surfaces.",
                Some("serviceTier"),
            ));
        }
        if !matches!(
            object
                .get("serviceTier")
                .and_then(serde_json::Value::as_str),
            Some("fast" | "priority")
        ) {
            return Err(surface.invalid(
                "Body parameter `serviceTier` must be `fast` or `priority`.",
                Some("serviceTier"),
            ));
        }
    }
    if let Some(service_tier) = object.get("service_tier") {
        if !matches!(service_tier.as_str(), Some("fast" | "priority")) {
            return Err(surface.invalid(
                "Fast service-tier compatibility selector conflicts with body parameter `service_tier`.",
                Some("service_tier"),
            ));
        }
    }
    if header_present && surface == Surface::Messages {
        if let Some(speed) = object.get("speed") {
            if speed.as_str() != Some("fast") {
                return Err(surface.invalid(
                    "Header `x-apitoken-service-tier` conflicts with body parameter `speed`.",
                    None,
                ));
            }
        }
    }
    object.remove("serviceTier");
    object.insert(
        "service_tier".to_string(),
        serde_json::Value::String("priority".to_string()),
    );
    Ok(())
}

fn fresh_execution_group_id() -> Result<String, ()> {
    let mut bytes = [0_u8; 16];
    if let Err(e) = getrandom::fill(&mut bytes) {
        elog::error("router", format!("getrandom failed for execution group id: {e}"));
        return Err(());
    }
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

fn request_from_parts(parts: &Parts, body: Bytes, permit: Option<BodyAdmissionPermit>) -> Request {
    let body = match permit {
        Some(permit) => upload_body(body, permit),
        None => Body::from(body),
    };
    let mut request = Request::builder()
        .method(parts.method.clone())
        .uri(parts.uri.clone())
        .version(parts.version)
        .body(body)
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
        NS_KIMI => Some(ProviderNamespace::Kimi),
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
    use tokio::sync::Semaphore;

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
    fn fast_selectors_normalize_equivalent_body_values_and_reject_conflicts() {
        for mut value in [
            serde_json::json!({"model": "openai/gpt-5.6"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "fast"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "priority"}),
        ] {
            normalize_fast_service_tier(&mut value, Surface::Responses, true).unwrap();
            assert_eq!(value["service_tier"], "priority");
        }

        for mut value in [
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": "default"}),
            serde_json::json!({"model": "openai/gpt-5.6", "service_tier": null}),
        ] {
            let response =
                normalize_fast_service_tier(&mut value, Surface::Chat, true).unwrap_err();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        for alias in ["fast", "priority"] {
            let mut value = serde_json::json!({
                "model": "openai/gpt-5.6",
                "serviceTier": alias,
                "service_tier": "priority"
            });
            normalize_fast_service_tier(&mut value, Surface::Chat, false).unwrap();
            assert_eq!(value["service_tier"], "priority");
            assert!(value.get("serviceTier").is_none());
        }

        for mut value in [
            serde_json::json!({"model": "openai/gpt-5.6", "serviceTier": "default"}),
            serde_json::json!({"model": "openai/gpt-5.6", "serviceTier": null}),
            serde_json::json!({
                "model": "openai/gpt-5.6",
                "serviceTier": "priority",
                "service_tier": "default"
            }),
        ] {
            let response =
                normalize_fast_service_tier(&mut value, Surface::Responses, false).unwrap_err();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        let mut messages = serde_json::json!({
            "model": "openai/gpt-5.6",
            "speed": "fast"
        });
        normalize_fast_service_tier(&mut messages, Surface::Messages, true).unwrap();
        assert_eq!(messages["service_tier"], "priority");

        messages["speed"] = serde_json::Value::String("standard".to_string());
        assert!(normalize_fast_service_tier(&mut messages, Surface::Messages, true).is_err());

        let mut messages_alias = serde_json::json!({
            "model": "openai/gpt-5.6",
            "serviceTier": "priority"
        });
        assert!(
            normalize_fast_service_tier(&mut messages_alias, Surface::Messages, false).is_err()
        );
    }

    #[test]
    fn body_admission_is_weighted_and_unknown_size_grows_dynamically() {
        let mut headers = HeaderMap::new();
        assert_eq!(declared_body_admission_units(&headers), Ok(None));

        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_static("1"),
        );
        assert_eq!(declared_body_admission_units(&headers), Ok(Some(1)));

        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_static("1048577"),
        );
        assert_eq!(declared_body_admission_units(&headers), Ok(Some(2)));

        headers.insert(
            axum::http::header::CONTENT_LENGTH,
            HeaderValue::from_static("999999999"),
        );
        assert_eq!(
            declared_body_admission_units(&headers),
            Err(BodyReadError::TooLarge)
        );
    }

    #[tokio::test]
    async fn outbound_upload_releases_admission_at_eof() {
        let semaphore = Arc::new(Semaphore::new(1));
        let metrics = Arc::new(RouterMetrics::new());
        let permit = semaphore.clone().try_acquire_owned().unwrap();
        let admission = BodyAdmissionPermit::new(permit, metrics.clone(), 1);
        let body = upload_body(Bytes::from_static(b"payload"), admission);

        assert_eq!(semaphore.available_permits(), 0);
        assert_eq!(
            axum::body::to_bytes(body, 1024).await.unwrap(),
            Bytes::from_static(b"payload")
        );
        assert_eq!(semaphore.available_permits(), 1);
        assert!(metrics
            .render()
            .contains("claude_router_active_body_admission_units 0"));
    }

    #[tokio::test]
    async fn body_read_idle_deadline_releases_admission_and_records_timeout() {
        let metrics = Arc::new(RouterMetrics::new());
        let state = AppState {
            cfg: crate::config::Config {
                host: "127.0.0.1".into(),
                port: 0,
                anthropic_origin: "http://127.0.0.1:1".into(),
                kimi_origin: "http://127.0.0.1:1".into(),
                openai_origin: "http://127.0.0.1:2".into(),
                gemini_origin: "http://127.0.0.1:3".into(),
                fallback_enabled: false,
            },
            client: crate::build_client().unwrap(),
            catalog: crate::catalog::Catalog::new(),
            metrics: metrics.clone(),
            body_admission: Arc::new(Semaphore::new(BODY_ADMISSION_UNITS)),
        };
        let body = Body::from_stream(stream::pending::<Result<Bytes, Infallible>>());
        assert!(matches!(
            read_body_with_timeouts(
                &state,
                &HeaderMap::new(),
                body,
                Duration::from_millis(20),
                Duration::from_secs(1),
            )
            .await,
            Err(BodyReadError::Timeout)
        ));
        assert_eq!(
            state.body_admission.available_permits(),
            BODY_ADMISSION_UNITS
        );
        let rendered = metrics.render();
        assert!(rendered.contains("claude_router_body_read_timeout_total 1"));
        assert!(rendered.contains("claude_router_active_body_admission_units 0"));
    }

    #[tokio::test]
    async fn body_read_progress_refreshes_idle_deadline() {
        let metrics = Arc::new(RouterMetrics::new());
        let state = AppState {
            cfg: crate::config::Config {
                host: "127.0.0.1".into(),
                port: 0,
                anthropic_origin: "http://127.0.0.1:1".into(),
                kimi_origin: "http://127.0.0.1:1".into(),
                openai_origin: "http://127.0.0.1:2".into(),
                gemini_origin: "http://127.0.0.1:3".into(),
                fallback_enabled: false,
            },
            client: crate::build_client().unwrap(),
            catalog: crate::catalog::Catalog::new(),
            metrics: metrics.clone(),
            body_admission: Arc::new(Semaphore::new(BODY_ADMISSION_UNITS)),
        };
        let chunks = stream::unfold(0, |index| async move {
            if index == 3 {
                return None;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
            Some((Ok::<Bytes, Infallible>(Bytes::from_static(b"x")), index + 1))
        });
        let (bytes, permit) = read_body_with_timeouts(
            &state,
            &HeaderMap::new(),
            Body::from_stream(chunks),
            Duration::from_millis(50),
            Duration::from_millis(200),
        )
        .await
        .unwrap();

        assert_eq!(bytes, Bytes::from_static(b"xxx"));
        drop(permit);
        assert_eq!(
            state.body_admission.available_permits(),
            BODY_ADMISSION_UNITS
        );
        assert!(metrics
            .render()
            .contains("claude_router_body_read_timeout_total 0"));
    }
}
