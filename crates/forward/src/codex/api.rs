//! OpenAI-compatible `/v1/responses` and model-discovery HTTP surface.

use super::billing::{begin_admission, AdmissionError};
use super::{
    new_id, CodexGateway, CodexModel, CodexTurnRequest, CodexTurnResult, CodexUsage, HistoryError,
    ProcessError, StoredHistory, TurnUpdate,
};
use crate::proxy::{authorize, with_not_started, Authz, TerminalErrorReason};
use crate::state::AppState;
use crate::validation::{optional_bool as strict_optional_bool, optional_positive_u64};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::Stream;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

pub(super) const OPENAI_BODY_LIMIT: usize = 8 * 1024 * 1024;
pub(super) const MAX_INSTRUCTIONS_BYTES: usize = 1024 * 1024;
const MAX_TOOLS: usize = 128;
const MAX_CLIENT_METADATA_KEYS: usize = 32;
const MAX_CLIENT_METADATA_VALUE_BYTES: usize = 16 * 1024;
const MAX_PROMPT_CACHE_KEY_BYTES: usize = 256;
const MAX_CUSTOM_TOOL_GRAMMAR_BYTES: usize = 256 * 1024;
/// Codex 0.146 exposes client-side deferred tool discovery as a Responses-native `tool_search`
/// tool. The pinned 0.145 upstream client does not know that wire type, so the gateway presents an
/// equivalent private dynamic function to the model and translates calls/results at the boundary.
const TOOL_SEARCH_DYNAMIC_NAME: &str = "__codex_client_tool_search";
/// Serialized-body bytes per input token. Used both by the public `input_tokens` estimate and by
/// the admission reserve so the two never disagree.
const BYTES_PER_TOKEN_ESTIMATE: u64 = 4;
pub(super) const STREAM_FRAME_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
/// Silence-bound interval for data-bearing SSE progress during long provider reasoning stretches.
pub(super) const SSE_HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug)]
pub(super) struct ApiError {
    pub(super) status: StatusCode,
    pub(super) message: String,
    kind: &'static str,
    param: Option<String>,
    code: Option<&'static str>,
    // `retry_after`/`reason` are read by the Messages skin (`skin.rs`, stage 5.1) to rebuild
    // the error in the Anthropic envelope with Retry-After and the audit reason preserved.
    pub(super) retry_after: Option<u64>,
    pub(super) reason: &'static str,
}

impl ApiError {
    pub(super) fn invalid(message: impl Into<String>, param: impl Into<Option<String>>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
            kind: "invalid_request_error",
            param: param.into(),
            code: None,
            retry_after: None,
            reason: "invalid_request",
        }
    }

    pub(super) fn not_found(message: impl Into<String>, param: impl Into<Option<String>>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
            kind: "invalid_request_error",
            param: param.into(),
            code: Some("model_not_found"),
            retry_after: None,
            reason: "resource_not_found",
        }
    }

    pub(super) fn unavailable() -> Self {
        Self::unavailable_for("codex_provider_unavailable")
    }

    fn unavailable_for(reason: &'static str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "The requested model is temporarily unavailable. Please retry.".to_string(),
            kind: "server_error",
            param: None,
            code: Some("service_unavailable"),
            retry_after: Some(2),
            reason,
        }
    }

    #[cfg(test)]
    fn rate_limited() -> Self {
        Self::rate_limited_for(Some(1))
    }

    #[cfg(test)]
    fn rate_limited_for(retry_after: Option<u64>) -> Self {
        Self::rate_limited_for_reason(retry_after, "codex_rate_limit")
    }

    fn rate_limited_for_reason(retry_after: Option<u64>, reason: &'static str) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Rate limit reached. Please retry shortly.".to_string(),
            kind: "rate_limit_error",
            param: None,
            code: Some("rate_limit_exceeded"),
            retry_after,
            reason,
        }
    }

    pub(super) fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "message": self.message,
                "type": self.kind,
                "param": self.param,
                "code": self.code
            }
        });
        let mut response = (self.status, axum::Json(body)).into_response();
        if let Some(seconds) = self.retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.to_string()) {
                response.headers_mut().insert("retry-after", value);
            }
        }
        response
            .extensions_mut()
            .insert(TerminalErrorReason(self.reason));
        // Normal ApiError branches fail before execution and may carry the router fencing proof.
        // Once the external fallback send started, execution is ambiguous even without a public
        // byte, so that one stable reason must return the same sanitized local error without the
        // `not_started` header.
        if self.reason == "claudestore_fallback_failed" {
            response
        } else {
            with_not_started(response)
        }
    }
}

impl From<AdmissionError> for ApiError {
    fn from(value: AdmissionError) -> Self {
        match value {
            AdmissionError::Unauthorized => Self {
                status: StatusCode::UNAUTHORIZED,
                message: "Incorrect API key provided.".to_string(),
                kind: "invalid_request_error",
                param: None,
                code: Some("invalid_api_key"),
                retry_after: None,
                reason: "invalid_key",
            },
            AdmissionError::Unavailable => Self::unavailable_for("codex_admission_unavailable"),
            AdmissionError::LowBalance => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message: "Your account balance is insufficient for this request.".to_string(),
                kind: "insufficient_quota",
                param: None,
                code: Some("insufficient_quota"),
                retry_after: None,
                reason: "billing_limit",
            },
        }
    }
}

impl From<ProcessError> for ApiError {
    fn from(value: ProcessError) -> Self {
        match value {
            ProcessError::ExternalFallbackFailed { local } => {
                let mut error = Self::from(*local);
                error.reason = "claudestore_fallback_failed";
                error
            }
            ProcessError::ContextWindowExceeded => Self {
                status: StatusCode::BAD_REQUEST,
                message: "This model's maximum context length was exceeded.".to_string(),
                kind: "invalid_request_error",
                param: Some("input".to_string()),
                code: Some("context_length_exceeded"),
                retry_after: None,
                reason: "context_window_exceeded",
            },
            ProcessError::UsageLimitExceeded { retry_after } => Self::rate_limited_for_reason(
                retry_after.or(Some(60)),
                "codex_upstream_usage_limit",
            ),
            ProcessError::BadRequest => Self::invalid(
                "The request could not be processed by the selected model.",
                None::<String>,
            ),
            other => {
                eprintln!(
                    "Codex provider request failed [{}]",
                    other.diagnostic_class()
                );
                Self::unavailable()
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(super) struct ParsedResponsesRequest {
    pub(super) public_model: CodexModel,
    pub(super) input: NormalizedInput,
    pub(super) instructions: Option<String>,
    previous_response_id: Option<String>,
    prompt_cache_key: Option<String>,
    /// Normalized upstream/OpenAI request value. `Some("priority")` is Codex Fast mode.
    pub(super) service_tier: Option<String>,
    reasoning_effort: Option<String>,
    reasoning_summary: Option<String>,
    output_schema: Option<Value>,
    verbosity: Option<String>,
    text: Value,
    original_tools: Vec<Value>,
    pub(super) dynamic_tools: Vec<Value>,
    pub(super) tool_choice: Value,
    parallel_tool_calls: bool,
    metadata: Value,
    store: bool,
    include_encrypted_reasoning: bool,
    /// Requested output-token cap (`max_output_tokens` on Responses, `max_tokens`/
    /// `max_completion_tokens` on Chat). Bounds the admission reserve and caps the billed output
    /// so a client is never charged past the ceiling it asked for. `None` means uncapped.
    pub(super) max_output_tokens: Option<u64>,
    pub(super) stream: bool,
}

#[derive(Clone, Debug)]
pub(super) struct NormalizedInput {
    pub(super) canonical_items: Vec<Value>,
    pub(super) prior_items: Vec<Value>,
    pub(super) turn_input: Vec<Value>,
}

pub(super) struct PreparedTurn {
    pub(super) request: ParsedResponsesRequest,
    pub(super) turn: CodexTurnRequest,
    pub(super) full_history_prefix: Vec<Value>,
    pub(super) estimated_input_tokens: u64,
}

pub async fn responses(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let (parts, body) = request.into_parts();
    // Authenticate before buffering or parsing an attacker-controlled body. Besides bounding
    // unauthenticated work, this preserves the normal OpenAI contract that a bad/missing key wins
    // over request-schema or model-discovery errors.
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let raw = match to_bytes(body, OPENAI_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => {
            return ApiError::invalid("Request body exceeds the 8 MiB limit.", None::<String>)
                .into_response()
        }
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return ApiError::invalid("Invalid JSON in request body.", None::<String>)
                .into_response()
        }
    };
    let parsed = match parse_responses_request(&gateway, value) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let tenant_scope = pending.tenant_scope().to_string();
    let prepared = match prepare_turn(&gateway, &tenant_scope, parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let routing = build_turn_routing(&app, &tenant_scope, &parts.headers, &prepared).await;
    let admission = match pending
        .reserve(
            &app,
            &prepared.request.public_model,
            prepared.estimated_input_tokens,
            prepared.request.max_output_tokens,
            gateway.config().reserve_overhead_tokens,
            prepared.request.service_tier.is_some(),
        )
        .await
    {
        Ok(admission) => admission,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let response_id = new_id("resp");
    let created_at = pool::now();

    if prepared.request.stream {
        // Reject before opening the SSE stream if the whole pool is genuinely unavailable, so the client
        // sees a real 429 + Retry-After instead of a 200 that fails mid-stream.
        if let Err(error) = gateway
            .preflight_capacity(&prepared.request.public_model)
            .await
        {
            return ApiError::from(error).into_response();
        }
        if let Err(error) = admission.mark_delivering().await {
            return ApiError::from(error).into_response();
        }
        return stream_responses(
            gateway,
            prepared,
            admission,
            tenant_scope,
            response_id,
            created_at,
            routing,
        )
        .await;
    }

    let result = match gateway.run_turn(prepared.turn.clone(), None, routing).await {
        Ok(result) => result,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if let Err(error) = admission.mark_delivering().await {
        return ApiError::from(error).into_response();
    }
    let response = build_completed_response(&prepared.request, &result, &response_id, created_at);
    persist_history(
        &gateway,
        &tenant_scope,
        &prepared,
        &result,
        &response_id,
        created_at,
    )
    .await;
    admission.settle(
        &prepared.request.public_model,
        &result.usage,
        prepared.request.max_output_tokens,
        result.effective_service_tier.as_deref() == Some("priority"),
    );
    let mut http_response = json_response(StatusCode::OK, response, &response_id);
    insert_extra_headers(&mut http_response, ratelimit_headers(&gateway).await);
    http_response
}

pub async fn models(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("No OpenAI-compatible models are enabled.", None::<String>)
            .into_response();
    };
    if let Err(error) = authorize_models(&app, &headers, &peer).await {
        return error.into_response();
    }
    // Codex 0.146 refreshes the same URL with its backend-native `ModelsResponse` schema, while
    // OpenAI SDKs require the public list envelope below. An empty native overlay is intentional:
    // Codex merges it with the model metadata bundled in that exact CLI build, avoiding schema
    // drift while the configured model id still comes from the customer profile.
    if requests_codex_models_envelope(&headers) {
        return json_response(StatusCode::OK, json!({"models": []}), &new_id("req"));
    }
    // Standard SDK discovery must not block on a live upstream catalog refresh. The health loop
    // updates the last-good intersection in the background; before its first success, the local
    // reviewed/configured catalog keeps the endpoint useful during startup or an upstream outage.
    let available = gateway.cached_model_catalog().await;
    let data = public_model_objects(&gateway, available.as_ref());
    json_response(
        StatusCode::OK,
        json!({"object": "list", "data": data}),
        &new_id("req"),
    )
}

fn requests_codex_models_envelope(headers: &HeaderMap) -> bool {
    ["originator", "user-agent"].iter().any(|name| {
        headers
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.to_ascii_lowercase().starts_with("codex"))
    })
}

fn public_model_objects(
    gateway: &CodexGateway,
    available: Option<&super::CodexModelCatalog>,
) -> Vec<Value> {
    gateway
        .config()
        .models
        .iter()
        .filter(|model| available.map_or(true, |catalog| catalog.models.contains(&model.upstream)))
        .map(|model| {
            model_object(
                model,
                available
                    .and_then(|catalog| catalog.input_token_limits.get(&model.upstream).copied()),
                available
                    .and_then(|catalog| catalog.display_names.get(&model.upstream))
                    .map(String::as_str),
            )
        })
        .collect()
}

pub async fn model(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(model_id): Path<String>,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found(
            format!("The model '{model_id}' does not exist or is unavailable."),
            Some("model".to_string()),
        )
        .into_response();
    };
    if let Err(error) = authorize_models(&app, &headers, &peer).await {
        return error.into_response();
    }
    let Some(model) = gateway.config().model(&model_id) else {
        return ApiError::not_found(
            format!("The model '{model_id}' does not exist or you do not have access to it."),
            Some("model".to_string()),
        )
        .into_response();
    };
    let available = match available_upstream_models(&gateway).await {
        Ok(available) => available,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if !available.models.contains(&model.upstream) {
        return ApiError::not_found(
            format!("The model '{model_id}' does not exist or you do not have access to it."),
            Some("model".to_string()),
        )
        .into_response();
    }
    json_response(
        StatusCode::OK,
        model_object(
            model,
            available.input_token_limits.get(&model.upstream).copied(),
            available
                .display_names
                .get(&model.upstream)
                .map(String::as_str),
        ),
        &new_id("req"),
    )
}

/// Validates the public `resp_*` id shape before any store lookup, so malformed ids get the
/// same 404 as unknown ones without touching storage.
fn valid_response_id(response_id: &str) -> bool {
    response_id.starts_with("resp_")
        && response_id.len() <= 128
        && response_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn response_not_found(response_id: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        axum::Json(json!({
            "error": {
                "message": format!("Response with id '{response_id}' not found."),
                "type": "invalid_request_error",
                "param": Value::Null,
                "code": Value::Null
            }
        })),
    )
        .into_response()
}

fn history_read_error(response_id: &str, error: HistoryError) -> Response {
    match error {
        HistoryError::NotFound | HistoryError::WrongTenant => response_not_found(response_id),
        HistoryError::TooLarge | HistoryError::Corrupt | HistoryError::Unavailable => {
            ApiError::unavailable().into_response()
        }
    }
}

pub async fn get_response(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(response_id): Path<String>,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    // Authentication only; retrieval never reserves or settles money.
    let pending = match begin_admission(&app, &headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if !valid_response_id(&response_id) {
        return response_not_found(&response_id);
    }
    match gateway
        .history()
        .get(pending.tenant_scope(), &response_id)
        .await
    {
        Ok(stored) => match stored.response {
            Some(response) => json_response(StatusCode::OK, response, &response_id),
            // Entries written before retrieval support have no stored response document.
            None => response_not_found(&response_id),
        },
        Err(error) => history_read_error(&response_id, error),
    }
}

pub async fn delete_response(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(response_id): Path<String>,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let pending = match begin_admission(&app, &headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if !valid_response_id(&response_id) {
        return response_not_found(&response_id);
    }
    match gateway
        .history()
        .delete(pending.tenant_scope(), &response_id)
        .await
    {
        Ok(()) => json_response(
            StatusCode::OK,
            json!({
                "id": response_id,
                "object": "response.deleted",
                "deleted": true
            }),
            &response_id,
        ),
        Err(error) => history_read_error(&response_id, error),
    }
}

pub async fn response_input_items(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(response_id): Path<String>,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let pending = match begin_admission(&app, &headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    if !valid_response_id(&response_id) {
        return response_not_found(&response_id);
    }
    match gateway
        .history()
        .get(pending.tenant_scope(), &response_id)
        .await
    {
        Ok(stored) => {
            let input_count = stored.input_count.unwrap_or(stored.items.len());
            let data: Vec<Value> = stored
                .items
                .iter()
                .take(input_count)
                .cloned()
                .map(|mut item| {
                    // Stored items are Responses-shaped already; strip encrypted reasoning state
                    // unless it is meaningless outside the continuity protocol.
                    if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                        item.as_object_mut()
                            .map(|object| object.remove("encrypted_content"));
                    }
                    item
                })
                .collect();
            let first_id = data
                .first()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("item_unknown");
            let last_id = data
                .last()
                .and_then(|item| item.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("item_unknown");
            json_response(
                StatusCode::OK,
                json!({
                    "object": "list",
                    "data": data,
                    "first_id": first_id,
                    "last_id": last_id,
                    "has_more": false
                }),
                &response_id,
            )
        }
        Err(error) => history_read_error(&response_id, error),
    }
}

/// `POST /v1/responses/input_tokens`: estimates the input token count of a Responses-shaped
/// body without running a turn or reserving balance. The estimate is the serialized byte length
/// (including the base64 image placeholder) at ~4 bytes per token — the same figure the billing
/// reserve uses, so the two never disagree.
pub async fn input_tokens(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let Some(gateway) = app.codex.as_ref().cloned() else {
        return ApiError::not_found("The requested endpoint is not enabled.", None::<String>)
            .into_response();
    };
    let (parts, body) = request.into_parts();
    let pending = match begin_admission(&app, &parts.headers, &peer).await {
        Ok(pending) => pending,
        Err(error) => return ApiError::from(error).into_response(),
    };
    let raw = match to_bytes(body, OPENAI_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => {
            return ApiError::invalid("Request body exceeds the 8 MiB limit.", None::<String>)
                .into_response()
        }
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return ApiError::invalid("Invalid JSON in request body.", None::<String>)
                .into_response()
        }
    };
    let parsed = match parse_responses_request(&gateway, value) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let prepared = match prepare_turn(&gateway, pending.tenant_scope(), parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let input_tokens = prepared.estimated_input_tokens;
    json_response(
        StatusCode::OK,
        json!({
            "object": "response.input_tokens",
            "input_tokens": input_tokens
        }),
        &new_id("req"),
    )
}

async fn authorize_models(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<(), ApiError> {
    match authorize(app, headers, peer).await {
        Authz::Admin { .. } | Authz::Metered { .. } => Ok(()),
        Authz::Unauthorized => Err(ApiError::from(AdmissionError::Unauthorized)),
        Authz::Unavailable => Err(ApiError::unavailable()),
    }
}

pub(super) async fn available_upstream_models(
    gateway: &CodexGateway,
) -> Result<super::CodexModelCatalog, ProcessError> {
    gateway.fetch_live_models().await
}

fn model_object(
    model: &CodexModel,
    input_token_limit: Option<u64>,
    display_name: Option<&str>,
) -> Value {
    let mut limits = Map::from_iter([("output".to_string(), Value::from(model.max_output_tokens))]);
    if let Some(input) = input_token_limit {
        if let Some(context) = input.checked_add(model.max_output_tokens) {
            limits.insert("context".to_string(), Value::from(context));
            limits.insert("input".to_string(), Value::from(input));
        }
    }
    let mut service_tiers = vec!["standard"];
    if model.supports_fast() {
        service_tiers.push("priority");
    }
    let mut value = json!({
        "id": model.id,
        "object": "model",
        "created": model.created,
        "owned_by": model.owned_by,
        "apitoken": {
            "limits": limits,
            "capabilities": {
                "reasoning_efforts": model.reasoning_efforts,
                "service_tiers": service_tiers,
                "input_modalities": model.input_modalities,
                "output_modalities": model.output_modalities,
                "tool_calling": model.tool_calling,
                "structured_outputs": model.structured_outputs,
                "streaming": true
            }
        }
    });
    if let Some(display_name) = display_name {
        value["name"] = Value::String(display_name.to_string());
    }
    value
}

/// Build the cache-first routing context for one turn, mirroring the Claude fleet's affinity flow.
///
/// The lineage is derived from the same tenant scope the Claude path uses and from the exact
/// conversation the model will see, projected onto the shared `AffinityStore`. `resolve` returns the
/// home this conversation is already pinned to; when it is new, `warm_homes` surfaces homes that hold
/// the shared system/tools cache root as a soft placement hint. It is a pure optimization: a `None`
/// result (no messages, or the affinity store declining) just falls back to least-loaded selection.
pub(super) async fn build_turn_routing(
    app: &AppState,
    tenant_scope: &str,
    headers: &HeaderMap,
    prepared: &PreparedTurn,
) -> Option<super::TurnRouting> {
    let store = app.affinity.clone();
    let instructions = combined_instructions(&prepared.turn);
    let input = store.infer_codex(
        tenant_scope,
        headers,
        &prepared.request.public_model.id,
        instructions.as_deref(),
        &prepared.turn.dynamic_tools,
        &prepared.full_history_prefix,
        prepared.request.prompt_cache_key.as_deref(),
    )?;
    let resolution = store.resolve(&input).await;
    let warm = if resolution.is_none() {
        store.warm_homes(&input).await
    } else {
        Vec::new()
    };
    Some(super::TurnRouting::new(store, input, resolution, warm))
}

/// The exact instruction text the model sees, combining the base (system/Responses `instructions`)
/// and developer instruction. Kept stable across a conversation so the cache-shape digest is stable.
fn combined_instructions(turn: &super::CodexTurnRequest) -> Option<String> {
    match (
        turn.base_instructions.as_deref(),
        turn.developer_instructions.as_deref(),
    ) {
        (None, None) => None,
        (Some(base), None) => Some(base.to_string()),
        (None, Some(developer)) => Some(developer.to_string()),
        (Some(base), Some(developer)) => Some(format!("{base}\n\n{developer}")),
    }
}

/// Drop reasoning items from a replayed history. Reasoning (and its `encrypted_content`) is
/// model-bound internal state; when a `previous_response_id` chain switches models, replaying it is
/// meaningless and can be rejected by the backend. The remaining message and tool-call items are
/// portable, and a reasoning-free history is structurally identical to any first-turn conversation
/// the backend already accepts.
fn strip_reasoning_items(items: Vec<Value>) -> Vec<Value> {
    items
        .into_iter()
        .filter(|item| item.get("type").and_then(Value::as_str) != Some("reasoning"))
        .collect()
}

pub(super) async fn prepare_turn(
    gateway: &CodexGateway,
    tenant_scope: &str,
    request: ParsedResponsesRequest,
) -> Result<PreparedTurn, ApiError> {
    let mut full_history_prefix = if let Some(previous) = &request.previous_response_id {
        match gateway.history().get(tenant_scope, previous).await {
            Ok(history) => {
                if history.model != request.public_model.id {
                    // The real Responses API permits switching models mid-chain. Reasoning items
                    // (and their encrypted_content) are model-bound internal state, so replaying
                    // them into a different model is meaningless and can be rejected upstream; drop
                    // them and keep the portable message / tool-call items. This can only improve
                    // on the prior hard 400: same-model chains are untouched, and a cross-model
                    // chain now behaves like the real API instead of always failing.
                    strip_reasoning_items(history.items)
                } else {
                    history.items
                }
            }
            Err(HistoryError::NotFound | HistoryError::WrongTenant) => {
                return Err(ApiError::invalid(
                    "previous_response_id was not found.",
                    Some("previous_response_id".to_string()),
                ))
            }
            Err(HistoryError::Unavailable) => return Err(ApiError::unavailable()),
            Err(HistoryError::TooLarge | HistoryError::Corrupt) => {
                return Err(ApiError::unavailable())
            }
        }
    } else {
        Vec::new()
    };
    let mut injected = full_history_prefix.clone();
    injected.extend(request.input.prior_items.clone());
    let injected = injected
        .into_iter()
        .map(tool_search_item_for_upstream)
        .collect::<Vec<_>>();
    let mut history_after_input = full_history_prefix.clone();
    history_after_input.extend(request.input.canonical_items.clone());
    full_history_prefix = history_after_input;

    let mut estimate_value = json!({
        "history": injected,
        "input": request.input.turn_input,
        "tools": request.dynamic_tools,
        "instructions": request.instructions
    });
    // The thread receives the real history; only the billing-reserve estimate below may see the
    // fixed-size image placeholders. Taking `injected_items` from `estimate_value` after
    // `sanitize_estimate_images` would ship `data:image/estimate…` URLs to the backend, which
    // cannot decode them and substitutes its own "image content omitted" placeholder.
    // The thread receives the real history; only the billing-reserve estimate below may see the
    // fixed-size image placeholders. Taking `injected_items` from `estimate_value` after
    // `sanitize_estimate_images` would ship `data:image/estimate…` URLs to the backend, which
    // cannot decode them and substitutes its own "image content omitted" placeholder.
    let injected_items = estimate_value
        .get("history")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    sanitize_estimate_images(&mut estimate_value);
    // Realistic input-token estimate: the serialized byte length at ~4 bytes/token, the same
    // conversion the public `input_tokens` endpoint reports. Reserving raw byte length treated
    // ~4 bytes as one token each, inflating the hold ~4x and false-402'ing low-balance clients on
    // requests they could afford. Settlement always uses exact upstream usage, so this only relaxes
    // admission; the output leg of the reserve (below) is the conservative part of the hold.
    let estimated_input_tokens = serde_json::to_vec(&estimate_value)
        .map(|bytes| (bytes.len() as u64) / BYTES_PER_TOKEN_ESTIMATE)
        .unwrap_or(u64::MAX / 2);
    let turn = CodexTurnRequest {
        model: request.public_model.clone(),
        // Filled from tenant-scoped affinity immediately before home selection. Keeping this
        // internal avoids forwarding a customer-chosen identifier into a shared subscription.
        prompt_cache_key: None,
        // Responses `instructions` is the request-owned base instruction field in the official
        // protocol. Passing it here replaces Codex's model-family prompt instead of adding a
        // gateway-authored developer message.
        base_instructions: request.instructions.clone(),
        developer_instructions: None,
        injected_items,
        turn_input: request.input.turn_input.clone(),
        dynamic_tools: request.dynamic_tools.clone(),
        service_tier: request.service_tier.clone(),
        reasoning_effort: request.reasoning_effort.clone(),
        reasoning_summary: request.reasoning_summary.clone(),
        output_schema: request.output_schema.clone(),
        verbosity: request.verbosity.clone(),
    };
    Ok(PreparedTurn {
        request,
        turn,
        full_history_prefix,
        estimated_input_tokens,
    })
}

pub(super) fn parse_responses_request(
    gateway: &CodexGateway,
    value: Value,
) -> Result<ParsedResponsesRequest, ApiError> {
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("Request body must be a JSON object.", None::<String>))?;
    // SDK compatibility: parameters the transport cannot honor (sampling controls, token caps,
    // truncation, background mode, future fields, …) are accepted and ignored rather than
    // rejected, so stock SDKs and agent terminals never fail on parameters they send by default.
    let requested_model_id = required_string(object, "model")?;
    // Universal router dispatch deliberately preserves the request body byte-for-byte. Each
    // provider plane therefore owns resolution of its public namespace before admission; this is
    // the OpenAI mirror of the Anthropic/Gemini adapters' prefix stripping.
    let model_id = requested_model_id
        .strip_prefix("openai/")
        .unwrap_or(requested_model_id);
    let public_model = gateway.config().model(model_id).cloned().ok_or_else(|| {
        ApiError::not_found(
            format!(
                "The model '{requested_model_id}' does not exist or you do not have access to it."
            ),
            Some("model".to_string()),
        )
    })?;
    let input_value = object.get("input").ok_or_else(|| {
        ApiError::invalid("Missing required parameter: input.", Some("input".into()))
    })?;
    let (input_value, additional_tools) = extract_additional_tools(input_value)?;
    let input = normalize_responses_input(&input_value)?;
    let instructions = optional_string(object, "instructions")?;
    if instructions
        .as_ref()
        .is_some_and(|value| value.len() > MAX_INSTRUCTIONS_BYTES)
    {
        return Err(ApiError::invalid(
            "instructions exceeds the 1 MiB limit.",
            Some("instructions".to_string()),
        ));
    }
    let previous_response_id = optional_string(object, "previous_response_id")?;
    if previous_response_id.as_ref().is_some_and(|id| {
        !id.starts_with("resp_")
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    }) {
        return Err(ApiError::invalid(
            "previous_response_id has an invalid format.",
            Some("previous_response_id".to_string()),
        ));
    }
    let prompt_cache_key = optional_string(object, "prompt_cache_key")?;
    if prompt_cache_key.as_ref().is_some_and(|key| {
        key.is_empty()
            || key.len() > MAX_PROMPT_CACHE_KEY_BYTES
            || key.chars().any(char::is_control)
    }) {
        return Err(ApiError::invalid(
            format!(
                "prompt_cache_key must be 1-{MAX_PROMPT_CACHE_KEY_BYTES} bytes without control characters."
            ),
            Some("prompt_cache_key".to_string()),
        ));
    }
    validate_client_metadata(object.get("client_metadata"))?;
    validate_optional_identifier(object.get("safety_identifier"), "safety_identifier")?;
    let service_tier = parse_service_tier(object.get("service_tier"), &public_model);
    let (reasoning_effort, reasoning_summary) =
        parse_reasoning(object.get("reasoning"), &public_model)?;
    let (text, output_schema, verbosity) = parse_text(object.get("text"))?;
    let tool_choice = object
        .get("tool_choice")
        .cloned()
        .unwrap_or_else(|| Value::String("auto".to_string()));
    let top_level_tools = parse_responses_tools(object.get("tools"))?;
    if additional_tools.is_some() && !top_level_tools.0.is_empty() {
        return Err(ApiError::invalid(
            "Top-level tools cannot be combined with an input additional_tools item.",
            Some("tools".to_string()),
        ));
    }
    let (original_tools, mut dynamic_tools) = additional_tools.unwrap_or(top_level_tools);
    // tool_choice: only "none" changes behavior (tools are hidden). "required" and named-tool
    // choices cannot be forced through the transport, so they degrade to "auto" instead of
    // failing the request.
    let tool_choice = match tool_choice.as_str() {
        Some("none") => {
            dynamic_tools.clear();
            Value::String("none".to_string())
        }
        _ if original_tools.is_empty() => {
            dynamic_tools.clear();
            Value::String("auto".to_string())
        }
        _ => Value::String("auto".to_string()),
    };
    // parallel_tool_calls=false cannot be enforced by the transport; accept and run with the
    // default parallel behavior instead of rejecting the request.
    let parallel_tool_calls = optional_bool(object, "parallel_tool_calls")?.unwrap_or(true);
    // Requested output cap bounds reserve and billed output. A present non-null value is strict:
    // silently treating malformed/zero as absence would let billed generation diverge from the
    // client's requested delivery limit.
    let max_output_tokens =
        optional_positive_u64(object, &["max_output_tokens"]).map_err(|field| {
            ApiError::invalid(
                "max_output_tokens must be a positive integer.",
                Some(field.to_string()),
            )
        })?;
    let metadata = match object.get("metadata") {
        None | Some(Value::Null) => json!({}),
        Some(Value::Object(metadata)) if metadata.len() <= 16 => Value::Object(metadata.clone()),
        Some(Value::Object(_)) => {
            return Err(ApiError::invalid(
                "metadata may contain at most 16 keys.",
                Some("metadata".to_string()),
            ))
        }
        Some(_) => {
            return Err(ApiError::invalid(
                "metadata must be an object.",
                Some("metadata".to_string()),
            ))
        }
    };
    let store = optional_bool(object, "store")?.unwrap_or(true);
    let stream = optional_bool(object, "stream")?.unwrap_or(false);
    let mut include_encrypted_reasoning = false;
    if let Some(include) = object.get("include").filter(|value| !value.is_null()) {
        let values = include.as_array().ok_or_else(|| {
            ApiError::invalid("include must be an array.", Some("include".to_string()))
        })?;
        // Unknown include values are ignored; only encrypted reasoning continuity is honored.
        include_encrypted_reasoning = values
            .iter()
            .any(|value| value.as_str() == Some("reasoning.encrypted_content"));
    }
    // stream_options (e.g. include_obfuscation) cannot be honored; accepted and ignored.
    Ok(ParsedResponsesRequest {
        public_model,
        input,
        instructions,
        previous_response_id,
        prompt_cache_key,
        service_tier,
        reasoning_effort,
        reasoning_summary,
        output_schema,
        verbosity,
        text,
        original_tools,
        dynamic_tools,
        tool_choice,
        parallel_tool_calls,
        metadata,
        store,
        include_encrypted_reasoning,
        max_output_tokens,
        stream,
    })
}

fn reject_unsupported_fields(
    object: &Map<String, Value>,
    supported: &[&str],
) -> Result<(), ApiError> {
    if let Some(field) = object
        .keys()
        .find(|field| !supported.iter().any(|supported| field == supported))
    {
        return Err(ApiError::invalid(
            format!("Parameter {field:?} is not supported by this endpoint."),
            Some(field.clone()),
        ));
    }
    Ok(())
}

fn required_string<'a>(object: &'a Map<String, Value>, field: &str) -> Result<&'a str, ApiError> {
    object.get(field).and_then(Value::as_str).ok_or_else(|| {
        ApiError::invalid(
            format!("Missing or invalid required parameter: {field}."),
            Some(field.to_string()),
        )
    })
}

fn optional_string(object: &Map<String, Value>, field: &str) -> Result<Option<String>, ApiError> {
    match object.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(ApiError::invalid(
            format!("{field} must be a string."),
            Some(field.to_string()),
        )),
    }
}

fn optional_bool(
    object: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<bool>, ApiError> {
    strict_optional_bool(object, field).map_err(|field| {
        ApiError::invalid(
            format!("{field} must be a boolean."),
            Some(field.to_string()),
        )
    })
}

/// Normalize the public Responses/Chat spelling onto the value used by the pinned client.
///
/// Codex exposes the feature to users as `fast`, while the current client and OpenAI Responses
/// wire use `priority`. Unknown tiers remain leniently accepted as standard service, preserving the
/// adapter's compatibility policy for SDK fields the ChatGPT-subscription transport cannot honor.
fn parse_service_tier(value: Option<&Value>, model: &CodexModel) -> Option<String> {
    let requested = value.and_then(Value::as_str)?;
    if model.supports_fast() && matches!(requested, "fast" | "priority") {
        Some("priority".to_string())
    } else {
        None
    }
}

fn validate_client_metadata(value: Option<&Value>) -> Result<(), ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(());
    };
    let metadata = value.as_object().ok_or_else(|| {
        ApiError::invalid(
            "client_metadata must be an object.",
            Some("client_metadata".to_string()),
        )
    })?;
    if metadata.len() > MAX_CLIENT_METADATA_KEYS {
        return Err(ApiError::invalid(
            format!("client_metadata may contain at most {MAX_CLIENT_METADATA_KEYS} keys."),
            Some("client_metadata".to_string()),
        ));
    }
    for (key, value) in metadata {
        if key.is_empty() || key.len() > 128 || key.chars().any(char::is_control) {
            return Err(ApiError::invalid(
                "client_metadata keys must be 1-128 bytes without control characters.",
                Some("client_metadata".to_string()),
            ));
        }
        let Some(value) = value.as_str() else {
            return Err(ApiError::invalid(
                "client_metadata values must be strings.",
                Some(format!("client_metadata.{key}")),
            ));
        };
        if value.len() > MAX_CLIENT_METADATA_VALUE_BYTES || value.chars().any(char::is_control) {
            return Err(ApiError::invalid(
                format!(
                    "client_metadata values must be at most {MAX_CLIENT_METADATA_VALUE_BYTES} bytes without control characters."
                ),
                Some(format!("client_metadata.{key}")),
            ));
        }
    }
    Ok(())
}

fn validate_optional_identifier(value: Option<&Value>, field: &str) -> Result<(), ApiError> {
    match value {
        None | Some(Value::Null) => Ok(()),
        Some(Value::String(value))
            if !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control) =>
        {
            Ok(())
        }
        Some(_) => Err(ApiError::invalid(
            format!("{field} must be null or a 1-256 byte string without control characters."),
            Some(field.to_string()),
        )),
    }
}

fn parse_reasoning(
    value: Option<&Value>,
    model: &CodexModel,
) -> Result<(Option<String>, Option<String>), ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((None, None));
    };
    let object = value.as_object().ok_or_else(|| {
        ApiError::invalid(
            "reasoning must be an object.",
            Some("reasoning".to_string()),
        )
    })?;
    // Unknown reasoning fields (generate_summary, context, future additions) are ignored.
    let effort = optional_string(object, "effort")?;
    // An effort the model does not advertise degrades to the model default instead of failing
    // the request: SDKs pin effort names across providers and must not 400 on a mismatch.
    let effort = effort.filter(|effort| model.supports_effort(effort));
    let summary = optional_string(object, "summary")?;
    let summary = summary
        .filter(|summary| matches!(summary.as_str(), "auto" | "concise" | "detailed" | "none"));
    Ok((effort, summary))
}

fn parse_text(value: Option<&Value>) -> Result<(Value, Option<Value>, Option<String>), ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((json!({"format": {"type": "text"}}), None, None));
    };
    let object = value
        .as_object()
        .ok_or_else(|| ApiError::invalid("text must be an object.", Some("text".to_string())))?;
    // Unknown text fields are ignored for forward compatibility.
    let verbosity = optional_string(object, "verbosity")?;
    if verbosity
        .as_deref()
        .is_some_and(|verbosity| !matches!(verbosity, "low" | "medium" | "high"))
    {
        return Err(ApiError::invalid(
            "text.verbosity must be low, medium, or high.",
            Some("text.verbosity".to_string()),
        ));
    }
    let format = object
        .get("format")
        .cloned()
        .unwrap_or_else(|| json!({"type": "text"}));
    let format_object = format.as_object().ok_or_else(|| {
        ApiError::invalid(
            "text.format must be an object.",
            Some("text.format".to_string()),
        )
    })?;
    let kind = required_string(format_object, "type")?;
    let schema = match kind {
        "text" => None,
        "json_object" => Some(json!({"type": "object", "additionalProperties": true})),
        "json_schema" => Some(format_object.get("schema").cloned().ok_or_else(|| {
            ApiError::invalid(
                "text.format.schema is required for json_schema.",
                Some("text.format.schema".to_string()),
            )
        })?),
        _ => {
            return Err(ApiError::invalid(
                "text.format.type must be text, json_object, or json_schema.",
                Some("text.format.type".to_string()),
            ))
        }
    };
    let mut normalized = object.clone();
    normalized.insert("format".to_string(), format);
    Ok((Value::Object(normalized), schema, verbosity))
}

fn extract_additional_tools(
    input: &Value,
) -> Result<(Value, Option<(Vec<Value>, Vec<Value>)>), ApiError> {
    let Some(items) = input.as_array() else {
        return Ok((input.clone(), None));
    };
    let mut filtered = Vec::with_capacity(items.len());
    let mut parsed_tools = None;
    for (index, item) in items.iter().enumerate() {
        if item.get("type").and_then(Value::as_str) != Some("additional_tools") {
            filtered.push(item.clone());
            continue;
        }
        if parsed_tools.is_some() {
            return Err(ApiError::invalid(
                "input may contain at most one additional_tools item.",
                Some(format!("input.{index}")),
            ));
        }
        let object = item.as_object().ok_or_else(|| {
            ApiError::invalid(
                "additional_tools must be an object.",
                Some(format!("input.{index}")),
            )
        })?;
        reject_unsupported_fields(object, &["type", "role", "tools"])?;
        if object.get("role").and_then(Value::as_str) != Some("developer") {
            return Err(ApiError::invalid(
                "additional_tools.role must be \"developer\".",
                Some(format!("input.{index}.role")),
            ));
        }
        parsed_tools = Some(parse_additional_tools(
            object.get("tools"),
            &format!("input.{index}.tools"),
        )?);
    }
    Ok((Value::Array(filtered), parsed_tools))
}

fn parse_additional_tools(
    value: Option<&Value>,
    param: &str,
) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let tools = value.and_then(Value::as_array).ok_or_else(|| {
        ApiError::invalid(
            "additional_tools.tools must be an array.",
            Some(param.to_string()),
        )
    })?;
    let dynamic = parse_dynamic_tools(tools, param, ToolListSource::Additional)?;
    Ok((tools.clone(), dynamic))
}

#[derive(Clone, Copy)]
enum ToolListSource {
    TopLevel,
    Additional,
}

fn parse_dynamic_tools(
    tools: &[Value],
    param: &str,
    source: ToolListSource,
) -> Result<Vec<Value>, ApiError> {
    let mut dynamic = Vec::with_capacity(tools.len());
    let mut names = HashSet::new();
    let mut namespaces = HashSet::new();
    let mut callable_count = 0usize;
    for (index, tool) in tools.iter().enumerate() {
        let tool_param = format!("{param}.{index}");
        let object = tool.as_object().ok_or_else(|| {
            ApiError::invalid(
                match source {
                    ToolListSource::TopLevel => "Each tool must be an object.",
                    ToolListSource::Additional => "Each additional tool must be an object.",
                },
                Some(tool_param.clone()),
            )
        })?;
        match object.get("type").and_then(Value::as_str) {
            Some("function") => {
                let parsed = match source {
                    ToolListSource::TopLevel => parse_top_level_function(object, &tool_param)?,
                    ToolListSource::Additional => parse_additional_function(object, &tool_param)?,
                };
                if !names.insert(parsed["name"].as_str().unwrap_or_default().to_string()) {
                    return Err(ApiError::invalid(
                        match source {
                            ToolListSource::TopLevel => "Tool names must be unique.",
                            ToolListSource::Additional => "Additional tool names must be unique.",
                        },
                        Some(format!("{tool_param}.name")),
                    ));
                }
                callable_count += 1;
                dynamic.push(parsed);
            }
            Some("custom") => {
                let parsed = parse_additional_custom(object, &tool_param)?;
                if !names.insert(parsed["name"].as_str().unwrap_or_default().to_string()) {
                    return Err(ApiError::invalid(
                        match source {
                            ToolListSource::TopLevel => "Tool names must be unique.",
                            ToolListSource::Additional => "Additional tool names must be unique.",
                        },
                        Some(format!("{tool_param}.name")),
                    ));
                }
                callable_count += 1;
                dynamic.push(parsed);
            }
            Some("namespace") => {
                reject_unsupported_fields(object, &["type", "name", "description", "tools"])?;
                let name = required_string(object, "name")?;
                validate_dynamic_tool_identifier(name, &format!("{tool_param}.name"), 64)?;
                if !namespaces.insert(name.to_string()) {
                    return Err(ApiError::invalid(
                        match source {
                            ToolListSource::TopLevel => "Tool namespace names must be unique.",
                            ToolListSource::Additional => {
                                "Additional tool namespace names must be unique."
                            }
                        },
                        Some(format!("{tool_param}.name")),
                    ));
                }
                let description =
                    optional_tool_description(object, &format!("{tool_param}.description"))?;
                let children = object
                    .get("tools")
                    .and_then(Value::as_array)
                    .filter(|children| !children.is_empty())
                    .ok_or_else(|| {
                        ApiError::invalid(
                            match source {
                                ToolListSource::TopLevel => {
                                    "Tool namespaces require a non-empty tools array."
                                }
                                ToolListSource::Additional => {
                                    "Additional tool namespaces require a non-empty tools array."
                                }
                            },
                            Some(format!("{tool_param}.tools")),
                        )
                    })?;
                let mut child_names = HashSet::new();
                let mut parsed_children = Vec::with_capacity(children.len());
                for (child_index, child) in children.iter().enumerate() {
                    let child_param = format!("{tool_param}.tools.{child_index}");
                    let child_object = child.as_object().ok_or_else(|| {
                        ApiError::invalid(
                            "Each namespaced tool must be an object.",
                            Some(child_param.clone()),
                        )
                    })?;
                    if child_object.get("type").and_then(Value::as_str) != Some("function") {
                        return Err(ApiError::invalid(
                            "Only function tools are supported inside a namespace.",
                            Some(format!("{child_param}.type")),
                        ));
                    }
                    let parsed = parse_additional_function(child_object, &child_param)?;
                    let child_name = parsed["name"].as_str().unwrap_or_default().to_string();
                    if !child_names.insert(child_name) {
                        return Err(ApiError::invalid(
                            "Namespaced tool names must be unique.",
                            Some(format!("{child_param}.name")),
                        ));
                    }
                    callable_count += 1;
                    parsed_children.push(parsed);
                }
                dynamic.push(json!({
                    "type": "namespace",
                    "name": name,
                    "description": description,
                    "tools": parsed_children
                }));
            }
            Some("tool_search") => {
                let parsed = parse_additional_tool_search(object, &tool_param)?;
                if !names.insert(TOOL_SEARCH_DYNAMIC_NAME.to_string()) {
                    return Err(ApiError::invalid(
                        match source {
                            ToolListSource::TopLevel => "Tool names must be unique.",
                            ToolListSource::Additional => "Additional tool names must be unique.",
                        },
                        Some(format!("{tool_param}.type")),
                    ));
                }
                callable_count += 1;
                dynamic.push(parsed);
            }
            _ => return Err(ApiError::invalid(
                match source {
                    ToolListSource::TopLevel => {
                        "Tool type must be function, custom, namespace, or tool_search."
                    }
                    ToolListSource::Additional => {
                        "Additional tool type must be function, custom, namespace, or tool_search."
                    }
                },
                Some(format!("{tool_param}.type")),
            )),
        }
        if callable_count > MAX_TOOLS {
            return Err(ApiError::invalid(
                format!("{param} may contain at most {MAX_TOOLS} callable tools."),
                Some(param.to_string()),
            ));
        }
    }
    Ok(dynamic)
}

fn parse_additional_function(object: &Map<String, Value>, param: &str) -> Result<Value, ApiError> {
    reject_unsupported_fields(
        object,
        &[
            "type",
            "name",
            "description",
            "parameters",
            "strict",
            "defer_loading",
        ],
    )?;
    if object
        .get("strict")
        .is_some_and(|strict| !strict.is_null() && strict.as_bool() != Some(false))
    {
        return Err(ApiError::invalid(
            "strict=true additional tools are not supported.",
            Some(format!("{param}.strict")),
        ));
    }
    let name = required_string(object, "name")?;
    validate_dynamic_tool_identifier(name, &format!("{param}.name"), 128)?;
    let description = optional_tool_description(object, &format!("{param}.description"))?;
    let parameters = object
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    if !parameters.is_object() {
        return Err(ApiError::invalid(
            "Function parameters must be a JSON Schema object.",
            Some(format!("{param}.parameters")),
        ));
    }
    let defer_loading = match object.get("defer_loading") {
        None | Some(Value::Null) => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => {
            return Err(ApiError::invalid(
                "defer_loading must be a boolean.",
                Some(format!("{param}.defer_loading")),
            ))
        }
    };
    Ok(json!({
        "type": "function",
        "name": name,
        "description": description,
        "inputSchema": parameters,
        "deferLoading": defer_loading
    }))
}

fn parse_additional_tool_search(
    object: &Map<String, Value>,
    param: &str,
) -> Result<Value, ApiError> {
    reject_unsupported_fields(object, &["type", "execution", "description", "parameters"])?;
    if object.get("execution").and_then(Value::as_str) != Some("client") {
        return Err(ApiError::invalid(
            "tool_search.execution must be \"client\".",
            Some(format!("{param}.execution")),
        ));
    }
    let description = optional_tool_description(object, &format!("{param}.description"))?;
    let parameters = object
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    if !parameters.is_object() {
        return Err(ApiError::invalid(
            "tool_search parameters must be a JSON Schema object.",
            Some(format!("{param}.parameters")),
        ));
    }
    Ok(json!({
        "type": "function",
        "name": TOOL_SEARCH_DYNAMIC_NAME,
        "description": description,
        "inputSchema": parameters,
        "deferLoading": false
    }))
}

fn parse_additional_custom(object: &Map<String, Value>, param: &str) -> Result<Value, ApiError> {
    reject_unsupported_fields(object, &["type", "name", "description", "format"])?;
    let name = required_string(object, "name")?;
    validate_dynamic_tool_identifier(name, &format!("{param}.name"), 128)?;
    let description = optional_tool_description(object, &format!("{param}.description"))?;
    let format = object
        .get("format")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            ApiError::invalid(
                "Custom tools require a format object.",
                Some(format!("{param}.format")),
            )
        })?;
    reject_unsupported_fields(format, &["type", "syntax", "definition"])?;
    if required_string(format, "type")? != "grammar" {
        return Err(ApiError::invalid(
            "Custom tool format.type must be \"grammar\".",
            Some(format!("{param}.format.type")),
        ));
    }
    if required_string(format, "syntax")? != "lark" {
        return Err(ApiError::invalid(
            "Custom tool format.syntax must be \"lark\".",
            Some(format!("{param}.format.syntax")),
        ));
    }
    let definition = required_string(format, "definition")?;
    if definition.is_empty() || definition.len() > MAX_CUSTOM_TOOL_GRAMMAR_BYTES {
        return Err(ApiError::invalid(
            format!("Custom tool grammar must be 1-{MAX_CUSTOM_TOOL_GRAMMAR_BYTES} bytes."),
            Some(format!("{param}.format.definition")),
        ));
    }
    Ok(json!({
        "type": "custom",
        "name": name,
        "description": description,
        "format": {
            "type": "grammar",
            "syntax": "lark",
            "definition": definition
        }
    }))
}

fn optional_tool_description(object: &Map<String, Value>, param: &str) -> Result<String, ApiError> {
    match object.get("description") {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(description)) => Ok(description.clone()),
        Some(_) => Err(ApiError::invalid(
            "Tool description must be a string.",
            Some(param.to_string()),
        )),
    }
}

fn validate_dynamic_tool_identifier(
    name: &str,
    param: &str,
    max_len: usize,
) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > max_len
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        return Err(ApiError::invalid(
            format!(
                "Dynamic tool identifiers must be 1-{max_len} ASCII letters, digits, underscore, or hyphen."
            ),
            Some(param.to_string()),
        ));
    }
    Ok(())
}

fn parse_responses_tools(value: Option<&Value>) -> Result<(Vec<Value>, Vec<Value>), ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let tools = value
        .as_array()
        .ok_or_else(|| ApiError::invalid("tools must be an array.", Some("tools".to_string())))?;
    if tools.len() > MAX_TOOLS {
        return Err(ApiError::invalid(
            format!("tools may contain at most {MAX_TOOLS} functions."),
            Some("tools".to_string()),
        ));
    }
    let dynamic = parse_dynamic_tools(tools, "tools", ToolListSource::TopLevel)?;
    Ok((tools.clone(), dynamic))
}

fn parse_top_level_function(object: &Map<String, Value>, param: &str) -> Result<Value, ApiError> {
    // Extra tool fields (strict, additionalProperties flags, future additions) are ignored;
    // strict=true silently degrades to non-strict rather than failing the request.
    let name = required_string(object, "name")?;
    validate_tool_name(name, &format!("{param}.name"))?;
    let description = object
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or("");
    let parameters = object
        .get("parameters")
        .cloned()
        .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
    if !parameters.is_object() {
        return Err(ApiError::invalid(
            "Function parameters must be a JSON Schema object.",
            Some(format!("{param}.parameters")),
        ));
    }
    Ok(json!({
        "type": "function",
        "name": name,
        "description": description,
        "inputSchema": parameters,
        "deferLoading": false
    }))
}

fn validate_tool_name(name: &str, param: &str) -> Result<(), ApiError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ApiError::invalid(
            "Function name must be 1-64 ASCII letters, digits, underscore, hyphen, or dot.",
            Some(param.to_string()),
        ));
    }
    Ok(())
}

fn normalize_responses_input(value: &Value) -> Result<NormalizedInput, ApiError> {
    if let Some(text) = value.as_str() {
        let message = canonical_message("user", text);
        return Ok(NormalizedInput {
            canonical_items: vec![message],
            prior_items: Vec::new(),
            turn_input: vec![json!({"type": "text", "text": text})],
        });
    }
    let items = value.as_array().ok_or_else(|| {
        ApiError::invalid(
            "input must be a string or an array of Responses items.",
            Some("input".to_string()),
        )
    })?;
    if items.is_empty() {
        return Err(ApiError::invalid(
            "input must not be empty.",
            Some("input".to_string()),
        ));
    }
    let mut canonical = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        canonical.push(normalize_response_item(item, index)?);
    }
    let mut prior = canonical.clone();
    let turn_input = if prior
        .last()
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        == Some("message")
        && prior
            .last()
            .and_then(|item| item.get("role"))
            .and_then(Value::as_str)
            == Some("user")
    {
        let message = prior.pop().expect("last item exists");
        user_inputs_from_message(&message)?
    } else {
        Vec::new()
    };
    Ok(NormalizedInput {
        canonical_items: canonical,
        prior_items: prior,
        turn_input,
    })
}

fn normalize_response_item(item: &Value, index: usize) -> Result<Value, ApiError> {
    let object = item.as_object().ok_or_else(|| {
        ApiError::invalid(
            "Each input item must be an object.",
            Some(format!("input.{index}")),
        )
    })?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .or_else(|| {
            object
                .get("role")
                .and_then(Value::as_str)
                .map(|_| "message")
        })
        .ok_or_else(|| {
            ApiError::invalid(
                "Each input item requires type.",
                Some(format!("input.{index}.type")),
            )
        })?;
    match kind {
        "message" => normalize_message_item(object, index),
        "reasoning"
        | "function_call"
        | "function_call_output"
        | "custom_tool_call"
        | "custom_tool_call_output" => Ok(item.clone()),
        "tool_search_call" => normalize_tool_search_call(object, index),
        "tool_search_output" => normalize_tool_search_output(object, index),
        _ => Err(ApiError::invalid(
            format!("Input item type {kind:?} is not supported by this endpoint."),
            Some(format!("input.{index}.type")),
        )),
    }
}

fn normalize_tool_search_call(
    object: &Map<String, Value>,
    index: usize,
) -> Result<Value, ApiError> {
    let call_id = object
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|call_id| !call_id.is_empty())
        .ok_or_else(|| {
            ApiError::invalid(
                "tool_search_call.call_id must be a non-empty string.",
                Some(format!("input.{index}.call_id")),
            )
        })?;
    if object.get("execution").and_then(Value::as_str) != Some("client") {
        return Err(ApiError::invalid(
            "tool_search_call.execution must be \"client\".",
            Some(format!("input.{index}.execution")),
        ));
    }
    let arguments = object.get("arguments").cloned().ok_or_else(|| {
        ApiError::invalid(
            "tool_search_call.arguments is required.",
            Some(format!("input.{index}.arguments")),
        )
    })?;
    let mut normalized = json!({
        "type": "tool_search_call",
        "status": object.get("status").and_then(Value::as_str).unwrap_or("completed"),
        "call_id": call_id,
        "execution": "client",
        "arguments": arguments
    });
    if let Some(id) = object.get("id").and_then(Value::as_str) {
        normalized["id"] = Value::String(id.to_string());
    }
    Ok(normalized)
}

fn normalize_tool_search_output(
    object: &Map<String, Value>,
    index: usize,
) -> Result<Value, ApiError> {
    let call_id = object
        .get("call_id")
        .and_then(Value::as_str)
        .filter(|call_id| !call_id.is_empty())
        .ok_or_else(|| {
            ApiError::invalid(
                "tool_search_output.call_id must be a non-empty string.",
                Some(format!("input.{index}.call_id")),
            )
        })?;
    if object.get("execution").and_then(Value::as_str) != Some("client") {
        return Err(ApiError::invalid(
            "tool_search_output.execution must be \"client\".",
            Some(format!("input.{index}.execution")),
        ));
    }
    let tools = object
        .get("tools")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::invalid(
                "tool_search_output.tools must be an array.",
                Some(format!("input.{index}.tools")),
            )
        })?
        .clone();
    let mut normalized = json!({
        "type": "tool_search_output",
        "status": object.get("status").and_then(Value::as_str).unwrap_or("completed"),
        "call_id": call_id,
        "execution": "client",
        "tools": tools
    });
    if let Some(id) = object.get("id").and_then(Value::as_str) {
        normalized["id"] = Value::String(id.to_string());
    }
    Ok(normalized)
}

fn tool_search_item_for_upstream(item: Value) -> Value {
    match item.get("type").and_then(Value::as_str) {
        Some("tool_search_call") => {
            let arguments = item
                .get("arguments")
                .and_then(|arguments| serde_json::to_string(arguments).ok())
                .unwrap_or_else(|| "{}".to_string());
            let mut translated = json!({
                "type": "function_call",
                "name": TOOL_SEARCH_DYNAMIC_NAME,
                "call_id": item.get("call_id").cloned().unwrap_or(Value::Null),
                "arguments": arguments
            });
            if let Some(id) = item.get("id").cloned() {
                translated["id"] = id;
            }
            translated
        }
        Some("tool_search_output") => {
            let output = serde_json::to_string(&json!({
                "status": item.get("status").and_then(Value::as_str).unwrap_or("completed"),
                "execution": "client",
                "tools": item.get("tools").cloned().unwrap_or_else(|| json!([]))
            }))
            .unwrap_or_else(|_| {
                r#"{"status":"completed","execution":"client","tools":[]}"#.to_string()
            });
            let mut translated = json!({
                "type": "function_call_output",
                "call_id": item.get("call_id").cloned().unwrap_or(Value::Null),
                "output": output
            });
            if let Some(id) = item.get("id").cloned() {
                translated["id"] = id;
            }
            translated
        }
        _ => item,
    }
}

fn normalize_message_item(object: &Map<String, Value>, index: usize) -> Result<Value, ApiError> {
    let role = required_string(object, "role")?;
    if !matches!(role, "user" | "assistant" | "system" | "developer") {
        return Err(ApiError::invalid(
            "Message role must be user, assistant, system, or developer.",
            Some(format!("input.{index}.role")),
        ));
    }
    // The public Responses surface accepts a system role, while the Codex Responses backend
    // accepts developer messages in model-visible history. Preserve the instruction rather than
    // forwarding a backend-invalid role. Top-level `instructions` remains the preferred exact
    // developer-instruction channel.
    let upstream_role = if role == "system" { "developer" } else { role };
    let content = object.get("content").ok_or_else(|| {
        ApiError::invalid(
            "Message content is required.",
            Some(format!("input.{index}.content")),
        )
    })?;
    let expected_type = if upstream_role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    let normalized_content = if let Some(text) = content.as_str() {
        vec![json!({"type": expected_type, "text": text})]
    } else {
        let values = content.as_array().ok_or_else(|| {
            ApiError::invalid(
                "Message content must be a string or an array.",
                Some(format!("input.{index}.content")),
            )
        })?;
        let mut normalized = Vec::with_capacity(values.len());
        for (content_index, part) in values.iter().enumerate() {
            let part_object = part.as_object().ok_or_else(|| {
                ApiError::invalid(
                    "Message content parts must be objects.",
                    Some(format!("input.{index}.content.{content_index}")),
                )
            })?;
            let part_param = format!("input.{index}.content.{content_index}");
            match part_object.get("type").and_then(Value::as_str) {
                Some("input_text" | "output_text" | "text") => {
                    let text =
                        part_object
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or_else(|| {
                                ApiError::invalid(
                                    "Text content requires text.",
                                    Some(format!("{part_param}.text")),
                                )
                            })?;
                    normalized.push(json!({"type": expected_type, "text": text}));
                }
                Some("input_image" | "image_url") if upstream_role != "assistant" => {
                    normalized.push(canonical_image_part(
                        part_object
                            .get("image_url")
                            .or_else(|| part_object.get("url"))
                            .ok_or_else(|| {
                                ApiError::invalid(
                                    "Image content part requires image_url.",
                                    Some(format!("{part_param}.image_url")),
                                )
                            })?,
                        part_object.get("detail"),
                        &part_param,
                    )?);
                }
                // Assistant image history cannot exist on the public surface; drop it rather
                // than failing a replayed conversation.
                Some("input_image" | "image_url") => {}
                Some(other) => {
                    return Err(ApiError::invalid(
                        format!("Content part type {other:?} is not supported."),
                        Some(format!("{part_param}.type")),
                    ));
                }
                None => {
                    return Err(ApiError::invalid(
                        "Message content parts require a type.",
                        Some(part_param),
                    ));
                }
            }
        }
        normalized
    };
    let mut message = json!({
        "type": "message",
        "role": upstream_role,
        "content": normalized_content
    });
    if let Some(id) = object.get("id").filter(|id| id.is_string()) {
        message["id"] = id.clone();
    }
    Ok(message)
}

/// Replaces inline `data:` image payloads with a fixed-size placeholder so the byte-length
/// input estimate used for the billing reserve reflects a typical image token cost instead of
/// the raw base64 size (which would over-reserve by orders of magnitude).
fn sanitize_estimate_images(value: &mut Value) {
    const IMAGE_ESTIMATE_PLACEHOLDER: &str = "data:image/estimate";
    match value {
        Value::String(text) if text.starts_with("data:") && text.len() > 1024 => {
            *text = IMAGE_ESTIMATE_PLACEHOLDER.repeat(128);
        }
        Value::Array(items) => items.iter_mut().for_each(sanitize_estimate_images),
        Value::Object(object) => object.values_mut().for_each(sanitize_estimate_images),
        _ => {}
    }
}

/// Builds a canonical Responses `input_image` content part from either Chat Completions
/// (`{"url": …, "detail": …}` or a bare string) or Responses (`"https://…"` / `"data:…"`
/// string) image references. Only transports the model can actually receive are accepted:
/// inline `data:` URLs and remote `http(s)://` URLs. `detail` is passed through when it is one
/// of the recognized values and dropped otherwise.
pub(super) fn canonical_image_part(
    reference: &Value,
    detail: Option<&Value>,
    param: &str,
) -> Result<Value, ApiError> {
    let (url, nested_detail) = match reference {
        Value::String(url) => (url.clone(), None),
        Value::Object(object) => {
            let url = object.get("url").and_then(Value::as_str).ok_or_else(|| {
                ApiError::invalid(
                    "image_url requires a url string.",
                    Some(format!("{param}.image_url.url")),
                )
            })?;
            (url.to_string(), object.get("detail"))
        }
        _ => {
            return Err(ApiError::invalid(
                "image_url must be a string or an object with a url.",
                Some(format!("{param}.image_url")),
            ))
        }
    };
    if !(url.starts_with("data:image/")
        || url.starts_with("https://")
        || url.starts_with("http://"))
    {
        return Err(ApiError::invalid(
            "image_url must be a data:image/… URL or an http(s):// URL.",
            Some(format!("{param}.image_url")),
        ));
    }
    let detail = detail
        .or(nested_detail)
        .and_then(Value::as_str)
        .filter(|detail| matches!(*detail, "auto" | "low" | "high" | "original"));
    let mut part = json!({"type": "input_image", "image_url": url});
    if let Some(detail) = detail {
        part["detail"] = Value::String(detail.to_string());
    }
    Ok(part)
}

fn canonical_message(role: &str, text: &str) -> Value {
    json!({
        "type": "message",
        "role": role,
        "content": [{"type": if role == "assistant" {"output_text"} else {"input_text"}, "text": text}]
    })
}

fn user_inputs_from_message(message: &Value) -> Result<Vec<Value>, ApiError> {
    let content = message
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::invalid("Invalid user message content.", Some("input".into())))?;
    Ok(content
        .iter()
        .filter_map(|part| match part.get("type").and_then(Value::as_str) {
            Some("input_image") => {
                let mut input = json!({
                    "type": "image",
                    "url": part.get("image_url").cloned().unwrap_or(Value::Null)
                });
                if let Some(detail) = part.get("detail").and_then(Value::as_str) {
                    input["detail"] = Value::String(detail.to_string());
                }
                Some(input)
            }
            _ => part
                .get("text")
                .and_then(Value::as_str)
                .map(|text| json!({"type": "text", "text": text})),
        })
        .collect())
}

fn build_completed_response(
    request: &ParsedResponsesRequest,
    result: &CodexTurnResult,
    response_id: &str,
    created_at: i64,
) -> Value {
    let output = result
        .output
        .iter()
        .filter_map(|item| {
            normalize_output_item_with_options(item, request.include_encrypted_reasoning)
        })
        .collect::<Vec<_>>();
    let mut response = response_object(
        request,
        response_id,
        created_at,
        "completed",
        output,
        Some(&result.usage),
    );
    // Publish the gateway's effective product tier. The private ChatGPT backend's completed
    // `service_tier` is retained separately for diagnostics because it commonly says `default`
    // while an accepted priority request is measurably served at the documented Fast cadence.
    response["service_tier"] = Value::String(
        result
            .effective_service_tier
            .as_deref()
            .unwrap_or("default")
            .to_string(),
    );
    response
}

fn response_object(
    request: &ParsedResponsesRequest,
    response_id: &str,
    created_at: i64,
    status: &str,
    output: Vec<Value>,
    usage: Option<&CodexUsage>,
) -> Value {
    json!({
        "id": response_id,
        "object": "response",
        "created_at": created_at,
        "status": status,
        "background": false,
        "completed_at": if status == "completed" { Some(pool::now()) } else { None },
        "conversation": Value::Null,
        "error": Value::Null,
        "incomplete_details": Value::Null,
        "instructions": request.instructions,
        "max_output_tokens": request.max_output_tokens,
        "max_tool_calls": Value::Null,
        "metadata": request.metadata,
        "model": request.public_model.id,
        "output": output,
        "parallel_tool_calls": request.parallel_tool_calls,
        "previous_response_id": request.previous_response_id,
        "prompt_cache_key": request.prompt_cache_key,
        "reasoning": {
            "effort": request.reasoning_effort,
            "summary": request.reasoning_summary
        },
        "safety_identifier": Value::Null,
        "service_tier": request.service_tier.as_deref().unwrap_or("default"),
        "store": request.store,
        "temperature": Value::Null,
        "text": request.text,
        "tool_choice": request.tool_choice,
        "tools": request.original_tools,
        "top_logprobs": 0,
        "top_p": Value::Null,
        "truncation": "disabled",
        "usage": usage.map(public_usage)
    })
}

fn public_usage(usage: &CodexUsage) -> Value {
    json!({
        "input_tokens": usage.input_tokens,
        "input_tokens_details": {
            "cache_write_tokens": usage.cache_write_input_tokens,
            "cached_tokens": usage.cached_input_tokens
        },
        "output_tokens": usage.output_tokens,
        "output_tokens_details": {
            "reasoning_tokens": usage.reasoning_output_tokens
        },
        "total_tokens": usage.total_tokens
    })
}

pub(super) fn normalize_output_item(item: &Value) -> Option<Value> {
    normalize_output_item_with_options(item, false)
}

fn normalize_output_item_with_options(
    item: &Value,
    include_encrypted_reasoning: bool,
) -> Option<Value> {
    match item.get("type").and_then(Value::as_str)? {
        "message" => {
            // Experimental raw events may include input/developer message items alongside the
            // actual model output. They are not Responses output items and must never be relabeled
            // as empty assistant messages.
            if item.get("role").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| new_id("msg"));
            let content = item
                .get("content")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) != Some("output_text") {
                        return None;
                    }
                    let text = part.get("text").and_then(Value::as_str)?;
                    if text.is_empty() {
                        return None;
                    }
                    Some(json!({
                        "type": "output_text",
                        "text": text,
                        "annotations": [],
                        "logprobs": []
                    }))
                })
                .collect::<Vec<_>>();
            if content.is_empty() {
                return None;
            }
            Some(json!({
                "id": id,
                "type": "message",
                "status": "completed",
                "role": "assistant",
                "content": content
            }))
        }
        "function_call" => {
            let arguments = item
                .get("arguments")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    item.get("arguments")
                        .and_then(|value| serde_json::to_string(value).ok())
                })
                .unwrap_or_else(|| "{}".to_string());
            if item.get("name").and_then(Value::as_str) == Some(TOOL_SEARCH_DYNAMIC_NAME) {
                let arguments =
                    serde_json::from_str::<Value>(&arguments).unwrap_or_else(|_| json!({}));
                return Some(json!({
                    "id": item.get("id").and_then(Value::as_str)
                        .map(str::to_string).unwrap_or_else(|| new_id("tsc")),
                    "type": "tool_search_call",
                    "status": "completed",
                    "call_id": item.get("call_id").and_then(Value::as_str)
                        .map(str::to_string).unwrap_or_else(|| new_id("call")),
                    "execution": "client",
                    "arguments": arguments
                }));
            }
            let mut output = json!({
                "id": item.get("id").and_then(Value::as_str)
                    .map(str::to_string).unwrap_or_else(|| new_id("fc")),
                "type": "function_call",
                "status": "completed",
                "call_id": item.get("call_id").and_then(Value::as_str)
                    .map(str::to_string).unwrap_or_else(|| new_id("call")),
                "name": item.get("name").and_then(Value::as_str).unwrap_or("unknown"),
                "arguments": arguments
            });
            if let Some(namespace) = item.get("namespace").and_then(Value::as_str) {
                output["namespace"] = Value::String(namespace.to_string());
            }
            Some(output)
        }
        "custom_tool_call" => {
            let mut output = json!({
                "id": item.get("id").and_then(Value::as_str)
                    .map(str::to_string).unwrap_or_else(|| new_id("ctc")),
                "type": "custom_tool_call",
                "status": "completed",
                "call_id": item.get("call_id").and_then(Value::as_str)
                    .map(str::to_string).unwrap_or_else(|| new_id("call")),
                "name": item.get("name").and_then(Value::as_str).unwrap_or("unknown"),
                "input": item.get("input").and_then(Value::as_str).unwrap_or("")
            });
            if let Some(namespace) = item.get("namespace").and_then(Value::as_str) {
                output["namespace"] = Value::String(namespace.to_string());
            }
            Some(output)
        }
        "reasoning" => {
            let summary = item
                .get("summary")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|part| {
                    if part.get("type").and_then(Value::as_str) != Some("summary_text") {
                        return None;
                    }
                    let text = part.get("text").and_then(Value::as_str)?;
                    Some(json!({"type": "summary_text", "text": text}))
                })
                .collect::<Vec<_>>();
            let mut output = json!({
                "id": item.get("id").and_then(Value::as_str)
                    .map(str::to_string).unwrap_or_else(|| new_id("rs")),
                "type": "reasoning",
                "status": "completed",
                "summary": summary
            });
            if include_encrypted_reasoning {
                if let Some(encrypted) = item.get("encrypted_content").and_then(Value::as_str) {
                    output["encrypted_content"] = Value::String(encrypted.to_string());
                }
            }
            Some(output)
        }
        _ => None,
    }
}

async fn persist_history(
    gateway: &CodexGateway,
    tenant_scope: &str,
    prepared: &PreparedTurn,
    result: &CodexTurnResult,
    response_id: &str,
    created_at: i64,
) {
    if !prepared.request.store {
        return;
    }
    let input_count = prepared.full_history_prefix.len();
    let mut items = prepared.full_history_prefix.clone();
    items.extend(result.output.clone());
    let response = build_completed_response(&prepared.request, result, response_id, created_at);
    if let Err(error) = gateway
        .history()
        .put(
            tenant_scope,
            StoredHistory {
                response_id: response_id.to_string(),
                model: prepared.request.public_model.id.clone(),
                items,
                created_at,
                input_count: Some(input_count),
                response: Some(response),
            },
        )
        .await
    {
        eprintln!("Codex response history persistence degraded: {error}");
    }
}

async fn stream_responses(
    gateway: Arc<CodexGateway>,
    prepared: PreparedTurn,
    admission: super::billing::CodexAdmission,
    tenant_scope: String,
    response_id: String,
    created_at: i64,
    routing: Option<super::TurnRouting>,
) -> Response {
    let task_permit = match gateway.track_background_task() {
        Ok(permit) => permit,
        Err(error) => return ApiError::from(error).into_response(),
    };
    // Snapshot the rate-limit window before the body starts. Streaming headers must precede the
    // SSE frames, and the real API sends x-ratelimit-* on streaming responses too.
    let ratelimit = ratelimit_headers(&gateway).await;
    let (frame_tx, frame_rx) = mpsc::channel::<Bytes>(128);
    let request_id_header = response_id.clone();
    tokio::spawn(async move {
        let _task_permit = task_permit;
        // Rebind after the permit so early returns drop billing admission before the shutdown permit.
        let admission = admission;
        let mut sequence = 0u64;
        let in_progress = response_object(
            &prepared.request,
            &response_id,
            created_at,
            "in_progress",
            Vec::new(),
            None,
        );
        if !send_sse(
            &frame_tx,
            "response.created",
            json!({
                "type": "response.created",
                "sequence_number": sequence,
                "response": in_progress.clone()
            }),
        )
        .await
        {
            return;
        }
        sequence += 1;
        if !send_sse(
            &frame_tx,
            "response.in_progress",
            json!({
                "type": "response.in_progress",
                "sequence_number": sequence,
                "response": in_progress.clone()
            }),
        )
        .await
        {
            return;
        }
        sequence += 1;

        let (update_tx, mut update_rx) = mpsc::channel(512);
        let run_gateway = gateway.clone();
        let turn = prepared.turn.clone();
        let run =
            tokio::spawn(async move { run_gateway.run_turn(turn, Some(update_tx), routing).await });
        let mut text_states = HashMap::<String, StreamTextState>::new();
        let mut reasoning_states = HashMap::<String, StreamReasoningState>::new();
        let mut next_output_index = 0usize;
        let mut emitted_non_messages = HashSet::<String>::new();
        let mut downstream_closed = false;
        let mut heartbeat = tokio::time::interval(SSE_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;

        'updates: loop {
            let update = tokio::select! {
                _ = frame_tx.closed() => {
                    downstream_closed = true;
                    break;
                }
                _ = heartbeat.tick() => {
                    // Codex times out decoded events, not raw bytes, so comments are insufficient.
                    if !send_sse(
                        &frame_tx,
                        "response.in_progress",
                        json!({
                            "type": "response.in_progress",
                            "sequence_number": sequence,
                            "response": in_progress.clone()
                        }),
                    )
                    .await
                    {
                        downstream_closed = true;
                        break;
                    }
                    sequence += 1;
                    continue;
                }
                update = update_rx.recv() => update,
            };
            let Some(update) = update else {
                break;
            };
            match update {
                TurnUpdate::TextDelta { item_id, delta } => {
                    let state = text_states.entry(item_id.clone()).or_insert_with(|| {
                        let state = StreamTextState {
                            output_index: next_output_index,
                            text: String::new(),
                        };
                        next_output_index += 1;
                        state
                    });
                    if state.text.is_empty() {
                        let item = json!({
                            "id": item_id,
                            "type": "message",
                            "status": "in_progress",
                            "role": "assistant",
                            "content": []
                        });
                        if !send_sse(
                            &frame_tx,
                            "response.output_item.added",
                            json!({
                                "type": "response.output_item.added",
                                "sequence_number": sequence,
                                "output_index": state.output_index,
                                "item": item
                            }),
                        )
                        .await
                        {
                            downstream_closed = true;
                            break 'updates;
                        }
                        sequence += 1;
                        if !send_sse(
                            &frame_tx,
                            "response.content_part.added",
                            json!({
                                "type": "response.content_part.added",
                                "sequence_number": sequence,
                                "item_id": item_id,
                                "output_index": state.output_index,
                                "content_index": 0,
                                "part": {"type": "output_text", "text": "", "annotations": [], "logprobs": []}
                            }),
                        )
                        .await
                        {
                            downstream_closed = true;
                            break 'updates;
                        }
                        sequence += 1;
                    }
                    state.text.push_str(&delta);
                    if !send_sse(
                        &frame_tx,
                        "response.output_text.delta",
                        json!({
                            "type": "response.output_text.delta",
                            "sequence_number": sequence,
                            "item_id": item_id,
                            "output_index": state.output_index,
                            "content_index": 0,
                            "delta": delta,
                            "logprobs": []
                        }),
                    )
                    .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                    sequence += 1;
                }
                TurnUpdate::ReasoningSummaryPartAdded {
                    item_id,
                    summary_index,
                } => {
                    if emitted_non_messages.contains(&item_id) {
                        continue;
                    }
                    let created = !reasoning_states.contains_key(&item_id);
                    if created {
                        reasoning_states.insert(
                            item_id.clone(),
                            StreamReasoningState {
                                output_index: next_output_index,
                                parts: BTreeMap::new(),
                            },
                        );
                        next_output_index += 1;
                    }
                    let state = reasoning_states.get_mut(&item_id).unwrap();
                    if created
                        && !emit_reasoning_item_added(
                            &frame_tx,
                            &mut sequence,
                            state.output_index,
                            &item_id,
                        )
                        .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                    let added_part = match state.parts.entry(summary_index) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(String::new());
                            true
                        }
                        std::collections::btree_map::Entry::Occupied(_) => false,
                    };
                    if added_part
                        && !emit_reasoning_summary_part_added(
                            &frame_tx,
                            &mut sequence,
                            state.output_index,
                            &item_id,
                            summary_index,
                        )
                        .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                }
                TurnUpdate::ReasoningSummaryDelta {
                    item_id,
                    summary_index,
                    delta,
                } => {
                    if emitted_non_messages.contains(&item_id) {
                        continue;
                    }
                    let created = !reasoning_states.contains_key(&item_id);
                    if created {
                        reasoning_states.insert(
                            item_id.clone(),
                            StreamReasoningState {
                                output_index: next_output_index,
                                parts: BTreeMap::new(),
                            },
                        );
                        next_output_index += 1;
                    }
                    let state = reasoning_states.get_mut(&item_id).unwrap();
                    if created
                        && !emit_reasoning_item_added(
                            &frame_tx,
                            &mut sequence,
                            state.output_index,
                            &item_id,
                        )
                        .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                    let added_part = match state.parts.entry(summary_index) {
                        std::collections::btree_map::Entry::Vacant(entry) => {
                            entry.insert(String::new());
                            true
                        }
                        std::collections::btree_map::Entry::Occupied(_) => false,
                    };
                    if added_part
                        && !emit_reasoning_summary_part_added(
                            &frame_tx,
                            &mut sequence,
                            state.output_index,
                            &item_id,
                            summary_index,
                        )
                        .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                    state
                        .parts
                        .get_mut(&summary_index)
                        .unwrap()
                        .push_str(&delta);
                    if !send_sse(
                        &frame_tx,
                        "response.reasoning_summary_text.delta",
                        json!({
                            "type": "response.reasoning_summary_text.delta",
                            "sequence_number": sequence,
                            "item_id": item_id,
                            "output_index": state.output_index,
                            "summary_index": summary_index,
                            "delta": delta
                        }),
                    )
                    .await
                    {
                        downstream_closed = true;
                        break 'updates;
                    }
                    sequence += 1;
                }
                TurnUpdate::RawItem(item)
                    if item.get("type").and_then(Value::as_str) == Some("reasoning") =>
                {
                    let key = output_item_key(&item);
                    if emitted_non_messages.insert(key) {
                        if let Some(normalized) = normalize_output_item_with_options(
                            &item,
                            prepared.request.include_encrypted_reasoning,
                        ) {
                            let item_id = normalized
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            if let Some(state) = reasoning_states.remove(&item_id) {
                                if !emit_completed_reasoning_item(
                                    &frame_tx,
                                    &mut sequence,
                                    &item_id,
                                    &state,
                                    &normalized,
                                )
                                .await
                                {
                                    downstream_closed = true;
                                    break 'updates;
                                }
                            } else {
                                if !emit_completed_item(
                                    &frame_tx,
                                    &mut sequence,
                                    next_output_index,
                                    &normalized,
                                )
                                .await
                                {
                                    downstream_closed = true;
                                    break 'updates;
                                }
                                next_output_index += 1;
                            }
                        }
                    }
                }
                TurnUpdate::RawItem(item)
                    if item.get("type").and_then(Value::as_str) != Some("message") =>
                {
                    let key = output_item_key(&item);
                    if emitted_non_messages.insert(key) {
                        if let Some(normalized) = normalize_output_item_with_options(
                            &item,
                            prepared.request.include_encrypted_reasoning,
                        ) {
                            if !emit_completed_item(
                                &frame_tx,
                                &mut sequence,
                                next_output_index,
                                &normalized,
                            )
                            .await
                            {
                                downstream_closed = true;
                                break 'updates;
                            }
                            next_output_index += 1;
                        }
                    }
                }
                TurnUpdate::RawItem(_) => {}
            }
        }
        if downstream_closed || frame_tx.is_closed() {
            // Match the existing Claude tee-meter invariant: once upstream sampling started, a
            // downstream disconnect must not turn already-consumed provider tokens into a free
            // partial response. Stop buffering updates, drain to authoritative usage under the
            // normal turn timeout, then persist and settle below.
            drop(update_rx);
        }

        let result = match run.await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => {
                let api_error = ApiError::from(error);
                if !downstream_closed {
                    emit_stream_failure(
                        &frame_tx,
                        &prepared,
                        &response_id,
                        created_at,
                        sequence,
                        api_error.code,
                        &api_error.message,
                    )
                    .await;
                }
                return;
            }
            Err(_) => {
                if !downstream_closed {
                    emit_stream_failure(
                        &frame_tx,
                        &prepared,
                        &response_id,
                        created_at,
                        sequence,
                        Some("server_error"),
                        "The model stream terminated unexpectedly.",
                    )
                    .await;
                }
                return;
            }
        };

        // Settlement and optional history persistence are independent of downstream delivery. Do
        // them as soon as authoritative usage arrives, before emitting the terminal SSE lifecycle.
        persist_history(
            &gateway,
            &tenant_scope,
            &prepared,
            &result,
            &response_id,
            created_at,
        )
        .await;
        admission.settle(
            &prepared.request.public_model,
            &result.usage,
            prepared.request.max_output_tokens,
            result.effective_service_tier.as_deref() == Some("priority"),
        );
        if downstream_closed || frame_tx.is_closed() {
            return;
        }

        let mut ordered_text_states = text_states.iter().collect::<Vec<_>>();
        ordered_text_states.sort_by_key(|(_, state)| state.output_index);
        for (item_id, state) in ordered_text_states {
            let authoritative = result.output.iter().find_map(|item| {
                let normalized = normalize_output_item(item)?;
                (normalized.get("type").and_then(Value::as_str) == Some("message")
                    && normalized.get("id").and_then(Value::as_str) == Some(item_id))
                .then_some(normalized)
            });
            let item = authoritative.unwrap_or_else(|| {
                json!({
                    "id": item_id,
                    "type": "message",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": state.text,
                        "annotations": [],
                        "logprobs": []
                    }]
                })
            });
            if !send_sse(
                &frame_tx,
                "response.output_text.done",
                json!({
                    "type": "response.output_text.done",
                    "sequence_number": sequence,
                    "item_id": item_id,
                    "output_index": state.output_index,
                    "content_index": 0,
                    "text": state.text,
                    "logprobs": []
                }),
            )
            .await
            {
                return;
            }
            sequence += 1;
            if !send_sse(
                &frame_tx,
                "response.content_part.done",
                json!({
                    "type": "response.content_part.done",
                    "sequence_number": sequence,
                    "item_id": item_id,
                    "output_index": state.output_index,
                    "content_index": 0,
                    "part": item.pointer("/content/0").cloned().unwrap_or(Value::Null)
                }),
            )
            .await
            {
                return;
            }
            sequence += 1;
            if !send_sse(
                &frame_tx,
                "response.output_item.done",
                json!({
                    "type": "response.output_item.done",
                    "sequence_number": sequence,
                    "output_index": state.output_index,
                    "item": item
                }),
            )
            .await
            {
                return;
            }
            sequence += 1;
        }

        // A zero-delta message still needs lifecycle events.
        for item in result
            .output
            .iter()
            .filter_map(|item| {
                normalize_output_item_with_options(
                    item,
                    prepared.request.include_encrypted_reasoning,
                )
            })
            .filter(|item| item.get("type").and_then(Value::as_str) == Some("message"))
        {
            let id = item.get("id").and_then(Value::as_str).unwrap_or("");
            if !text_states.contains_key(id) {
                if !emit_completed_item(&frame_tx, &mut sequence, next_output_index, &item).await {
                    return;
                }
                next_output_index += 1;
            }
        }

        let mut remaining_reasoning = reasoning_states.into_iter().collect::<Vec<_>>();
        remaining_reasoning.sort_by_key(|(_, state)| state.output_index);
        for (item_id, state) in remaining_reasoning {
            let authoritative = result.output.iter().find_map(|item| {
                let normalized = normalize_output_item_with_options(
                    item,
                    prepared.request.include_encrypted_reasoning,
                )?;
                (normalized.get("type").and_then(Value::as_str) == Some("reasoning")
                    && normalized.get("id").and_then(Value::as_str) == Some(&item_id))
                .then_some(normalized)
            });
            let item = authoritative.unwrap_or_else(|| {
                json!({
                    "id": item_id,
                    "type": "reasoning",
                    "status": "completed",
                    "summary": state.parts.values().map(|text| {
                        json!({"type": "summary_text", "text": text})
                    }).collect::<Vec<_>>()
                })
            });
            if !emit_completed_reasoning_item(&frame_tx, &mut sequence, &item_id, &state, &item)
                .await
            {
                return;
            }
            emitted_non_messages.insert(item_id);
        }

        // Backfill any authoritative non-message item for which the stream emitted no raw
        // completion notification. This preserves a valid lifecycle without inventing content.
        for item in result.output.iter().filter_map(|item| {
            normalize_output_item_with_options(item, prepared.request.include_encrypted_reasoning)
        }) {
            if item.get("type").and_then(Value::as_str) == Some("message") {
                continue;
            }
            let key = output_item_key(&item);
            if emitted_non_messages.insert(key) {
                if !emit_completed_item(&frame_tx, &mut sequence, next_output_index, &item).await {
                    return;
                }
                next_output_index += 1;
            }
        }

        let completed =
            build_completed_response(&prepared.request, &result, &response_id, created_at);
        let _ = send_sse(
            &frame_tx,
            "response.completed",
            json!({
                "type": "response.completed",
                "sequence_number": sequence,
                "response": completed
            }),
        )
        .await;
    });

    let mut response = Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-cache")
        .header("connection", "keep-alive")
        .header("x-request-id", request_id_header)
        .body(Body::from_stream(ReceiverStream { receiver: frame_rx }))
        .unwrap();
    insert_extra_headers(&mut response, ratelimit);
    response
}

#[derive(Debug)]
struct StreamTextState {
    output_index: usize,
    text: String,
}

#[derive(Debug)]
struct StreamReasoningState {
    output_index: usize,
    parts: BTreeMap<u64, String>,
}

async fn emit_reasoning_item_added(
    sender: &mpsc::Sender<Bytes>,
    sequence: &mut u64,
    output_index: usize,
    item_id: &str,
) -> bool {
    if !send_sse(
        sender,
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "sequence_number": *sequence,
            "output_index": output_index,
            "item": {
                "id": item_id,
                "type": "reasoning",
                "status": "in_progress",
                "summary": []
            }
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    true
}

async fn emit_reasoning_summary_part_added(
    sender: &mpsc::Sender<Bytes>,
    sequence: &mut u64,
    output_index: usize,
    item_id: &str,
    summary_index: u64,
) -> bool {
    if !send_sse(
        sender,
        "response.reasoning_summary_part.added",
        json!({
            "type": "response.reasoning_summary_part.added",
            "sequence_number": *sequence,
            "item_id": item_id,
            "output_index": output_index,
            "summary_index": summary_index,
            "part": {"type": "summary_text", "text": ""}
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    true
}

async fn emit_completed_reasoning_item(
    sender: &mpsc::Sender<Bytes>,
    sequence: &mut u64,
    item_id: &str,
    state: &StreamReasoningState,
    item: &Value,
) -> bool {
    for (summary_index, streamed_text) in &state.parts {
        let final_text = usize::try_from(*summary_index)
            .ok()
            .and_then(|index| item.get("summary")?.as_array()?.get(index))
            .and_then(|part| part.get("text"))
            .and_then(Value::as_str)
            .unwrap_or(streamed_text);
        if !send_sse(
            sender,
            "response.reasoning_summary_text.done",
            json!({
                "type": "response.reasoning_summary_text.done",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": state.output_index,
                "summary_index": summary_index,
                "text": final_text
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
        if !send_sse(
            sender,
            "response.reasoning_summary_part.done",
            json!({
                "type": "response.reasoning_summary_part.done",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": state.output_index,
                "summary_index": summary_index,
                "part": {"type": "summary_text", "text": final_text}
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
    }
    if !send_sse(
        sender,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "sequence_number": *sequence,
            "output_index": state.output_index,
            "item": item
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    true
}

async fn emit_completed_item(
    sender: &mpsc::Sender<Bytes>,
    sequence: &mut u64,
    output_index: usize,
    item: &Value,
) -> bool {
    let mut started = item.clone();
    started["status"] = Value::String("in_progress".to_string());
    if started.get("type").and_then(Value::as_str) == Some("function_call") {
        started["arguments"] = Value::String(String::new());
    } else if started.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
        started["input"] = Value::String(String::new());
    } else if started.get("type").and_then(Value::as_str) == Some("reasoning") {
        started["summary"] = json!([]);
        if let Some(item) = started.as_object_mut() {
            item.remove("encrypted_content");
        }
    }
    if !send_sse(
        sender,
        "response.output_item.added",
        json!({
            "type": "response.output_item.added",
            "sequence_number": *sequence,
            "output_index": output_index,
            "item": started
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    if item.get("type").and_then(Value::as_str) == Some("function_call") {
        let arguments = item.get("arguments").and_then(Value::as_str).unwrap_or("");
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
        if !send_sse(
            sender,
            "response.function_call_arguments.delta",
            json!({
                "type": "response.function_call_arguments.delta",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": output_index,
                "delta": arguments
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
        if !send_sse(
            sender,
            "response.function_call_arguments.done",
            json!({
                "type": "response.function_call_arguments.done",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": output_index,
                "arguments": arguments
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
    }
    if item.get("type").and_then(Value::as_str) == Some("custom_tool_call") {
        let input = item.get("input").and_then(Value::as_str).unwrap_or("");
        let item_id = item.get("id").and_then(Value::as_str).unwrap_or("");
        if !send_sse(
            sender,
            "response.custom_tool_call_input.delta",
            json!({
                "type": "response.custom_tool_call_input.delta",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": output_index,
                "delta": input
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
        if !send_sse(
            sender,
            "response.custom_tool_call_input.done",
            json!({
                "type": "response.custom_tool_call_input.done",
                "sequence_number": *sequence,
                "item_id": item_id,
                "output_index": output_index,
                "input": input
            }),
        )
        .await
        {
            return false;
        }
        *sequence += 1;
    }
    if !send_sse(
        sender,
        "response.output_item.done",
        json!({
            "type": "response.output_item.done",
            "sequence_number": *sequence,
            "output_index": output_index,
            "item": item
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    true
}

fn output_item_key(item: &Value) -> String {
    item.get("id")
        .and_then(Value::as_str)
        .or_else(|| item.get("call_id").and_then(Value::as_str))
        .map(str::to_string)
        .unwrap_or_else(|| {
            serde_json::to_vec(item)
                .map(|bytes| blake3::hash(&bytes).to_hex().to_string())
                .unwrap_or_else(|_| new_id("item"))
        })
}

async fn send_sse(sender: &mpsc::Sender<Bytes>, event: &str, value: Value) -> bool {
    let data = serde_json::to_string(&value).unwrap_or_else(|_| {
        r#"{"type":"error","code":"server_error","message":"serialization failed"}"#.to_string()
    });
    let frame = Bytes::from(format!("event: {event}\ndata: {data}\n\n"));
    send_sse_bytes(sender, frame).await
}

async fn send_sse_bytes(sender: &mpsc::Sender<Bytes>, frame: Bytes) -> bool {
    matches!(
        tokio::time::timeout(STREAM_FRAME_SEND_TIMEOUT, sender.send(frame)).await,
        Ok(Ok(()))
    )
}

/// Terminal failure lifecycle for a streaming Responses call: the OpenAI-shaped `error` event
/// plus a `response.failed` event carrying the full failed response object, which is what
/// spec-compliant SDKs wait on to surface a terminal error.
#[allow(clippy::too_many_arguments)]
async fn emit_stream_failure(
    sender: &mpsc::Sender<Bytes>,
    prepared: &PreparedTurn,
    response_id: &str,
    created_at: i64,
    sequence: u64,
    code: Option<&str>,
    message: &str,
) {
    let _ = send_sse(
        sender,
        "error",
        json!({
            "type": "error",
            "sequence_number": sequence,
            "code": code,
            "message": message,
            "param": Value::Null
        }),
    )
    .await;
    let mut failed = response_object(
        &prepared.request,
        response_id,
        created_at,
        "failed",
        Vec::new(),
        None,
    );
    failed["error"] = json!({
        "code": code,
        "message": message
    });
    let _ = send_sse(
        sender,
        "response.failed",
        json!({
            "type": "response.failed",
            "sequence_number": sequence + 1,
            "response": failed
        }),
    )
    .await;
}

struct ReceiverStream {
    receiver: mpsc::Receiver<Bytes>,
}

impl Stream for ReceiverStream {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.receiver.poll_recv(cx).map(|item| item.map(Ok))
    }
}

pub(super) fn json_response(status: StatusCode, body: Value, request_id: &str) -> Response {
    let mut response = (status, axum::Json(body)).into_response();
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

/// The provider rate-limit window is a percent-used budget, not a request/token count, so the
/// `x-ratelimit-*` headers expose it on a 100-unit basis: `remaining` is the headroom percent
/// and `reset` is the wall-clock seconds until the constrained window rolls over. This mirrors
/// how OpenAI reports its own windows and gives SDK retry logic something truthful to read.
pub(super) async fn ratelimit_headers(gateway: &CodexGateway) -> Vec<(&'static str, String)> {
    let Some(limits) = gateway.operational_status().await.rate_limits else {
        return Vec::new();
    };
    let Some(window) = limits
        .primary
        .iter()
        .chain(limits.secondary.iter())
        .max_by_key(|window| window.used_percent)
    else {
        return Vec::new();
    };
    let remaining = 100i64.saturating_sub(window.used_percent);
    let mut headers = vec![
        ("x-ratelimit-limit-tokens", "100".to_string()),
        ("x-ratelimit-remaining-tokens", remaining.to_string()),
    ];
    if let Some(resets_at) = window.resets_at {
        let seconds = resets_at.saturating_sub(pool::now()).max(0);
        headers.push(("x-ratelimit-reset-tokens", format!("{seconds}s")));
    }
    headers
}

pub(super) fn insert_extra_headers(response: &mut Response, headers: Vec<(&'static str, String)>) {
    for (name, value) in headers {
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            response.headers_mut().insert(name, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::{CodexConfig, CodexPrices};
    use std::collections::BTreeMap;

    #[test]
    fn strip_reasoning_items_keeps_portable_items_only() {
        let items = vec![
            json!({"type": "message", "role": "user", "content": []}),
            json!({"type": "reasoning", "encrypted_content": "secret", "summary": []}),
            json!({"type": "function_call", "name": "f", "arguments": "{}"}),
            json!({"type": "function_call_output", "output": "ok"}),
        ];
        let kept = strip_reasoning_items(items);
        let types: Vec<_> = kept
            .iter()
            .map(|item| item["type"].as_str().unwrap())
            .collect();
        // Reasoning (model-bound) is dropped on a cross-model replay; messages and tool items stay.
        assert_eq!(
            types,
            vec!["message", "function_call", "function_call_output"]
        );
    }

    /// Every public error the OpenAI-compatible surface can produce.
    fn all_public_errors() -> Vec<ApiError> {
        let mut errors = vec![
            ApiError::invalid("test", None::<String>),
            ApiError::not_found("test", None::<String>),
            ApiError::unavailable(),
            ApiError::rate_limited(),
            ApiError::rate_limited_for(Some(42)),
        ];
        for admission in [
            AdmissionError::Unauthorized,
            AdmissionError::Unavailable,
            AdmissionError::LowBalance,
        ] {
            errors.push(ApiError::from(admission));
        }
        for process in [
            ProcessError::Disabled,
            ProcessError::InvalidConfig("credential file unreadable".to_string()),
            ProcessError::Closed,
            ProcessError::Timeout("turn completion"),
            ProcessError::Protocol("upstream served an unexpected model".to_string()),
            ProcessError::ContextWindowExceeded,
            ProcessError::UsageLimitExceeded {
                retry_after: Some(60),
            },
            ProcessError::BadRequest,
            ProcessError::AuthenticationRequired,
            ProcessError::SubscriptionRequired,
        ] {
            errors.push(ApiError::from(process));
        }
        errors
    }

    #[test]
    fn public_errors_never_leak_internal_architecture() {
        // The client believes it is talking to an OpenAI-compatible endpoint. No public field may
        // reveal how the provider is built: the home pool, the app-server child, the pinned binary,
        // the ChatGPT profile behind it, or any upstream diagnostic text. This is the Codex twin of
        // `proxy::tests::local_err_never_leaks_internal_architecture`.
        let forbidden = [
            "codex",
            "app-server",
            "app server",
            "chatgpt",
            "subscription",
            "home",
            "pool",
            "upstream",
            "authority",
            "cooling",
            "rotat",
            "binary",
            "digest",
            "sha256",
            "profile",
            "device",
            "/srv/",
            "sensitive upstream diagnostic",
        ];
        for error in all_public_errors() {
            let haystack = format!("{} {}", error.kind, error.message).to_lowercase();
            for term in forbidden {
                assert!(
                    !haystack.contains(term),
                    "public error leaks internal term {term:?}: {haystack:?}"
                );
            }
        }
    }

    #[test]
    fn failed_external_fallback_keeps_local_status_but_removes_not_started_proof() {
        let response = ApiError::from(ProcessError::ExternalFallbackFailed {
            local: Box::new(ProcessError::UsageLimitExceeded {
                retry_after: Some(42),
            }),
        })
        .into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(response.headers().get("retry-after").unwrap(), "42");
        assert!(response
            .headers()
            .get("x-apitoken-execution-state")
            .is_none());
        assert_eq!(
            response.extensions().get::<TerminalErrorReason>(),
            Some(&TerminalErrorReason("claudestore_fallback_failed"))
        );
    }

    #[test]
    fn public_errors_keep_openai_shaped_status_and_type_pairs() {
        // A client's retry logic keys on these pairs; an internal fault must always be retryable
        // rather than surfacing as a client error it would never retry.
        for error in all_public_errors() {
            let expected_kind = match error.status {
                StatusCode::BAD_REQUEST => "invalid_request_error",
                StatusCode::UNAUTHORIZED => "invalid_request_error",
                StatusCode::NOT_FOUND => "invalid_request_error",
                StatusCode::PAYMENT_REQUIRED => "insufficient_quota",
                StatusCode::TOO_MANY_REQUESTS => "rate_limit_error",
                StatusCode::SERVICE_UNAVAILABLE => "server_error",
                other => panic!("unexpected public status {other}"),
            };
            assert_eq!(error.kind, expected_kind, "status {}", error.status);
        }
    }

    #[test]
    fn every_public_error_marks_the_execution_not_started() {
        // Каждый публичный отказ OpenAI-конверта — не-2xx до границы доставки: reserve (если
        // успели взять) возвращает дроп CodexAdmission → HoldGuard, ни байта клиенту не ушло.
        // Значит все они обязаны нести x-apitoken-execution-state: not_started, а успешный
        // json_response (2xx) — не нести никогда.
        for error in all_public_errors() {
            let response = error.into_response();
            assert!(!response.status().is_success());
            assert_eq!(
                response
                    .headers()
                    .get(crate::proxy::EXECUTION_STATE_HEADER)
                    .unwrap(),
                crate::proxy::EXECUTION_STATE_NOT_STARTED
            );
        }
        let ok = json_response(StatusCode::OK, json!({"id": "resp_1"}), "req_1");
        assert!(ok
            .headers()
            .get(crate::proxy::EXECUTION_STATE_HEADER)
            .is_none());
    }

    #[test]
    fn a_subscription_limit_is_advertised_with_a_wait_a_client_can_honour() {
        let limited = ApiError::from(ProcessError::UsageLimitExceeded {
            retry_after: Some(123),
        });
        assert_eq!(limited.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(limited.retry_after, Some(123));
        // A limit with no published reset must still carry a usable wait, never none.
        let unknown = ApiError::from(ProcessError::UsageLimitExceeded { retry_after: None });
        assert!(unknown.retry_after.is_some_and(|seconds| seconds > 0));
    }

    #[test]
    fn codex_catalog_clients_get_their_native_models_envelope() {
        for (header, value) in [
            ("originator", "codex_exec"),
            ("originator", "codex_cli_rs"),
            ("user-agent", "codex_cli_rs/0.146.0"),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header, HeaderValue::from_static(value));
            assert!(
                requests_codex_models_envelope(&headers),
                "{header}: {value}"
            );
        }

        let mut openai_headers = HeaderMap::new();
        openai_headers.insert("user-agent", HeaderValue::from_static("OpenAI/Python 2.0"));
        assert!(!requests_codex_models_envelope(&openai_headers));
        let mut near_match = HeaderMap::new();
        near_match.insert("originator", HeaderValue::from_static("my-codex-proxy"));
        assert!(!requests_codex_models_envelope(&near_match));
    }

    #[test]
    fn standard_model_list_falls_back_to_the_configured_catalog() {
        let gateway = gateway();
        let data = public_model_objects(&gateway, None);
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "gpt-5.6");
        assert_eq!(data[0]["apitoken"]["limits"], json!({"output": 128000}));
        assert_eq!(
            data[0]["apitoken"]["capabilities"]["service_tiers"],
            json!(["standard", "priority"])
        );
        assert_eq!(
            data[0]["apitoken"]["capabilities"]["reasoning_efforts"],
            json!(["none", "low", "medium", "high", "xhigh", "max"])
        );
        assert_eq!(
            data[0]["apitoken"]["capabilities"],
            json!({
                "reasoning_efforts": ["none", "low", "medium", "high", "xhigh", "max"],
                "service_tiers": ["standard", "priority"],
                "input_modalities": ["text", "image"],
                "output_modalities": ["text"],
                "tool_calling": true,
                "structured_outputs": true,
                "streaming": true
            })
        );
        assert!(data[0].get("name").is_none());
    }

    #[test]
    fn standard_model_list_uses_the_last_good_upstream_intersection() {
        let gateway = gateway();
        let available = crate::codex::CodexModelCatalog {
            models: HashSet::from(["different-upstream-model".to_string()]),
            ..Default::default()
        };
        assert!(public_model_objects(&gateway, Some(&available)).is_empty());

        let available = crate::codex::CodexModelCatalog {
            models: HashSet::from(["gpt-5.6-sol".to_string()]),
            input_token_limits: HashMap::from([("gpt-5.6-sol".to_string(), 272_000)]),
            display_names: HashMap::from([(
                "gpt-5.6-sol".to_string(),
                "GPT 5.6 Thinking".to_string(),
            )]),
            ..Default::default()
        };
        let data = public_model_objects(&gateway, Some(&available));
        assert_eq!(data.len(), 1);
        assert_eq!(data[0]["id"], "gpt-5.6");
        assert_eq!(
            data[0]["apitoken"]["limits"],
            json!({"context": 400000, "input": 272000, "output": 128000})
        );
        assert_eq!(data[0]["name"], "GPT 5.6 Thinking");
    }

    fn model() -> CodexModel {
        CodexModel {
            id: "gpt-5.6".to_string(),
            upstream: "gpt-5.6-sol".to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: 128_000,
            reasoning_efforts: ["none", "low", "medium", "high", "xhigh", "max"]
                .into_iter()
                .map(str::to_string)
                .collect(),
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: Some(25_000),
            prices: CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                api_fast_multiplier_basis_points: 25_000,
                long_context_threshold: 272_000,
                long_input_basis_points: 20_000,
                long_output_basis_points: 15_000,
            },
        }
    }

    fn gateway() -> CodexGateway {
        let root =
            std::env::temp_dir().join(format!("claude-api-codex-api-test-{}", new_id("roster")));
        let credentials = root.join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        let keyring =
            codex_credential::CredentialKeyring::parse(&format!("current:{}", "ab".repeat(32)))
                .unwrap();
        let credential = codex_credential::CodexCredential {
            version: 1,
            access_token: "test-access-token".to_string(),
            refresh_token: "test-refresh-token".to_string(),
            expires_at: i64::MAX / 2,
            oauth_client_id: codex_credential::CODEX_OFFICIAL_OAUTH_CLIENT_ID.to_string(),
            token_uri: codex_credential::CODEX_OFFICIAL_TOKEN_URI.to_string(),
            account_id: "acct_test_1234".to_string(),
            email: "owner@example.com".to_string(),
            plan: "chatgpt_plus".to_string(),
            proxy: String::new(),
            proxy_order_id: 0,
            issued_at: 0,
        };
        let envelope = keyring.seal("current", "alpha", &credential).unwrap();
        std::fs::write(
            credentials.join("alpha.json"),
            codex_credential::encode_envelope(&envelope).unwrap(),
        )
        .unwrap();
        std::fs::write(
            root.join("profiles.json"),
            serde_json::to_vec(&serde_json::json!({
                "profiles": [{
                    "id": "alpha",
                    "credential_file": credentials.join("alpha.json").to_str().unwrap(),
                }]
            }))
            .unwrap(),
        )
        .unwrap();
        CodexGateway::new(CodexConfig {
            enabled: true,
            base_url: codex_credential::CODEX_DEFAULT_BASE_URL.to_string(),
            profiles_file: root.join("profiles.json").to_str().unwrap().to_string(),
            credential_keys: keyring,
            cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
            request_timeout_ms: 1_000,
            turn_timeout_ms: 1_000,
            turn_silence_timeout_ms: 1_000,
            health_probe_interval_secs: 300,
            reserve_5h: 0.10,
            reserve_7d: 0.03,
            reserve_jitter: 0.0,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: Some("test".to_string()),
            history_redis_timeout_ms: 10,
            default_proxy_env: BTreeMap::new(),
            models: vec![model()],
        })
        .unwrap()
    }

    #[test]
    fn string_input_becomes_one_user_turn_without_duplicate_injection() {
        let normalized = normalize_responses_input(&json!("hello")).unwrap();
        assert_eq!(normalized.canonical_items.len(), 1);
        assert!(normalized.prior_items.is_empty());
        assert_eq!(
            normalized.turn_input,
            vec![json!({"type": "text", "text": "hello"})]
        );
    }

    #[test]
    fn full_history_injects_prefix_and_sends_only_final_user_message() {
        let normalized = normalize_responses_input(&json!([
            {"role": "user", "content": "one"},
            {"role": "assistant", "content": "two"},
            {"role": "user", "content": [{"type": "input_text", "text": "three"}]}
        ]))
        .unwrap();
        assert_eq!(normalized.canonical_items.len(), 3);
        assert_eq!(normalized.prior_items.len(), 2);
        assert_eq!(
            normalized.turn_input,
            vec![json!({"type": "text", "text": "three"})]
        );
    }

    #[test]
    fn responses_system_history_is_preserved_as_backend_supported_developer_history() {
        let normalized = normalize_responses_input(&json!([
            {"role": "system", "content": "follow this policy"},
            {"role": "user", "content": "hello"}
        ]))
        .unwrap();
        assert_eq!(normalized.prior_items[0]["role"], "developer");
        assert_eq!(
            normalized.prior_items[0]["content"][0]["text"],
            "follow this policy"
        );
        assert_eq!(normalized.turn_input[0]["text"], "hello");
    }

    #[tokio::test]
    async fn responses_instructions_replace_the_upstream_base_prompt() {
        let gateway = gateway();
        let parsed = parse_responses_request(
            &gateway,
            json!({
                "model": "gpt-5.6",
                "instructions": "Only the client's instruction.",
                "input": "hello"
            }),
        )
        .unwrap();
        let prepared = prepare_turn(&gateway, "tenant", parsed).await.unwrap();
        assert_eq!(
            prepared.turn.base_instructions.as_deref(),
            Some("Only the client's instruction.")
        );
        assert!(prepared.turn.developer_instructions.is_none());
    }

    #[test]
    fn parser_ignores_fields_the_backend_cannot_honor() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "temperature": 0.2,
                "top_p": 0.5,
                "max_output_tokens": 512,
                "truncation": "auto",
                "user": "end-user",
                "background": true,
                "max_tool_calls": 3,
                "top_logprobs": 2,
                "service_tier": "flex",
                "stream_options": {"include_obfuscation": true},
                "some_future_field": {"anything": true}
            }),
        )
        .expect("parameters the transport cannot honor must be ignored, not rejected");
        assert_eq!(parsed.input.turn_input.len(), 1);
        assert!(parsed.service_tier.is_none());
        assert_eq!(parsed.max_output_tokens, Some(512));
        let response =
            response_object(&parsed, "resp_with_cap", 0, "in_progress", Vec::new(), None);
        assert_eq!(response["max_output_tokens"], 512);
    }

    #[test]
    fn responses_output_limit_is_strict_but_null_remains_absent() {
        for value in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("512"),
            json!({}),
            serde_json::from_str("18446744073709551616").unwrap(),
        ] {
            let error = parse_responses_request(
                &gateway(),
                json!({"model": "gpt-5.6", "input": "hi", "max_output_tokens": value}),
            )
            .unwrap_err();
            assert_eq!(error.status, StatusCode::BAD_REQUEST);
            assert_eq!(error.param.as_deref(), Some("max_output_tokens"));
            let response = error.into_response();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }

        let parsed = parse_responses_request(
            &gateway(),
            json!({"model": "gpt-5.6", "input": "hi", "max_output_tokens": null}),
        )
        .unwrap();
        assert_eq!(parsed.max_output_tokens, None);
    }

    #[test]
    fn parser_normalizes_codex_fast_and_openai_priority_service_tiers() {
        for requested in ["fast", "priority"] {
            let parsed = parse_responses_request(
                &gateway(),
                json!({
                    "model": "gpt-5.6",
                    "input": "hi",
                    "service_tier": requested
                }),
            )
            .unwrap();
            assert_eq!(parsed.service_tier.as_deref(), Some("priority"));
            let response =
                response_object(&parsed, "resp_fast", 0, "in_progress", Vec::new(), None);
            assert_eq!(response["service_tier"], "priority");
            let missing_effective_tier = build_completed_response(
                &parsed,
                &CodexTurnResult {
                    output: Vec::new(),
                    usage: CodexUsage::default(),
                    effective_service_tier: None,
                    provider_reported_service_tier: None,
                },
                "resp_fast",
                0,
            );
            assert_eq!(missing_effective_tier["service_tier"], "default");
            let accepted_fast_with_default_provider_report = build_completed_response(
                &parsed,
                &CodexTurnResult {
                    output: Vec::new(),
                    usage: CodexUsage::default(),
                    effective_service_tier: Some("priority".to_string()),
                    provider_reported_service_tier: Some("default".to_string()),
                },
                "resp_fast",
                0,
            );
            assert_eq!(
                accepted_fast_with_default_provider_report["service_tier"],
                "priority"
            );
        }
        for requested in ["default", "auto", "flex", "future-tier"] {
            let parsed = parse_responses_request(
                &gateway(),
                json!({
                    "model": "gpt-5.6",
                    "input": "hi",
                    "service_tier": requested
                }),
            )
            .unwrap();
            assert_eq!(parsed.service_tier, None, "{requested}");
        }
    }

    #[test]
    fn responses_accept_namespaced_openai_catalog_ids() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "openai/gpt-5.6",
                "input": "hi",
                "service_tier": "priority"
            }),
        )
        .expect("the OpenAI plane must resolve the namespace published by the router catalog");

        assert_eq!(parsed.public_model.id, "gpt-5.6");
        assert_eq!(parsed.service_tier.as_deref(), Some("priority"));
    }

    #[test]
    fn unenforceable_tool_controls_degrade_instead_of_failing() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "weather?",
                "tools": [{
                    "type": "function",
                    "name": "get_weather",
                    "parameters": {"type": "object"}
                }],
                "tool_choice": {"type": "function", "name": "get_weather"},
                "parallel_tool_calls": false
            }),
        )
        .expect("forced tool choice and parallel=false must degrade, not fail");
        assert_eq!(parsed.dynamic_tools.len(), 1);
        assert_eq!(parsed.tool_choice, json!("auto"));

        let required = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "weather?",
                "tools": [{
                    "type": "function",
                    "name": "get_weather",
                    "parameters": {"type": "object"}
                }],
                "tool_choice": "required"
            }),
        )
        .expect("tool_choice=required must degrade to auto");
        assert_eq!(required.tool_choice, json!("auto"));
        assert_eq!(required.dynamic_tools.len(), 1);
    }

    #[test]
    fn unsupported_reasoning_effort_degrades_to_model_default() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "reasoning": {"effort": "minimal", "summary": "verbose", "context": "last_turn"}
            }),
        )
        .expect("unsupported effort/summary must degrade, not fail");
        assert_eq!(parsed.reasoning_effort, None);
        assert_eq!(parsed.reasoning_summary, None);
    }

    #[test]
    fn responses_input_image_parts_translate_to_turn_image_inputs() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {"type": "input_text", "text": "first"},
                            {"type": "input_image", "image_url": "data:image/png;base64,iVBORw0KGgo=", "detail": "low"}
                        ]
                    },
                    {
                        "role": "user",
                        "content": [
                            {"type": "input_image", "image_url": "https://example.com/x.png"},
                            {"type": "input_text", "text": "second"}
                        ]
                    }
                ]
            }),
        )
        .expect("input_image parts must translate");
        // First user message is history and keeps canonical Responses image parts.
        let history = &parsed.input.prior_items[0];
        assert_eq!(history["content"][1]["type"], "input_image");
        assert_eq!(history["content"][1]["detail"], "low");
        // Final user message becomes upstream image turn inputs.
        assert_eq!(parsed.input.turn_input[0]["type"], "image");
        assert_eq!(
            parsed.input.turn_input[0]["url"],
            "https://example.com/x.png"
        );
        assert_eq!(
            parsed.input.turn_input[1],
            json!({"type": "text", "text": "second"})
        );
    }

    #[test]
    fn data_url_images_do_not_inflate_the_billing_estimate() {
        let mut value = json!({
            "input": [
                {"type": "image", "url": format!("data:image/png;base64,{}", "A".repeat(1_000_000))},
                {"type": "text", "text": "describe"}
            ]
        });
        sanitize_estimate_images(&mut value);
        let bytes = serde_json::to_vec(&value).unwrap().len();
        assert!(
            bytes < 16_000,
            "estimate must not carry raw base64: {bytes}"
        );
    }

    #[tokio::test]
    async fn injected_history_keeps_data_url_images_verbatim() {
        let data_url = format!("data:image/png;base64,{}", "A".repeat(200_000));
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": [
                    {
                        "role": "user",
                        "content": [
                            {"type": "input_text", "text": "first"},
                            {"type": "input_image", "image_url": data_url}
                        ]
                    },
                    {"role": "user", "content": [{"type": "input_text", "text": "second"}]}
                ]
            }),
        )
        .expect("data-url history image must parse");
        let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
        // The backend cannot decode the fixed-size estimate placeholder; injecting it would
        // surface codex's "image content omitted" text instead of the screenshot.
        let injected_image = prepared.turn.injected_items[0]["content"][1]["image_url"]
            .as_str()
            .expect("history image part must survive");
        assert_eq!(injected_image, data_url);
        // The billing reserve still sees the fixed-size placeholder, not the raw payload.
        assert!(
            prepared.estimated_input_tokens < 100_000,
            "estimate must not carry raw base64: {}",
            prepared.estimated_input_tokens
        );
    }

    #[test]
    fn function_tools_translate_to_dynamic_tool_schema() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "weather?",
                "tools": [{
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": {
                        "type": "object",
                        "properties": {"city": {"type": "string"}},
                        "required": ["city"]
                    },
                    "strict": false
                }]
            }),
        )
        .unwrap();
        assert_eq!(parsed.dynamic_tools.len(), 1);
        assert_eq!(parsed.dynamic_tools[0]["type"], "function");
        assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
        assert_eq!(
            parsed.dynamic_tools[0]["inputSchema"]["required"],
            json!(["city"])
        );
    }

    #[test]
    fn codex_0146_top_level_tools_translate_current_client_forms() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "Reply briefly.",
                "tools": [
                    {
                        "type": "function",
                        "name": "list_mcp_resources",
                        "description": "List resources",
                        "parameters": {"type": "object", "properties": {}},
                        "strict": false
                    },
                    {
                        "type": "function",
                        "name": "read_mcp_resource",
                        "description": "Read a resource",
                        "parameters": {
                            "type": "object",
                            "properties": {"uri": {"type": "string"}},
                            "required": ["uri"]
                        },
                        "strict": false
                    },
                    {
                        "type": "custom",
                        "name": "apply_patch",
                        "description": "Apply a patch",
                        "format": {
                            "type": "grammar",
                            "syntax": "lark",
                            "definition": "start: /[\\s\\S]+/"
                        }
                    },
                    {
                        "type": "function",
                        "name": "view_image",
                        "description": "View an image",
                        "parameters": {
                            "type": "object",
                            "properties": {"path": {"type": "string"}},
                            "required": ["path"]
                        },
                        "strict": false
                    },
                    {
                        "type": "tool_search",
                        "execution": "client",
                        "description": "Search available tools",
                        "parameters": {
                            "type": "object",
                            "properties": {"query": {"type": "string"}},
                            "required": ["query"]
                        }
                    }
                ]
            }),
        )
        .expect("Codex 0.146 top-level client tools must parse");

        assert_eq!(parsed.original_tools.len(), 5);
        assert_eq!(parsed.dynamic_tools.len(), 5);
        assert_eq!(parsed.dynamic_tools[0]["type"], "function");
        assert_eq!(parsed.dynamic_tools[0]["name"], "list_mcp_resources");
        assert_eq!(parsed.dynamic_tools[2]["type"], "custom");
        assert_eq!(parsed.dynamic_tools[2]["name"], "apply_patch");
        assert_eq!(
            parsed.dynamic_tools[2]["format"]["definition"],
            "start: /[\\s\\S]+/"
        );
        assert_eq!(parsed.dynamic_tools[4]["type"], "function");
        assert_eq!(parsed.dynamic_tools[4]["name"], TOOL_SEARCH_DYNAMIC_NAME);
        assert_eq!(
            parsed.dynamic_tools[4]["inputSchema"]["required"],
            json!(["query"])
        );
    }

    #[test]
    fn official_codex_cli_request_shape_translates_all_additional_tool_kinds() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": [
                    {
                        "type": "additional_tools",
                        "role": "developer",
                        "tools": [
                            {
                                "type": "custom",
                                "name": "exec",
                                "description": "Run source",
                                "format": {
                                    "type": "grammar",
                                    "syntax": "lark",
                                    "definition": "start: /[\\s\\S]+/"
                                }
                            },
                            {
                                "type": "function",
                                "name": "wait",
                                "description": "Wait for a task",
                                "parameters": {
                                    "type": "object",
                                    "properties": {"id": {"type": "string"}},
                                    "required": ["id"]
                                },
                                "strict": false,
                                "defer_loading": true
                            },
                            {
                                "type": "namespace",
                                "name": "collaboration",
                                "description": "Agent coordination",
                                "tools": [{
                                    "type": "function",
                                    "name": "list_agents",
                                    "description": "List agents",
                                    "parameters": {
                                        "type": "object",
                                        "properties": {}
                                    },
                                    "strict": false
                                }]
                            },
                            {
                                "type": "tool_search",
                                "execution": "client",
                                "description": "Search available tools",
                                "parameters": {
                                    "type": "object",
                                    "properties": {
                                        "query": {"type": "string"},
                                        "limit": {"type": "integer"}
                                    },
                                    "required": ["query"]
                                }
                            }
                        ]
                    },
                    {
                        "role": "developer",
                        "content": "Follow the caller's policy."
                    },
                    {
                        "role": "user",
                        "content": "Reply briefly."
                    }
                ],
                "include": ["reasoning.encrypted_content"],
                "parallel_tool_calls": false,
                "tool_choice": "auto",
                "reasoning": {"effort": "low", "context": "all_turns"},
                "text": {"verbosity": "low"},
                "prompt_cache_key": "session-123",
                "client_metadata": {
                    "session_id": "session-123",
                    "turn_id": "turn-456",
                    "x-codex-turn-metadata": "opaque"
                },
                "store": false,
                "stream": true
            }),
        )
        .unwrap();

        assert_eq!(parsed.prompt_cache_key.as_deref(), Some("session-123"));
        assert_eq!(parsed.verbosity.as_deref(), Some("low"));
        assert_eq!(parsed.reasoning_effort.as_deref(), Some("low"));
        assert!(!parsed.parallel_tool_calls);
        assert!(parsed.stream);
        assert_eq!(parsed.original_tools.len(), 4);
        assert_eq!(parsed.dynamic_tools.len(), 4);
        assert_eq!(parsed.dynamic_tools[0]["type"], "custom");
        assert_eq!(parsed.dynamic_tools[0]["name"], "exec");
        assert_eq!(
            parsed.dynamic_tools[0]["format"]["definition"],
            "start: /[\\s\\S]+/"
        );
        assert_eq!(
            parsed.dynamic_tools[1]["inputSchema"]["required"],
            json!(["id"])
        );
        assert_eq!(parsed.dynamic_tools[1]["deferLoading"], true);
        assert_eq!(parsed.dynamic_tools[2]["type"], "namespace");
        assert_eq!(parsed.dynamic_tools[2]["name"], "collaboration");
        assert_eq!(parsed.dynamic_tools[2]["tools"][0]["name"], "list_agents");
        assert_eq!(parsed.dynamic_tools[3]["type"], "function");
        assert_eq!(parsed.dynamic_tools[3]["name"], TOOL_SEARCH_DYNAMIC_NAME);
        assert_eq!(
            parsed.dynamic_tools[3]["inputSchema"]["required"],
            json!(["query"])
        );
        assert_eq!(parsed.input.canonical_items.len(), 2);
        assert_eq!(parsed.input.prior_items.len(), 1);
        assert_eq!(
            parsed.input.turn_input,
            vec![json!({"type": "text", "text": "Reply briefly."})]
        );
    }

    #[test]
    fn codex_diagnostic_metadata_is_bounded() {
        let metadata_error = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "client_metadata": {"turn_id": 42}
            }),
        )
        .unwrap_err();
        assert_eq!(
            metadata_error.param.as_deref(),
            Some("client_metadata.turn_id")
        );
    }

    #[test]
    fn strict_function_tools_are_silently_downgraded() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "weather?",
                "tools": [{
                    "type": "function",
                    "name": "get_weather",
                    "parameters": {"type": "object"},
                    "strict": true
                }]
            }),
        )
        .expect("strict=true must downgrade to a non-strict dynamic tool");
        assert_eq!(parsed.dynamic_tools.len(), 1);
        assert_eq!(parsed.dynamic_tools[0]["name"], "get_weather");
        assert!(parsed.dynamic_tools[0].get("strict").is_none());
    }

    #[test]
    fn response_id_validation_matches_history_write_format() {
        assert!(valid_response_id("resp_abc123_XYZ"));
        assert!(!valid_response_id("chatcmpl_123"));
        assert!(!valid_response_id("resp_a b"));
        assert!(!valid_response_id(&format!("resp_{}", "a".repeat(200))));
    }

    #[tokio::test]
    async fn stream_failure_emits_error_event_then_failed_response() {
        let (sender, mut receiver) = mpsc::channel(8);
        let gateway = gateway();
        let parsed =
            parse_responses_request(&gateway, json!({"model": "gpt-5.6", "input": "hi"})).unwrap();
        let prepared = prepare_turn(&gateway, "tenant", parsed).await.unwrap();
        emit_stream_failure(
            &sender,
            &prepared,
            "resp_x",
            42,
            7,
            Some("server_error"),
            "boom",
        )
        .await;
        drop(sender);
        let frames = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 2);
        assert!(frames[0].starts_with("event: error\n"));
        assert!(frames[0].contains("\"code\":\"server_error\""));
        assert!(frames[1].starts_with("event: response.failed\n"));
        assert!(frames[1].contains("\"status\":\"failed\""));
        assert!(frames[1].contains("\"message\":\"boom\""));
    }

    #[test]
    fn public_usage_preserves_cached_write_and_reasoning_details() {
        let usage = public_usage(&CodexUsage {
            input_tokens: 100,
            cached_input_tokens: 40,
            cache_write_input_tokens: 10,
            output_tokens: 20,
            reasoning_output_tokens: 12,
            total_tokens: 120,
        });
        assert_eq!(usage["input_tokens_details"]["cached_tokens"], 40);
        assert_eq!(usage["input_tokens_details"]["cache_write_tokens"], 10);
        assert_eq!(usage["output_tokens_details"]["reasoning_tokens"], 12);
    }

    #[test]
    fn structured_provider_limits_map_to_openai_style_errors() {
        let usage_error = ApiError::from(ProcessError::UsageLimitExceeded {
            retry_after: Some(123),
        });
        assert_eq!(usage_error.status, StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(usage_error.code, Some("rate_limit_exceeded"));
        assert_eq!(usage_error.retry_after, Some(123));

        let context_error = ApiError::from(ProcessError::ContextWindowExceeded);
        assert_eq!(context_error.status, StatusCode::BAD_REQUEST);
        assert_eq!(context_error.code, Some("context_length_exceeded"));
        assert_eq!(context_error.param.as_deref(), Some("input"));
    }

    #[test]
    fn output_normalization_strips_internal_passthrough_metadata() {
        let item = json!({
            "type": "message",
            "id": "msg_1",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "hello"}],
            "internal_chat_message_metadata_passthrough": {"secret": true}
        });
        let output = normalize_output_item(&item).unwrap();
        assert_eq!(output["status"], "completed");
        assert_eq!(output["content"][0]["annotations"], json!([]));
        assert!(output
            .get("internal_chat_message_metadata_passthrough")
            .is_none());
    }

    #[test]
    fn output_normalization_drops_raw_input_and_empty_message_items() {
        assert!(normalize_output_item(&json!({
            "type": "message",
            "id": "msg_user",
            "role": "user",
            "content": [{"type": "input_text", "text": "private request"}]
        }))
        .is_none());
        assert!(normalize_output_item(&json!({
            "type": "message",
            "id": "msg_empty",
            "role": "assistant",
            "content": []
        }))
        .is_none());
        assert!(normalize_output_item(&json!({
            "type": "message",
            "id": "msg_empty_text",
            "role": "assistant",
            "content": [{"type": "output_text", "text": ""}]
        }))
        .is_none());
        assert!(normalize_output_item(&json!({
            "type": "message",
            "id": "msg_non_output",
            "role": "assistant",
            "content": [{"type": "input_text", "text": "not model output"}]
        }))
        .is_none());
    }

    #[test]
    fn public_reasoning_hides_raw_chain_of_thought_and_gates_encrypted_content() {
        let item = json!({
            "type": "reasoning",
            "id": "rs_1",
            "summary": [{
                "type": "summary_text",
                "text": "Checked the inputs.",
                "internal_provider_metadata": "must not escape"
            }],
            "content": [{"type": "reasoning_text", "text": "private chain of thought"}],
            "encrypted_content": "ciphertext"
        });
        let default_output = normalize_output_item(&item).unwrap();
        assert_eq!(default_output["summary"][0]["text"], "Checked the inputs.");
        assert!(default_output["summary"][0]
            .get("internal_provider_metadata")
            .is_none());
        assert!(default_output.get("content").is_none());
        assert!(default_output.get("encrypted_content").is_none());

        let included_output = normalize_output_item_with_options(&item, true).unwrap();
        assert_eq!(included_output["encrypted_content"], "ciphertext");
        assert!(included_output.get("content").is_none());
    }

    #[test]
    fn function_call_normalization_always_returns_public_string_fields() {
        let output = normalize_output_item(&json!({
            "type": "function_call",
            "id": "fc_1",
            "call_id": "call_1",
            "name": "lookup",
            "arguments": {"query": "safe"},
            "internal_provider_metadata": {"must": "not escape"}
        }))
        .unwrap();
        assert_eq!(output["arguments"], r#"{"query":"safe"}"#);
        assert!(output.get("internal_provider_metadata").is_none());
    }

    #[test]
    fn internal_tool_search_function_normalizes_to_codex_0146_wire_item() {
        let output = normalize_output_item(&json!({
            "type": "function_call",
            "id": "fc_search",
            "call_id": "call_search",
            "name": TOOL_SEARCH_DYNAMIC_NAME,
            "arguments": "{\"query\":\"calendar create\",\"limit\":2}"
        }))
        .unwrap();
        assert_eq!(output["type"], "tool_search_call");
        assert_eq!(output["execution"], "client");
        assert_eq!(output["call_id"], "call_search");
        assert_eq!(output["arguments"]["query"], "calendar create");
        assert_eq!(output["arguments"]["limit"], 2);
        assert!(output.get("name").is_none());
    }

    #[tokio::test]
    async fn codex_tool_search_history_roundtrips_through_pinned_client() {
        let parsed = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": [
                    {
                        "type": "tool_search_call",
                        "id": "tsc_1",
                        "call_id": "call_search",
                        "status": "completed",
                        "execution": "client",
                        "arguments": {"query": "calendar create", "limit": 2}
                    },
                    {
                        "type": "tool_search_output",
                        "call_id": "call_search",
                        "status": "completed",
                        "execution": "client",
                        "tools": [{
                            "type": "function",
                            "name": "create_event",
                            "description": "Create a calendar event",
                            "parameters": {"type": "object"}
                        }]
                    },
                    {
                        "role": "user",
                        "content": "Continue."
                    }
                ]
            }),
        )
        .unwrap();
        assert_eq!(parsed.input.canonical_items[0]["type"], "tool_search_call");
        assert_eq!(
            parsed.input.canonical_items[1]["type"],
            "tool_search_output"
        );

        let prepared = prepare_turn(&gateway(), "tenant", parsed).await.unwrap();
        assert_eq!(prepared.turn.injected_items[0]["type"], "function_call");
        assert_eq!(
            prepared.turn.injected_items[0]["name"],
            TOOL_SEARCH_DYNAMIC_NAME
        );
        assert_eq!(
            prepared.turn.injected_items[0]["arguments"],
            r#"{"limit":2,"query":"calendar create"}"#
        );
        assert_eq!(
            prepared.turn.injected_items[1]["type"],
            "function_call_output"
        );
        assert_eq!(prepared.turn.injected_items[1]["call_id"], "call_search");
        let output: Value =
            serde_json::from_str(prepared.turn.injected_items[1]["output"].as_str().unwrap())
                .unwrap();
        assert_eq!(output["execution"], "client");
        assert_eq!(output["tools"][0]["name"], "create_event");
        assert_eq!(prepared.full_history_prefix[0]["type"], "tool_search_call");
        assert_eq!(
            prepared.full_history_prefix[1]["type"],
            "tool_search_output"
        );
    }

    #[test]
    fn custom_tool_call_normalization_preserves_only_public_fields() {
        let output = normalize_output_item(&json!({
            "type": "custom_tool_call",
            "id": "ctc_1",
            "call_id": "call_1",
            "name": "exec",
            "input": "text('ok')",
            "internal_provider_metadata": {"must": "not escape"}
        }))
        .unwrap();
        assert_eq!(output["type"], "custom_tool_call");
        assert_eq!(output["input"], "text('ok')");
        assert!(output.get("internal_provider_metadata").is_none());
    }

    #[test]
    fn reasoning_encrypted_content_requires_explicit_include() {
        let default_request =
            parse_responses_request(&gateway(), json!({"model": "gpt-5.6", "input": "hi"}))
                .unwrap();
        assert!(!default_request.include_encrypted_reasoning);

        let included_request = parse_responses_request(
            &gateway(),
            json!({
                "model": "gpt-5.6",
                "input": "hi",
                "include": ["reasoning.encrypted_content"]
            }),
        )
        .unwrap();
        assert!(included_request.include_encrypted_reasoning);
    }

    #[tokio::test]
    async fn reasoning_completion_events_use_authoritative_summary_text() {
        let (sender, mut receiver) = mpsc::channel(8);
        let mut sequence = 0;
        emit_reasoning_item_added(&sender, &mut sequence, 0, "rs_1").await;
        emit_reasoning_summary_part_added(&sender, &mut sequence, 0, "rs_1", 0).await;
        let state = StreamReasoningState {
            output_index: 0,
            parts: BTreeMap::from([(0, "partial".to_string())]),
        };
        emit_completed_reasoning_item(
            &sender,
            &mut sequence,
            "rs_1",
            &state,
            &json!({
                "id": "rs_1",
                "type": "reasoning",
                "status": "completed",
                "summary": [{"type": "summary_text", "text": "final"}]
            }),
        )
        .await;
        drop(sender);

        let frames = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 5);
        assert!(frames[0].starts_with("event: response.output_item.added\n"));
        assert!(frames[1].starts_with("event: response.reasoning_summary_part.added\n"));
        assert!(frames[2].contains("\"type\":\"response.reasoning_summary_text.done\""));
        assert!(frames[2].contains("\"text\":\"final\""));
        assert!(frames[3].contains("\"type\":\"response.reasoning_summary_part.done\""));
        assert!(frames[4].starts_with("event: response.output_item.done\n"));
    }

    #[tokio::test]
    async fn custom_tool_call_emits_the_responses_stream_lifecycle() {
        let (sender, mut receiver) = mpsc::channel(8);
        let mut sequence = 0;
        assert!(
            emit_completed_item(
                &sender,
                &mut sequence,
                0,
                &json!({
                    "id": "ctc_1",
                    "type": "custom_tool_call",
                    "status": "completed",
                    "call_id": "call_1",
                    "name": "exec",
                    "input": "text('ok')"
                }),
            )
            .await
        );
        drop(sender);

        let frames = std::iter::from_fn(|| receiver.try_recv().ok())
            .map(|frame| String::from_utf8(frame.to_vec()).unwrap())
            .collect::<Vec<_>>();
        assert_eq!(frames.len(), 4);
        assert!(frames[0].starts_with("event: response.output_item.added\n"));
        assert!(frames[0].contains("\"input\":\"\""));
        assert!(frames[1].starts_with("event: response.custom_tool_call_input.delta\n"));
        assert!(frames[1].contains("\"delta\":\"text('ok')\""));
        assert!(frames[2].starts_with("event: response.custom_tool_call_input.done\n"));
        assert!(frames[2].contains("\"input\":\"text('ok')\""));
        assert!(frames[3].starts_with("event: response.output_item.done\n"));
    }

    #[tokio::test]
    async fn sse_send_stops_immediately_after_downstream_disconnect() {
        let (sender, receiver) = mpsc::channel(1);
        drop(receiver);
        assert!(!send_sse(&sender, "response.test", json!({"type": "response.test"})).await);
    }
}
