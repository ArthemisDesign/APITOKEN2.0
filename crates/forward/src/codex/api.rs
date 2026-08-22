//! OpenAI-compatible `/v1/responses` and model-discovery HTTP surface.

use super::billing::{
    begin_admission, AdmissionError, CodexBillableRequestSpec, CodexRequestFactSeed,
};
use super::{
    new_id, CodexGateway, CodexModel, CodexTurnRequest, CodexTurnResult, CodexUsage, HistoryError,
    ProcessError, StoredHistory, TurnUpdate,
};
use crate::proxy::{
    authorize, read_body_bounded, with_not_started, Authz, BodyAdmitError, TerminalErrorReason,
    UNSUPPORTED_CONTENT_ENCODING_MESSAGE,
};
use crate::request_classification::{classify_openai_responses, RequestClassification};
use crate::state::AppState;
use crate::validation::{optional_bool as strict_optional_bool, optional_positive_u64};
use axum::body::Body;
use axum::extract::{ConnectInfo, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::Stream;
use registry::request_facts::MAX_REQUEST_FACT_MODEL_LEN;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::sync::mpsc;

pub(super) const OPENAI_BODY_LIMIT: usize =
    api_limits::current::OPENAI_TEXT_REQUEST.bytes() as usize;
const _: () = assert!(OPENAI_BODY_LIMIT == 256 * 1024 * 1024);
pub(super) const MAX_INSTRUCTIONS_BYTES: usize = 16 * 1024 * 1024;
/// Sanity bound against a pathological body, not a model of the provider's own tool-count limit —
/// the 256 MiB body cap is what really bounds parsing work, and the backend is the authority on how
/// many tools it accepts. It sits far above any real client: a Codex config wiring several MCP
/// servers routinely declares a few hundred tools (namespace children included), and the old
/// 128-tool cap would have failed those turns locally with a deterministic 400 before the provider
/// ever saw them.
const MAX_TOOLS: usize = 1024;
const MAX_CUSTOM_TOOL_GRAMMAR_BYTES: usize = 4 * 1024 * 1024;
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

    pub(super) fn request_body_too_large() -> Self {
        Self::invalid(
            format!(
                "Request body exceeds the {} limit.",
                api_limits::current::OPENAI_TEXT_REQUEST
            ),
            None::<String>,
        )
    }

    pub(super) fn unsupported_content_encoding() -> Self {
        Self {
            status: StatusCode::UNSUPPORTED_MEDIA_TYPE,
            message: UNSUPPORTED_CONTENT_ENCODING_MESSAGE.to_string(),
            kind: "invalid_request_error",
            param: None,
            code: Some("unsupported_content_encoding"),
            retry_after: None,
            reason: "unsupported_content_encoding",
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
        if matches!(
            self.reason,
            "claudestore_fallback_failed" | "codex_missing_authoritative_usage"
        ) {
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
        let diagnostic = format!("codex run_turn rejected: {value}");
        match value {
            ProcessError::ExternalFallbackFailed { local } => {
                elog::warn("codex", &diagnostic);
                let mut error = Self::from(*local);
                error.reason = "claudestore_fallback_failed";
                error
            }
            ProcessError::ContextWindowExceeded => {
                elog::warn("codex", &diagnostic);
                Self {
                    status: StatusCode::BAD_REQUEST,
                    message: "This model's maximum context length was exceeded.".to_string(),
                    kind: "invalid_request_error",
                    param: Some("input".to_string()),
                    code: Some("context_length_exceeded"),
                    retry_after: None,
                    reason: "context_window_exceeded",
                }
            }
            ProcessError::UsageLimitExceeded { retry_after } => {
                elog::warn("codex", &diagnostic);
                Self::rate_limited_for_reason(
                    retry_after.or(Some(60)),
                    "codex_upstream_usage_limit",
                )
            }
            ProcessError::BadRequest => {
                elog::warn("codex", &diagnostic);
                Self::invalid(
                    "The request could not be processed by the selected model.",
                    None::<String>,
                )
            }
            ProcessError::PolicyViolation => {
                elog::warn("codex", &diagnostic);
                Self {
                    status: StatusCode::BAD_REQUEST,
                    message: "This request was blocked by provider safety policy.".to_string(),
                    kind: "invalid_request_error",
                    param: None,
                    code: Some("misalignment_policy_violation"),
                    retry_after: None,
                    reason: "provider_policy_violation",
                }
            }
            ProcessError::MissingAuthoritativeUsage => {
                elog::error("codex", &diagnostic);
                Self::unavailable_for("codex_missing_authoritative_usage")
            }
            other => {
                elog::error(
                    "codex",
                    format!(
                        "Codex provider request failed [{}]",
                        other.diagnostic_class()
                    ),
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
    reasoning_context: Option<String>,
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
    let bounded_body = match read_body_bounded(
        &app,
        &parts.headers,
        body,
        api_limits::current::OPENAI_TEXT_REQUEST,
    )
    .await
    {
        Ok(body) => body,
        Err(BodyAdmitError::ContentEncoding) => {
            return ApiError::unsupported_content_encoding().into_response()
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::TooLarge))
        | Err(BodyAdmitError::Storage(bounded_body::StorageError::ArithmeticOverflow)) => {
            return ApiError::request_body_too_large().into_response()
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::Io)) => {
            return ApiError::invalid("Could not read request body.", None::<String>)
                .into_response()
        }
        Err(BodyAdmitError::Storage(_)) => {
            return ApiError::unavailable_for("codex_body_storage_unavailable").into_response()
        }
    };
    let raw = bounded_body.bytes.clone();
    let _body_lease = bounded_body._lease;
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return ApiError::invalid("Invalid JSON in request body.", None::<String>)
                .into_response()
        }
    };
    let classification = classify_openai_responses(&value);
    let requested_model = value
        .get("model")
        .and_then(Value::as_str)
        .and_then(bounded_request_fact_model);
    let parsed = match parse_responses_request(&gateway, value) {
        Ok(parsed) => parsed,
        Err(error) => return error.into_response(),
    };
    let tenant_scope = pending.tenant_scope().to_string();
    let mut prepared = match prepare_turn(&gateway, &tenant_scope, parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return error.into_response(),
    };
    let routing = build_turn_routing(&app, &tenant_scope, &parts.headers, &prepared).await;
    let fact_seed = pending.request_fact_seed(
        parts.extensions.get::<crate::execution::LogicalRequestId>(),
        parts
            .extensions
            .get::<crate::execution::ClientAttribution>(),
        parts
            .extensions
            .get::<crate::execution::RequestLifecycleClock>(),
        pool::now(),
    );
    let billable_fact = fact_seed.map(|seed| {
        let spec = CodexBillableRequestSpec::native_responses(
            requested_model,
            bounded_request_fact_model(&prepared.request.public_model.id),
            prepared.request.stream,
            classification,
        );
        (seed, spec)
    });
    let admission = match pending
        .reserve(
            &app,
            &prepared.request.public_model,
            prepared.estimated_input_tokens,
            prepared.request.max_output_tokens,
            gateway.config().reserve_overhead_tokens,
            prepared.request.service_tier.is_some(),
            billable_fact,
        )
        .await
    {
        Ok(admission) => admission,
        Err(error) => return ApiError::from(error).into_response(),
    };
    prepared.turn.attempts = admission.attempt_observer();
    let response_id = new_id("resp");
    let created_at = pool::now();

    if prepared.request.stream {
        // Reject before opening the SSE stream if the whole pool is genuinely unavailable, so the client
        // sees a real 429 + Retry-After instead of a 200 that fails mid-stream.
        if let Err(error) = gateway
            .preflight_capacity(&prepared.request.public_model)
            .await
        {
            admission.settle_error(&error);
            return ApiError::from(error).into_response();
        }
        if let Err(error) = admission.mark_delivering().await {
            elog::error("codex", "codex delivery marker failed");
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
        Err(error) => {
            admission.settle_error(&error);
            return ApiError::from(error).into_response();
        }
    };
    if let Err(error) = admission.mark_delivering().await {
        elog::error("codex", "codex delivery marker failed after completed turn");
        admission.settle_after_delivery_marker_failure(
            &prepared.request.public_model,
            &result,
            prepared.request.max_output_tokens,
            result.effective_service_tier.as_deref() == Some("priority"),
        );
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
        &result,
        prepared.request.max_output_tokens,
        result.effective_service_tier.as_deref() == Some("priority"),
        None,
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
    let mut data = public_model_objects(&gateway, available.as_ref());
    data.extend(public_image_model_objects());
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

/// Discovery entries for the Images API models.
///
/// They are not part of `config().models`: the image pool has no upstream text catalog to
/// intersect with, and its serving reality is exactly "the image routes are mounted". Their
/// capability block is the honest one — image output, no streaming, no tools, no reasoning — so a
/// client that reads modalities never sends them to a text lane.
fn public_image_model_objects() -> Vec<Value> {
    super::PUBLIC_IMAGE_MODEL_IDS
        .iter()
        .map(|id| image_model_object(id))
        .collect()
}

fn image_model_object(id: &str) -> Value {
    json!({
        "id": id,
        "object": "model",
        "created": metering::GPT_IMAGE_2_CREATED,
        "owned_by": "apitoken",
        "apitoken": {
            "endpoints": ["/v1/images/generations", "/v1/images/edits"],
            "capabilities": {
                "reasoning_efforts": [],
                "service_tiers": ["standard"],
                "input_modalities": ["text", "image"],
                "output_modalities": ["image"],
                "tool_calling": false,
                "structured_outputs": false,
                "reasoning": false,
                "streaming": false
            }
        }
    })
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
    // The image models are served by the mounted image routes, not by the text pool, so they are
    // answered before the upstream text-catalog intersection below.
    if let Some(id) = super::PUBLIC_IMAGE_MODEL_IDS
        .iter()
        .find(|id| **id == model_id)
    {
        return json_response(StatusCode::OK, image_model_object(id), &new_id("req"));
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
        error @ (HistoryError::TooLarge | HistoryError::Corrupt | HistoryError::Unavailable) => {
            elog::error(
                "codex",
                format!("codex history unavailable for read: {error}"),
            );
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
    let admitted_at = pool::now();
    let fact_seed = pending.request_fact_seed(
        parts.extensions.get::<crate::execution::LogicalRequestId>(),
        parts
            .extensions
            .get::<crate::execution::ClientAttribution>(),
        parts
            .extensions
            .get::<crate::execution::RequestLifecycleClock>(),
        admitted_at,
    );
    let tenant_scope = pending.tenant_scope().to_owned();
    let (response, evidence) =
        input_tokens_after_admission(&app, gateway, &tenant_scope, &parts.headers, body).await;
    submit_input_tokens_fact(
        app.billing.as_deref(),
        fact_seed,
        response.status(),
        evidence,
    );
    response
}

#[derive(Default)]
struct InputTokensFactEvidence {
    requested_model: Option<String>,
    executable_model: Option<String>,
    classification: Option<RequestClassification>,
}

async fn input_tokens_after_admission(
    app: &AppState,
    gateway: Arc<CodexGateway>,
    tenant_scope: &str,
    headers: &HeaderMap,
    body: Body,
) -> (Response, InputTokensFactEvidence) {
    let bounded = match read_body_bounded(
        app,
        headers,
        body,
        api_limits::current::OPENAI_TEXT_REQUEST,
    )
    .await
    {
        Ok(body) => body,
        Err(BodyAdmitError::ContentEncoding) => {
            return (
                ApiError::unsupported_content_encoding().into_response(),
                InputTokensFactEvidence::default(),
            )
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::TooLarge))
        | Err(BodyAdmitError::Storage(bounded_body::StorageError::ArithmeticOverflow)) => {
            return (
                ApiError::request_body_too_large()
                    .into_response(),
                InputTokensFactEvidence::default(),
            )
        }
        Err(BodyAdmitError::Storage(bounded_body::StorageError::Io)) => {
            return (
                ApiError::invalid("Could not read request body.", None::<String>).into_response(),
                InputTokensFactEvidence::default(),
            )
        }
        Err(BodyAdmitError::Storage(_)) => {
            return (
                ApiError::unavailable().into_response(),
                InputTokensFactEvidence::default(),
            )
        }
    };
    let raw = bounded.bytes.clone();
    let _body_lease = bounded._lease;
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => {
            return (
                ApiError::invalid("Invalid JSON in request body.", None::<String>).into_response(),
                InputTokensFactEvidence::default(),
            )
        }
    };
    // These are untrusted, content-free candidates only. The owning Responses parser below is the
    // acceptance boundary; no field is published if parsing rejects the client value.
    let classification_candidate = classify_openai_responses(&value);
    let requested_model_candidate = value
        .get("model")
        .and_then(Value::as_str)
        .and_then(bounded_request_fact_model);
    let parsed = match parse_responses_request(&gateway, value) {
        Ok(parsed) => parsed,
        Err(error) => return (error.into_response(), InputTokensFactEvidence::default()),
    };
    let evidence = InputTokensFactEvidence {
        requested_model: requested_model_candidate,
        executable_model: bounded_request_fact_model(&parsed.public_model.id),
        classification: Some(classification_candidate),
    };
    let prepared = match prepare_turn(&gateway, tenant_scope, parsed).await {
        Ok(prepared) => prepared,
        Err(error) => return (error.into_response(), evidence),
    };
    let input_tokens = prepared.estimated_input_tokens;
    (
        json_response(
            StatusCode::OK,
            json!({
                "object": "response.input_tokens",
                "input_tokens": input_tokens
            }),
            &new_id("req"),
        ),
        evidence,
    )
}

pub(super) fn bounded_request_fact_model(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= MAX_REQUEST_FACT_MODEL_LEN
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control()))
    .then(|| value.to_owned())
}

fn submit_input_tokens_fact(
    billing: Option<&crate::billing::AsyncBilling>,
    fact_seed: Option<CodexRequestFactSeed>,
    status: StatusCode,
    evidence: InputTokensFactEvidence,
) {
    let (Some(billing), Some(fact_seed)) = (billing, fact_seed) else {
        return;
    };
    let fact = fact_seed.terminal_input_tokens_fact(
        status,
        evidence.requested_model,
        evidence.executable_model,
        evidence.classification,
    );
    let _ = billing.try_submit_terminal_request_fact(fact);
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
    context_window: Option<u64>,
    display_name: Option<&str>,
) -> Value {
    let mut limits = Map::from_iter([("output".to_string(), Value::from(model.max_output_tokens))]);
    if let Some(context) = context_window {
        if let Some(input) = context.checked_sub(model.max_output_tokens) {
            if input > 0 {
                limits.insert("context".to_string(), Value::from(context));
                limits.insert("input".to_string(), Value::from(input));
            }
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

/// Drop replayed reasoning items that carry no `encrypted_content`. The ChatGPT Responses backend
/// accepts a client-replayed reasoning item only when it holds the encrypted continuation state;
/// without it the item is dead weight the backend cannot resolve, and a live probe (2026-08-18)
/// showed the whole turn failing with `The request could not be processed by the selected model`.
/// Items that do carry the key are kept: replaying them preserves reasoning continuity and is
/// exactly what the official client does. The remaining history is structurally identical to a
/// conversation that simply never had reasoning items — which the backend already accepts.
fn drop_unencrypted_reasoning(items: Vec<Value>) -> (Vec<Value>, usize) {
    let mut dropped = 0usize;
    let kept = items
        .into_iter()
        .filter(|item| {
            let is_unencrypted_reasoning = item.get("type").and_then(Value::as_str)
                == Some("reasoning")
                && item
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .is_none();
            if is_unencrypted_reasoning {
                dropped += 1;
            }
            !is_unencrypted_reasoning
        })
        .collect();
    (kept, dropped)
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
            Err(error @ HistoryError::Unavailable) => {
                elog::error(
                    "codex",
                    format!("codex history unavailable for prepare_turn: {error}"),
                );
                return Err(ApiError::unavailable());
            }
            Err(error @ (HistoryError::TooLarge | HistoryError::Corrupt)) => {
                elog::error(
                    "codex",
                    format!("codex history unavailable for prepare_turn: {error}"),
                );
                return Err(ApiError::unavailable());
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
    // Replayed reasoning without its encrypted continuation key is unresolvable upstream and
    // fails the whole turn; strip it before the body is built. The canonical items and the stored
    // history keep the item untouched — this only affects what the backend is asked to accept.
    let (injected, dropped_reasoning) = drop_unencrypted_reasoning(injected);
    if dropped_reasoning > 0 {
        elog::info(
            "codex",
            format!(
                "dropped {dropped_reasoning} replayed reasoning item(s) without encrypted_content"
            ),
        );
    }
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
        parallel_tool_calls: request.parallel_tool_calls,
        service_tier: request.service_tier.clone(),
        reasoning_effort: request.reasoning_effort.clone(),
        reasoning_summary: request.reasoning_summary.clone(),
        reasoning_context: request.reasoning_context.clone(),
        output_schema: request.output_schema.clone(),
        verbosity: request.verbosity.clone(),
        attempts: None,
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
        // The image models are published in `/v1/models`, so "does not exist" would be a lie
        // here. Fail closed with the endpoint that actually serves them instead of pretending a
        // text lane could run an image model.
        if super::PUBLIC_IMAGE_MODEL_IDS.contains(&model_id) {
            return ApiError::invalid(
                format!(
                    "The model '{requested_model_id}' is an image model. Use \
                     POST /v1/images/generations or POST /v1/images/edits."
                ),
                Some("model".to_string()),
            );
        }
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
            "instructions exceeds the 16 MiB limit.",
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
    // `prompt_cache_key` never reaches upstream verbatim: the runner passes it through
    // `bounded_cache_key`, which hashes anything the native backend would not take. So an
    // unusual key is normalized there, never rejected here — an empty one simply means "no key",
    // and the client keeps seeing its own value echoed in the response.
    let prompt_cache_key =
        optional_string(object, "prompt_cache_key")?.filter(|key| !key.trim().is_empty());
    // `client_metadata` and `safety_identifier` are the caller's own diagnostic fields. The
    // transport rebuilds `client_metadata` from OUR wire identity and the public response pins
    // `safety_identifier` to null, so neither one can leave the gateway. Validating a discarded
    // field can only reject otherwise-valid traffic: newer Codex CLI builds ship a turn-metadata
    // blob that tripped the old size/control-character gate and made every turn fail with a
    // deterministic 400. Accept any shape for both and ignore them.
    let service_tier = parse_service_tier(object.get("service_tier"), &public_model);
    let (reasoning_effort, reasoning_summary, reasoning_context) =
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
        reasoning_context,
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

fn parse_reasoning(
    value: Option<&Value>,
    model: &CodexModel,
) -> Result<(Option<String>, Option<String>, Option<String>), ApiError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((None, None, None));
    };
    let object = value.as_object().ok_or_else(|| {
        ApiError::invalid(
            "reasoning must be an object.",
            Some("reasoning".to_string()),
        )
    })?;
    let context = optional_string(object, "context")?.filter(|context| context == "all_turns");
    let effort = optional_string(object, "effort")?;
    // An effort the model does not advertise degrades to the model default instead of failing
    // the request: SDKs pin effort names across providers and must not 400 on a mismatch.
    let effort = effort.filter(|effort| model.supports_effort(effort));
    let summary = optional_string(object, "summary")?;
    let summary = summary
        .filter(|summary| matches!(summary.as_str(), "auto" | "concise" | "detailed" | "none"));
    Ok((effort, summary, context))
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
                    // Codex CLI 0.147 moved the previously sibling Lark `exec` tool INTO the
                    // `functions` namespace, so a namespaced child is no longer function-only.
                    // Both forms are client-executed and keep their existing validation.
                    let parsed = match child_object.get("type").and_then(Value::as_str) {
                        Some("function") => parse_additional_function(child_object, &child_param)?,
                        Some("custom") => parse_additional_custom(child_object, &child_param)?,
                        _ => {
                            return Err(ApiError::invalid(
                                "Only function and custom tools are supported inside a namespace.",
                                Some(format!("{child_param}.type")),
                            ))
                        }
                    };
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
            // Hosted `web_search` is server-executed and billed per call by the provider, so this
            // gateway cannot meter it and never forwards it. Codex CLI sends the descriptor by
            // default (mode `cached`), and rejecting the list made every default Codex config
            // unusable on the models that carry it. Accept it as a declaration we cannot honor —
            // the same leniency this endpoint already applies to service_tier, tool_choice and
            // parallel_tool_calls — and drop it: the model simply gets no web search tool.
            //
            // Every other unknown tool type takes the same route. A descriptor this gateway does
            // not understand is never forwarded, so it can neither run nor bill; failing the whole
            // turn over it only breaks the caller on the next client release that adds a type,
            // which is precisely how the stock `web_search` descriptor once bricked every default
            // config. The turn proceeds with the tools we do understand. Namespace children stay
            // strict on purpose: there the type decides whether a member is client-executed.
            _ => {}
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
    // Unknown descriptor fields are ignored and `strict` is degraded rather than rejected, exactly
    // as on the top-level list: the same tool must not fail on the gpt-5.6 family (which carries
    // client tools in `additional_tools`) while it is accepted on every other model.
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
    // The dot is part of the charset on the top-level list (MCP-style `server.tool` names reach
    // this endpoint through both lists), so it is accepted here too — same tool, same verdict.
    if name.is_empty()
        || name.len() > max_len
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ApiError::invalid(
            format!(
                "Dynamic tool identifiers must be 1-{max_len} ASCII letters, digits, underscore, hyphen, or dot."
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
    // Same bound as the `additional_tools` path: one tool must not be accepted on the gpt-5.6
    // family and rejected on every other model just because the client puts it in a different list.
    if name.is_empty()
        || name.len() > 128
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err(ApiError::invalid(
            "Function name must be 1-128 ASCII letters, digits, underscore, hyphen, or dot.",
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
        "agent_message" => normalize_agent_message_item(object, index),
        // A history item type this gateway has no translation for is forwarded verbatim, like
        // `reasoning` and the tool-call items above. Input items are conversation history, never a
        // capability grant — the backend executes only what the tool list declares — so passing an
        // unknown one through cannot start an unmetered hosted call. What it does avoid is the
        // failure mode that keeps recurring here: the client ships a new item type (`agent_message`
        // was the last one) and every turn replaying that history dies locally on a deterministic
        // 400 until the gateway is taught the type. Now the authority on what the Responses
        // backend accepts is the backend itself.
        _ => Ok(item.clone()),
    }
}

/// Codex's multi-agent collaboration (spawn_agent / InterAgentCommunication) persists inter-agent
/// messages as `agent_message` history items and replays them into `input` on the next turn.
/// The Responses backend accepts no such type, so without this translation every turn that
/// follows an agent-to-agent message dies with `Input item type "agent_message" is not supported`.
/// An agent message is conversational content: a message addressed to the root agent is a new
/// user-level instruction, everything else is assistant output the model itself produced. The
/// private `author`/`recipient` agent paths are kept in the visible text, otherwise the model
/// loses who addressed whom; the transport only carries messages.
///
/// The client-owned `amsg_*` id is deliberately NOT carried upstream. Live probe (2026-08-14):
/// the backend treats an input `agent_message` id as a replay reference, fails the turn with an
/// in-stream `response.failed` (no code) when it cannot resolve it, and the whole retry loop then
/// reproduces the same failure five times. Dropping the id serves the same turn successfully.
fn normalize_agent_message_item(
    object: &Map<String, Value>,
    index: usize,
) -> Result<Value, ApiError> {
    let content = object
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ApiError::invalid(
                "agent_message content must be an array of text parts.",
                Some(format!("input.{index}.content")),
            )
        })?;
    let author = object
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("agent");
    let recipient = object
        .get("recipient")
        .and_then(Value::as_str)
        .unwrap_or("/root");
    let text = content
        .iter()
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n");
    let role = if recipient == "/root" {
        "user"
    } else {
        "assistant"
    };
    let message = canonical_message(
        role,
        &format!("Agent message from {author} to {recipient}:\n{text}"),
    );
    normalize_message_item(message.as_object().expect("message object"), index)
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
        elog::warn(
            "codex",
            format!("Codex response history persistence degraded: {error}"),
        );
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
            admission.record_downstream_disconnect_if_closed(&frame_tx);
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
            admission.record_downstream_disconnect_if_closed(&frame_tx);
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
        let mut emitted_messages = HashSet::<String>::new();
        let mut downstream_closed = false;
        let mut heartbeat = tokio::time::interval(SSE_HEARTBEAT_INTERVAL);
        heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        heartbeat.tick().await;

        'updates: loop {
            let update = tokio::select! {
                _ = frame_tx.closed() => {
                    admission.record_downstream_disconnect();
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                            admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                            admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                        admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                                    admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                                    admission.record_downstream_disconnect_if_closed(&frame_tx);
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
                                admission.record_downstream_disconnect_if_closed(&frame_tx);
                                downstream_closed = true;
                                break 'updates;
                            }
                            next_output_index += 1;
                        }
                    }
                }
                // Message items are the remaining case. Closing them here — at the provider's
                // own item boundary — is what keeps one streamed answer one message downstream.
                TurnUpdate::RawItem(item) => {
                    let Some(normalized) = normalize_output_item(&item) else {
                        continue;
                    };
                    let item_id = normalized
                        .get("id")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    if item_id.is_empty() || !emitted_messages.insert(item_id.clone()) {
                        continue;
                    }
                    match text_states.remove(&item_id) {
                        Some(state) => {
                            if !emit_completed_message_item(
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
                        }
                        // A message the provider produced without any delta still owes the client
                        // a full lifecycle, in the position the provider gave it.
                        None => {
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
                admission.settle_error(&error);
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
            Err(error) => {
                elog::error("codex", format!("codex stream task failed: {error}"));
                admission.settle_join_error();
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
            &result,
            prepared.request.max_output_tokens,
            result.effective_service_tier.as_deref() == Some("priority"),
            None,
        );
        if downstream_closed || frame_tx.is_closed() {
            return;
        }

        // Backfill only what the provider never closed for us: a message whose raw completion
        // never arrived (a truncated or non-conforming upstream turn). Everything the provider did
        // close was already emitted in order inside the loop above.
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
            if !emit_completed_message_item(&frame_tx, &mut sequence, item_id, state, &item).await {
                return;
            }
            emitted_messages.insert(item_id.clone());
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
            if !text_states.contains_key(id) && emitted_messages.insert(id.to_string()) {
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

/// Close one streamed assistant message: the text/part/item terminators the Responses
/// protocol owes an item that was opened by `response.output_item.added` and fed by deltas.
///
/// Callers must emit this at the provider's own item boundary. A client renders a streamed
/// message as a live cell and finalizes that cell as soon as the next output item opens; a
/// message closed after a later item has already come and gone therefore arrives as a *second*
/// message and is rendered twice.
async fn emit_completed_message_item(
    sender: &mpsc::Sender<Bytes>,
    sequence: &mut u64,
    item_id: &str,
    state: &StreamTextState,
    item: &Value,
) -> bool {
    if !send_sse(
        sender,
        "response.output_text.done",
        json!({
            "type": "response.output_text.done",
            "sequence_number": *sequence,
            "item_id": item_id,
            "output_index": state.output_index,
            "content_index": 0,
            "text": state.text,
            "logprobs": []
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
    if !send_sse(
        sender,
        "response.content_part.done",
        json!({
            "type": "response.content_part.done",
            "sequence_number": *sequence,
            "item_id": item_id,
            "output_index": state.output_index,
            "content_index": 0,
            "part": item.pointer("/content/0").cloned().unwrap_or(Value::Null)
        }),
    )
    .await
    {
        return false;
    }
    *sequence += 1;
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
    send_sse_bytes_with_timeout(sender, frame, STREAM_FRAME_SEND_TIMEOUT).await
}

async fn send_sse_bytes_with_timeout(
    sender: &mpsc::Sender<Bytes>,
    frame: Bytes,
    timeout: std::time::Duration,
) -> bool {
    matches!(
        tokio::time::timeout(timeout, sender.send(frame)).await,
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
mod tests;
