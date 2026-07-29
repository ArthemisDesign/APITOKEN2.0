//! Native Gemini Developer API surface with pre-byte rotation and authoritative stream settlement.

use super::billing::{begin_admission, AdmissionError, GeminiAdmission};
use super::config::GeminiModel;
use super::pool::{GeminiGateway, GeminiLease, GeminiProfile};
use crate::metrics::Metrics;
use crate::proxy::TerminalErrorReason;
use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

const GEMINI_BODY_LIMIT: usize = 32 * 1024 * 1024;
const DOWNSTREAM_SEND_TIMEOUT: Duration = Duration::from_secs(5);

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

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: &'static str,
    google_status: &'static str,
    retry_after: Option<u64>,
    reason: &'static str,
}

impl ApiError {
    fn invalid(message: &'static str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message,
            google_status: "INVALID_ARGUMENT",
            retry_after: None,
            reason: "invalid_request",
        }
    }

    fn not_found() -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: "Requested entity was not found.",
            google_status: "NOT_FOUND",
            retry_after: None,
            reason: "resource_not_found",
        }
    }

    fn unavailable(reason: &'static str) -> Self {
        Self {
            status: StatusCode::SERVICE_UNAVAILABLE,
            message: "The service is currently unavailable. Please retry shortly.",
            google_status: "UNAVAILABLE",
            retry_after: Some(2),
            reason,
        }
    }

    fn rate_limited(retry_after: Option<u64>) -> Self {
        Self {
            status: StatusCode::TOO_MANY_REQUESTS,
            message: "Resource has been exhausted. Please retry later.",
            google_status: "RESOURCE_EXHAUSTED",
            retry_after: retry_after.or(Some(60)),
            reason: "gemini_capacity_exhausted",
        }
    }

    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.status.as_u16(),
                "message": self.message,
                "status": self.google_status
            }
        });
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
            AdmissionError::Unauthorized => Self {
                status: StatusCode::UNAUTHORIZED,
                message: "API key not valid. Please pass a valid API key.",
                google_status: "UNAUTHENTICATED",
                retry_after: None,
                reason: "invalid_key",
            },
            AdmissionError::Unavailable => Self::unavailable("gemini_admission_unavailable"),
            AdmissionError::Busy => Self::rate_limited(Some(1)),
            AdmissionError::LowBalance => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message: "The account balance is insufficient for this request.",
                google_status: "FAILED_PRECONDITION",
                retry_after: None,
                reason: "billing_limit",
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

fn model_value(model: &GeminiModel) -> Value {
    json!({
        "name": format!("models/{}", model.id),
        "version": model.id,
        "displayName": model.display_name,
        "description": model.display_name,
        "inputTokenLimit": model.input_token_limit,
        "outputTokenLimit": model.output_token_limit,
        "supportedGenerationMethods": [
            "generateContent", "streamGenerateContent", "countTokens"
        ]
    })
}

fn query_for_upstream(query: Option<&str>, streaming: bool) -> Result<String, ApiError> {
    let mut parts = Vec::new();
    for part in query
        .unwrap_or_default()
        .split('&')
        .filter(|part| !part.is_empty())
    {
        let raw_name = part.split('=').next().unwrap_or_default();
        let name = percent_decode_query_name(raw_name)?;
        if name.eq_ignore_ascii_case("key") || name.eq_ignore_ascii_case("api_key") {
            return Err(ApiError::invalid(
                "Query-string API keys are not accepted. Use the x-goog-api-key header.",
            ));
        }
        if streaming && name.eq_ignore_ascii_case("alt") {
            continue;
        }
        parts.push(part.to_string());
    }
    if streaming {
        parts.push("alt=sse".to_string());
    }
    Ok(parts.join("&"))
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

fn request_header_allowed(name: &str) -> bool {
    !matches!(
        name,
        "host"
            | "content-length"
            | "connection"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-authorization"
            | "proxy-connection"
            | "authorization"
            | "x-api-key"
            | "x-goog-api-key"
            | "x-goog-user-project"
            | "x-apitoken-api-plane"
            | "x-forwarded-for"
            | "x-forwarded-host"
            | "x-forwarded-proto"
            | "forwarded"
            | "x-real-ip"
            | "via"
            | "cf-connecting-ip"
            | "true-client-ip"
            | "accept-encoding"
    )
}

fn response_header_allowed(name: &str) -> bool {
    !matches!(
        name,
        "content-length"
            | "content-encoding"
            | "connection"
            | "transfer-encoding"
            | "upgrade"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "x-goog-api-key"
            | "x-goog-user-project"
    ) && !name.starts_with("x-ratelimit")
}

fn copy_response_headers(from: &wreq::header::HeaderMap, to: &mut HeaderMap) {
    for (name, value) in from {
        if response_header_allowed(name.as_str()) {
            to.append(name.clone(), value.clone());
        }
    }
}

fn retry_after(headers: &wreq::header::HeaderMap, body: &[u8], default_secs: i64) -> i64 {
    if let Some(seconds) = headers
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<i64>().ok())
    {
        return seconds.clamp(1, 86_400);
    }
    if let Ok(value) = serde_json::from_slice::<Value>(body) {
        if let Some(delay) = value
            .pointer("/error/details")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|detail| {
                detail
                    .get("@type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| kind.ends_with("google.rpc.RetryInfo"))
            })
            .and_then(|detail| detail.get("retryDelay"))
            .and_then(Value::as_str)
            .and_then(|delay| delay.strip_suffix('s'))
            .and_then(|seconds| seconds.parse::<f64>().ok())
        {
            return (delay.ceil() as i64).clamp(1, 86_400);
        }
    }
    default_secs.clamp(1, 86_400)
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

fn upstream_response(
    status: StatusCode,
    headers: &wreq::header::HeaderMap,
    body: Bytes,
) -> Response {
    let mut response = Response::builder()
        .status(status)
        .body(Body::from(body))
        .unwrap();
    copy_response_headers(headers, response.headers_mut());
    response
}

struct UsageTracker {
    pending: Vec<u8>,
    usage: metering::GeminiUsage,
}

impl UsageTracker {
    fn new() -> Self {
        Self {
            pending: Vec::new(),
            usage: metering::GeminiUsage::default(),
        }
    }

    fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let mut line = self.pending.drain(..=newline).collect::<Vec<_>>();
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            self.parse_line(&line);
        }
        // A single Gemini SSE JSON line is bounded by one response chunk in practice. Refuse an
        // unbounded malformed line while still forwarding it byte-for-byte to the client.
        if self.pending.len() > GEMINI_BODY_LIMIT {
            self.pending.clear();
        }
    }

    fn parse_line(&mut self, line: &[u8]) {
        let Ok(line) = std::str::from_utf8(line) else {
            return;
        };
        let Some(data) = line.strip_prefix("data:") else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
            return;
        };
        metering::gemini::merge_stream_response_value(&mut self.usage, &value);
    }

    fn finish(mut self) -> metering::GeminiUsage {
        if !self.pending.is_empty() {
            let line = std::mem::take(&mut self.pending);
            self.parse_line(&line);
        }
        self.usage
    }
}

async fn send_upstream(
    profile: &GeminiProfile,
    url: &str,
    headers: &HeaderMap,
    body: Bytes,
) -> wreq::Result<wreq::Response> {
    let mut request = profile.request(wreq::Method::POST, url);
    for (name, value) in headers {
        if request_header_allowed(name.as_str()) {
            request = request.header(name, value);
        }
    }
    request.body(body).send().await
}

async fn stream_response(
    gateway: Arc<GeminiGateway>,
    metrics: Arc<Metrics>,
    profile: Arc<GeminiProfile>,
    lease: GeminiLease,
    admission: GeminiAdmission,
    model: GeminiModel,
    status: StatusCode,
    headers: wreq::header::HeaderMap,
    first: Bytes,
    mut upstream: impl futures_util::Stream<Item = wreq::Result<Bytes>> + Send + Unpin + 'static,
) -> Result<Response, ApiError> {
    admission.mark_delivering().await?;
    let background = gateway
        .track_background_task()
        .map_err(|_| ApiError::unavailable("gemini_shutdown"))?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::spawn(async move {
        let _background = background;
        let _lease = lease;
        let mut tracker = UsageTracker::new();
        let mut deliver = true;
        let mut clean_eof = true;
        let mut aborted = false;

        tracker.push(&first);
        tokio::select! {
            _ = gateway.stream_abort_requested() => aborted = true,
            result = tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(first)) => {
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(_)) | Err(_) => deliver = false,
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
                    tracker.push(&chunk);
                    if deliver {
                        match tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(chunk))
                            .await
                        {
                            Ok(Ok(())) => {}
                            Ok(Err(_)) | Err(_) => deliver = false,
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
            profile.mark_healthy();
        }
        admission.settle(&model, &tracker.finish());
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
    copy_response_headers(&headers, response.headers_mut());
    Ok(response)
}

pub async fn api(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    match api_inner(app, peer, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
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
    let query = query_for_upstream(
        request.uri().query(),
        route.operation == Operation::StreamGenerate,
    )?;
    let pending = begin_admission(&app, request.headers(), &peer).await?;

    if route.operation == Operation::Models {
        let models = gateway
            .config()
            .models
            .iter()
            .map(model_value)
            .collect::<Vec<_>>();
        let _admission = pending.without_reserve();
        return Ok((StatusCode::OK, axum::Json(json!({"models": models}))).into_response());
    }
    let model_id = route.model.as_deref().ok_or_else(ApiError::not_found)?;
    let model = gateway
        .config()
        .model(model_id)
        .cloned()
        .ok_or_else(ApiError::not_found)?;
    if route.operation == Operation::Model {
        let _admission = pending.without_reserve();
        return Ok((StatusCode::OK, axum::Json(model_value(&model))).into_response());
    }

    let (parts, body) = request.into_parts();
    let mut body = to_bytes(body, GEMINI_BODY_LIMIT)
        .await
        .map_err(|_| ApiError::invalid("The request body is invalid or too large."))?;
    let mut value: Value = serde_json::from_slice(&body)
        .map_err(|_| ApiError::invalid("The request body is not valid JSON."))?;
    if !value.is_object() {
        return Err(ApiError::invalid("The request body must be a JSON object."));
    }
    validate_tools(&value)?;
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
        body = Bytes::from(
            serde_json::to_vec(&value)
                .map_err(|_| ApiError::invalid("The request body is not valid JSON."))?,
        );
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
    let mut url = format!(
        "{}/v1beta/models/{}:{suffix}",
        gateway.config().upstream,
        model.id
    );
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query);
    }

    let mut excluded = HashSet::new();
    let mut transport_failures = 0usize;
    let mut saw_quota = false;
    let mut saw_auth = false;
    loop {
        let Some(lease) = gateway.select(&excluded) else {
            Metrics::inc(&app.metrics.exhausted);
            let retry = gateway
                .soonest_ready(&HashSet::new())
                .map(|until| until.saturating_sub(pool::now()).max(1) as u64);
            return if saw_quota {
                Err(ApiError::rate_limited(retry))
            } else if saw_auth || transport_failures > 0 {
                Err(ApiError::unavailable("gemini_profiles_unavailable"))
            } else {
                Err(ApiError::rate_limited(retry))
            };
        };
        let profile = lease.profile().clone();
        let response = match send_upstream(&profile, &url, &parts.headers, body.clone()).await {
            Ok(response) => response,
            Err(_) => {
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
        let status = StatusCode::from_u16(response.status().as_u16())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let response_headers = response.headers().clone();

        if status.is_success() && route.operation == Operation::StreamGenerate {
            let mut stream = response.bytes_stream();
            let first = match stream.next().await {
                Some(Ok(chunk)) => chunk,
                Some(Err(_)) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    transport_failures += 1;
                    if transport_failures > gateway.config().max_transport_retries {
                        return Err(ApiError::unavailable("gemini_stream_start_failed"));
                    }
                    continue;
                }
                None => {
                    profile.mark_healthy();
                    admission.mark_delivering().await?;
                    admission.settle(&model, &metering::GeminiUsage::default());
                    return Ok(upstream_response(status, &response_headers, Bytes::new()));
                }
            };
            profile.mark_healthy();
            return stream_response(
                gateway,
                app.metrics.clone(),
                profile,
                lease,
                admission,
                model,
                status,
                response_headers,
                first,
                stream,
            )
            .await;
        }

        let response_body = match response.bytes().await {
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
                profile.cool_until(pool::now() + delay);
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
                profile.mark_healthy();
                admission.mark_delivering().await?;
                let usage = if route.operation == Operation::Generate {
                    metering::gemini::usage_from_response_json(&response_body)
                } else {
                    metering::GeminiUsage::default()
                };
                admission.settle(&model, &usage);
                return Ok(upstream_response(status, &response_headers, response_body));
            }
            _ => {
                // Validation/model/safety errors belong to the request. They are native Google
                // errors and can be returned verbatim; never cool a healthy project for them.
                profile.mark_healthy();
                return Ok(upstream_response(status, &response_headers, response_body));
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
            .get("x-goog-api-key")
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        state.seen.lock().unwrap().push(SeenRequest {
            credential: credential.clone(),
            uri: uri.to_string(),
            body,
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
        let keys = [PROFILE_A_KEY, PROFILE_B_KEY];
        let mut profiles = Vec::new();
        for (index, proxy) in proxies.iter().enumerate() {
            let key_file = directory.join(format!("profile-{index}.key"));
            fs::write(&key_file, keys[index]).unwrap();
            fs::set_permissions(&key_file, fs::Permissions::from_mode(0o600)).unwrap();
            let mut profile = json!({
                "id": format!("profile_{}", (b'a' + index as u8) as char),
                "project_id": format!("paid-project-{:02}", index + 1),
                "api_key_file": key_file,
            });
            if let Some(proxy) = proxy {
                profile["proxy"] = Value::String((*proxy).to_string());
            }
            profiles.push(profile);
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
            models: vec![model],
            connect_timeout_secs: 1,
            read_timeout_secs: 5,
            max_transport_retries,
            auth_quarantine_secs: 900,
            transport_cool_secs: 30,
            default_rate_limit_cool_secs: 60,
            health_probe_interval_secs: 60,
            reserve_overhead_tokens: 10,
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
        let method = if streaming {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        let request = axum::extract::Request::builder()
            .method(Method::POST)
            .uri(format!("/v1beta/models/gemini-integration-model:{method}"))
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
                    json!({"candidates": [], "usageMetadata": {"promptTokenCount": 3}}),
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
        let _ = response_json(response).await;
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
    async fn low_balance_caps_max_output_tokens_before_the_google_request() {
        let server = start_mock(MockState::with_replies([(
            PROFILE_A_KEY,
            vec![MockReply::json(
                StatusCode::OK,
                json!({"candidates": [], "usageMetadata": {}}),
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
        assert_eq!(upstream_body["generationConfig"]["maxOutputTokens"], 7);
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
                    json!({"error": {"status": "INVALID_ARGUMENT", "message": "bad input"}}),
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
        assert_eq!(server.state.seen().len(), 1);
        assert_eq!(server.state.seen()[0].credential, PROFILE_A_KEY);
    }

    #[tokio::test]
    async fn auth_failure_quarantines_only_the_failed_project_and_rotates() {
        let server = start_mock(MockState::with_replies([
            (
                PROFILE_A_KEY,
                vec![MockReply::json(
                    StatusCode::UNAUTHORIZED,
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
            StatusCode::UNAUTHORIZED,
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
            .all(|profile| profile.cooling_until >= now + 2));
    }

    #[tokio::test]
    async fn sse_is_forwarded_across_upstream_chunk_boundaries_without_retry_after_first_byte() {
        let first = Bytes::from_static(
            b"data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"one\"}]}}]}\n\n",
        );
        let final_usage = Bytes::from_static(
            b"data: {\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":4}}\n\n",
        );
        let (reply, drained) = MockReply::stream(vec![
            MockChunk::Data(first.clone()),
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
        assert_eq!(bytes, [first.as_ref(), final_usage.as_ref()].concat());
        assert!(drained.load(Ordering::Acquire));
        assert!(server.state.seen()[0].uri.ends_with("?alt=sse"));

        let (broken, _) = MockReply::stream(vec![
            MockChunk::Data(Bytes::from_static(b"data: {\"first\":true}\n\n")),
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
        assert_eq!(body, Bytes::from_static(b"data: {\"first\":true}\n\n"));
        assert_eq!(broken_server.state.seen().len(), 1);
        assert_eq!(broken_server.state.seen()[0].credential, PROFILE_A_KEY);
    }

    #[tokio::test]
    async fn downstream_disconnect_still_drains_final_usage_and_settles_google_ledger() {
        let first = Bytes::from_static(b"data: {\"candidates\":[{\"content\":{}}]}\n\n");
        let final_usage = Bytes::from_static(
            b"data: {\"usageMetadata\":{\"promptTokenCount\":10,\"candidatesTokenCount\":5}}\n\n",
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
    async fn shutdown_deadline_aborts_stalled_stream_then_settles_last_known_usage_before_returning(
    ) {
        let first = Bytes::from_static(
            b"data: {\"usageMetadata\":{\"promptTokenCount\":7,\"candidatesTokenCount\":2}}\n\n",
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
    fn query_credentials_are_rejected_and_streaming_forces_sse() {
        assert!(query_for_upstream(Some("key=secret"), false).is_err());
        assert!(query_for_upstream(Some("%6bey=secret"), false).is_err());
        assert!(query_for_upstream(Some("API%5fKEY=secret"), false).is_err());
        assert!(query_for_upstream(Some("%zz=broken"), false).is_err());
        assert_eq!(
            query_for_upstream(Some("alt=json&foo=bar"), true).unwrap(),
            "foo=bar&alt=sse"
        );
    }

    #[test]
    fn independently_billed_or_unknown_server_tools_fail_closed() {
        for body in [
            json!({"tools": [{"googleMaps": {}}]}),
            json!({"tools": [{"fileSearch": {"fileSearchStoreNames": ["stores/a"]}}]}),
            json!({"tools": [{"futurePaidTool": {}}]}),
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
        let headers = wreq::header::HeaderMap::new();
        let body = br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"2.25s"}]}}"#;
        assert_eq!(retry_after(&headers, body, 60), 3);
    }

    #[test]
    fn incremental_sse_tracker_keeps_last_usage_across_chunk_boundaries() {
        let mut tracker = UsageTracker::new();
        tracker.push(b"data: {\"usageMetadata\":{\"promptToken");
        tracker.push(b"Count\":10}}\n\ndata: {\"candidates\":[{\"groundingMetadata\":{");
        tracker.push(b"\"webSearchQueries\":[\"one\",\"two\"]}}]}\n\ndata: {\"usageMetadata\":{\"promptTokenCount\":20,");
        tracker.push(b"\"candidatesTokenCount\":5}}\n\n");
        assert_eq!(
            tracker.finish(),
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
