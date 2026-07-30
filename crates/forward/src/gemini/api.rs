//! Native Gemini-compatible surface backed by encrypted paid-subscription OAuth profiles.

use super::billing::{begin_admission, AdmissionError, GeminiAdmission};
use super::config::GeminiModel;
use super::pool::{GeminiGateway, GeminiLease, GeminiProfile, TokenError};
use super::transport::{TransportError, TransportResponse};
use crate::metrics::Metrics;
use crate::proxy::TerminalErrorReason;
use crate::state::AppState;
use crate::{AffinityInput, AffinityResolution};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use gemini_credential::OAuthKind;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const GEMINI_BODY_LIMIT: usize = 32 * 1024 * 1024;
const DOWNSTREAM_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_START_TIMEOUT: Duration = Duration::from_secs(30);
const STREAM_START_MAX_BYTES: usize = 1024 * 1024;
const STREAM_START_MAX_CHUNKS: usize = 1024;

/// Native Gemini accepts proto-JSON in either camelCase or snake_case. Code Assist and the public
/// surface are canonicalized to camelCase so a snake_case client is not silently dropped. Only the
/// documented top-level GenerateContentRequest fields are aliased; anything else is left untouched.
const REQUEST_FIELD_ALIASES: &[(&str, &str)] = &[
    ("system_instruction", "systemInstruction"),
    ("safety_settings", "safetySettings"),
    ("generation_config", "generationConfig"),
    ("tool_config", "toolConfig"),
    ("cached_content", "cachedContent"),
];

/// snake_case aliases for the recognized tool keys. Normalizing them keeps `validate_tools` and the
/// upstream wrapper consistent, and preserves the fail-closed rejection of googleMaps/fileSearch.
const TOOL_KEY_ALIASES: &[(&str, &str)] = &[
    ("function_declarations", "functionDeclarations"),
    ("code_execution", "codeExecution"),
    ("google_search", "googleSearch"),
    ("google_search_retrieval", "googleSearchRetrieval"),
    ("url_context", "urlContext"),
    ("computer_use", "computerUse"),
    ("google_maps", "googleMaps"),
    ("file_search", "fileSearch"),
];

/// Canonicalize a native request object in place: snake_case aliases become camelCase for the known
/// top-level fields and recognized tool keys. When both spellings are present the camelCase value
/// wins and the snake_case duplicate is discarded, matching Google's proto-JSON precedence.
fn canonicalize_native_request(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    for (snake, camel) in REQUEST_FIELD_ALIASES {
        if let Some(aliased) = object.remove(*snake) {
            object.entry((*camel).to_string()).or_insert(aliased);
        }
    }
    if let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) {
        for tool in tools {
            let Some(tool) = tool.as_object_mut() else {
                continue;
            };
            for (snake, camel) in TOOL_KEY_ALIASES {
                if let Some(aliased) = tool.remove(*snake) {
                    tool.entry((*camel).to_string()).or_insert(aliased);
                }
            }
        }
    }
}

/// A fresh native-shaped `responseId`: Google returns a short URL-safe base64 token on every
/// generateContent response and SSE chunk. We synthesize our own instead of exposing the Code
/// Assist wrapper `traceId`, which is a correlatable upstream identifier.
fn fresh_response_id() -> String {
    let mut bytes = [0u8; 9];
    getrandom::fill(&mut bytes).expect("operating-system CSPRNG unavailable");
    base64url(&bytes)
}

fn base64url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;
        out.push(ALPHABET[b0 >> 2] as char);
        out.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[b2 & 0x3f] as char);
        }
    }
    out
}

/// Both reviewed Cloud Code wrappers keep one UUID-shaped session id for a conversation. Derive it
/// from the affinity layer's keyed digest: growing histories keep their resolved lineage, while
/// different tenants or explicit sessions cannot collide through caller-controlled plaintext.
fn session_id_from_lineage(lineage: &str) -> String {
    let mut bytes = *blake3::hash(lineage.as_bytes()).as_bytes();
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

/// The legacy Gemini CLI wrapper identifies one top-level human turn as
/// `<session UUID>########<prompt count>`. Tool-result-only user contents stay inside the current
/// turn, so count only user contents that carry at least one non-function-response part. Native API
/// clients do not expose Gemini CLI's in-memory counter; transcript-derived ordinal is the closest
/// stable equivalent and preserves the exact official wire shape without accepting a caller id.
fn official_user_prompt_id(session_id: &str, native: &Value) -> String {
    let ordinal = native
        .get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|content| content.get("role").and_then(Value::as_str) != Some("model"))
        .filter(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .is_some_and(|parts| {
                    parts.iter().any(|part| {
                        part.as_object()
                            .is_none_or(|part| !part.contains_key("functionResponse"))
                    })
                })
        })
        .count()
        .max(1);
    format!("{session_id}########{ordinal}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Operation {
    Models,
    Model,
    Generate,
    StreamGenerate,
    CountTokens,
}

#[derive(Debug)]
struct ParsedRoute {
    operation: Operation,
    model: Option<String>,
}

/// How a streaming response is framed back to the client. Upstream Code Assist only speaks SSE, so
/// this only governs the downstream shape: `alt=sse` yields Server-Sent Events, and the native
/// default (no alt / alt=json) yields a streamed JSON array, exactly like generativelanguage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StreamFraming {
    Sse,
    JsonArray,
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: &'static str,
    google_status: &'static str,
    retry_after: Option<u64>,
    reason: &'static str,
    /// Public google.rpc.ErrorInfo.reason echoed in `error.details`. None omits the detail, which
    /// matches Google for generic malformed-request errors.
    error_info_reason: Option<&'static str>,
}

impl ApiError {
    fn invalid(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
            google_status: "INVALID_ARGUMENT",
            retry_after: None,
            reason: "invalid_request",
            error_info_reason: None,
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "Requested entity was not found.",
            google_status: "NOT_FOUND",
            retry_after: None,
            reason: "resource_not_found",
            error_info_reason: None,
        }
    }

    fn unavailable(reason: &'static str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "The service is currently unavailable. Please retry shortly.",
            google_status: "UNAVAILABLE",
            retry_after: Some(2),
            reason,
            error_info_reason: None,
        }
    }

    fn rate_limited(retry_after: Option<u64>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Resource has been exhausted. Please retry later.",
            google_status: "RESOURCE_EXHAUSTED",
            retry_after: retry_after.or(Some(60)),
            reason: "gemini_capacity_exhausted",
            error_info_reason: Some("RATE_LIMIT_EXCEEDED"),
        }
    }

    fn provider_rejected(status: StatusCode) -> Self {
        // Derive the HTTP status from the google.rpc status so the pair is one Google can actually
        // return (e.g. FAILED_PRECONDITION is always 400, never 413). Unknown deterministic client
        // rejections collapse to the native INVALID_ARGUMENT/400 pair.
        let (http, message, google_status) = match status.as_u16() {
            403 => (
                StatusCode::FORBIDDEN,
                "The caller does not have permission for this request.",
                "PERMISSION_DENIED",
            ),
            404 => (
                StatusCode::NOT_FOUND,
                "The requested model resource was not found.",
                "NOT_FOUND",
            ),
            409 => (
                StatusCode::CONFLICT,
                "The request could not be completed in its current state.",
                "ABORTED",
            ),
            412 => (
                StatusCode::BAD_REQUEST,
                "A precondition for this request was not satisfied.",
                "FAILED_PRECONDITION",
            ),
            _ => (
                StatusCode::BAD_REQUEST,
                "The model service rejected this request.",
                "INVALID_ARGUMENT",
            ),
        };
        Self {
            status: http,
            message,
            google_status,
            retry_after: None,
            reason: "gemini_request_rejected",
            error_info_reason: None,
        }
    }

    fn into_response(self) -> Response {
        // Build `error.details` the way generativelanguage does: an ErrorInfo (when we have a
        // machine reason) plus a RetryInfo whenever a retry delay is known.
        let mut details = Vec::new();
        if let Some(reason) = self.error_info_reason {
            details.push(json!({
                "@type": "type.googleapis.com/google.rpc.ErrorInfo",
                "reason": reason,
                "domain": "googleapis.com",
                "metadata": {"service": "generativelanguage.googleapis.com"}
            }));
        }
        if let Some(seconds) = self.retry_after {
            details.push(json!({
                "@type": "type.googleapis.com/google.rpc.RetryInfo",
                "retryDelay": format!("{}s", seconds.max(1))
            }));
        }
        let mut error = serde_json::Map::new();
        error.insert("code".to_string(), json!(self.status.as_u16()));
        error.insert("message".to_string(), json!(self.message));
        error.insert("status".to_string(), json!(self.google_status));
        if !details.is_empty() {
            error.insert("details".to_string(), json!(details));
        }
        let body = json!({ "error": Value::Object(error) });
        let mut response = (self.status, axum::Json(body)).into_response();
        if let Some(seconds) = self.retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.max(1).to_string()) {
                response.headers_mut().insert("retry-after", value);
            }
        }
        response
            .extensions_mut()
            .insert(TerminalErrorReason(self.reason));
        response
    }
}

impl From<AdmissionError> for ApiError {
    fn from(error: AdmissionError) -> Self {
        match error {
            // Real generativelanguage rejects an invalid API key as 400 INVALID_ARGUMENT with an
            // ErrorInfo reason of API_KEY_INVALID — not 401 UNAUTHENTICATED.
            AdmissionError::Unauthorized => Self {
                status: StatusCode::BAD_REQUEST,
                message: "API key not valid. Please pass a valid API key.",
                google_status: "INVALID_ARGUMENT",
                retry_after: None,
                reason: "invalid_key",
                error_info_reason: Some("API_KEY_INVALID"),
            },
            AdmissionError::Unavailable => Self::unavailable("gemini_admission_unavailable"),
            AdmissionError::Busy => Self::rate_limited(Some(1)),
            // Reseller balance is a documented account state the customer must be able to detect and
            // act on (top up), kept as the cross-provider 402 contract. The envelope stays native.
            AdmissionError::LowBalance => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message: "The account balance is insufficient for this request.",
                google_status: "FAILED_PRECONDITION",
                retry_after: None,
                reason: "billing_limit",
                error_info_reason: None,
            },
        }
    }
}

fn parse_route(method: &Method, path: &str) -> Result<ParsedRoute, ApiError> {
    if path == "/v1beta/models" && method == Method::GET {
        return Ok(ParsedRoute {
            operation: Operation::Models,
            model: None,
        });
    }
    let Some(tail) = path.strip_prefix("/v1beta/models/") else {
        return Err(ApiError::not_found());
    };
    if tail.is_empty() || tail.contains('/') {
        return Err(ApiError::not_found());
    }
    let (model, operation) = if let Some(model) = tail.strip_suffix(":generateContent") {
        (model, Operation::Generate)
    } else if let Some(model) = tail.strip_suffix(":streamGenerateContent") {
        (model, Operation::StreamGenerate)
    } else if let Some(model) = tail.strip_suffix(":countTokens") {
        (model, Operation::CountTokens)
    } else {
        (tail, Operation::Model)
    };
    if model.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(ApiError::not_found());
    }
    let expected = match operation {
        Operation::Model => Method::GET,
        Operation::Generate | Operation::StreamGenerate | Operation::CountTokens => Method::POST,
        Operation::Models => unreachable!(),
    };
    if method != expected {
        return Err(ApiError::not_found());
    }
    Ok(ParsedRoute {
        operation,
        model: Some(model.to_string()),
    })
}

fn model_version(id: &str) -> String {
    // Google exposes the family version (e.g. "2.5") in the model resource. Extract the first
    // "<major>.<minor>" numeric token from the id; fall back to the id when none is present.
    for token in id.split('-') {
        let mut parts = token.split('.');
        if let (Some(major), Some(minor), None) = (parts.next(), parts.next(), parts.next()) {
            if !major.is_empty()
                && major.bytes().all(|byte| byte.is_ascii_digit())
                && !minor.is_empty()
                && minor.bytes().all(|byte| byte.is_ascii_digit())
            {
                return token.to_string();
            }
        }
    }
    id.to_string()
}

fn model_value(model: &GeminiModel) -> Value {
    // Mirror the native ListModels/GetModel resource shape, including the sampling defaults Google
    // publishes for the Gemini families, so the catalogue is not a thin, obviously-synthetic subset.
    json!({
        "name": format!("models/{}", model.id),
        "version": model_version(&model.id),
        "displayName": model.display_name,
        "description": format!("Google {} model served through the Gemini API.", model.display_name),
        "inputTokenLimit": model.input_token_limit,
        "outputTokenLimit": model.output_token_limit,
        "supportedGenerationMethods": [
            "generateContent", "streamGenerateContent", "countTokens"
        ],
        "temperature": 1.0,
        "topP": 0.95,
        "topK": 64,
        "maxTemperature": 2.0
    })
}

#[derive(Debug, Clone, Copy)]
struct ListPage {
    start: usize,
    size: usize,
}

fn parse_list_models_query(query: Option<&str>) -> Result<ListPage, ApiError> {
    // Native ListModels supports pageSize (default 50, max 1000) and an opaque pageToken, and
    // ignores unknown query parameters. We encode the token as the start index of our small
    // catalogue, which stays opaque to the client.
    let mut size = 50usize;
    let mut start = 0usize;
    for part in query
        .unwrap_or_default()
        .split('&')
        .filter(|part| !part.is_empty())
    {
        let (raw_name, raw_value) = part.split_once('=').unwrap_or((part, ""));
        let name = percent_decode_query_name(raw_name)?;
        let value = percent_decode_query_name(raw_value)?;
        match name.as_str() {
            "pageSize" => {
                size = value
                    .parse::<usize>()
                    .map_err(|_| ApiError::invalid("The pageSize must be an integer."))?
                    .clamp(1, 1000);
            }
            "pageToken" => {
                start = value
                    .parse::<usize>()
                    .map_err(|_| ApiError::invalid("The page token is invalid."))?;
            }
            "key" | "api_key" => {
                return Err(ApiError::invalid(
                    "Query-string API keys are not accepted. Use the x-goog-api-key header.",
                ));
            }
            _ => {}
        }
    }
    Ok(ListPage { start, size })
}

fn parse_stream_query(
    query: Option<&str>,
    streaming: bool,
) -> Result<(String, StreamFraming), ApiError> {
    let mut saw_alt = false;
    // A streaming call with no alt yields the native JSON array; a non-streaming call never frames.
    let mut framing = StreamFraming::JsonArray;
    for part in query
        .unwrap_or_default()
        .split('&')
        .filter(|part| !part.is_empty())
    {
        let (raw_name, raw_value) = part.split_once('=').unwrap_or((part, ""));
        let name = percent_decode_query_name(raw_name)?;
        if name.eq_ignore_ascii_case("key") || name.eq_ignore_ascii_case("api_key") {
            return Err(ApiError::invalid(
                "Query-string API keys are not accepted. Use the x-goog-api-key header.",
            ));
        }
        if !name.eq_ignore_ascii_case("alt") {
            return Err(ApiError::invalid(
                "This query parameter is not supported by the Gemini gateway.",
            ));
        }
        if saw_alt || !streaming {
            return Err(ApiError::invalid(
                "This query parameter is not supported for the requested operation.",
            ));
        }
        let value = percent_decode_query_name(raw_value)?;
        framing = if value.eq_ignore_ascii_case("sse") {
            StreamFraming::Sse
        } else if value.eq_ignore_ascii_case("json") {
            StreamFraming::JsonArray
        } else {
            return Err(ApiError::invalid(
                "Streaming Gemini requests only support alt=sse or alt=json.",
            ));
        };
        saw_alt = true;
    }
    // Upstream Code Assist streams only via SSE regardless of how we frame the client response.
    let upstream = if streaming { "alt=sse" } else { "" };
    Ok((upstream.to_string(), framing))
}

fn percent_decode_query_name(raw: &str) -> Result<String, ApiError> {
    let mut decoded = Vec::with_capacity(raw.len());
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'%' if index + 2 < bytes.len() => {
                let high = hex_value(bytes[index + 1]);
                let low = hex_value(bytes[index + 2]);
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(ApiError::invalid(
                        "The query string contains invalid encoding.",
                    ));
                };
                decoded.push((high << 4) | low);
                index += 3;
            }
            b'%' => {
                return Err(ApiError::invalid(
                    "The query string contains invalid encoding.",
                ))
            }
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded)
        .map_err(|_| ApiError::invalid("The query string contains invalid encoding."))
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn retry_after(headers: &HeaderMap, body: &[u8], default_secs: i64) -> i64 {
    if let Some(seconds) = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
    {
        return seconds.clamp(1, 86_400);
    }
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(delay) = retry_info_delay(&value) {
            return delay;
        }
    }
    default_secs.clamp(1, 86_400)
}

fn retry_info_delay(value: &Value) -> Option<i64> {
    let delay = value
        .pointer("/error/details")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|detail| {
            detail
                .get("@type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind.ends_with("google.rpc.RetryInfo"))
        })?
        .get("retryDelay")?
        .as_str()?
        .strip_suffix('s')?
        .parse::<f64>()
        .ok()?;
    delay
        .is_finite()
        .then(|| (delay.ceil() as i64).clamp(1, 86_400))
}

fn generation_controls(body: &Value, model: &GeminiModel, overhead: u64) -> (u64, u64, bool) {
    let output = body
        .pointer("/generationConfig/maxOutputTokens")
        .and_then(Value::as_u64)
        .unwrap_or(model.output_token_limit)
        .clamp(1, model.output_token_limit);
    let grounding = body
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| {
            tools.iter().any(|tool| {
                tool.get("googleSearch").is_some()
                    || tool.get("google_search").is_some()
                    || tool.get("googleSearchRetrieval").is_some()
            })
        });
    // One input token cannot encode less than one body byte for inline UTF-8/base64 content. URI
    // content can exceed the body estimate; exact settlement plus the account overdraft floor keeps
    // that unavoidable provider-side expansion bounded.
    let estimate = (body.to_string().len() as u64).saturating_add(overhead);
    (estimate, output, grounding)
}

fn cap_generation_output(body: &mut Value, max_output_tokens: u64) -> Result<(), ApiError> {
    let object = body
        .as_object_mut()
        .ok_or_else(|| ApiError::invalid("The request body must be a JSON object."))?;
    let generation_config = object
        .entry("generationConfig")
        .or_insert_with(|| json!({}));
    let generation_config = generation_config
        .as_object_mut()
        .ok_or_else(|| ApiError::invalid("The generationConfig field must be a JSON object."))?;
    generation_config.insert("maxOutputTokens".to_string(), json!(max_output_tokens));
    Ok(())
}

fn validate_tools(body: &Value) -> Result<(), ApiError> {
    if body.get("cachedContent").is_some() {
        // A native cached-content resource is scoped to one Google project. It cannot safely
        // survive subscription rotation and may encode a caller-selected upstream identity.
        return Err(ApiError::invalid(
            "Explicit cachedContent resources are not supported by this gateway.",
        ));
    }
    let Some(tools) = body.get("tools").and_then(Value::as_array) else {
        return Ok(());
    };
    for tool in tools {
        let Some(tool) = tool.as_object() else {
            continue;
        };
        for name in tool.keys() {
            match name.as_str() {
                // These tools are either free beyond their model tokens or fully represented by
                // usageMetadata/toolUsePromptTokenCount. Search has its own exact settlement path.
                "functionDeclarations"
                | "codeExecution"
                | "googleSearch"
                | "googleSearchRetrieval"
                | "urlContext"
                | "computerUse" => {}
                // Maps has a separate $/grounded-prompt SKU and File Search can accrue embedding
                // charges not present in GenerateContent usageMetadata. Keep both fail-closed until
                // the ledger has dedicated authoritative dimensions for them.
                "googleMaps" | "fileSearch" => {
                    return Err(ApiError::invalid(
                        "This separately billed Gemini tool is not available through this gateway.",
                    ));
                }
                // A newly introduced server tool could add an unmetered provider SKU. Requiring an
                // explicit review is safer than silently proxying an unknown charge category.
                _ => {
                    return Err(ApiError::invalid(
                        "This Gemini tool type is not supported by this gateway.",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn translated_response(status: StatusCode, _headers: &HeaderMap, body: Bytes) -> Response {
    let mut response = Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap();
    response.headers_mut().insert(
        "content-type",
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn wrap_code_assist_request(
    operation: Operation,
    oauth_kind: OAuthKind,
    model: &str,
    project: &str,
    native: &Value,
    user_prompt_id: &str,
    session_id: Option<&str>,
    request_id: Option<&str>,
) -> Result<Bytes, ApiError> {
    let wrapped = match operation {
        Operation::Generate | Operation::StreamGenerate => {
            let native = native
                .as_object()
                .ok_or_else(|| ApiError::invalid("The request body must be a JSON object."))?;
            // Reconstruct the documented native request. Code Assist-only session, project and
            // identity fields supplied by a caller must never cross the public/private boundary.
            let mut request = serde_json::Map::new();
            for field in [
                "contents",
                "systemInstruction",
                "tools",
                "toolConfig",
                "safetySettings",
                "generationConfig",
            ] {
                if let Some(value) = native.get(field) {
                    request.insert(field.to_string(), value.clone());
                }
            }
            if let Some(session_id) = session_id {
                let field = match oauth_kind {
                    OAuthKind::Antigravity => "sessionId",
                    OAuthKind::LegacyGeminiCli => "session_id",
                };
                request.insert(field.to_string(), json!(session_id));
            }
            match oauth_kind {
                OAuthKind::Antigravity => json!({
                    "model": model,
                    "project": project,
                    "request": request,
                    "userAgent": "antigravity",
                    "requestType": "agent",
                    "requestId": request_id.unwrap_or_default(),
                }),
                OAuthKind::LegacyGeminiCli => json!({
                    "model": model,
                    "project": project,
                    "user_prompt_id": user_prompt_id,
                    "request": request,
                }),
            }
        }
        Operation::CountTokens => {
            let native = native
                .as_object()
                .ok_or_else(|| ApiError::invalid("The request body must be a JSON object."))?;
            let contents = native.get("contents").cloned().unwrap_or_else(|| json!([]));
            // Real countTokens only counts a system instruction / tool declarations when they are
            // supplied inside `generateContentRequest`; a bare top-level `contents` undercounts.
            // Mirror that: use the request form when the client sent anything beyond contents,
            // otherwise keep the minimal contents form for maximum upstream compatibility.
            let extra_fields = [
                "systemInstruction",
                "tools",
                "toolConfig",
                "generationConfig",
            ];
            if extra_fields.iter().any(|field| native.contains_key(*field)) {
                // Code Assist's countTokens `request` IS a GenerateContentRequest; it has no
                // `generateContentRequest` sub-field (upstream rejects that with
                // fieldViolations{field:"request"}). Inline model/contents/system/tools directly.
                let mut request = serde_json::Map::new();
                request.insert("model".to_string(), json!(format!("models/{model}")));
                request.insert("contents".to_string(), contents);
                for field in extra_fields {
                    if let Some(value) = native.get(field) {
                        request.insert(field.to_string(), value.clone());
                    }
                }
                json!({ "request": Value::Object(request) })
            } else {
                json!({
                    "request": {
                        "model": format!("models/{model}"),
                        "contents": contents,
                    }
                })
            }
        }
        Operation::Models | Operation::Model => return Err(ApiError::not_found()),
    };
    serde_json::to_vec(&wrapped)
        .map(Bytes::from)
        .map_err(|_| ApiError::invalid("The request body is not valid JSON."))
}

fn unwrap_code_assist_response(operation: Operation, bytes: &[u8]) -> Result<Bytes, ()> {
    let mut value: Value = serde_json::from_slice(bytes).map_err(|_| ())?;
    if operation == Operation::CountTokens {
        if !value.is_object() || !value.get("totalTokens").is_some_and(Value::is_number) {
            return Err(());
        }
        retain_public_fields(
            &mut value,
            &[
                "totalTokens",
                "cachedContentTokenCount",
                "promptTokensDetails",
                "cacheTokensDetails",
            ],
        )?;
        return serde_json::to_vec(&value).map(Bytes::from).map_err(|_| ());
    }
    if !value.is_object() {
        return Err(());
    }
    let mut native = match value
        .as_object_mut()
        .and_then(|object| object.remove("response"))
    {
        Some(native) if native.is_object() => native,
        Some(_) | None => return Err(()),
    };
    // The Code Assist wrapper can gain account, project, credit or trace fields without notice.
    // Reconstruct the documented native response instead of trusting the private envelope. In
    // particular, wrapper traceId is deliberately not exposed as responseId: it is a correlatable
    // upstream identifier, not a value supplied by the native Gemini surface.
    retain_public_fields(
        &mut native,
        &[
            "candidates",
            "promptFeedback",
            "usageMetadata",
            "modelVersion",
        ],
    )?;
    // Real generateContent responses always carry a responseId. Synthesize a native-shaped one
    // rather than exposing the correlatable Code Assist wrapper traceId.
    if let Some(object) = native.as_object_mut() {
        object.insert("responseId".to_string(), json!(fresh_response_id()));
    }
    serde_json::to_vec(&native).map(Bytes::from).map_err(|_| ())
}

/// Canonical google.rpc.Code status strings. Used to echo an upstream stream error's status only
/// when it is a known-safe enum value; anything else falls back to a generic INTERNAL.
fn is_google_rpc_status(status: &str) -> bool {
    matches!(
        status,
        "OK" | "CANCELLED"
            | "UNKNOWN"
            | "INVALID_ARGUMENT"
            | "DEADLINE_EXCEEDED"
            | "NOT_FOUND"
            | "ALREADY_EXISTS"
            | "PERMISSION_DENIED"
            | "UNAUTHENTICATED"
            | "RESOURCE_EXHAUSTED"
            | "FAILED_PRECONDITION"
            | "ABORTED"
            | "OUT_OF_RANGE"
            | "UNIMPLEMENTED"
            | "INTERNAL"
            | "UNAVAILABLE"
            | "DATA_LOSS"
    )
}

/// Build a sanitized native error value from a Code Assist stream wrapper that carried an `error`.
/// Only the numeric code and a known google.rpc status enum are echoed; the upstream message is
/// replaced so no account/project/endpoint detail can leak mid-stream. Framing is applied by the
/// caller so the element matches the client's SSE or JSON-array wire shape.
fn native_stream_error_value(wrapper: &Value) -> Option<Value> {
    let error = wrapper.get("error").filter(|value| value.is_object())?;
    let code = error
        .get("code")
        .and_then(Value::as_u64)
        .filter(|code| (400..600).contains(code))
        .unwrap_or(500);
    let status = error
        .get("status")
        .and_then(Value::as_str)
        .filter(|status| is_google_rpc_status(status))
        .unwrap_or("INTERNAL");
    Some(json!({
        "error": {
            "code": code,
            "message": "The model service returned an error while streaming.",
            "status": status,
        }
    }))
}

fn retain_public_fields(value: &mut Value, fields: &[&str]) -> Result<(), ()> {
    let object = value.as_object_mut().ok_or(())?;
    object.retain(|name, _| fields.contains(&name.as_str()));
    Ok(())
}

struct SseTranslator {
    pending: Vec<u8>,
    usage: metering::GeminiUsage,
    provider_error: Option<u16>,
    provider_retry_after: Option<i64>,
    response_id: String,
    framing: StreamFraming,
    started: bool,
}

impl SseTranslator {
    fn new(framing: StreamFraming) -> Self {
        Self {
            pending: Vec::new(),
            usage: metering::GeminiUsage::default(),
            provider_error: None,
            provider_retry_after: None,
            response_id: fresh_response_id(),
            framing,
            started: false,
        }
    }

    /// Frame one translated native value into the client's chosen wire shape.
    fn frame(&mut self, value: &Value) -> Result<Bytes, ()> {
        let encoded = serde_json::to_vec(value).map_err(|_| ())?;
        let mut framed = Vec::with_capacity(encoded.len() + 8);
        match self.framing {
            StreamFraming::Sse => {
                framed.extend_from_slice(b"data: ");
                framed.extend_from_slice(&encoded);
                framed.extend_from_slice(b"\n\n");
            }
            StreamFraming::JsonArray => {
                framed.extend_from_slice(if self.started { b"," } else { b"[" });
                self.started = true;
                framed.extend_from_slice(&encoded);
            }
        }
        Ok(Bytes::from(framed))
    }

    /// Closing bytes for the whole stream. SSE needs none; a JSON array must be terminated (or
    /// emitted as an empty array when no element was ever produced).
    fn finish_stream(&mut self) -> Option<Bytes> {
        match self.framing {
            StreamFraming::Sse => None,
            StreamFraming::JsonArray if self.started => Some(Bytes::from_static(b"]")),
            StreamFraming::JsonArray => Some(Bytes::from_static(b"[]")),
        }
    }

    fn push(&mut self, bytes: &[u8]) -> Result<Vec<Bytes>, ()> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > GEMINI_BODY_LIMIT {
            return Err(());
        }
        let mut output = Vec::new();
        while let Some((index, delimiter)) = event_boundary(&self.pending) {
            let event = self.pending.drain(..index).collect::<Vec<_>>();
            self.pending.drain(..delimiter);
            if let Some(chunk) = self.translate_event(&event)? {
                output.push(chunk);
            }
        }
        Ok(output)
    }

    fn translate_event(&mut self, event: &[u8]) -> Result<Option<Bytes>, ()> {
        let event = std::str::from_utf8(event).map_err(|_| ())?;
        let data = event
            .lines()
            .filter_map(|line| line.trim_end_matches('\r').strip_prefix("data:"))
            .map(str::trim_start)
            .collect::<Vec<_>>()
            .join("\n");
        if data.is_empty() {
            return Ok(None);
        }
        let mut wrapper: Value = serde_json::from_str(&data).map_err(|_| ())?;
        if !wrapper.is_object() {
            return Err(());
        }
        let Some(mut native) = wrapper
            .as_object_mut()
            .and_then(|object| object.remove("response"))
        else {
            // A mid-stream upstream error must reach the client as a native error element rather
            // than a clean truncation that looks like success. Genuinely private credit/accounting
            // events carry no `error` and have no public representation, so they stay consumed.
            if let Some(error) = native_stream_error_value(&wrapper) {
                self.provider_retry_after = retry_info_delay(&wrapper);
                self.provider_error = error
                    .pointer("/error/code")
                    .and_then(Value::as_u64)
                    .and_then(|code| u16::try_from(code).ok());
                return Ok(Some(self.frame(&error)?));
            }
            return Ok(None);
        };
        if !native.is_object() {
            return Err(());
        }
        retain_public_fields(
            &mut native,
            &[
                "candidates",
                "promptFeedback",
                "usageMetadata",
                "modelVersion",
            ],
        )?;
        if native.as_object().is_none_or(serde_json::Map::is_empty) {
            // Unknown/private response-only events have no public representation.
            return Ok(None);
        }
        metering::gemini::merge_stream_response_value(&mut self.usage, &native);
        // Real Gemini SSE chunks carry a stable responseId for the whole response; mirror it.
        if let Some(object) = native.as_object_mut() {
            object.insert("responseId".to_string(), json!(self.response_id));
        }
        Ok(Some(self.frame(&native)?))
    }

    fn finish_pending(&mut self) -> Result<Vec<Bytes>, ()> {
        if !self.pending.is_empty() {
            let event = std::mem::take(&mut self.pending);
            return Ok(self.translate_event(&event)?.into_iter().collect());
        }
        Ok(Vec::new())
    }
}

fn event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|i| (i, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|i| (i, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
}

fn account_stream_start_chunk(
    observed_bytes: &mut usize,
    observed_chunks: &mut usize,
    chunk_bytes: usize,
) -> Result<(), ()> {
    *observed_bytes = observed_bytes.saturating_add(chunk_bytes);
    *observed_chunks = observed_chunks.saturating_add(1);
    if *observed_bytes > STREAM_START_MAX_BYTES || *observed_chunks > STREAM_START_MAX_CHUNKS {
        return Err(());
    }
    Ok(())
}

#[derive(Debug)]
enum SendError {
    Token(TokenError),
    Transport,
}

async fn send_upstream(
    profile: &GeminiProfile,
    url: &str,
    _headers: &HeaderMap,
    body: Bytes,
    rejected_token: Option<&str>,
    user_agent: &str,
) -> Result<(TransportResponse, gemini_credential::SecretString), SendError> {
    let access_token = match rejected_token {
        Some(rejected) => profile.access_token_after_rejection(rejected).await,
        None => profile.access_token(false).await,
    }
    .map_err(SendError::Token)?;
    // No customer header is required by Code Assist. Constructing the complete upstream header
    // set locally prevents cookies, trace ids, origins or future identity headers from crossing the
    // provider boundary when a denylist inevitably becomes stale.
    let response = profile
        .request(
            url,
            &access_token,
            user_agent,
            (profile.oauth_kind() == OAuthKind::LegacyGeminiCli
                && !url.contains(":streamGenerateContent"))
            .then_some("application/json"),
            "application/json",
            body,
        )
        .await
        .map_err(|_| SendError::Transport)?;
    Ok((response, access_token))
}

async fn read_upstream_body(response: TransportResponse) -> Result<Bytes, ()> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| ())?;
        if body.len().saturating_add(chunk.len()) > GEMINI_BODY_LIMIT {
            return Err(());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

/// TEMPORARY streaming-diagnostic (secret-safe). The gateway deliberately collapses the private
/// Code Assist error envelope to a public status class, which hides why the Antigravity streaming
/// method is rejected while the identical non-streaming body is accepted. Log ONLY the public
/// google.rpc `code`/`status` and a project/profile-scrubbed, truncated `message` so the exact
/// rejection reason is observable in the journal. Remove together with the streaming fix.
fn debug_log_upstream_rejection(
    op: &str,
    http_status: StatusCode,
    body: &[u8],
    project: &str,
    profile_id: &str,
) {
    let (code, status, message) = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            let error = value.get("error")?;
            Some((
                error.get("code").and_then(Value::as_i64).unwrap_or(0),
                error
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            ))
        })
        .unwrap_or((0, String::new(), String::new()));
    let mut scrubbed = message;
    if !project.is_empty() {
        scrubbed = scrubbed.replace(project, "<project>");
    }
    if !profile_id.is_empty() {
        scrubbed = scrubbed.replace(profile_id, "<profile>");
    }
    let scrubbed: String = scrubbed.chars().take(240).collect();
    // Google's generic "Request contains an invalid argument." carries the field-level cause in
    // error.details[].fieldViolations; capture the whole scrubbed envelope (bounded) so the exact
    // rejected argument is visible without a second deploy.
    let mut raw = String::from_utf8_lossy(body).into_owned();
    if !project.is_empty() {
        raw = raw.replace(project, "<project>");
    }
    if !profile_id.is_empty() {
        raw = raw.replace(profile_id, "<profile>");
    }
    let raw: String = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let raw: String = raw.chars().take(700).collect();
    eprintln!(
        "GEMINI-DIAG upstream rejection op={op} http={} code={code} status={status} msg={scrubbed} body={raw}",
        http_status.as_u16()
    );
}

/// Fires at most ONCE per process (bounded cost, no env flip needed). On the already-failed
/// streaming path, replay the identical Antigravity body against the streaming method with three
/// query variants and log each upstream status + scrubbed body, so the exact accepted streaming
/// contract is observable. Removed together with the streaming fix.
static STREAM_PROBE_DONE: AtomicBool = AtomicBool::new(false);

async fn debug_probe_stream_variants(
    profile: &GeminiProfile,
    base_upstream: &str,
    body: Bytes,
    user_agent: &str,
    project: &str,
    profile_id: &str,
) {
    for variant in ["", "?alt=sse", "?alt=json"] {
        let url = format!("{base_upstream}/v1internal:streamGenerateContent{variant}");
        match send_upstream(
            profile,
            &url,
            &HeaderMap::new(),
            body.clone(),
            None,
            user_agent,
        )
        .await
        {
            Ok((response, _)) => {
                let status = response.status();
                let bytes = read_upstream_body(response).await.unwrap_or_default();
                let mut snippet = String::from_utf8_lossy(&bytes).into_owned();
                if !project.is_empty() {
                    snippet = snippet.replace(project, "<project>");
                }
                if !profile_id.is_empty() {
                    snippet = snippet.replace(profile_id, "<profile>");
                }
                let snippet: String = snippet
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(300)
                    .collect();
                eprintln!(
                    "GEMINI-DIAG stream-probe variant='{variant}' http={} body={snippet}",
                    status.as_u16()
                );
            }
            Err(_) => eprintln!("GEMINI-DIAG stream-probe variant='{variant}' send_error"),
        }
    }
}

async fn stream_response(
    gateway: Arc<GeminiGateway>,
    metrics: Arc<Metrics>,
    profile: Arc<GeminiProfile>,
    lease: GeminiLease,
    admission: GeminiAdmission,
    model: GeminiModel,
    status: StatusCode,
    headers: HeaderMap,
    mut translator: SseTranslator,
    initial: Vec<Bytes>,
    mut upstream: impl futures_util::Stream<Item = Result<Bytes, TransportError>>
        + Send
        + Unpin
        + 'static,
) -> Result<Response, ApiError> {
    let framing = translator.framing;
    // Register with the shutdown barrier before the durable delivery transition. Otherwise a
    // shutdown can observe zero background tasks, flush billing, and race a late mark/refund from
    // this narrow await window. No downstream byte is exposed until both steps have succeeded.
    let background = gateway
        .track_background_task()
        .map_err(|_| ApiError::unavailable("gemini_shutdown"))?;
    admission.mark_delivering().await?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::spawn(async move {
        let _background = background;
        let _lease = lease;
        let mut deliver = true;
        let mut clean_eof = true;
        let mut aborted = false;
        let mut private_bytes = 0usize;
        let mut private_chunks = 0usize;

        for chunk in initial {
            if deliver {
                tokio::select! {
                    _ = gateway.stream_abort_requested() => {
                        aborted = true;
                        break;
                    }
                    result = tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(chunk)) => {
                        match result {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) | Err(_) => deliver = false,
                        }
                    }
                }
            }
        }
        while !aborted {
            let chunk = tokio::select! {
                _ = gateway.stream_abort_requested() => {
                    aborted = true;
                    None
                }
                chunk = upstream.next() => chunk,
            };
            let Some(chunk) = chunk else {
                break;
            };
            match chunk {
                Ok(chunk) => {
                    let chunk_len = chunk.len();
                    let translated = match translator.push(&chunk) {
                        Ok(translated) => translated,
                        Err(()) => {
                            clean_eof = false;
                            break;
                        }
                    };
                    if translated.is_empty() {
                        if account_stream_start_chunk(
                            &mut private_bytes,
                            &mut private_chunks,
                            chunk_len,
                        )
                        .is_err()
                        {
                            clean_eof = false;
                            break;
                        }
                    } else {
                        private_bytes = 0;
                        private_chunks = 0;
                    }
                    for translated in translated {
                        if deliver {
                            match tokio::time::timeout(
                                DOWNSTREAM_SEND_TIMEOUT,
                                sender.send(translated),
                            )
                            .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(_)) | Err(_) => deliver = false,
                            }
                        }
                    }
                }
                Err(_) => {
                    clean_eof = false;
                    Metrics::inc(&metrics.upstream_5xx);
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    break;
                }
            }
        }
        if clean_eof && !aborted {
            match translator.finish_pending() {
                Ok(chunks) => {
                    for chunk in chunks {
                        if deliver {
                            let _ =
                                tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(chunk))
                                    .await;
                        }
                    }
                }
                Err(()) => clean_eof = false,
            }
        }
        // A JSON-array stream must be closed with `]` (or emitted as `[]` when empty); SSE needs no
        // terminator. Only close on a clean end — a truncated array mirrors a truncated SSE stream.
        if clean_eof && !aborted {
            if let Some(close) = translator.finish_stream() {
                if deliver {
                    let _ = tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(close)).await;
                }
            }
        }
        if !aborted {
            match translator.provider_error {
                Some(401 | 403) => {
                    Metrics::inc(&metrics.upstream_auth);
                    profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                }
                Some(429) => {
                    Metrics::inc(&metrics.upstream_429);
                    profile.cool_model_until(
                        &model.id,
                        pool::now()
                            + translator
                                .provider_retry_after
                                .unwrap_or(gateway.config().default_rate_limit_cool_secs),
                    );
                }
                Some(408 | 409 | 425 | 500..=599) => {
                    Metrics::inc(&metrics.upstream_5xx);
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                }
                Some(_) if clean_eof => profile.mark_healthy_for(&model.id),
                None if clean_eof => profile.mark_healthy_for(&model.id),
                _ => profile.cool_until(pool::now() + gateway.config().transport_cool_secs),
            }
        }
        let usage = (!translator.usage.is_zero()).then_some(&translator.usage);
        if usage.is_none() && admission.requires_usage() {
            Metrics::inc(&metrics.gemini_usage_missing);
        }
        admission.settle(&model, usage);
    });
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver
            .recv()
            .await
            .map(|chunk| (Ok::<Bytes, Infallible>(chunk), receiver))
    });
    let mut response = Response::builder()
        .status(status)
        .body(Body::from_stream(stream))
        .unwrap();
    let _ = headers;
    // SSE keeps its exact media type; the native JSON-array default is served as application/json.
    let content_type = match framing {
        StreamFraming::Sse => HeaderValue::from_static("text/event-stream; charset=utf-8"),
        StreamFraming::JsonArray => HeaderValue::from_static("application/json"),
    };
    response.headers_mut().insert("content-type", content_type);
    Ok(response)
}

async fn record_affinity_success(
    store: &Arc<crate::AffinityStore>,
    input: Option<&AffinityInput>,
    resolution: &mut Option<AffinityResolution>,
    profile_id: &str,
) {
    let Some(input) = input else {
        return;
    };
    let served_home = store.home_id(profile_id);
    match resolution {
        Some(resolution) => {
            if resolution.home != served_home {
                store.rebind(resolution, &served_home).await;
            }
            store.remember(input, resolution).await;
        }
        None => {
            let claimed = store.claim(input, &served_home).await;
            store.remember(input, &claimed).await;
            *resolution = Some(claimed);
        }
    }
    store.mark_cache_warm(input, &served_home);
}

pub async fn api(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    // A browser SDK (@google/genai) issues a CORS preflight before the cross-origin call; the real
    // endpoint answers it without auth. Handle it before routing, which otherwise 404s on OPTIONS.
    if request.method() == Method::OPTIONS {
        return cors_preflight_response();
    }
    let mut response = match api_inner(app, peer, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    apply_native_response_headers(&mut response);
    response
}

fn cors_preflight_response() -> Response {
    let mut response = Response::builder()
        .status(StatusCode::NO_CONTENT)
        .body(Body::empty())
        .unwrap();
    let headers = response.headers_mut();
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-allow-methods",
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        "access-control-allow-headers",
        HeaderValue::from_static(
            "Authorization, Content-Type, X-Goog-Api-Key, X-Goog-Api-Client, X-Goog-User-Project",
        ),
    );
    headers.insert("access-control-max-age", HeaderValue::from_static("3600"));
    headers.insert(
        "vary",
        HeaderValue::from_static("Origin, X-Origin, Referer"),
    );
    response
}

/// Decorate every Gemini response with the headers the real generativelanguage endpoint returns:
/// canonical content-type casing, the standard security headers, and permissive CORS so browser
/// SDKs can read the body. Applied uniformly to success, streaming and error responses.
fn apply_native_response_headers(response: &mut Response) {
    let headers = response.headers_mut();
    if let Some(current) = headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
    {
        let normalized = if current.starts_with("application/json") {
            Some(HeaderValue::from_static("application/json; charset=UTF-8"))
        } else if current.starts_with("text/event-stream") {
            Some(HeaderValue::from_static("text/event-stream"))
        } else {
            None
        };
        if let Some(normalized) = normalized {
            headers.insert("content-type", normalized);
        }
    }
    headers.insert(
        "vary",
        HeaderValue::from_static("Origin, X-Origin, Referer"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("SAMEORIGIN"));
    headers.insert("x-xss-protection", HeaderValue::from_static("0"));
    headers.insert("access-control-allow-origin", HeaderValue::from_static("*"));
    headers.insert(
        "access-control-expose-headers",
        HeaderValue::from_static("content-encoding, content-length, date, server, vary"),
    );
}

async fn api_inner(
    app: AppState,
    peer: SocketAddr,
    request: axum::extract::Request,
) -> Result<Response, ApiError> {
    let Some(gateway) = app.gemini.as_ref().cloned() else {
        return Err(ApiError::not_found());
    };
    let route = parse_route(request.method(), request.uri().path())?;
    let pending = begin_admission(&app, request.headers(), &peer).await?;

    if route.operation == Operation::Models {
        let page = parse_list_models_query(request.uri().query())?;
        let all = &gateway.config().models;
        let start = page.start.min(all.len());
        let end = start.saturating_add(page.size).min(all.len());
        let models = all[start..end].iter().map(model_value).collect::<Vec<_>>();
        let mut body = serde_json::Map::new();
        body.insert("models".to_string(), json!(models));
        if end < all.len() {
            body.insert("nextPageToken".to_string(), json!(end.to_string()));
        }
        let _admission = pending.without_reserve();
        return Ok((StatusCode::OK, axum::Json(Value::Object(body))).into_response());
    }
    let model_id = route.model.as_deref().ok_or_else(ApiError::not_found)?;
    let model = gateway
        .config()
        .model(model_id)
        .cloned()
        .ok_or_else(ApiError::not_found)?;
    if route.operation == Operation::Model {
        // A native GetModel ignores query parameters entirely.
        let _admission = pending.without_reserve();
        return Ok((StatusCode::OK, axum::Json(model_value(&model))).into_response());
    }

    // Only the upstream-bound operations carry an alt query; validate it here rather than for the
    // model-metadata routes, which do not reach Code Assist. `framing` decides the downstream wire
    // shape (SSE vs the native JSON array) and is only meaningful for a streaming operation.
    let (query, framing) = parse_stream_query(
        request.uri().query(),
        route.operation == Operation::StreamGenerate,
    )?;

    let (parts, body) = request.into_parts();
    let body = to_bytes(body, GEMINI_BODY_LIMIT)
        .await
        .map_err(|_| ApiError::invalid("The request body is invalid or too large."))?;
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::invalid("The request body is not valid JSON."))?;
    if !value.is_object() {
        return Err(ApiError::invalid("The request body must be a JSON object."));
    }
    // Accept proto-JSON snake_case (system_instruction, safety_settings, google_search, …) exactly
    // like the real API: normalize to camelCase up front so validation, reservation, the upstream
    // wrapper and settlement all see a single canonical shape instead of silently dropping fields.
    canonicalize_native_request(&mut value);
    validate_tools(&value)?;
    let affinity_input = pending.affinity_scope().and_then(|scope| {
        app.affinity
            .infer_gemini(scope, &parts.headers, model_id, &value)
    });
    let mut affinity_resolution = match affinity_input.as_ref() {
        Some(input) => app.affinity.resolve(input).await,
        None => None,
    };
    let generation = matches!(
        route.operation,
        Operation::Generate | Operation::StreamGenerate
    );
    let upstream_session_id = generation.then(|| {
        affinity_input
            .as_ref()
            .map_or_else(crate::fresh_request_id, |input| {
                let lineage = affinity_resolution
                    .as_ref()
                    .map(|resolution| resolution.session_id.as_str())
                    .unwrap_or_else(|| input.primary_lineage());
                session_id_from_lineage(&input.provider_lineage(lineage))
            })
    });
    let user_prompt_id = upstream_session_id
        .as_deref()
        .map(|session_id| official_user_prompt_id(session_id, &value))
        .unwrap_or_default();
    // Antigravity expects a fresh request id, but rotation must not turn one customer request into
    // multiple logical agent turns. Generate it once before selecting the first subscription.
    let upstream_request_id = generation.then(|| format!("agent-{}", crate::fresh_request_id()));
    let admission = if matches!(
        route.operation,
        Operation::Generate | Operation::StreamGenerate
    ) {
        let (input, output, grounding) =
            generation_controls(&value, &model, gateway.config().reserve_overhead_tokens);
        let (admission, effective_output) = pending
            .reserve(&app, &model, input, output, grounding)
            .await?;
        // Always write the validated ceiling: this also clamps a hostile value above the model
        // limit (and normalizes zero) even when the account can afford the complete request.
        cap_generation_output(&mut value, effective_output)?;
        admission
    } else {
        pending.without_reserve()
    };

    let suffix = match route.operation {
        Operation::Generate => "generateContent",
        Operation::StreamGenerate => "streamGenerateContent",
        Operation::CountTokens => "countTokens",
        Operation::Models | Operation::Model => unreachable!(),
    };
    let mut excluded = HashSet::new();
    let mut transport_failures = 0usize;
    let mut saw_quota = false;
    let mut saw_auth = false;
    loop {
        let preferred_id = affinity_resolution
            .as_ref()
            .and_then(|resolution| gateway.profile_id_for_home(&app.affinity, &resolution.home));
        let Some(lease) = gateway.select(&model.id, &excluded, preferred_id.as_deref()) else {
            Metrics::inc(&app.metrics.exhausted);
            let retry = gateway
                .soonest_ready(&model.id, &HashSet::new())
                .map(|until| until.saturating_sub(pool::now()).max(1) as u64);
            return if !gateway.has_authenticated_profiles() {
                Err(ApiError::unavailable("gemini_profiles_unauthenticated"))
            } else if saw_quota {
                Err(ApiError::rate_limited(retry))
            } else if saw_auth || transport_failures > 0 {
                Err(ApiError::unavailable("gemini_profiles_unavailable"))
            } else {
                Err(ApiError::rate_limited(retry))
            };
        };
        let profile = lease.profile().clone();
        let oauth_kind = profile.oauth_kind();
        let upstream_user_agent = gateway.config().user_agent(oauth_kind, &model.id);
        let mut url = format!(
            "{}/v1internal:{suffix}",
            gateway.config().upstream_for(oauth_kind)
        );
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        let project = profile.project_id().await;
        let upstream_body = wrap_code_assist_request(
            route.operation,
            oauth_kind,
            &model.id,
            &project,
            &value,
            &user_prompt_id,
            upstream_session_id.as_deref(),
            upstream_request_id.as_deref(),
        )?;
        let (mut response, rejected_token) = match send_upstream(
            &profile,
            &url,
            &parts.headers,
            upstream_body.clone(),
            None,
            &upstream_user_agent,
        )
        .await
        {
            Ok(response) => response,
            Err(SendError::Token(TokenError::Invalid)) => {
                Metrics::inc(&app.metrics.upstream_auth);
                saw_auth = true;
                excluded.insert(profile.id().to_string());
                profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                continue;
            }
            Err(SendError::Token(TokenError::Temporary) | SendError::Transport) => {
                Metrics::inc(&app.metrics.upstream_5xx);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                transport_failures += 1;
                if transport_failures > gateway.config().max_transport_retries {
                    return Err(ApiError::unavailable("gemini_transport_unavailable"));
                }
                continue;
            }
        };
        let mut status = response.status();

        // A bearer can be revoked before its local expiry. Refresh once on the same profile. The
        // rejected-token compare in the profile mutex ensures a concurrent 401 burst performs one
        // refresh rather than one refresh per request.
        if status == StatusCode::UNAUTHORIZED {
            Metrics::inc(&app.metrics.upstream_auth);
            match send_upstream(
                &profile,
                &url,
                &parts.headers,
                upstream_body,
                Some(&rejected_token),
                &upstream_user_agent,
            )
            .await
            {
                Ok((retried, _)) => {
                    response = retried;
                    status = response.status();
                }
                Err(SendError::Token(TokenError::Invalid)) => {
                    saw_auth = true;
                    excluded.insert(profile.id().to_string());
                    profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                    continue;
                }
                Err(SendError::Token(TokenError::Temporary) | SendError::Transport) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    transport_failures += 1;
                    if transport_failures > gateway.config().max_transport_retries {
                        return Err(ApiError::unavailable("gemini_token_refresh_unavailable"));
                    }
                    continue;
                }
            }
        }
        let response_headers = response.headers().clone();

        if status.is_success() && route.operation == Operation::StreamGenerate {
            let mut stream = response.bytes_stream();
            let mut translator = SseTranslator::new(framing);
            // Do not return 200 until at least one public native event exists, because retries are
            // forbidden after delivery. Bound this private prelude independently from per-event
            // framing: an upstream that emits endless credit/accounting events (or empty chunks)
            // must not hold a lease, customer reserve and global admission forever.
            let startup = tokio::time::timeout(STREAM_START_TIMEOUT, async {
                let mut observed_bytes = 0usize;
                let mut observed_chunks = 0usize;
                loop {
                    match stream.next().await {
                        Some(Ok(chunk)) => {
                            account_stream_start_chunk(
                                &mut observed_bytes,
                                &mut observed_chunks,
                                chunk.len(),
                            )?;
                            match translator.push(&chunk) {
                                Ok(translated) if translated.is_empty() => {}
                                Ok(translated) => return Ok(translated),
                                Err(()) => return Err(()),
                            }
                        }
                        Some(Err(_)) => return Err(()),
                        None => {
                            return match translator.finish_pending() {
                                Ok(translated) if !translated.is_empty() => Ok(translated),
                                Ok(_) | Err(()) => Err(()),
                            };
                        }
                    }
                }
            })
            .await;
            let initial = match startup {
                Ok(Ok(initial)) => initial,
                Ok(Err(())) | Err(_) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    transport_failures += 1;
                    if transport_failures > gateway.config().max_transport_retries {
                        return Err(ApiError::unavailable("gemini_stream_start_failed"));
                    }
                    continue;
                }
            };
            if let Some(code) = translator.provider_error {
                match code {
                    401 | 403 => {
                        Metrics::inc(&app.metrics.upstream_auth);
                        saw_auth = true;
                        excluded.insert(profile.id().to_string());
                        profile
                            .mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                        continue;
                    }
                    429 => {
                        Metrics::inc(&app.metrics.upstream_429);
                        saw_quota = true;
                        excluded.insert(profile.id().to_string());
                        profile.cool_model_until(
                            &model.id,
                            pool::now()
                                + translator
                                    .provider_retry_after
                                    .unwrap_or(gateway.config().default_rate_limit_cool_secs),
                        );
                        continue;
                    }
                    408 | 409 | 425 | 500..=599 => {
                        Metrics::inc(&app.metrics.upstream_5xx);
                        excluded.insert(profile.id().to_string());
                        profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                        transport_failures += 1;
                        if transport_failures > gateway.config().max_transport_retries {
                            return Err(ApiError::unavailable("gemini_stream_start_failed"));
                        }
                        continue;
                    }
                    _ => {}
                }
            }
            if initial.is_empty() {
                // The startup future only returns a non-empty vector. Keep this defensive branch
                // local so a later translator refactor cannot accidentally relax the no-byte retry
                // boundary without being classified as a transport failure.
                Metrics::inc(&app.metrics.upstream_5xx);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                transport_failures += 1;
                if transport_failures > gateway.config().max_transport_retries {
                    return Err(ApiError::unavailable("gemini_stream_start_failed"));
                }
                continue;
            }
            profile.mark_healthy_for(&model.id);
            record_affinity_success(
                &app.affinity,
                affinity_input.as_ref(),
                &mut affinity_resolution,
                profile.id(),
            )
            .await;
            return stream_response(
                gateway,
                app.metrics.clone(),
                profile,
                lease,
                admission,
                model,
                status,
                response_headers,
                translator,
                initial,
                stream,
            )
            .await;
        }

        let response_body = match read_upstream_body(response).await {
            Ok(bytes) => bytes,
            Err(_) => {
                Metrics::inc(&app.metrics.upstream_5xx);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                transport_failures += 1;
                if transport_failures > gateway.config().max_transport_retries {
                    return Err(ApiError::unavailable("gemini_response_read_failed"));
                }
                continue;
            }
        };
        match status.as_u16() {
            401 | 403 => {
                Metrics::inc(&app.metrics.upstream_auth);
                saw_auth = true;
                excluded.insert(profile.id().to_string());
                profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                continue;
            }
            429 => {
                Metrics::inc(&app.metrics.upstream_429);
                saw_quota = true;
                excluded.insert(profile.id().to_string());
                let delay = retry_after(
                    &response_headers,
                    &response_body,
                    gateway.config().default_rate_limit_cool_secs,
                );
                profile.cool_model_until(&model.id, pool::now() + delay);
                continue;
            }
            408 | 409 | 425 | 500..=599 => {
                Metrics::inc(&app.metrics.upstream_5xx);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                transport_failures += 1;
                if transport_failures > gateway.config().max_transport_retries {
                    return Err(ApiError::unavailable("gemini_backend_unavailable"));
                }
                continue;
            }
            _ if status.is_success() => {
                let native_body = match unwrap_code_assist_response(route.operation, &response_body)
                {
                    Ok(body) => body,
                    Err(()) => {
                        Metrics::inc(&app.metrics.upstream_5xx);
                        excluded.insert(profile.id().to_string());
                        profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                        transport_failures += 1;
                        if transport_failures > gateway.config().max_transport_retries {
                            return Err(ApiError::unavailable("gemini_malformed_response"));
                        }
                        continue;
                    }
                };
                profile.mark_healthy_for(&model.id);
                record_affinity_success(
                    &app.affinity,
                    affinity_input.as_ref(),
                    &mut affinity_resolution,
                    profile.id(),
                )
                .await;
                let usage = if route.operation == Operation::Generate {
                    serde_json::from_slice::<Value>(&native_body)
                        .ok()
                        .and_then(|value| metering::gemini::usage_from_response_value(&value))
                        .filter(|usage| !usage.is_zero())
                } else {
                    None
                };
                if route.operation == Operation::Generate
                    && admission.requires_usage()
                    && usage.is_none()
                {
                    Metrics::inc(&app.metrics.gemini_usage_missing);
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    return Err(ApiError::unavailable("gemini_usage_metadata_missing"));
                }
                admission.mark_delivering().await?;
                admission.settle(&model, usage.as_ref());
                return Ok(translated_response(status, &response_headers, native_body));
            }
            _ if status.is_client_error() => {
                debug_log_upstream_rejection(
                    suffix,
                    status,
                    &response_body,
                    &project,
                    profile.id(),
                );
                if suffix == "streamGenerateContent"
                    && STREAM_PROBE_DONE
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    if let Ok(probe_body) = wrap_code_assist_request(
                        route.operation,
                        oauth_kind,
                        &model.id,
                        &project,
                        &value,
                        &user_prompt_id,
                        upstream_session_id.as_deref(),
                        upstream_request_id.as_deref(),
                    ) {
                        debug_probe_stream_variants(
                            &profile,
                            gateway.config().upstream_for(oauth_kind),
                            probe_body,
                            &upstream_user_agent,
                            &project,
                            profile.id(),
                        )
                        .await;
                    }
                }
                // The private Code Assist error envelope can contain account, project, plan or
                // internal endpoint details. Preserve only the public status class.
                profile.mark_healthy_for(&model.id);
                return Err(ApiError::provider_rejected(status));
            }
            _ => {
                Metrics::inc(&app.metrics.upstream_5xx);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                transport_failures += 1;
                if transport_failures > gateway.config().max_transport_retries {
                    return Err(ApiError::unavailable("gemini_backend_protocol_error"));
                }
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AffinityStore, AsyncBilling, Breaker, Clients, KeyLimiter, ProxyConfig};
    use axum::http::Uri;
    use axum::routing::any;
    use futures_util::stream;
    use gemini_credential::{encode_envelope, CredentialKeyring, GeminiCredential};
    use pool::{Pool, Reserve};
    use std::collections::{HashMap, VecDeque};
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{SystemTime, UNIX_EPOCH};

    const CUSTOMER_KEY: &str = "sk-pool-gemini-integration";
    const PROFILE_A_KEY: &str = "gemini-profile-a-key-that-is-secret";
    const PROFILE_B_KEY: &str = "gemini-profile-b-key-that-is-secret";
    const ACCOUNT_ID: &str = "gemini-integration-account";

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    #[derive(Clone)]
    enum MockChunk {
        Data(Bytes),
        Error,
    }

    #[derive(Clone)]
    enum MockReply {
        Json {
            status: StatusCode,
            body: Value,
            retry_after: Option<&'static str>,
        },
        Stream {
            chunks: Vec<MockChunk>,
            inter_chunk_delay: Duration,
            drained: Arc<AtomicBool>,
        },
        Stalled {
            first: Bytes,
        },
    }

    impl MockReply {
        fn json(status: StatusCode, body: Value) -> Self {
            let body = if status.is_success()
                && body.get("response").is_none()
                && body.get("totalTokens").is_none()
            {
                json!({
                    "response": body,
                    "traceId": "private-trace-id",
                    "consumedCredits": [{"creditType": "G1", "creditAmount": "9"}],
                    "remainingCredits": [{"creditType": "G1", "creditAmount": "91"}]
                })
            } else {
                body
            };
            Self::Json {
                status,
                body,
                retry_after: None,
            }
        }

        fn stream(chunks: Vec<MockChunk>) -> (Self, Arc<AtomicBool>) {
            let drained = Arc::new(AtomicBool::new(false));
            (
                Self::Stream {
                    chunks,
                    inter_chunk_delay: Duration::from_millis(40),
                    drained: drained.clone(),
                },
                drained,
            )
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct SeenRequest {
        credential: String,
        uri: String,
        body: Bytes,
        user_agent: String,
        google_api_client: String,
        client_metadata: String,
        has_private_client_headers: bool,
    }

    #[derive(Default)]
    struct MockState {
        replies: Mutex<HashMap<String, VecDeque<MockReply>>>,
        seen: Mutex<Vec<SeenRequest>>,
    }

    impl MockState {
        fn with_replies(entries: impl IntoIterator<Item = (&'static str, Vec<MockReply>)>) -> Self {
            Self {
                replies: Mutex::new(
                    entries
                        .into_iter()
                        .map(|(key, replies)| (key.to_string(), replies.into()))
                        .collect(),
                ),
                seen: Mutex::new(Vec::new()),
            }
        }

        fn seen(&self) -> Vec<SeenRequest> {
            self.seen.lock().unwrap().clone()
        }
    }

    struct MockServer {
        upstream: String,
        state: Arc<MockState>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for MockServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn mock_upstream(
        State(state): State<Arc<MockState>>,
        uri: Uri,
        headers: HeaderMap,
        body: Bytes,
    ) -> Response {
        let credential = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .unwrap_or_default()
            .to_string();
        state.seen.lock().unwrap().push(SeenRequest {
            credential: credential.clone(),
            uri: uri.to_string(),
            body,
            user_agent: headers
                .get("user-agent")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            google_api_client: headers
                .get("x-goog-api-client")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            client_metadata: headers
                .get("client-metadata")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_string(),
            has_private_client_headers: [
                "x-goog-user-project",
                "x-goog-request-params",
                "x-forwarded-for",
                "forwarded",
                "x-real-ip",
            ]
            .iter()
            .any(|name| headers.contains_key(*name)),
        });
        let reply = state
            .replies
            .lock()
            .unwrap()
            .get_mut(&credential)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| {
                MockReply::json(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": {"message": "unexpected mock request"}}),
                )
            });
        match reply {
            MockReply::Json {
                status,
                body,
                retry_after,
            } => {
                let mut response = Response::builder()
                    .status(status)
                    .header("content-type", "application/json");
                if let Some(retry_after) = retry_after {
                    response = response.header("retry-after", retry_after);
                }
                response
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap()
            }
            MockReply::Stream {
                chunks,
                inter_chunk_delay,
                drained,
            } => {
                let body = stream::unfold(
                    (chunks.into_iter(), true, drained),
                    move |(mut chunks, first, drained)| async move {
                        let Some(chunk) = chunks.next() else {
                            drained.store(true, Ordering::Release);
                            return None;
                        };
                        if !first {
                            tokio::time::sleep(inter_chunk_delay).await;
                        }
                        let chunk = match chunk {
                            MockChunk::Data(bytes) => Ok(bytes),
                            MockChunk::Error => Err(std::io::Error::other("mock stream failure")),
                        };
                        Some((chunk, (chunks, false, drained)))
                    },
                );
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(body))
                    .unwrap()
            }
            MockReply::Stalled { first } => {
                let body = stream::once(async move { Ok::<_, std::io::Error>(first) })
                    .chain(stream::pending());
                Response::builder()
                    .status(StatusCode::OK)
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(body))
                    .unwrap()
            }
        }
    }

    async fn start_mock(state: MockState) -> MockServer {
        let state = Arc::new(state);
        let router = axum::Router::new()
            .fallback(any(mock_upstream))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        MockServer {
            upstream: format!("http://{address}"),
            state,
            task,
        }
    }

    struct GatewayFixture {
        gateway: Arc<GeminiGateway>,
        directory: PathBuf,
    }

    impl Drop for GatewayFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    fn gateway_fixture(
        upstream: &str,
        proxies: &[Option<&str>],
        max_transport_retries: usize,
    ) -> GatewayFixture {
        gateway_fixture_with_oauth_kind(
            upstream,
            proxies,
            max_transport_retries,
            None,
            OAuthKind::LegacyGeminiCli,
        )
    }

    fn gateway_fixture_with_token_uri(
        upstream: &str,
        proxies: &[Option<&str>],
        max_transport_retries: usize,
        token_uri: Option<&str>,
    ) -> GatewayFixture {
        gateway_fixture_with_oauth_kind(
            upstream,
            proxies,
            max_transport_retries,
            token_uri,
            OAuthKind::LegacyGeminiCli,
        )
    }

    fn gateway_fixture_with_oauth_kind(
        upstream: &str,
        proxies: &[Option<&str>],
        max_transport_retries: usize,
        token_uri: Option<&str>,
        oauth_kind: OAuthKind,
    ) -> GatewayFixture {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "gemini-api-integration-{}-{unique}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let credential_directory = directory.join("credentials");
        fs::create_dir_all(&credential_directory).unwrap();
        fs::set_permissions(&credential_directory, fs::Permissions::from_mode(0o700)).unwrap();
        let keys = [PROFILE_A_KEY, PROFILE_B_KEY];
        let ring = CredentialKeyring::parse(&format!("test:{}", "42".repeat(32))).unwrap();
        let mut profiles = Vec::new();
        for (index, proxy) in proxies.iter().enumerate() {
            let profile_id = format!("profile_{}", (b'a' + index as u8) as char);
            let credential_file = credential_directory.join(format!("{profile_id}.json"));
            let (oauth_client_id, oauth_client_secret) = match oauth_kind {
                OAuthKind::Antigravity => (
                    gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_ID,
                    gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_SECRET,
                ),
                OAuthKind::LegacyGeminiCli => (
                    gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID,
                    gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET,
                ),
            };
            let credential = GeminiCredential {
                version: 1,
                access_token: keys[index].to_string(),
                refresh_token: format!("refresh-token-value-{index}"),
                expires_at: pool::now() + 3_600,
                oauth_client_id: oauth_client_id.to_string(),
                oauth_client_secret: oauth_client_secret.to_string(),
                token_uri: token_uri
                    .unwrap_or("https://oauth2.googleapis.com/token")
                    .to_string(),
                subject: format!("google-subject-{index}"),
                email: format!("owner-{index}@example.invalid"),
                project_id: format!("paid-project-{:02}", index + 1),
                tier_id: "paid-tier".to_string(),
                tier_name: "Google AI Pro".to_string(),
                plan: "google_ai_pro".to_string(),
                proxy: proxy.unwrap_or_default().to_string(),
                proxy_order_id: 0,
                issued_at: pool::now(),
            };
            let envelope = ring.seal("test", &profile_id, &credential).unwrap();
            fs::write(&credential_file, encode_envelope(&envelope).unwrap()).unwrap();
            fs::set_permissions(&credential_file, fs::Permissions::from_mode(0o600)).unwrap();
            profiles.push(json!({
                "id": profile_id,
                "credential_file": credential_file,
            }));
        }
        let profiles_file = directory.join("profiles.json");
        fs::write(
            &profiles_file,
            serde_json::to_vec(&json!({"profiles": profiles})).unwrap(),
        )
        .unwrap();
        fs::set_permissions(&profiles_file, fs::Permissions::from_mode(0o600)).unwrap();
        let model = GeminiModel {
            id: "gemini-integration-model".to_string(),
            display_name: "Gemini Integration Model".to_string(),
            input_token_limit: 1_000_000,
            output_token_limit: 64,
            prices: metering::GeminiPrices {
                input: 1,
                audio_input: 1,
                cached_input: 1,
                cached_audio_input: 1,
                output: 1,
                long_context_threshold: u64::MAX,
                long_input: 1,
                long_audio_input: 1,
                long_cached_input: 1,
                long_cached_audio_input: 1,
                long_output: 1,
                search: metering::GeminiSearchBilling::PerGroundedPrompt { nano: 1 },
            },
        };
        let gateway = GeminiGateway::new(super::super::config::GeminiConfig {
            enabled: true,
            upstream: upstream.to_string(),
            profiles_file: profiles_file.to_string_lossy().into_owned(),
            credential_keys: ring,
            models: vec![model],
            connect_timeout_secs: 1,
            read_timeout_secs: 5,
            max_transport_retries,
            auth_quarantine_secs: 900,
            transport_cool_secs: 30,
            default_rate_limit_cool_secs: 60,
            health_probe_interval_secs: 60,
            reserve_overhead_tokens: 10,
            antigravity_version: gemini_credential::ANTIGRAVITY_VERSION.to_string(),
            node_binary: "/usr/bin/node".to_string(),
            node_version: "v24.18.0".to_string(),
            node_sha256: "0".repeat(64),
        })
        .unwrap();
        GatewayFixture {
            gateway: Arc::new(gateway),
            directory,
        }
    }

    fn proxy_config(admin_key: bool) -> Arc<ProxyConfig> {
        Arc::new(ProxyConfig {
            api_keys: if admin_key {
                vec![CUSTOMER_KEY.to_string()]
            } else {
                Vec::new()
            },
            control_keys: Vec::new(),
            panel_keys: Vec::new(),
            default_mult_bp: 10_000,
            pricing_bridge: crate::PricingBridgeConfig::disabled(),
            trust_loopback: false,
            upstream: "http://127.0.0.1:1".to_string(),
            max_tries: 2,
            max_inflight_per_key: 10,
            util_cap: 1.0,
            cool_secs: 60,
            smooth_wait_ms: 0,
            affinity_wait_ms: 0,
            affinity_wait_min_bytes: 0,
            poll: false,
            inject_identity: false,
            identity: String::new(),
            inject_billing: false,
            cc_version: String::new(),
            cc_entrypoint: String::new(),
            default_beta: String::new(),
            user_agent: "gemini-integration-test".to_string(),
            user_agents: Vec::new(),
            ua_spread: 0,
            anthropic_version: String::new(),
            connect_timeout: 1,
            x_app: String::new(),
            stainless_lang: String::new(),
            stainless_runtime: String::new(),
            stainless_runtime_version: String::new(),
            stainless_package_version: String::new(),
            stainless_os: String::new(),
            stainless_arch: String::new(),
        })
    }

    fn app_state(gateway: Arc<GeminiGateway>, billing: Option<Arc<AsyncBilling>>) -> AppState {
        let cfg = proxy_config(billing.is_none());
        AppState {
            provider: crate::ProviderMode::Gemini,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                ":memory:".to_string(),
                None,
            )),
            data_db_path: Arc::new(":memory:".to_string()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            codex: None,
            gemini: Some(gateway),
            billing,
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            key_limiter: Arc::new(KeyLimiter::new()),
            concurrency: Arc::new(tokio::sync::Semaphore::new(100)),
            probe_poke: None,
            cfg,
        }
    }

    async fn invoke(app: AppState, body: Value, streaming: bool) -> Response {
        invoke_with_identity(app, body, streaming, CUSTOMER_KEY, None).await
    }

    async fn invoke_with_identity(
        app: AppState,
        body: Value,
        streaming: bool,
        key: &str,
        session_id: Option<&str>,
    ) -> Response {
        // Existing streaming tests assert the SSE downstream shape, so request alt=sse explicitly;
        // the JSON-array default (no alt) has its own dedicated coverage.
        let uri = if streaming {
            "/v1beta/models/gemini-integration-model:streamGenerateContent?alt=sse".to_string()
        } else {
            "/v1beta/models/gemini-integration-model:generateContent".to_string()
        };
        let mut builder = axum::extract::Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json")
            .header("x-goog-api-key", key);
        if let Some(session_id) = session_id {
            builder = builder.header("x-session-id", session_id);
        }
        let request = builder
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
            Ok(response) => response,
            Err(error) => error.into_response(),
        }
    }

    async fn invoke_uri(app: AppState, uri: &str, body: Value) -> Response {
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri(uri)
            .header("content-type", "application/json")
            .header("x-goog-api-key", CUSTOMER_KEY)
            .body(Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        match api_inner(app, "198.51.100.10:12345".parse().unwrap(), request).await {
            Ok(response) => response,
            Err(error) => error.into_response(),
        }
    }

    async fn response_json(response: Response) -> Value {
        serde_json::from_slice(
            &to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn canonicalize_promotes_snake_case_and_normalizes_tools() {
        let mut value = json!({
            "contents": [],
            "system_instruction": {"parts": [{"text": "be terse"}]},
            "safety_settings": [],
            "generation_config": {"maxOutputTokens": 10},
            "tools": [{"google_search": {}}]
        });
        canonicalize_native_request(&mut value);
        assert!(value.get("systemInstruction").is_some());
        assert!(value.get("system_instruction").is_none());
        assert!(value.get("safetySettings").is_some());
        assert!(value.get("generationConfig").is_some());
        let tool = &value["tools"][0];
        assert!(tool.get("googleSearch").is_some());
        assert!(tool.get("google_search").is_none());
        // The normalized tool must pass validation just like its camelCase form.
        assert!(validate_tools(&value).is_ok());
    }

    #[test]
    fn camel_case_wins_over_snake_case_duplicate() {
        let mut value = json!({
            "systemInstruction": {"parts": [{"text": "camel"}]},
            "system_instruction": {"parts": [{"text": "snake"}]}
        });
        canonicalize_native_request(&mut value);
        assert_eq!(value["systemInstruction"]["parts"][0]["text"], "camel");
        assert!(value.get("system_instruction").is_none());
    }

    #[test]
    fn model_value_is_native_shaped() {
        let model = GeminiModel {
            id: "gemini-2.5-flash".to_string(),
            display_name: "Gemini 2.5 Flash".to_string(),
            input_token_limit: 1_048_576,
            output_token_limit: 65_536,
            prices: metering::GeminiPrices {
                input: 1,
                audio_input: 1,
                cached_input: 1,
                cached_audio_input: 1,
                output: 1,
                long_context_threshold: u64::MAX,
                long_input: 1,
                long_audio_input: 1,
                long_cached_input: 1,
                long_cached_audio_input: 1,
                long_output: 1,
                search: metering::GeminiSearchBilling::PerGroundedPrompt { nano: 1 },
            },
        };
        let value = model_value(&model);
        assert_eq!(value["name"], "models/gemini-2.5-flash");
        assert_eq!(value["version"], "2.5");
        assert_ne!(value["description"], value["displayName"]);
        assert!(value["temperature"].is_number());
        assert!(value["topP"].is_number());
        assert!(value["topK"].is_number());
        assert!(value["maxTemperature"].is_number());
    }

    #[test]
    fn parse_list_models_query_supports_pagination() {
        let page = parse_list_models_query(Some("pageSize=2&pageToken=3&irrelevant=x")).unwrap();
        assert_eq!(page.size, 2);
        assert_eq!(page.start, 3);
        // Default when absent, and clamped upper bound.
        assert_eq!(parse_list_models_query(None).unwrap().size, 50);
        assert_eq!(
            parse_list_models_query(Some("pageSize=999999"))
                .unwrap()
                .size,
            1000
        );
        // Query-string API keys stay rejected.
        assert!(parse_list_models_query(Some("key=leak")).is_err());
    }

    #[test]
    fn native_stream_error_value_is_sanitized() {
        let wrapper = json!({
            "error": {
                "code": 429,
                "status": "RESOURCE_EXHAUSTED",
                "message": "project paid-project-99 for owner@example.invalid exceeded quota"
            }
        });
        let value = native_stream_error_value(&wrapper).expect("error value");
        assert_eq!(value["error"]["status"], "RESOURCE_EXHAUSTED");
        assert_eq!(value["error"]["code"], 429);
        // Upstream private detail must never survive into the public element.
        let text = value.to_string();
        assert!(!text.contains("paid-project-99"));
        assert!(!text.contains("owner@example.invalid"));
        // A credit/accounting-only frame with no error has no public representation.
        assert!(native_stream_error_value(&json!({"consumedCredits": 3})).is_none());
    }

    #[test]
    fn json_array_framing_wraps_elements_and_closes() {
        let mut translator = SseTranslator::new(StreamFraming::JsonArray);
        let first = translator.frame(&json!({"a": 1})).unwrap();
        let second = translator.frame(&json!({"b": 2})).unwrap();
        let close = translator.finish_stream().unwrap();
        let whole = [first.as_ref(), second.as_ref(), close.as_ref()].concat();
        let parsed: Value = serde_json::from_slice(&whole).unwrap();
        assert_eq!(parsed, json!([{"a": 1}, {"b": 2}]));
        // An empty JSON-array stream still closes as a valid empty array.
        let mut empty = SseTranslator::new(StreamFraming::JsonArray);
        assert_eq!(empty.finish_stream().unwrap().as_ref(), b"[]");
        // SSE framing has no terminator and keeps the data: envelope.
        let mut sse = SseTranslator::new(StreamFraming::Sse);
        let frame = sse.frame(&json!({"a": 1})).unwrap();
        assert!(frame.starts_with(b"data: "));
        assert!(sse.finish_stream().is_none());
    }

    #[test]
    fn private_stream_prelude_has_independent_byte_and_chunk_bounds() {
        let mut bytes = 0usize;
        let mut chunks = 0usize;
        account_stream_start_chunk(&mut bytes, &mut chunks, STREAM_START_MAX_BYTES).unwrap();
        assert!(account_stream_start_chunk(&mut bytes, &mut chunks, 1).is_err());

        let mut bytes = 0usize;
        let mut chunks = 0usize;
        for _ in 0..STREAM_START_MAX_CHUNKS {
            account_stream_start_chunk(&mut bytes, &mut chunks, 0).unwrap();
        }
        assert!(account_stream_start_chunk(&mut bytes, &mut chunks, 0).is_err());
    }

    #[tokio::test]
    async fn streaming_without_alt_returns_a_native_json_array() {
        let first = Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]},\"traceId\":\"t\"}\n\n",
        );
        let usage = Bytes::from_static(
            b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}}}\n\n",
        );
        let (reply, _drained) =
            MockReply::stream(vec![MockChunk::Data(first), MockChunk::Data(usage)]);
        let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let response = invoke_uri(
            app_state(fixture.gateway.clone(), None),
            "/v1beta/models/gemini-integration-model:streamGenerateContent",
            json!({"contents": []}),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        // The native default streams a JSON array, not Server-Sent Events.
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let bytes = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        let parsed: Value = serde_json::from_slice(&bytes).expect("valid JSON array body");
        let array = parsed.as_array().expect("top-level array");
        assert_eq!(array.len(), 2);
        assert!(array[0]["candidates"].is_array());
        assert_eq!(array[1]["usageMetadata"]["promptTokenCount"], 10);
        // Private wrapper fields never surface, and each element carries the same responseId.
        assert!(!parsed.to_string().contains("traceId"));
        assert_eq!(array[0]["responseId"], array[1]["responseId"]);
        assert!(array[0]["responseId"]
            .as_str()
            .is_some_and(|id| !id.is_empty()));
    }

    #[tokio::test]
    async fn invalid_key_maps_to_native_400_api_key_invalid() {
        let response = ApiError::from(AdmissionError::Unauthorized).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], 400);
        assert_eq!(body["error"]["status"], "INVALID_ARGUMENT");
        let details = body["error"]["details"].as_array().expect("details");
        assert!(details.iter().any(|detail| {
            detail["@type"]
                .as_str()
                .is_some_and(|kind| kind.ends_with("google.rpc.ErrorInfo"))
                && detail["reason"] == "API_KEY_INVALID"
        }));
    }

    #[tokio::test]
    async fn rate_limited_error_carries_retry_info_detail() {
        let response = ApiError::rate_limited(Some(7)).into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok()),
            Some("7")
        );
        let body = response_json(response).await;
        assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");
        let details = body["error"]["details"].as_array().expect("details");
        assert!(details.iter().any(|detail| {
            detail["@type"]
                .as_str()
                .is_some_and(|kind| kind.ends_with("google.rpc.RetryInfo"))
                && detail["retryDelay"] == "7s"
        }));
    }

    #[test]
    fn provider_rejected_never_emits_an_impossible_status_pair() {
        // A 413 upstream rejection must collapse to the native INVALID_ARGUMENT/400 pair, never
        // 413/FAILED_PRECONDITION.
        let error = ApiError::provider_rejected(StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(error.status, StatusCode::BAD_REQUEST);
        assert_eq!(error.google_status, "INVALID_ARGUMENT");
        let forbidden = ApiError::provider_rejected(StatusCode::FORBIDDEN);
        assert_eq!(forbidden.status, StatusCode::FORBIDDEN);
        assert_eq!(forbidden.google_status, "PERMISSION_DENIED");
    }

    async fn billed_app_with_balance(
        gateway: Arc<GeminiGateway>,
        balance_nano: i64,
    ) -> (AppState, Arc<AsyncBilling>, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "gemini-api-billing-{}-{unique}-{}.sqlite",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start Gemini integration billing"),
        );
        billing
            .create_account(ACCOUNT_ID, None, 10_000)
            .await
            .unwrap();
        billing
            .topup(ACCOUNT_ID, balance_nano, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key(CUSTOMER_KEY, ACCOUNT_ID, None, None, None)
            .await
            .unwrap();
        (app_state(gateway, Some(billing.clone())), billing, path)
    }

    async fn billed_app(gateway: Arc<GeminiGateway>) -> (AppState, Arc<AsyncBilling>, PathBuf) {
        billed_app_with_balance(gateway, 1_000_000_000).await
    }

    #[tokio::test]
    async fn quota_rotates_to_the_next_project_and_client_credential_never_reaches_google() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::Json {
                    status: StatusCode::TOO_MANY_REQUESTS,
                    body: json!({"error": {"status": "RESOURCE_EXHAUSTED"}}),
                    retry_after: Some("12"),
                }],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({
                        "candidates": [],
                        "usageMetadata": {"promptTokenCount": 3},
                        "internalIdentity": "paid-project-02 owner@example.invalid"
                    }),
                )],
            ),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let public = response_json(response).await;
        assert!(public.get("response").is_none());
        assert!(public.get("consumedCredits").is_none());
        assert!(public.get("remainingCredits").is_none());
        // A native-shaped responseId is synthesized locally; it must be present but must not be the
        // correlatable upstream wrapper trace id.
        let response_id = public
            .get("responseId")
            .and_then(Value::as_str)
            .expect("native responseId is present");
        assert!(!response_id.is_empty());
        assert_ne!(response_id, "private-trace-id");
        assert!(!public.to_string().contains("private-trace-id"));
        assert!(public.get("internalIdentity").is_none());
        let seen = server.state.seen();
        assert_eq!(
            seen.iter()
                .map(|request| request.credential.as_str())
                .collect::<Vec<_>>(),
            [PROFILE_A_KEY, PROFILE_B_KEY]
        );
        assert!(seen
            .iter()
            .all(|request| request.credential != CUSTOMER_KEY));
    }

    #[tokio::test]
    async fn concurrent_401s_refresh_once_and_retry_with_the_new_bearer() {
        let refreshed = "gemini-profile-a-refreshed-access-token";
        let unauthorized = MockReply::json(
            StatusCode::UNAUTHORIZED,
            json!({"error": {"message": "private rejected token"}}),
        );
        let success = MockReply::json(
            StatusCode::OK,
            json!({"candidates": [], "usageMetadata": {}}),
        );
        let token_reply = MockReply::Json {
            status: StatusCode::OK,
            body: json!({"access_token": refreshed, "expires_in": 3600}),
            retry_after: None,
        };
        let server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![unauthorized.clone(), unauthorized]),
            (refreshed, vec![success.clone(), success]),
            ("", vec![token_reply]),
        ]))
        .await;
        let token_uri = format!("{}/token", server.upstream);
        let fixture =
            gateway_fixture_with_token_uri(&server.upstream, &[None], 1, Some(&token_uri));
        let app = app_state(fixture.gateway.clone(), None);
        let body = json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]});
        let (first, second) = tokio::join!(
            invoke(app.clone(), body.clone(), false),
            invoke(app, body, false)
        );
        assert_eq!(first.status(), StatusCode::OK);
        assert_eq!(second.status(), StatusCode::OK);
        let seen = server.state.seen();
        assert_eq!(
            seen.iter()
                .filter(|request| request.uri == "/token")
                .count(),
            1
        );
        assert_eq!(
            seen.iter()
                .filter(|request| request.uri == "/v1internal:generateContent")
                .filter(|request| request.credential == refreshed)
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn client_identity_headers_are_stripped_and_runtime_identity_is_truthful() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [],
                    "usageMetadata": {
                        "promptTokenCount": 25,
                        "candidatesTokenCount": 7
                    }
                }),
            )],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1beta/models/gemini-integration-model:generateContent")
            .header("content-type", "application/json")
            .header("x-goog-api-key", CUSTOMER_KEY)
            .header("authorization", "Bearer customer-secret")
            .header("user-agent", "GeminiCLI/forged")
            .header("client-metadata", "customer-identity")
            .header("x-goog-user-project", "customer-project")
            .header("x-goog-api-client", "forged-client")
            .header("x-forwarded-for", "203.0.113.9")
            .body(Body::from(
                br#"{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}"#.as_slice(),
            ))
            .unwrap();
        let response = api_inner(
            app_state(fixture.gateway.clone(), None),
            "198.51.100.10:12345".parse().unwrap(),
            request,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let seen = server.state.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].credential, PROFILE_A_KEY);
        assert_eq!(
            seen[0].user_agent,
            "GeminiCLI/0.53.0/gemini-integration-model (linux; x64; cli) google-api-nodejs-client/10.9.0"
        );
        assert!(seen[0].client_metadata.is_empty());
        assert_eq!(seen[0].google_api_client, "gl-node/24.18.0");
        assert!(!seen[0].has_private_client_headers);
    }

    #[tokio::test]
    async fn antigravity_generation_uses_agent_wrapper_and_keeps_ids_across_rotation() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::TOO_MANY_REQUESTS,
                    json!({"error": {"message": "private quota"}}),
                )],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                )],
            ),
        ]))
        .await;
        let fixture = gateway_fixture_with_oauth_kind(
            &server.upstream,
            &[None, None],
            1,
            None,
            OAuthKind::Antigravity,
        );
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": [{"role": "user", "parts": [{"text": "hello"}]}]}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let seen = server.state.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].credential, PROFILE_A_KEY);
        assert_eq!(seen[1].credential, PROFILE_B_KEY);
        let bodies = seen
            .iter()
            .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
            .collect::<Vec<_>>();
        for (index, body) in bodies.iter().enumerate() {
            assert_eq!(body["userAgent"], "antigravity");
            assert_eq!(body["requestType"], "agent");
            assert_eq!(body["project"], format!("paid-project-{:02}", index + 1));
            assert!(body.get("user_prompt_id").is_none());
            assert!(body["request"].get("session_id").is_none());
            assert!(body["request"]["sessionId"].as_str().is_some());
            assert!(body["requestId"]
                .as_str()
                .is_some_and(|value| value.starts_with("agent-") && value.len() > 16));
        }
        assert_eq!(bodies[0]["requestId"], bodies[1]["requestId"]);
        assert_eq!(
            bodies[0]["request"]["sessionId"],
            bodies[1]["request"]["sessionId"]
        );
        assert!(seen.iter().all(|request| {
            request.user_agent == "antigravity/hub/2.2.1 darwin/arm64"
                && request.google_api_client.is_empty()
        }));
    }

    #[tokio::test]
    async fn antigravity_refresh_uses_go_identity_without_legacy_google_header() {
        let refreshed = "antigravity-refreshed-access-token";
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::UNAUTHORIZED,
                    json!({"error": {"message": "expired"}}),
                )],
            ),
            (
                "",
                vec![MockReply::Json {
                    status: StatusCode::OK,
                    body: json!({"access_token": refreshed, "expires_in": 3600}),
                    retry_after: None,
                }],
            ),
            (
                refreshed,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                )],
            ),
        ]))
        .await;
        let token_uri = format!("{}/token", server.upstream);
        let fixture = gateway_fixture_with_oauth_kind(
            &server.upstream,
            &[None],
            1,
            Some(&token_uri),
            OAuthKind::Antigravity,
        );
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let seen = server.state.seen();
        let refresh = seen
            .iter()
            .find(|request| request.uri == "/token")
            .expect("one token refresh request");
        assert_eq!(refresh.user_agent, "Go-http-client/2.0");
        assert!(refresh.google_api_client.is_empty());
    }

    #[tokio::test]
    async fn antigravity_health_fetches_model_quotas_and_cools_explicit_zero() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![
                MockReply::Json {
                    status: StatusCode::OK,
                    body: json!({"cloudaicompanionProject": "paid-project-01"}),
                    retry_after: None,
                },
                MockReply::Json {
                    status: StatusCode::OK,
                    body: json!({
                        "models": {
                            "gemini-integration-model": {
                                "displayName": "Gemini Integration Model",
                                "quotaInfo": {
                                    "remainingFraction": 0.0,
                                    "resetTime": "2099-01-01T00:00:00Z"
                                }
                            }
                        }
                    }),
                    retry_after: None,
                },
            ],
        )]))
        .await;
        let fixture = gateway_fixture_with_oauth_kind(
            &server.upstream,
            &[None],
            1,
            None,
            OAuthKind::Antigravity,
        );
        fixture.gateway.probe_health().await;

        let seen = server.state.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].uri, "/v1internal:loadCodeAssist");
        assert_eq!(seen[1].uri, "/v1internal:fetchAvailableModels");
        let load_body: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(load_body, json!({"metadata": {"ideType": "ANTIGRAVITY"}}));
        let quota_body: Value = serde_json::from_slice(&seen[1].body).unwrap();
        assert_eq!(quota_body, json!({"project": "paid-project-01"}));
        assert!(seen.iter().all(|request| {
            request.user_agent == "antigravity/hub/2.2.1 darwin/arm64"
                && request.google_api_client.is_empty()
        }));

        let status = fixture.gateway.operational_status().await;
        assert_eq!(status.models[0].available, 0);
        assert_eq!(status.profiles[0].quotas.len(), 1);
        let quota = &status.profiles[0].quotas[0];
        assert_eq!(quota.model_id, "gemini-integration-model");
        assert_eq!(quota.remaining_fraction, Some(0.0));
        assert_eq!(quota.reset_time.as_deref(), Some("2099-01-01T00:00:00Z"));
        assert_eq!(quota.token_type.as_deref(), Some("antigravity_model"));
    }

    #[tokio::test]
    async fn affinity_keeps_a_growing_conversation_on_the_same_subscription() {
        let success = MockReply::json(
            StatusCode::OK,
            json!({"candidates": [], "usageMetadata": {}}),
        );
        let server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![success.clone(), success.clone()]),
            (PROFILE_B_KEY, vec![success]),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let app = app_state(fixture.gateway.clone(), None);
        let first = json!({
            "contents": [{"role": "user", "parts": [{"text": "turn one"}]}]
        });
        let second = json!({
            "contents": [
                {"role": "user", "parts": [{"text": "turn one"}]},
                {"role": "model", "parts": [{"text": "answer"}]},
                {"role": "user", "parts": [{"text": "turn two"}]}
            ]
        });
        assert_eq!(
            invoke(app.clone(), first, false).await.status(),
            StatusCode::OK
        );
        assert_eq!(invoke(app, second, false).await.status(), StatusCode::OK);
        let seen = server.state.seen();
        let credentials = seen
            .iter()
            .map(|request| request.credential.as_str())
            .collect::<Vec<_>>();
        assert_eq!(credentials, [PROFILE_A_KEY, PROFILE_A_KEY]);
        let wire_bodies = seen
            .iter()
            .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
            .collect::<Vec<_>>();
        let sessions = wire_bodies
            .iter()
            .map(|body| body["request"]["session_id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        let prompts = wire_bodies
            .iter()
            .map(|body| body["user_prompt_id"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(sessions[0], sessions[1]);
        assert_eq!(sessions[0].len(), 36);
        assert_eq!(prompts[0], format!("{}########1", sessions[0]));
        assert_eq!(prompts[1], format!("{}########2", sessions[1]));
        assert!(!sessions[0].contains("turn one"));
    }

    #[tokio::test]
    async fn upstream_session_is_stable_but_isolated_by_explicit_session_and_tenant() {
        const OTHER_ACCOUNT: &str = "gemini-integration-account-other";
        const OTHER_KEY: &str = "sk-gemini-customer-other";
        let success = MockReply::json(
            StatusCode::OK,
            json!({
                "candidates": [],
                "usageMetadata": {"promptTokenCount": 3, "candidatesTokenCount": 1}
            }),
        );
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![success.clone(), success.clone(), success.clone(), success],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
        billing
            .create_account(OTHER_ACCOUNT, None, 10_000)
            .await
            .unwrap();
        billing
            .topup(OTHER_ACCOUNT, 1_000_000_000, Some("seed-other"))
            .await
            .unwrap();
        billing
            .issue_key(OTHER_KEY, OTHER_ACCOUNT, None, None, None)
            .await
            .unwrap();
        let body = json!({
            "contents": [{"role": "user", "parts": [{"text": "raw prompt secret"}]}]
        });
        for (key, session) in [
            (CUSTOMER_KEY, "raw-client-session-a"),
            (CUSTOMER_KEY, "raw-client-session-a"),
            (CUSTOMER_KEY, "raw-client-session-b"),
            (OTHER_KEY, "raw-client-session-a"),
        ] {
            let response =
                invoke_with_identity(app.clone(), body.clone(), false, key, Some(session)).await;
            assert_eq!(response.status(), StatusCode::OK);
        }
        let seen = server.state.seen();
        let sessions = seen
            .iter()
            .map(|request| {
                let value: Value = serde_json::from_slice(&request.body).unwrap();
                value["request"]["session_id"].as_str().unwrap().to_string()
            })
            .collect::<Vec<_>>();
        assert_eq!(sessions[0], sessions[1]);
        assert_ne!(sessions[0], sessions[2]);
        assert_ne!(sessions[0], sessions[3]);
        assert!(sessions.iter().all(|session| session.len() == 36));
        for request in &seen {
            let wire = String::from_utf8_lossy(&request.body);
            assert!(!wire.contains("raw-client-session"));
            assert!(!wire.contains(CUSTOMER_KEY));
            assert!(!wire.contains(OTHER_KEY));
        }
        billing.flush().await.unwrap();
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn count_tokens_uses_the_private_shape_and_returns_only_native_json() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({
                    "totalTokens": 17,
                    "privateProject": "paid-project-01",
                    "traceId": "private-count-trace"
                }),
            )],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri("/v1beta/models/gemini-integration-model:countTokens")
            .header("content-type", "application/json")
            .header("x-goog-api-key", CUSTOMER_KEY)
            .body(Body::from(
                br#"{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}"#.as_slice(),
            ))
            .unwrap();
        let response = api_inner(
            app_state(fixture.gateway.clone(), None),
            "198.51.100.10:12345".parse().unwrap(),
            request,
        )
        .await
        .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_json(response).await, json!({"totalTokens": 17}));
        let seen = server.state.seen();
        assert_eq!(seen[0].uri, "/v1internal:countTokens");
        let private: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(
            private["request"]["model"],
            "models/gemini-integration-model"
        );
        assert!(private.get("project").is_none());
        assert!(private.get("user_prompt_id").is_none());
    }

    #[tokio::test]
    async fn malformed_private_success_is_never_exposed_and_rotates() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::Json {
                    status: StatusCode::OK,
                    body: json!(["cloudcode-pa", "owner@example.invalid", "secret-token"]),
                    retry_after: None,
                }],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                )],
            ),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let public = response_json(response).await.to_string();
        for forbidden in ["cloudcode-pa", "owner@example.invalid", "secret-token"] {
            assert!(!public.contains(forbidden));
        }
        assert_eq!(server.state.seen().len(), 2);
    }

    #[tokio::test]
    async fn low_balance_caps_max_output_tokens_before_the_google_request() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({
                    "candidates": [],
                    "usageMetadata": {
                        "promptTokenCount": 25,
                        "candidatesTokenCount": 7
                    }
                }),
            )],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        // Compact request length 15 + fixture overhead 10 + seven output tokens at 1 nano each.
        let (app, billing, db_path) = billed_app_with_balance(fixture.gateway.clone(), 32).await;
        let response = invoke(app.clone(), json!({"contents": []}), false).await;
        assert_eq!(response.status(), StatusCode::OK);
        let _ = response_json(response).await;
        let seen = server.state.seen();
        assert_eq!(seen.len(), 1);
        let upstream_body: Value = serde_json::from_slice(&seen[0].body).unwrap();
        assert_eq!(
            upstream_body["request"]["generationConfig"]["maxOutputTokens"],
            7
        );
        assert_eq!(upstream_body["project"], "paid-project-01");
        assert!(upstream_body.get("user_prompt_id").is_some());
        billing.flush().await.unwrap();
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn deterministic_client_400_is_returned_without_pool_rotation() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::BAD_REQUEST,
                    json!({
                        "error": {
                            "status": "INVALID_ARGUMENT",
                            "message": "cloudcode-pa Code Assist paid-project-01 owner@example.invalid refresh-token"
                        }
                    }),
                )],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(StatusCode::OK, json!({"unexpected": true}))],
            ),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let public = response_json(response).await.to_string();
        for secret in [
            "cloudcode-pa",
            "Code Assist",
            "paid-project-01",
            "owner@example.invalid",
            "refresh-token",
        ] {
            assert!(!public.contains(secret), "leaked {secret}: {public}");
        }
        assert_eq!(server.state.seen().len(), 1);
        assert_eq!(server.state.seen()[0].credential, PROFILE_A_KEY);
    }

    #[tokio::test]
    async fn auth_failure_quarantines_only_the_failed_project_and_rotates() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::FORBIDDEN,
                    json!({"error": {"status": "UNAUTHENTICATED"}}),
                )],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                )],
            ),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let status = fixture.gateway.operational_status().await;
        let failed = status
            .profiles
            .iter()
            .find(|profile| profile.id == "profile_a")
            .unwrap();
        let healthy = status
            .profiles
            .iter()
            .find(|profile| profile.id == "profile_b")
            .unwrap();
        assert!(!failed.authenticated);
        assert!(failed.cooling_until > pool::now());
        assert!(healthy.authenticated);
    }

    #[tokio::test]
    async fn exhausted_auth_and_transport_faults_return_one_native_503() {
        let auth = MockReply::json(
            StatusCode::FORBIDDEN,
            json!({"error": {"status": "UNAUTHENTICATED"}}),
        );
        let auth_server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![auth.clone()]),
            (PROFILE_B_KEY, vec![auth]),
        ]))
        .await;
        let auth_fixture = gateway_fixture(&auth_server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(auth_fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], 503);
        assert_eq!(body["error"]["status"], "UNAVAILABLE");
        assert_eq!(auth_server.state.seen().len(), 2);

        let transport = MockReply::json(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error": {"status": "UNAVAILABLE"}}),
        );
        let transport_server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![transport.clone()]),
            (PROFILE_B_KEY, vec![transport]),
        ]))
        .await;
        let transport_fixture = gateway_fixture(&transport_server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(transport_fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], 503);
        assert_eq!(body["error"]["status"], "UNAVAILABLE");
        assert_eq!(transport_server.state.seen().len(), 2);
    }

    #[tokio::test]
    async fn transport_failure_and_backend_5xx_each_rotate_within_the_transport_budget() {
        // Profile A cannot reach its configured proxy; profile B reaches the mock directly.
        let network_server = start_mock(MockState::with_replies([(
            PROFILE_B_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
            )],
        )]))
        .await;
        let network_fixture = gateway_fixture(
            &network_server.upstream,
            &[Some("http://127.0.0.1:9"), None],
            1,
        );
        let response = invoke(
            app_state(network_fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(network_server.state.seen()[0].credential, PROFILE_B_KEY);

        let backend_server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::SERVICE_UNAVAILABLE,
                    json!({"error": {"status": "UNAVAILABLE"}}),
                )],
            ),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(
                    StatusCode::OK,
                    json!({"candidates": [], "usageMetadata": {}}),
                )],
            ),
        ]))
        .await;
        let backend_fixture = gateway_fixture(&backend_server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(backend_fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(backend_server.state.seen().len(), 2);
    }

    #[tokio::test]
    async fn retry_info_cools_the_exact_project_and_all_quota_returns_one_native_429() {
        let quota = MockReply::Json {
            status: StatusCode::TOO_MANY_REQUESTS,
            body: json!({
                "error": {
                    "status": "RESOURCE_EXHAUSTED",
                    "details": [{
                        "@type": "type.googleapis.com/google.rpc.RetryInfo",
                        "retryDelay": "2.25s"
                    }]
                }
            }),
            retry_after: None,
        };
        let server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![quota.clone()]),
            (PROFILE_B_KEY, vec![quota]),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            false,
        )
        .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert!(response.headers().contains_key("retry-after"));
        let body = response_json(response).await;
        assert_eq!(body["error"]["code"], 429);
        assert_eq!(body["error"]["status"], "RESOURCE_EXHAUSTED");
        assert_eq!(server.state.seen().len(), 2);
        let now = pool::now();
        let status = fixture.gateway.operational_status().await;
        assert!(status
            .profiles
            .iter()
            .all(|profile| profile.model_cooling.iter().any(|cooling| {
                cooling.model_id == "gemini-integration-model" && cooling.cooling_until >= now + 2
            })));
    }

    #[tokio::test]
    async fn quota_error_before_first_stream_byte_rotates_without_false_health() {
        let (quota, _quota_drained) = MockReply::stream(vec![MockChunk::Data(
            Bytes::from_static(
                b"data: {\"error\":{\"code\":429,\"status\":\"RESOURCE_EXHAUSTED\",\"message\":\"private\",\"details\":[{\"@type\":\"type.googleapis.com/google.rpc.RetryInfo\",\"retryDelay\":\"2.25s\"}]}}\n\n",
            ),
        )]);
        let (healthy, _healthy_drained) = MockReply::stream(vec![MockChunk::Data(
            Bytes::from_static(
                b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]}}],\"usageMetadata\":{\"promptTokenCount\":2,\"candidatesTokenCount\":1}}}\n\n",
            ),
        )]);
        let server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![quota]),
            (PROFILE_B_KEY, vec![healthy]),
        ]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("ok"));
        assert_eq!(server.state.seen().len(), 2);
        let now = pool::now();
        let status = fixture.gateway.operational_status().await;
        assert!(status.profiles[0].model_cooling.iter().any(|cooling| {
            cooling.model_id == "gemini-integration-model" && cooling.cooling_until >= now + 2
        }));
    }

    #[tokio::test]
    async fn sse_is_forwarded_across_upstream_chunk_boundaries_without_retry_after_first_byte() {
        let first = Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]},\"traceId\":\"stream-trace\",\"remainingCredits\":[{\"creditAmount\":\"91\"}]}\n\n",
        );
        let final_usage = Bytes::from_static(
            b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}},\"consumedCredits\":[{\"creditAmount\":\"9\"}]}\n\n",
        );
        let split = first.len() / 2;
        let (reply, drained) = MockReply::stream(vec![
            MockChunk::Data(first.slice(..split)),
            MockChunk::Data(first.slice(split..)),
            MockChunk::Data(final_usage.clone()),
        ]);
        let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let response = invoke(
            app_state(fixture.gateway.clone(), None),
            json!({"contents": []}),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        let text = std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("stream-trace"));
        assert!(text.contains("promptTokenCount"));
        assert!(!text.contains("remainingCredits"));
        assert!(!text.contains("consumedCredits"));
        assert!(!text.contains("\"response\""));
        assert!(drained.load(Ordering::Acquire));
        assert_eq!(
            server.state.seen()[0].uri,
            "/v1internal:streamGenerateContent?alt=sse"
        );

        let (broken, _) = MockReply::stream(vec![
            MockChunk::Data(Bytes::from_static(
                b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"first\"}]}}]}}\n\n",
            )),
            MockChunk::Error,
        ]);
        let broken_server = start_mock(MockState::with_replies([
            (PROFILE_A_KEY, vec![broken]),
            (
                PROFILE_B_KEY,
                vec![MockReply::json(StatusCode::OK, json!({"mustNot": "retry"}))],
            ),
        ]))
        .await;
        let broken_fixture = gateway_fixture(&broken_server.upstream, &[None, None], 1);
        let response = invoke(
            app_state(broken_fixture.gateway.clone(), None),
            json!({"contents": []}),
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("first"));
        assert_eq!(broken_server.state.seen().len(), 1);
        assert_eq!(broken_server.state.seen()[0].credential, PROFILE_A_KEY);
    }

    #[tokio::test]
    async fn downstream_disconnect_still_drains_final_usage_and_settles_google_ledger() {
        let first =
            Bytes::from_static(b"data: {\"response\":{\"candidates\":[{\"content\":{}}]}}\n\n");
        let final_usage = Bytes::from_static(
            b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5}}}\n\n",
        );
        let (reply, drained) =
            MockReply::stream(vec![MockChunk::Data(first), MockChunk::Data(final_usage)]);
        let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
        let response = invoke(app.clone(), json!({"contents": []}), true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut downstream = response.into_body().into_data_stream();
        assert!(downstream.next().await.unwrap().is_ok());
        drop(downstream);

        fixture.gateway.shutdown_until(None).await;
        billing.flush().await.unwrap();
        assert!(drained.load(Ordering::Acquire));
        let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
        assert_eq!(account.reserved_nano, 0);
        assert_eq!(account.spent_nano, 15);
        let usage = billing.usage_by_model(ACCOUNT_ID, 0).await.unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].model, "gemini-integration-model");
        assert_eq!(usage[0].input_tokens, 10);
        assert_eq!(usage[0].output_tokens, 5);
        let providers = billing.spend_by_provider(0).await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider, registry::PROVIDER_GOOGLE);
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn metered_non_stream_success_without_usage_is_withheld_and_refunded() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [{"content": {"parts": [{"text": "private success"}]}}]}),
            )],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
        let response = invoke(app.clone(), json!({"contents": []}), false).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        let public = response_json(response).await;
        assert_eq!(public["error"]["status"], "UNAVAILABLE");
        assert!(!public.to_string().contains("private success"));

        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
        assert_eq!(account.reserved_nano, 0);
        assert_eq!(account.spent_nano, 0);
        assert!(billing
            .usage_by_model(ACCOUNT_ID, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(Metrics::get(&app.metrics.gemini_usage_missing), 1);
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn metered_stream_without_final_usage_charges_hold_without_fake_usage() {
        let (reply, drained) = MockReply::stream(vec![MockChunk::Data(Bytes::from_static(
            b"data: {\"response\":{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"delivered\"}]}}]}}\n\n",
        ))]);
        let server = start_mock(MockState::with_replies([(PROFILE_A_KEY, vec![reply])])).await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
        let response = invoke(app.clone(), json!({"contents": []}), true).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), GEMINI_BODY_LIMIT)
            .await
            .unwrap();
        assert!(std::str::from_utf8(&body).unwrap().contains("delivered"));

        fixture.gateway.shutdown_until(None).await;
        billing.flush().await.unwrap();
        assert!(drained.load(Ordering::Acquire));
        let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
        assert_eq!(account.reserved_nano, 0);
        assert!(
            account.spent_nano > 0,
            "the conservative hold must be charged"
        );
        assert!(billing
            .usage_by_model(ACCOUNT_ID, 0)
            .await
            .unwrap()
            .is_empty());
        assert_eq!(Metrics::get(&app.metrics.gemini_usage_missing), 1);
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[tokio::test]
    async fn shutdown_deadline_aborts_stalled_stream_then_settles_last_known_usage_before_returning(
    ) {
        let first = Bytes::from_static(
            b"data: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":2}}}\n\n",
        );
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::Stalled { first }],
        )]))
        .await;
        let fixture = gateway_fixture(&server.upstream, &[None], 1);
        let (app, billing, db_path) = billed_app(fixture.gateway.clone()).await;
        let response = invoke(app.clone(), json!({"contents": []}), true).await;
        assert_eq!(response.status(), StatusCode::OK);
        drop(response);

        tokio::time::timeout(
            Duration::from_secs(1),
            fixture.gateway.shutdown_until(Some(
                tokio::time::Instant::now() + Duration::from_millis(25),
            )),
        )
        .await
        .expect("shutdown barrier did not abort the stalled Gemini stream");
        billing.flush().await.unwrap();
        let account = billing.account(ACCOUNT_ID).await.unwrap().unwrap();
        assert_eq!(account.reserved_nano, 0);
        assert_eq!(account.spent_nano, 9);
        drop(app);
        drop(billing);
        let _ = fs::remove_file(db_path);
    }

    #[test]
    fn route_allowlist_is_native_and_closed() {
        assert_eq!(
            parse_route(&Method::GET, "/v1beta/models")
                .unwrap()
                .operation,
            Operation::Models
        );
        assert_eq!(
            parse_route(
                &Method::POST,
                "/v1beta/models/gemini-2.5-pro:streamGenerateContent"
            )
            .unwrap()
            .operation,
            Operation::StreamGenerate
        );
        assert!(parse_route(&Method::POST, "/v1beta/files").is_err());
        assert!(parse_route(&Method::GET, "/v1beta/models/x:generateContent").is_err());
    }

    #[test]
    fn query_credentials_are_rejected_and_streaming_framing_is_native() {
        assert!(parse_stream_query(Some("key=secret"), false).is_err());
        assert!(parse_stream_query(Some("%6bey=secret"), false).is_err());
        assert!(parse_stream_query(Some("API%5fKEY=secret"), false).is_err());
        assert!(parse_stream_query(Some("%zz=broken"), false).is_err());
        assert!(parse_stream_query(Some("foo=bar"), true).is_err());
        // An unknown alt value is rejected; sse and json are the two native framings.
        assert!(parse_stream_query(Some("alt=media"), true).is_err());
        // alt on a non-streaming operation is rejected.
        assert!(parse_stream_query(Some("alt=sse"), false).is_err());
        // Upstream is always alt=sse for streaming; the downstream framing follows the client.
        let (upstream, framing) = parse_stream_query(Some("alt=sse"), true).unwrap();
        assert_eq!(upstream, "alt=sse");
        assert_eq!(framing, StreamFraming::Sse);
        let (upstream, framing) = parse_stream_query(Some("alt=json"), true).unwrap();
        assert_eq!(upstream, "alt=sse");
        assert_eq!(framing, StreamFraming::JsonArray);
        // No alt on a streaming call yields the native JSON array, not SSE.
        let (upstream, framing) = parse_stream_query(None, true).unwrap();
        assert_eq!(upstream, "alt=sse");
        assert_eq!(framing, StreamFraming::JsonArray);
        // Non-streaming carries no upstream query.
        assert_eq!(parse_stream_query(None, false).unwrap().0, "");
    }

    #[test]
    fn independently_billed_or_unknown_server_tools_fail_closed() {
        for body in [
            json!({"tools": [{"googleMaps": {}}]}),
            json!({"tools": [{"fileSearch": {"fileSearchStoreNames": ["stores/a"]}}]}),
            json!({"tools": [{"futurePaidTool": {}}]}),
            json!({"cachedContent": "cachedContents/customer-selected-resource"}),
        ] {
            assert!(validate_tools(&body).is_err());
        }
        for body in [
            json!({"tools": [{"googleSearch": {}}]}),
            json!({"tools": [{"urlContext": {}}]}),
            json!({"tools": [{"codeExecution": {}}]}),
            json!({"tools": [{"functionDeclarations": []}]}),
        ] {
            validate_tools(&body).unwrap();
        }
    }

    #[test]
    fn retry_info_and_headers_are_parsed_without_exposing_body() {
        let headers = HeaderMap::new();
        let body = br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2.25s"}]}}"#;
        assert_eq!(retry_after(&headers, body, 60), 3);
    }

    #[test]
    fn incremental_sse_tracker_keeps_last_usage_across_chunk_boundaries() {
        let mut tracker = SseTranslator::new(StreamFraming::Sse);
        tracker
            .push(b"data: {\"response\":{\"usageMetadata\":{\"promptToken")
            .unwrap();
        tracker
            .push(b"Count\":10}}}\n\ndata: {\"response\":{\"candidates\":[{\"groundingMetadata\":{")
            .unwrap();
        tracker
            .push(b"\"webSearchQueries\":[\"one\",\"two\"]}}]}}\n\ndata: {\"response\":{\"usageMetadata\":{\"promptTokenCount\":20,")
            .unwrap();
        tracker.push(b"\"candidatesTokenCount\":5}}}\n\n").unwrap();
        assert_eq!(
            tracker.usage,
            metering::GeminiUsage {
                input_tokens: 20,
                output_tokens: 5,
                search_queries: 2,
                grounded_search_prompts: 1,
                ..metering::GeminiUsage::default()
            }
        );
    }

    #[test]
    fn synthetic_errors_never_leak_internal_architecture() {
        let errors = [
            ApiError::not_found(),
            ApiError::unavailable("test"),
            ApiError::rate_limited(Some(1)),
            ApiError::from(AdmissionError::Unauthorized),
            ApiError::from(AdmissionError::LowBalance),
        ];
        for error in errors {
            let body = error.into_response();
            let debug = format!("{body:?}").to_ascii_lowercase();
            for forbidden in [
                "profile",
                "project_id",
                "api_key_file",
                "credential pool",
                "upstream",
                "cooling",
                "billing authority",
            ] {
                assert!(!debug.contains(forbidden), "leaked {forbidden}: {debug}");
            }
        }
    }
}
