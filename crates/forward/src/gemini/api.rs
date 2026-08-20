//! Native Gemini-compatible surface backed by encrypted paid-subscription OAuth profiles.

use super::billing::{
    begin_admission, AdmissionError, GeminiAdmission, GeminiBillableRequestSpec,
    GeminiRequestFactSeed,
};
use super::config::{GeminiConfig, GeminiModel};
use super::pool::{GeminiGateway, GeminiLease, GeminiProfile, TokenAcquisitionPolicy, TokenError};
use super::rate_limit::{self, RateLimitDiagnostic};
use super::transport::{
    ActualSendObserver, TransportError, TransportResponse, TransportRetryPolicy,
};
use super::REPLAYED_FUNCTION_CALL_THOUGHT_SIGNATURE;
use crate::metrics::Metrics;
use crate::proxy::{with_not_started, TerminalErrorReason};
use crate::request_classification::{classify_gemini_generate_content, RequestClassification};
use crate::state::AppState;
use crate::{AffinityInput, AffinityResolution};
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, State};
use axum::http::{HeaderMap, HeaderValue, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use base64::Engine as _;
use bytes::Bytes;
use futures_util::StreamExt;
use gemini_credential::OAuthKind;
use registry::request_facts::{
    DeliveryState, ProviderTerminalClass, RequestFactTerminalEvidence, TerminalRequestFact,
    MAX_REQUEST_FACT_MODEL_LEN,
};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::convert::Infallible;
use std::fmt;
use std::io::Cursor;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

// Preserve the existing text-request envelope. Gemini's documented inline-media request ceiling is
// lower, so image generation is bounded independently after the route model is resolved. Generated
// 4K images are returned as base64 inside JSON and need a larger, still bounded response envelope.
const GEMINI_TEXT_REQUEST_BODY_LIMIT: usize = 32 * 1024 * 1024;
const GEMINI_IMAGE_REQUEST_BODY_LIMIT: usize = 20 * 1024 * 1024;
const GEMINI_BODY_LIMIT: usize = 64 * 1024 * 1024;
const DOWNSTREAM_SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Bounds the private prelude while a retry is still possible — nothing more. The abuse it was
/// once alone against (an upstream emitting endless credit/accounting frames or empty chunks
/// without ever producing a public event) is carried by STREAM_START_MAX_BYTES and
/// STREAM_START_MAX_CHUNKS below, which stop an endless prologue regardless of how slow it is.
/// Time therefore no longer has to stand in for that limit, and it must not: on a long prompt a
/// reasoning model's time-to-first-token legitimately exceeds any small value, and the old 30s
/// turned that into a 503 plus a spurious model-failure mark on a healthy profile.
const STREAM_START_TIMEOUT: Duration = Duration::from_secs(600);
const STREAM_START_MAX_BYTES: usize = 1024 * 1024;
const STREAM_START_MAX_CHUNKS: usize = 1024;
const IMAGE_STREAM_START_MAX_CHUNKS: usize = 8192;
// The public Gemini catalogue advertises 65,536 output tokens, but the private Antigravity
// generation endpoint rejects that exact boundary while accepting 65,535. Keep the public model
// contract intact and adapt only the private wire request.
const ANTIGRAVITY_WIRE_OUTPUT_TOKEN_LIMIT: u64 = 65_535;
const GEMINI_37_MODEL: &str = "gemini-3.7-flash";
const CALIBRATION_DISPATCH_HEADER: &str = "x-apitoken-calibration-dispatch-ms";

/// Native Gemini accepts proto-JSON in either camelCase or snake_case. Code Assist and the public
/// surface are canonicalized to camelCase so a snake_case client is not silently dropped. Only the
/// documented top-level GenerateContentRequest fields are aliased; anything else is left untouched.
const REQUEST_FIELD_ALIASES: &[(&str, &str)] = &[
    ("system_instruction", "systemInstruction"),
    ("safety_settings", "safetySettings"),
    ("generation_config", "generationConfig"),
    ("tool_config", "toolConfig"),
    ("cached_content", "cachedContent"),
    ("service_tier", "serviceTier"),
    ("generate_content_request", "generateContentRequest"),
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

const GENERATION_CONFIG_FIELD_ALIASES: &[(&str, &str)] = &[
    ("candidate_count", "candidateCount"),
    ("stop_sequences", "stopSequences"),
    ("max_output_tokens", "maxOutputTokens"),
    ("top_p", "topP"),
    ("top_k", "topK"),
    ("response_mime_type", "responseMimeType"),
    ("response_schema", "responseSchema"),
    ("response_json_schema", "responseJsonSchema"),
    ("presence_penalty", "presencePenalty"),
    ("frequency_penalty", "frequencyPenalty"),
    ("response_logprobs", "responseLogprobs"),
    ("thinking_config", "thinkingConfig"),
    ("image_config", "imageConfig"),
    ("response_modalities", "responseModalities"),
];

const PART_FIELD_ALIASES: &[(&str, &str)] = &[
    ("inline_data", "inlineData"),
    ("file_data", "fileData"),
    ("thought_signature", "thoughtSignature"),
];

const MEDIA_DATA_FIELD_ALIASES: &[(&str, &str)] = &[("mime_type", "mimeType")];

const THINKING_CONFIG_FIELD_ALIASES: &[(&str, &str)] = &[
    ("thinking_level", "thinkingLevel"),
    ("thinking_budget", "thinkingBudget"),
    ("include_thoughts", "includeThoughts"),
];

fn promote_aliases(object: &mut serde_json::Map<String, Value>, aliases: &[(&str, &str)]) {
    for (snake, camel) in aliases {
        if let Some(aliased) = object.remove(*snake) {
            object.entry((*camel).to_string()).or_insert(aliased);
        }
    }
}

/// Canonicalize a native request object in place: snake_case aliases become camelCase for the known
/// top-level fields and recognized tool keys. When both spellings are present the camelCase value
/// wins and the snake_case duplicate is discarded, matching Google's proto-JSON precedence.
pub(crate) fn canonicalize_native_request(value: &mut Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    promote_aliases(object, REQUEST_FIELD_ALIASES);
    for content_name in ["contents", "systemInstruction"] {
        let contents = match object.get_mut(content_name) {
            Some(Value::Array(contents)) => contents.as_mut_slice(),
            Some(content) => std::slice::from_mut(content),
            None => continue,
        };
        for content in contents {
            let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) else {
                continue;
            };
            for part in parts {
                let Some(part) = part.as_object_mut() else {
                    continue;
                };
                promote_aliases(part, PART_FIELD_ALIASES);
                for media_name in ["inlineData", "fileData"] {
                    if let Some(media) = part.get_mut(media_name).and_then(Value::as_object_mut) {
                        promote_aliases(media, MEDIA_DATA_FIELD_ALIASES);
                    }
                }
            }
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
    if let Some(nested) = object.get_mut("generateContentRequest") {
        canonicalize_native_request(nested);
    }
    if let Some(generation_config) = object
        .get_mut("generationConfig")
        .and_then(Value::as_object_mut)
    {
        promote_aliases(generation_config, GENERATION_CONFIG_FIELD_ALIASES);
        if let Some(thinking_config) = generation_config
            .get_mut("thinkingConfig")
            .and_then(Value::as_object_mut)
        {
            promote_aliases(thinking_config, THINKING_CONFIG_FIELD_ALIASES);
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

/// Antigravity image generation uses a distinct first-party request lineage. It is not an agent
/// UUID with a different requestType: the timestamp and fixed terminal segment are part of the
/// captured image_gen wire identity. Build it once per public request so profile rotation cannot
/// duplicate a logical generation.
fn fresh_antigravity_request_id(image_generation: bool) -> String {
    if !image_generation {
        return format!("agent-{}", crate::fresh_request_id());
    }
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("image_gen/{millis}/{}/12", crate::fresh_request_id())
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

impl Operation {
    fn diagnostic_name(self) -> &'static str {
        match self {
            Self::Models => "list_models",
            Self::Model => "get_model",
            Self::Generate => "generate",
            Self::StreamGenerate => "stream_generate",
            Self::CountTokens => "count_tokens",
        }
    }
}

#[derive(Debug)]
struct ParsedRoute {
    operation: Operation,
    model: Option<String>,
}

/// Content-free accepted public-route intent created only after a universal adapter accepts the
/// original client body. The native execution owner consumes it to admit exactly one fact under the
/// public route semantics; no request JSON, tool name, arbitrary identifier, or header crosses this
/// boundary.
#[derive(Clone)]
pub(crate) enum UniversalGenerationOrigin {
    Chat {
        requested_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    },
    Responses {
        requested_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    },
    Messages {
        requested_model: Option<String>,
        stream_flag: bool,
        classification: RequestClassification,
    },
}

impl fmt::Debug for UniversalGenerationOrigin {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("UniversalGenerationOrigin(<redacted>)")
    }
}

impl UniversalGenerationOrigin {
    fn into_spec(self, executable_model: Option<String>) -> GeminiBillableRequestSpec {
        match self {
            Self::Chat {
                requested_model,
                stream_flag,
                classification,
            } => GeminiBillableRequestSpec::universal_chat(
                requested_model,
                executable_model,
                stream_flag,
                classification,
            ),
            Self::Responses {
                requested_model,
                stream_flag,
                classification,
            } => GeminiBillableRequestSpec::universal_responses(
                requested_model,
                executable_model,
                stream_flag,
                classification,
            ),
            Self::Messages {
                requested_model,
                stream_flag,
                classification,
            } => GeminiBillableRequestSpec::universal_messages(
                requested_model,
                executable_model,
                stream_flag,
                classification,
            ),
        }
    }
}

#[derive(Clone)]
pub(crate) struct UniversalCountTokensIntent {
    pub(crate) requested_model: Option<String>,
    pub(crate) classification: RequestClassification,
}

#[derive(Clone, Copy)]
struct CountTokensTerminalEvidence {
    provider_terminal_class: ProviderTerminalClass,
    delivery_state: DeliveryState,
    attempts_exhaustive: bool,
}

impl CountTokensTerminalEvidence {
    fn local(observer: &ActualSendObserver) -> Self {
        Self {
            provider_terminal_class: ProviderTerminalClass::Unknown,
            delivery_state: match observer.count() {
                Some(0) => DeliveryState::NotStarted,
                _ => DeliveryState::Unknown,
            },
            attempts_exhaustive: true,
        }
    }

    fn body(status: StatusCode) -> Self {
        Self {
            provider_terminal_class: provider_status_class(status),
            delivery_state: DeliveryState::Completed,
            attempts_exhaustive: true,
        }
    }

    fn headers(status: StatusCode) -> Self {
        Self {
            provider_terminal_class: provider_status_class(status),
            delivery_state: DeliveryState::Started,
            attempts_exhaustive: true,
        }
    }

    fn transport(error: TransportError) -> Self {
        Self {
            provider_terminal_class: match error {
                TransportError::Timeout => ProviderTerminalClass::Timeout,
                TransportError::Protocol | TransportError::BodyTooLarge => {
                    ProviderTerminalClass::ProtocolError
                }
                TransportError::Spawn | TransportError::Closed | TransportError::Network => {
                    ProviderTerminalClass::Transport
                }
                TransportError::CalibrationExpired => ProviderTerminalClass::Unknown,
            },
            delivery_state: DeliveryState::Unknown,
            attempts_exhaustive: true,
        }
    }

    fn protocol() -> Self {
        Self {
            provider_terminal_class: ProviderTerminalClass::ProtocolError,
            delivery_state: DeliveryState::Unknown,
            attempts_exhaustive: true,
        }
    }
}

fn billable_generation_fact_eligible(operation: Operation, image_generation: bool) -> bool {
    matches!(operation, Operation::Generate | Operation::StreamGenerate) && !image_generation
}

fn settle_billable_failure(
    admission: &mut Option<GeminiAdmission>,
    http_status: StatusCode,
    provider_terminal_class: ProviderTerminalClass,
    delivery_state: DeliveryState,
) {
    if let Some(admission) = admission.take() {
        admission.settle_failure(http_status, provider_terminal_class, delivery_state);
    }
}

fn settle_observed_billable_failure(
    admission: &mut Option<GeminiAdmission>,
    http_status: StatusCode,
    provider_terminal_class: ProviderTerminalClass,
) {
    let delivery_state = admission
        .as_ref()
        .map(GeminiAdmission::observed_failure_delivery)
        .unwrap_or(DeliveryState::Unknown);
    settle_billable_failure(
        admission,
        http_status,
        provider_terminal_class,
        delivery_state,
    );
}

fn provider_status_class(status: StatusCode) -> ProviderTerminalClass {
    match status.as_u16() {
        200..=299 => ProviderTerminalClass::Success,
        401 | 403 => ProviderTerminalClass::Auth,
        408 => ProviderTerminalClass::Timeout,
        409 | 425 | 500..=599 => ProviderTerminalClass::UpstreamError,
        429 => ProviderTerminalClass::Quota,
        400..=499 => ProviderTerminalClass::ClientError,
        _ => ProviderTerminalClass::Unknown,
    }
}

/// Shared exactly-once owner used by the universal response-extension handoff. Dropping the last
/// reference without `finish` is cancellation/panic safety and emits only conservative evidence.
struct GeminiCountTokensFactState {
    billing: Arc<crate::billing::AsyncBilling>,
    seed: GeminiRequestFactSeed,
    route_class: &'static str,
    requested_model: Option<String>,
    executable_model: Option<String>,
    classification: Option<RequestClassification>,
    actual_sends: ActualSendObserver,
    submitted: AtomicBool,
}

#[derive(Clone)]
pub(crate) struct GeminiCountTokensFactHandoff {
    state: Arc<GeminiCountTokensFactState>,
    terminal_evidence: CountTokensTerminalEvidence,
}

struct GeminiCountTokensFactGuard {
    state: Arc<GeminiCountTokensFactState>,
    terminal_evidence: CountTokensTerminalEvidence,
}

impl GeminiCountTokensFactGuard {
    fn new(
        billing: Arc<crate::billing::AsyncBilling>,
        seed: GeminiRequestFactSeed,
        intent: Option<UniversalCountTokensIntent>,
    ) -> Self {
        let (route_class, requested_model, classification) = match intent {
            Some(intent) => (
                "universal",
                intent.requested_model,
                Some(intent.classification),
            ),
            None => ("native", None, None),
        };
        let actual_sends = ActualSendObserver::default();
        Self {
            state: Arc::new(GeminiCountTokensFactState {
                billing,
                seed,
                route_class,
                requested_model,
                executable_model: None,
                classification,
                actual_sends: actual_sends.clone(),
                submitted: AtomicBool::new(false),
            }),
            terminal_evidence: CountTokensTerminalEvidence::local(&actual_sends),
        }
    }

    fn actual_send_observer(&self) -> ActualSendObserver {
        self.state.actual_sends.clone()
    }

    fn update_after_native_accept(&mut self, model: &str, value: &Value) {
        let state =
            Arc::get_mut(&mut self.state).expect("request fact state is unique before send");
        if state.route_class == "native" {
            state.requested_model = bounded_request_fact_model(model);
            state.classification = Some(classify_gemini_generate_content(
                value.get("generateContentRequest").unwrap_or(value),
            ));
        }
    }

    fn resolve_executable_model(&mut self, model: &str) {
        Arc::get_mut(&mut self.state)
            .expect("request fact state is unique before send")
            .executable_model = bounded_request_fact_model(model);
    }

    fn observe(&mut self, evidence: CountTokensTerminalEvidence) {
        self.terminal_evidence = evidence;
    }

    fn terminal_response(self, response: &mut Response) {
        if self.state.route_class == "universal" {
            response
                .extensions_mut()
                .insert(GeminiCountTokensFactHandoff {
                    state: Arc::clone(&self.state),
                    terminal_evidence: self.terminal_evidence,
                });
        } else {
            submit_gemini_count_tokens_fact(
                &self.state,
                Some(response.status()),
                self.terminal_evidence,
                true,
            );
        }
    }
}

impl Drop for GeminiCountTokensFactGuard {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) == 1 {
            submit_gemini_count_tokens_fact(
                &self.state,
                None,
                CountTokensTerminalEvidence {
                    provider_terminal_class: ProviderTerminalClass::Unknown,
                    delivery_state: DeliveryState::Unknown,
                    attempts_exhaustive: false,
                },
                false,
            );
        }
    }
}

impl GeminiCountTokensFactHandoff {
    pub(crate) fn finish(self, status: StatusCode, mapping_protocol_error: bool) {
        let evidence = if !mapping_protocol_error {
            self.terminal_evidence
        } else {
            CountTokensTerminalEvidence {
                provider_terminal_class: ProviderTerminalClass::ProtocolError,
                delivery_state: self.terminal_evidence.delivery_state,
                attempts_exhaustive: self.terminal_evidence.attempts_exhaustive,
            }
        };
        submit_gemini_count_tokens_fact(&self.state, Some(status), evidence, true);
    }
}

impl Drop for GeminiCountTokensFactHandoff {
    fn drop(&mut self) {
        if Arc::strong_count(&self.state) == 1 {
            submit_gemini_count_tokens_fact(
                &self.state,
                None,
                CountTokensTerminalEvidence {
                    provider_terminal_class: ProviderTerminalClass::Unknown,
                    delivery_state: DeliveryState::Unknown,
                    attempts_exhaustive: false,
                },
                false,
            );
        }
    }
}

fn submit_gemini_count_tokens_fact(
    state: &GeminiCountTokensFactState,
    status: Option<StatusCode>,
    evidence: CountTokensTerminalEvidence,
    exhaustive: bool,
) {
    if state.submitted.swap(true, Ordering::AcqRel) {
        return;
    }
    let seed = &state.seed;
    let terminal_at = pool::now().max(seed.admitted_at);
    let first_public_byte_at = seed
        .lifecycle_clock
        .seal_first_public_byte_for_terminal(seed.admitted_at, terminal_at);
    let attempts = (exhaustive && evidence.attempts_exhaustive)
        .then(|| state.actual_sends.count())
        .flatten()
        .and_then(|count| i32::try_from(count).ok());
    let classification = state.classification.as_ref();
    let fact = TerminalRequestFact {
        logical_request_id: seed.logical_request_id.clone(),
        billing_request_id: None,
        execution_group_id: seed.execution.group_id().map(str::to_owned),
        attempt: seed.execution.attempt(),
        account_id: seed.account_id.clone(),
        key_id: seed.key_id.clone(),
        client_kind: seed.client_attribution.kind(),
        client_source: seed.client_attribution.source(),
        client_version: seed.client_attribution.version().map(str::to_owned),
        provider_plane: "gemini".into(),
        route_class: state.route_class.into(),
        request_class: "count_tokens".into(),
        requested_model: state.requested_model.clone(),
        executable_model: state.executable_model.clone(),
        stream_flag: false,
        tools_declared_count: classification.and_then(RequestClassification::tools_declared_count),
        tool_classes: classification.and_then(RequestClassification::tool_classes),
        tool_choice_mode: classification.and_then(RequestClassification::tool_choice_mode),
        parallel_tools_requested: classification
            .and_then(RequestClassification::parallel_tools_requested),
        tool_results_in_input: classification
            .and_then(RequestClassification::tool_results_in_input),
        structured_output_flag: classification
            .and_then(RequestClassification::structured_output_flag),
        reasoning_flag: classification.and_then(RequestClassification::reasoning_flag),
        service_tier: classification
            .and_then(RequestClassification::service_tier)
            .map(str::to_owned),
        input_modalities: classification.and_then(RequestClassification::input_modalities),
        output_modalities: classification.and_then(RequestClassification::output_modalities),
        admitted_at: seed.admitted_at,
        terminal: RequestFactTerminalEvidence {
            terminal_at,
            http_status_code: status.map(|status| i32::from(status.as_u16())),
            provider_terminal_class: evidence.provider_terminal_class,
            delivery_state: evidence.delivery_state,
            downstream_disconnect: None,
            upstream_request_id: None,
            first_public_byte_at,
            internal_attempt_count: attempts,
            failure_class: None,
            tool_calls_in_output: None,
        },
    };
    let _ = state.billing.try_submit_terminal_request_fact(fact);
}

pub(crate) fn bounded_request_fact_model(value: &str) -> Option<String> {
    (!value.is_empty()
        && value.len() <= MAX_REQUEST_FACT_MODEL_LEN
        && value.is_ascii()
        && !value.bytes().any(|byte| byte.is_ascii_control()))
    .then(|| value.to_owned())
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
    /// True only while the plane can authoritatively prove that provider execution never started.
    /// Exact-target transport/status/body failures after the first send are ambiguous even though
    /// no public response byte was emitted and an optional customer reserve will be refunded.
    execution_not_started: bool,
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
            execution_not_started: true,
        }
    }

    /// A rejection this gateway makes on purpose, carrying a stable machine reason.
    ///
    /// Native `google.rpc.ErrorInfo` is how Google itself reports a machine-readable cause, and an
    /// SDK can branch on it. Without one, a caller only sees prose and cannot tell "this gateway
    /// will never accept this" from "try again": a customer spent hours on an unsupported input
    /// because the refusal named nothing they could act on.
    fn unsupported(message: &'static str, error_info_reason: &'static str) -> Self {
        Self {
            error_info_reason: Some(error_info_reason),
            ..Self::invalid(message)
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
            execution_not_started: true,
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
            execution_not_started: true,
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
            execution_not_started: true,
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
            execution_not_started: true,
        }
    }

    fn after_dispatch(mut self) -> Self {
        self.execution_not_started = false;
        self
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
        // Give the caller something to quote. Every terminal error is journalled with a request id,
        // but until now the Gemini plane never sent one back, so a customer reporting "your API
        // returns 503" left support with no way to find their request among everyone else's.
        if let Ok(value) = HeaderValue::from_str(&crate::fresh_request_id()) {
            response.headers_mut().insert("x-request-id", value);
        }
        if let Some(seconds) = self.retry_after {
            if let Ok(value) = HeaderValue::from_str(&seconds.max(1).to_string()) {
                response.headers_mut().insert("retry-after", value);
            }
        }
        response
            .extensions_mut()
            .insert(TerminalErrorReason(self.reason));
        // Local/pre-send errors satisfy the internal retry proof. A one-shot exact generation
        // switches this flag off immediately after entering the transport: helper failure, HTTP
        // status and response decoding can no longer prove that Google did not execute the POST.
        if self.execution_not_started {
            with_not_started(response)
        } else {
            response
        }
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
                execution_not_started: true,
            },
            AdmissionError::Unavailable => Self::unavailable("gemini_admission_unavailable"),
            // Reseller balance is a documented account state the customer must be able to detect and
            // act on (top up), kept as the cross-provider 402 contract. The envelope stays native.
            AdmissionError::LowBalance => Self {
                status: StatusCode::PAYMENT_REQUIRED,
                message: "The account balance is insufficient for this request.",
                google_status: "FAILED_PRECONDITION",
                retry_after: None,
                reason: "billing_limit",
                error_info_reason: None,
                execution_not_started: true,
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

fn model_value(model: &GeminiModel, batch_public: bool) -> Value {
    // Mirror the native ListModels/GetModel resource shape, including the sampling defaults Google
    // publishes for the Gemini families, so the catalogue is not a thin, obviously-synthetic subset.
    let supported_generation_methods = if batch_public && !model.is_image_generation() {
        json!([
            "generateContent",
            "streamGenerateContent",
            "countTokens",
            "batchGenerateContent"
        ])
    } else {
        json!(["generateContent", "streamGenerateContent", "countTokens"])
    };
    let mut value = json!({
        "name": format!("models/{}", model.id),
        "version": model_version(&model.id),
        "displayName": model.display_name,
        "description": format!("Google {} model served through the Gemini API.", model.display_name),
        "created": model.created,
        "inputTokenLimit": model.input_token_limit,
        "outputTokenLimit": model.output_token_limit,
        "supportedGenerationMethods": supported_generation_methods,
        "apitoken": {
            "limits": {
                "context": model.input_token_limit,
                "input": model.input_token_limit,
                "output": model.output_token_limit
            },
            "capabilities": {
                "reasoning_efforts": model.reasoning_efforts(),
                "service_tiers": ["standard"],
                "input_modalities": model.input_modalities(),
                "output_modalities": model.output_modalities(),
                "tool_calling": model.tool_calling(),
                "structured_outputs": model.structured_outputs(),
                "streaming": true
            }
        }
    });
    if model.id != "gemini-3.7-flash" {
        let object = value
            .as_object_mut()
            .expect("the native Gemini model resource is always an object");
        object.insert("temperature".to_string(), json!(1.0));
        object.insert("topP".to_string(), json!(0.95));
        object.insert("topK".to_string(), json!(64));
        object.insert("maxTemperature".to_string(), json!(2.0));
    }
    value
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

/// Decide how long to cool a model after an upstream 429.
///
/// Two independent safety valves keep a healthy profile from being parked too long:
/// * an unhinted 429 whose fresh quota catalogue still reports a positive remainder is an
///   RPM/concurrency stall, so it cools for the short `rate_limit_rpm_cool_secs`;
/// * a hinted 429 whose error reason is NOT a real `QUOTA_EXHAUSTED` is likewise a transient
///   profile stall (observed on Antigravity image generation with a near-full catalogue, where the
///   account stayed usable via the official client), so its hint is capped at
///   `rate_limit_unknown_cool_secs` rather than honoured verbatim. Only a genuine
///   `QUOTA_EXHAUSTED` is trusted with the full provider hint.
fn generation_429_cool_secs(
    hint: Option<i64>,
    diagnostic: &RateLimitDiagnostic,
    quota_has_remaining: bool,
    config: &GeminiConfig,
) -> i64 {
    if let Some(secs) = hint {
        if diagnostic.error_reason() == "QUOTA_EXHAUSTED" {
            secs
        } else {
            secs.min(config.rate_limit_unknown_cool_secs)
        }
    } else if quota_has_remaining {
        config.rate_limit_rpm_cool_secs
    } else {
        config.default_rate_limit_cool_secs
    }
}

fn log_rate_limit_attempt(
    request_id: &str,
    operation: &'static str,
    phase: &'static str,
    routing_attempt: usize,
    public_model_id: &str,
    wire_model_id: &str,
    profile_id: &str,
    oauth_kind: OAuthKind,
    diagnostic: &RateLimitDiagnostic,
    applied_cool_secs: i64,
    quota_evidence: &super::pool::GeminiRateLimitQuotaEvidence,
) {
    let oauth_kind = match oauth_kind {
        OAuthKind::Antigravity => "antigravity",
        OAuthKind::LegacyGeminiCli => "legacy_gemini_cli",
    };
    elog::warn(
        "gemini-rate-limit",
        format!(
            "gemini upstream 429: request_id={request_id} operation={operation} phase={phase} routing_attempt={routing_attempt} public_model={public_model_id} wire_model={wire_model_id} profile={profile_id} oauth_kind={oauth_kind} {} {quota_evidence}",
            diagnostic.fields(applied_cool_secs),
        ),
    );
}

fn log_rate_limit_exhausted(
    request_id: &str,
    public_model_id: &str,
    wire_model_id: &str,
    rate_limit_attempts: usize,
    routing_attempts: usize,
    distinct_profiles: usize,
    retry_after_secs: u64,
) {
    elog::warn(
        "gemini-rate-limit",
        format!(
            "gemini 429 rotation exhausted: request_id={request_id} public_model={public_model_id} wire_model={wire_model_id} rate_limit_attempts={rate_limit_attempts} routing_attempts={routing_attempts} distinct_profiles={distinct_profiles} retry_after_secs={retry_after_secs}"
        ),
    );
}

pub(crate) fn batch_generation_controls(
    body: &Value,
    model: &GeminiModel,
) -> (u64, u64, u64, bool) {
    generation_controls(body, model, 0, AudioUsageHint::default())
}

fn generation_controls(
    body: &Value,
    model: &GeminiModel,
    overhead: u64,
    audio_hint: AudioUsageHint,
) -> (u64, u64, u64, bool) {
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
    // Text keeps the longstanding byte-conservative estimate. Base64 bytes are transport, not
    // image tokens: Gemini 3 allocates at most 2,240 input tokens per inline image, so replacing
    // their encoded payload length avoids rejecting an affordable edit solely because its JPEG is
    // large. Authoritative usageMetadata still decides settlement.
    let mut estimate = (body.to_string().len() as u64).saturating_add(overhead);
    // Inline audio base64 is transport rather than model input. Flash Preview's exact request-side
    // hint replaces those encoded characters with the official 32-token/second media count; the
    // reservation still prices every estimated input token at the most expensive input SKU.
    estimate = estimate
        .saturating_sub(audio_hint.encoded_data_bytes)
        .saturating_add(audio_hint.tokens);
    if model.is_image_generation() {
        for inline in inline_image_data(body) {
            if let Some(data) = inline.get("data").and_then(Value::as_str) {
                estimate = estimate
                    .saturating_sub(data.len() as u64)
                    .saturating_add(2_240);
            }
        }
    }
    let media_output = model
        .is_image_generation()
        .then(|| image_output_tokens(body))
        .unwrap_or(0);
    (estimate, output, media_output, grounding)
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

/// Public Gemini permits `Content.role` to be blank or omitted. Antigravity's private generation
/// endpoint requires every turn to carry one, so infer only unset roles while preserving explicit
/// `user`/`model` values (and any invalid value for Google's normal request validation).
fn normalize_private_content_roles(request: &mut serde_json::Map<String, Value>) {
    let Some(contents) = request.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    let mut next_role = "user";
    for content in contents {
        let Some(content) = content.as_object_mut() else {
            continue;
        };
        let role_is_unset = match content.get("role") {
            None | Some(Value::Null) => true,
            Some(Value::String(role)) => role.is_empty(),
            _ => false,
        };
        if role_is_unset {
            content.insert("role".to_string(), json!(next_role));
            next_role = if next_role == "user" { "model" } else { "user" };
        } else {
            match content.get("role").and_then(Value::as_str) {
                Some("user") => next_role = "model",
                Some("model") => next_role = "user",
                _ => {}
            }
        }
    }
}

/// Make a native Gemini tool replay acceptable to the private Code Assist wire without retaining
/// any gateway-side signature state.
///
/// Correct native clients replay Google's opaque `thoughtSignature`; keep that value byte-for-byte.
/// Some clients (notably Kimi Code 0.33) consume the signature from SSE but discard it while
/// rebuilding the next request. Code Assist rejects the otherwise valid functionResponse turn, so
/// add Google's accepted stateless marker only when the replayed functionCall omitted the field.
fn ensure_replayed_function_call_signatures(request: &mut serde_json::Map<String, Value>) {
    let Some(contents) = request.get_mut("contents").and_then(Value::as_array_mut) else {
        return;
    };
    for content in contents {
        let Some(parts) = content.get_mut("parts").and_then(Value::as_array_mut) else {
            continue;
        };
        for part in parts {
            let Some(part) = part.as_object_mut() else {
                continue;
            };
            if part.get("functionCall").is_some_and(Value::is_object)
                && !part.contains_key("thoughtSignature")
            {
                part.insert(
                    "thoughtSignature".to_string(),
                    json!(REPLAYED_FUNCTION_CALL_THOUGHT_SIGNATURE),
                );
            }
        }
    }
}

fn adapt_antigravity_generation_request(
    request: &mut serde_json::Map<String, Value>,
    image_generation: bool,
) {
    normalize_private_content_roles(request);
    let generation_config = request
        .entry("generationConfig")
        .or_insert_with(|| json!({}));
    let Some(generation_config) = generation_config.as_object_mut() else {
        return;
    };
    if image_generation {
        generation_config.insert("candidateCount".to_string(), json!(1));
        generation_config.insert("responseModalities".to_string(), json!(["TEXT", "IMAGE"]));
        let image_config = generation_config
            .entry("imageConfig")
            .or_insert_with(|| json!({}));
        if let Some(image_config) = image_config.as_object_mut() {
            image_config
                .entry("aspectRatio".to_string())
                .or_insert_with(|| json!("1:1"));
            image_config
                .entry("imageSize".to_string())
                .or_insert_with(|| json!("1K"));
        }
        return;
    }
    let max_output_tokens = generation_config
        .get("maxOutputTokens")
        .and_then(Value::as_u64);
    if max_output_tokens.is_some_and(|limit| limit > ANTIGRAVITY_WIRE_OUTPUT_TOKEN_LIMIT) {
        generation_config.insert(
            "maxOutputTokens".to_string(),
            json!(ANTIGRAVITY_WIRE_OUTPUT_TOKEN_LIMIT),
        );
    }
}

const IMAGE_ASPECT_RATIOS: &[&str] = &[
    "1:1", "1:4", "1:8", "2:3", "3:2", "3:4", "4:1", "4:3", "4:5", "5:4", "8:1", "9:16", "16:9",
    "21:9",
];
const IMAGE_MIME_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/webp",
    "image/heic",
    "image/heif",
];

fn image_output_tokens(body: &Value) -> u64 {
    // Billing follows the paid-tier pricing table, which is the authoritative source for
    // Developer API dollar equivalence. The image-generation resolution table currently lists
    // different 2K/4K processing-token figures; those must not replace the explicitly published
    // billable image-token SKUs below.
    match body
        .pointer("/generationConfig/imageConfig/imageSize")
        .and_then(Value::as_str)
        .unwrap_or("1K")
    {
        "1K" => 1_120,
        "2K" => 1_680,
        "4K" => 2_520,
        _ => 2_520,
    }
}

fn inline_image_data(body: &Value) -> impl Iterator<Item = &Value> {
    body.get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|part| part.get("inlineData"))
}

fn request_parts(body: &Value) -> impl Iterator<Item = &Value> {
    body.get("contents")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(body.get("systemInstruction"))
        .flat_map(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct AudioUsageHint {
    tokens: u64,
    encoded_data_bytes: u64,
}

fn is_audio_mime_type(mime_type: &str) -> bool {
    mime_type
        .split(';')
        .next()
        .unwrap_or(mime_type)
        .trim()
        .get(..6)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("audio/"))
}

/// The private Flash Preview route currently omits the public per-modality usage split. Google
/// documents audio as exactly 32 tokens per second, but does not document fractional-duration
/// rounding. Keep the request-derived fallback exact by accepting only inline PCM WAV media whose
/// frame duration produces an integral token count. Hound is used for strict RIFF/PCM validation;
/// no media is fetched over the network.
fn flash_preview_audio_usage_hint(body: &Value) -> Result<AudioUsageHint, ApiError> {
    let mut hint = AudioUsageHint::default();
    for part in request_parts(body) {
        if let Some(file_data) = part.get("fileData") {
            let file_data = file_data
                .as_object()
                .ok_or_else(|| ApiError::invalid("The fileData field must be a JSON object."))?;
            let mime_type = file_data
                .get("mimeType")
                .and_then(Value::as_str)
                .ok_or_else(|| ApiError::invalid("The fileData mimeType must be a string."))?;
            if is_audio_mime_type(mime_type) {
                return Err(ApiError::invalid(
                    "Gemini 3 Flash Preview audio currently requires inline PCM WAV data.",
                ));
            }
        }

        let Some(inline_data) = part.get("inlineData") else {
            continue;
        };
        let inline_data = inline_data
            .as_object()
            .ok_or_else(|| ApiError::invalid("The inlineData field must be a JSON object."))?;
        let mime_type = inline_data
            .get("mimeType")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("The inlineData mimeType must be a string."))?;
        if !is_audio_mime_type(mime_type) {
            continue;
        }
        if !mime_type.eq_ignore_ascii_case("audio/wav") {
            return Err(ApiError::invalid(
                "Gemini 3 Flash Preview audio currently requires inline PCM WAV data.",
            ));
        }
        let data = inline_data
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("The inline audio data must be a base64 string."))?;
        let tokens = exact_pcm_wav_audio_tokens(data)?;
        hint.tokens = hint
            .tokens
            .checked_add(tokens)
            .ok_or_else(|| ApiError::invalid("The inline audio duration is too large."))?;
        hint.encoded_data_bytes = hint
            .encoded_data_bytes
            .checked_add(data.len() as u64)
            .ok_or_else(|| ApiError::invalid("The inline audio data is too large."))?;
    }
    Ok(hint)
}

fn exact_pcm_wav_audio_tokens(data: &str) -> Result<u64, ApiError> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| ApiError::invalid("The inline audio data is not valid base64."))?;
    if decoded.is_empty() {
        return Err(ApiError::invalid(
            "The inline audio data must not be empty.",
        ));
    }
    let declared_len = hound::read_wave_header(&mut Cursor::new(&decoded))
        .map_err(|_| ApiError::invalid("The inline audio data is not a valid PCM WAV file."))?;
    if declared_len != decoded.len() as u64 {
        return Err(ApiError::invalid(
            "The inline audio data is not a complete PCM WAV file.",
        ));
    }
    let mut reader = hound::WavReader::new(Cursor::new(decoded))
        .map_err(|_| ApiError::invalid("The inline audio data is not a valid PCM WAV file."))?;
    let spec = reader.spec();
    let frames = u64::from(reader.duration());
    if frames == 0 || spec.sample_rate == 0 {
        return Err(ApiError::invalid(
            "The inline audio data must contain at least one PCM frame.",
        ));
    }
    let samples_are_complete = match spec.sample_format {
        hound::SampleFormat::Int => reader.samples::<i32>().all(|sample| sample.is_ok()),
        hound::SampleFormat::Float => reader.samples::<f32>().all(|sample| sample.is_ok()),
    };
    if !samples_are_complete {
        return Err(ApiError::invalid(
            "The inline audio data is not a complete PCM WAV file.",
        ));
    }
    let token_numerator = frames
        .checked_mul(32)
        .ok_or_else(|| ApiError::invalid("The inline audio duration is too large."))?;
    let sample_rate = u64::from(spec.sample_rate);
    if token_numerator % sample_rate != 0 {
        return Err(ApiError::invalid(
            "Gemini 3 Flash Preview audio duration must be an exact multiple of 1/32 second.",
        ));
    }
    Ok(token_numerator / sample_rate)
}

fn content_has_inline_audio_data(content: &Value) -> bool {
    let contents = match content {
        Value::Array(contents) => contents.as_slice(),
        content => std::slice::from_ref(content),
    };
    contents
        .iter()
        .flat_map(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .filter_map(|part| part.get("inlineData"))
        .filter_map(|inline| inline.get("mimeType"))
        .filter_map(Value::as_str)
        .any(is_audio_mime_type)
}

fn has_inline_audio_data(body: &Value) -> bool {
    ["contents", "systemInstruction"]
        .into_iter()
        .filter_map(|field| body.get(field))
        .any(content_has_inline_audio_data)
}

fn content_has_file_data(content: &Value) -> bool {
    let contents = match content {
        Value::Array(contents) => contents.as_slice(),
        content => std::slice::from_ref(content),
    };
    contents
        .iter()
        .flat_map(|content| {
            content
                .get("parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .any(|part| part.get("fileData").is_some())
}

/// True when the request references an uploaded Files API resource instead of inlining the bytes.
fn has_file_data(body: &Value) -> bool {
    ["contents", "systemInstruction"]
        .into_iter()
        .filter_map(|field| body.get(field))
        .any(content_has_file_data)
}

/// Reject a Files API reference before dispatch, naming the supported alternative.
///
/// A `files/…` resource belongs to the Google project that uploaded it. This gateway calls the
/// provider under its own pooled subscription, so the customer's file is invisible to us and every
/// profile answers `PERMISSION_DENIED` — identically, which used to read as a fleet outage rather
/// than an unsupported input. The same reasoning already rejects `cachedContent`.
fn validate_no_file_data(body: &Value) -> Result<(), ApiError> {
    if has_file_data(body) {
        return Err(ApiError::unsupported(
            "Files API references (fileData/file_uri) are not supported by this gateway. \
             Send the file inline as inlineData with its mimeType and base64 data.",
            "FILE_URI_UNSUPPORTED",
        ));
    }
    Ok(())
}

fn validate_image_generation_request(body: &Value, model: &GeminiModel) -> Result<(), ApiError> {
    validate_no_file_data(body)?;
    if body.get("systemInstruction").is_some() {
        return Err(ApiError::invalid(
            "systemInstruction is not supported by the subscription image route.",
        ));
    }
    if body
        .get("tools")
        .is_some_and(|tools| tools.as_array().is_none_or(|tools| !tools.is_empty()))
    {
        return Err(ApiError::invalid(
            "Tools are not supported by the subscription image route.",
        ));
    }
    let contents = body
        .get("contents")
        .and_then(Value::as_array)
        .filter(|contents| !contents.is_empty())
        .ok_or_else(|| ApiError::invalid("Image generation requires non-empty contents."))?;
    let has_prompt = contents.iter().any(|content| {
        content
            .get("parts")
            .and_then(Value::as_array)
            .is_some_and(|parts| {
                parts.iter().any(|part| {
                    part.get("text")
                        .and_then(Value::as_str)
                        .is_some_and(|text| !text.trim().is_empty())
                })
            })
    });
    if !has_prompt {
        return Err(ApiError::invalid(
            "Image generation requires at least one non-empty text prompt.",
        ));
    }

    let config = body.get("generationConfig");
    if config.is_some_and(|value| !value.is_object()) {
        return Err(ApiError::invalid(
            "The generationConfig field must be a JSON object.",
        ));
    }
    if let Some(config) = config.and_then(Value::as_object) {
        if let Some(max_output_tokens) = config.get("maxOutputTokens") {
            let max_output_tokens = max_output_tokens.as_u64().ok_or_else(|| {
                ApiError::invalid("generationConfig.maxOutputTokens must be an integer.")
            })?;
            if !(1..=model.output_token_limit).contains(&max_output_tokens) {
                return Err(ApiError::invalid(
                    "generationConfig.maxOutputTokens is outside the image model limit.",
                ));
            }
        }
        if config
            .get("candidateCount")
            .is_some_and(|count| count.as_u64() != Some(1))
        {
            return Err(ApiError::invalid(
                "Gemini 3.1 Flash Image supports candidateCount=1 only.",
            ));
        }
        if let Some(modalities) = config.get("responseModalities") {
            let modalities = modalities.as_array().ok_or_else(|| {
                ApiError::invalid("generationConfig.responseModalities must be an array.")
            })?;
            let text = modalities
                .iter()
                .filter(|value| value.as_str() == Some("TEXT"))
                .count();
            let image = modalities
                .iter()
                .filter(|value| value.as_str() == Some("IMAGE"))
                .count();
            if modalities.len() != 2 || text != 1 || image != 1 {
                return Err(ApiError::invalid(
                    "The subscription image route requires responseModalities TEXT and IMAGE.",
                ));
            }
        }
        for unsupported in [
            "thinkingConfig",
            "responseMimeType",
            "responseSchema",
            "responseJsonSchema",
            "responseLogprobs",
            "logprobs",
        ] {
            if config.contains_key(unsupported) {
                return Err(ApiError::invalid(
                    "This generationConfig control is not supported by the subscription image route.",
                ));
            }
        }
        if let Some(image_config) = config.get("imageConfig") {
            let image_config = image_config.as_object().ok_or_else(|| {
                ApiError::invalid("generationConfig.imageConfig must be a JSON object.")
            })?;
            if image_config
                .keys()
                .any(|name| !matches!(name.as_str(), "aspectRatio" | "imageSize"))
            {
                return Err(ApiError::invalid(
                    "The imageConfig contains an unsupported field.",
                ));
            }
            if let Some(ratio) = image_config.get("aspectRatio") {
                let ratio = ratio.as_str().ok_or_else(|| {
                    ApiError::invalid("imageConfig.aspectRatio must be a string.")
                })?;
                if !IMAGE_ASPECT_RATIOS.contains(&ratio) {
                    return Err(ApiError::invalid(
                        "The requested image aspect ratio is not supported.",
                    ));
                }
            }
            if let Some(size) = image_config.get("imageSize") {
                let size = size
                    .as_str()
                    .ok_or_else(|| ApiError::invalid("imageConfig.imageSize must be a string."))?;
                // The public Developer API documents 0.5K for this model, but the paid
                // Antigravity subscription route rejects that value with INVALID_ARGUMENT. Keep
                // the private capability allowlist tied to live evidence instead of assuming the
                // two provider surfaces advance together.
                if !matches!(size, "1K" | "2K" | "4K") {
                    return Err(ApiError::invalid(
                        "The subscription image route supports imageSize 1K, 2K, or 4K.",
                    ));
                }
            }
        }
    }

    let inline_images = inline_image_data(body)
        .filter(|inline| {
            // application/pdf is document input, not an image reference: it bypasses the
            // image MIME allowlist and the 14-reference cap.
            inline
                .get("mimeType")
                .and_then(Value::as_str)
                .is_none_or(|mime| !mime.eq_ignore_ascii_case("application/pdf"))
        })
        .collect::<Vec<_>>();
    if inline_images.len() > 14 {
        return Err(ApiError::invalid(
            "Gemini 3.1 Flash Image accepts at most 14 reference images.",
        ));
    }
    let mut decoded_bytes = 0usize;
    for inline in inline_images {
        let inline = inline
            .as_object()
            .ok_or_else(|| ApiError::invalid("The inlineData field must be a JSON object."))?;
        let mime_type = inline
            .get("mimeType")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("The inline image mimeType must be a string."))?;
        let data = inline
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| ApiError::invalid("The inline image data must be a base64 string."))?;
        if !IMAGE_MIME_TYPES.contains(&mime_type) {
            return Err(ApiError::invalid(
                "The inline image MIME type is not supported.",
            ));
        }
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| ApiError::invalid("The inline image data is not valid base64."))?;
        if decoded.is_empty() {
            return Err(ApiError::invalid(
                "The inline image data must not be empty.",
            ));
        }
        decoded_bytes = decoded_bytes.saturating_add(decoded.len());
        if decoded_bytes > GEMINI_IMAGE_REQUEST_BODY_LIMIT {
            return Err(ApiError::invalid("The inline image data is too large."));
        }
    }
    if contents.iter().any(|content| {
        content
            .get("parts")
            .and_then(Value::as_array)
            .is_some_and(|parts| parts.iter().any(|part| part.get("fileData").is_some()))
    }) {
        return Err(ApiError::invalid(
            "fileData is not supported because subscription rotation cannot preserve project-scoped files.",
        ));
    }
    Ok(())
}

fn validate_generation_request(
    body: &Value,
    model: &GeminiModel,
    exact_calibration: bool,
) -> Result<(), ApiError> {
    validate_no_file_data(body)?;
    if model.is_image_generation() && has_inline_audio_data(body) {
        // The image-generation model has no official audio surface. Published text models accept
        // inline audio/wav: 3-flash-preview and 3.7-flash were live-admitted first, and the
        // fleet-wide media matrix (2026-08-16) carries the same perception-marker contract for
        // every remaining text model.
        return Err(ApiError::unsupported(
            "Audio input is not available for this model on the subscription gateway.",
            "AUDIO_INPUT_UNSUPPORTED",
        ));
    }
    if body.get("serviceTier").is_some() {
        // Priority/flex tiers have distinct provider admission and billing semantics. The private
        // subscription surface cannot prove or settle either, so silently degrading to standard
        // would be a protocol and billing lie.
        return Err(ApiError::unsupported(
            "Explicit serviceTier is not supported by this subscription gateway.",
            "SERVICE_TIER_UNSUPPORTED",
        ));
    }
    if body.get("store").is_some() {
        // `store` overrides project-level logging. Dropping even `false` can change data retention,
        // so reject the unsupported control instead of pretending it was applied.
        return Err(ApiError::unsupported(
            "Explicit store logging controls are not supported by this subscription gateway.",
            "STORE_CONTROL_UNSUPPORTED",
        ));
    }
    if body.get("cachedContent").is_some() {
        // A native cached-content resource is scoped to one Google project. It cannot safely
        // survive subscription rotation and may encode a caller-selected upstream identity.
        return Err(ApiError::unsupported(
            "Explicit cachedContent resources are not supported by this gateway. \
             Send the content inline instead.",
            "CACHED_CONTENT_UNSUPPORTED",
        ));
    }
    if model.id == "gemini-3.7-flash" {
        validate_gemini_37_request(body, exact_calibration)?;
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

/// Gemini 3.7 removed legacy sampling and numeric thinking-budget controls. The private Code
/// Assist wrapper would otherwise accept and silently discard some of them, which would make the
/// public request appear honored when it was not. The same release also removed prefilled model
/// turns: a transcript may contain historical model content, but its final turn must be user input
/// carrying at least one non-empty part. A final turn of only functionResponse parts — the exact
/// transcript every portable tool-calling client (OpenCode and other AI SDKs) produces after
/// executing a call — is admitted for all traffic: the closed one-shot exact-profile calibration
/// lane proved its wire contract with live evidence (run gemini-cal-1787152582-af5e9cfb, request
/// fa042530-3c7d-4aff-b795-ea1c3b2b0122: upstream gemini-3.7-flash-tiered returned incremental
/// SSE with visible text and terminal usage). Image-only final turns and prefilled model turns
/// remain fail-closed.
fn validate_gemini_37_request(body: &Value, exact_calibration: bool) -> Result<(), ApiError> {
    if let Some(generation_config) = body.get("generationConfig") {
        let generation_config = generation_config.as_object().ok_or_else(|| {
            ApiError::invalid("The generationConfig field must be a JSON object.")
        })?;
        for field in ["temperature", "topP", "topK", "candidateCount"] {
            if generation_config.contains_key(field) {
                return Err(ApiError::invalid(
                    "Gemini 3.7 Flash does not support legacy sampling or candidateCount controls.",
                ));
            }
        }
        if let Some(thinking_config) = generation_config.get("thinkingConfig") {
            let thinking_config = thinking_config.as_object().ok_or_else(|| {
                ApiError::invalid("The generationConfig.thinkingConfig field must be an object.")
            })?;
            if thinking_config.contains_key("thinkingBudget") {
                return Err(ApiError::invalid(
                    "Gemini 3.7 Flash supports thinkingLevel, not thinkingBudget.",
                ));
            }
        }
    }
    // Validate the same effective transcript that the Antigravity wrapper will dispatch. Merely
    // treating an omitted final role as `user` is unsafe after an explicit user turn: the private
    // normalizer infers that omitted role as `model`, which would otherwise reopen model prefill.
    let mut normalized = body
        .as_object()
        .cloned()
        .ok_or_else(|| ApiError::invalid("The request body must be a JSON object."))?;
    normalize_private_content_roles(&mut normalized);
    let final_content = normalized
        .get("contents")
        .and_then(Value::as_array)
        .and_then(|contents| contents.last())
        .ok_or_else(|| {
            ApiError::invalid("Gemini 3.7 Flash requires a final user turn with non-empty text.")
        })?;
    let final_role = final_content.get("role").and_then(Value::as_str);
    let final_parts = final_content.get("parts").and_then(Value::as_array);
    let has_non_empty_text = final_parts.is_some_and(|parts| {
        parts.iter().any(|part| {
            part.get("text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.trim().is_empty())
        })
    });
    // The tool-result continuation is the exact shape the closed live gate proved: a user final
    // turn whose parts are exclusively functionResponse objects. It is admitted for ordinary
    // traffic as well as the calibration lane, and an empty final parts array is never a
    // tool-result continuation. An empty final parts array is never a tool-result continuation.
    // The exact_calibration flag no longer widens admission here; it is kept in the signature
    // because the remaining validators still scope it.
    let _ = exact_calibration;
    let tool_result_only_final_turn = final_parts.is_some_and(|parts| {
        !parts.is_empty()
            && parts
                .iter()
                .all(|part| part.get("functionResponse").is_some_and(Value::is_object))
    });
    let admitted_final_turn = has_non_empty_text || tool_result_only_final_turn;
    if final_role != Some("user") || !admitted_final_turn {
        return Err(ApiError::invalid(
            "Gemini 3.7 Flash requires a final user turn with non-empty text; prefilled model, \
             image-only, and tool-result-only final turns are not admitted.",
        ));
    }
    Ok(())
}

fn validate_native_request(
    operation: Operation,
    body: &Value,
    model: &GeminiModel,
    exact_calibration: bool,
) -> Result<(), ApiError> {
    if operation != Operation::CountTokens {
        validate_generation_request(body, model, exact_calibration)?;
        return if model.is_image_generation() {
            validate_image_generation_request(body, model)
        } else {
            Ok(())
        };
    }
    let object = body
        .as_object()
        .ok_or_else(|| ApiError::invalid("The request body must be a JSON object."))?;
    let nested = object.get("generateContentRequest");
    if nested.is_some() && object.contains_key("contents") {
        return Err(ApiError::invalid(
            "contents and generateContentRequest are mutually exclusive.",
        ));
    }
    match nested {
        Some(request) if request.is_object() => {
            validate_generation_request(request, model, exact_calibration)?;
            if model.is_image_generation() {
                validate_image_generation_request(request, model)
            } else {
                Ok(())
            }
        }
        Some(_) => Err(ApiError::invalid(
            "The generateContentRequest field must be a JSON object.",
        )),
        None => {
            validate_generation_request(body, model, exact_calibration)?;
            if model.is_image_generation() {
                validate_image_generation_request(body, model)
            } else {
                Ok(())
            }
        }
    }
}

pub(crate) fn validate_batch_generate_request(
    body: &Value,
    model: &GeminiModel,
) -> Result<(), String> {
    validate_generation_request(body, model, false).map_err(|error| error.message.to_owned())?;
    if model.is_image_generation() {
        return Err("Image-output models are not supported by Gemini Batch.".to_owned());
    }
    Ok(())
}

fn wire_model_for_request(
    operation: Operation,
    model: &GeminiModel,
    body: &Value,
) -> Result<String, ApiError> {
    let generation_request = if operation == Operation::CountTokens {
        body.get("generateContentRequest").unwrap_or(body)
    } else {
        body
    };
    let thinking_config = generation_request.pointer("/generationConfig/thinkingConfig");
    let thinking_level = match thinking_config {
        None => None,
        Some(Value::Object(config)) => match config.get("thinkingLevel") {
            None | Some(Value::Null) => None,
            Some(Value::String(level)) => Some(level.as_str()),
            Some(_) => {
                return Err(ApiError::invalid(
                    "The generationConfig.thinkingConfig.thinkingLevel field must be a string.",
                ));
            }
        },
        Some(_) => {
            return Err(ApiError::invalid(
                "The generationConfig.thinkingConfig field must be an object.",
            ));
        }
    };
    model
        .wire_model_id(thinking_level)
        .map(str::to_string)
        .map_err(ApiError::invalid)
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

fn attach_calibration_dispatch_ms(response: &mut Response, dispatch_ms: Option<u64>) {
    let Some(dispatch_ms) = dispatch_ms else {
        return;
    };
    // `u64::to_string` is the one canonical positive decimal spelling. Transport already proved
    // the Node/local-mock timestamp is nonzero and strictly before the absolute deadline.
    let value = HeaderValue::from_str(&dispatch_ms.to_string())
        .expect("a positive decimal millisecond timestamp is a valid header value");
    response
        .headers_mut()
        .insert(CALIBRATION_DISPATCH_HEADER, value);
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
            ensure_replayed_function_call_signatures(&mut request);
            let image_generation = model == "gemini-3.1-flash-image";
            if oauth_kind == OAuthKind::Antigravity {
                adapt_antigravity_generation_request(&mut request, image_generation);
            }
            if let Some((field, session_id)) = session_id.and_then(|session_id| {
                match (oauth_kind, image_generation) {
                    // Antigravity's image_gen identity is stateless and rejects the ordinary agent
                    // session field. Its continuity lives only in the public affinity layer.
                    (OAuthKind::Antigravity, true) => None,
                    (OAuthKind::Antigravity, false) => Some(("sessionId", session_id)),
                    (OAuthKind::LegacyGeminiCli, _) => Some(("session_id", session_id)),
                }
            }) {
                request.insert(field.to_string(), json!(session_id));
            }
            match oauth_kind {
                OAuthKind::Antigravity => json!({
                    "model": model,
                    "project": project,
                    "request": request,
                    "userAgent": "antigravity",
                    "requestType": if image_generation { "image_gen" } else { "agent" },
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
            let request_source = native
                .get("generateContentRequest")
                .and_then(Value::as_object)
                .unwrap_or(native);
            let contents = request_source
                .get("contents")
                .cloned()
                .unwrap_or_else(|| json!([]));
            // Code Assist's private `request` is a GenerateContentRequest even though the public
            // CountTokensRequest nests that message as `generateContentRequest`. Reconstruct the
            // documented fields inline and replace its ignored model with the route-authoritative
            // model so callers cannot select a private model/project through the body.
            let extra_fields = [
                "systemInstruction",
                "tools",
                "toolConfig",
                "safetySettings",
                "generationConfig",
            ];
            let mut request = serde_json::Map::new();
            request.insert("model".to_string(), json!(format!("models/{model}")));
            request.insert("contents".to_string(), contents);
            for field in extra_fields {
                if let Some(value) = request_source.get(field) {
                    request.insert(field.to_string(), value.clone());
                }
            }
            ensure_replayed_function_call_signatures(&mut request);
            json!({ "request": Value::Object(request) })
        }
        Operation::Models | Operation::Model => return Err(ApiError::not_found()),
    };
    serde_json::to_vec(&wrapped)
        .map(Bytes::from)
        .map_err(|_| ApiError::invalid("The request body is not valid JSON."))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseDecodeError {
    Malformed,
    AudioUsage,
}

impl From<()> for ResponseDecodeError {
    fn from(_: ()) -> Self {
        Self::Malformed
    }
}

fn unwrap_code_assist_response(
    operation: Operation,
    bytes: &[u8],
    public_model: &str,
    audio_hint: AudioUsageHint,
    preserve_upstream_model_version: bool,
) -> Result<Bytes, ResponseDecodeError> {
    let mut value: Value =
        serde_json::from_slice(bytes).map_err(|_| ResponseDecodeError::Malformed)?;
    if operation == Operation::CountTokens {
        if !value.is_object() || !value.get("totalTokens").is_some_and(Value::is_number) {
            return Err(ResponseDecodeError::Malformed);
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
        return serde_json::to_vec(&value)
            .map(Bytes::from)
            .map_err(|_| ResponseDecodeError::Malformed);
    }
    if !value.is_object() {
        return Err(ResponseDecodeError::Malformed);
    }
    let mut native = match value
        .as_object_mut()
        .and_then(|object| object.remove("response"))
    {
        Some(native) if native.is_object() => native,
        Some(_) | None => return Err(ResponseDecodeError::Malformed),
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
    apply_audio_usage_fallback(&mut native, audio_hint)
        .map_err(|_| ResponseDecodeError::AudioUsage)?;
    // Real generateContent responses always carry a responseId. Synthesize a native-shaped one
    // rather than exposing the correlatable Code Assist wrapper traceId.
    if let Some(object) = native.as_object_mut() {
        if object.contains_key("modelVersion") && !preserve_upstream_model_version {
            // Tiered Antigravity ids are private routing details, not native Gemini model versions.
            object.insert("modelVersion".to_string(), json!(public_model));
        }
        object.insert("responseId".to_string(), json!(fresh_response_id()));
    }
    serde_json::to_vec(&native)
        .map(Bytes::from)
        .map_err(|_| ResponseDecodeError::Malformed)
}

/// Read the `error.status` enum from an upstream error body, when it carries a known one.
///
/// Used only to classify our own routing reaction; the string is never echoed to the customer.
fn google_error_status(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let status = value.get("error")?.get("status")?.as_str()?;
    is_google_rpc_status(status).then(|| status.to_owned())
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

fn response_has_inline_image(value: &Value) -> bool {
    value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|candidate| {
            candidate
                .pointer("/content/parts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
        })
        .any(|part| {
            ["inlineData", "inline_data"].into_iter().any(|field| {
                part.get(field)
                    .and_then(|inline| inline.get("data"))
                    .and_then(Value::as_str)
                    .is_some_and(|data| !data.is_empty())
            })
        })
}

/// The private Antigravity image response currently omits `candidatesTokensDetails`, although the
/// official Developer API bills a fixed number of IMAGE tokens for the requested size. Its aggregate
/// candidate count still contains that image component. Split the official fixed component out of
/// text/thinking only when a real image was delivered and Google did not provide the authoritative
/// modality breakdown; an explicit breakdown always wins.
fn apply_image_usage_fallback(
    usage: &mut metering::GeminiUsage,
    image_output_tokens: u64,
    image_delivered: bool,
) {
    if !image_delivered || image_output_tokens == 0 || usage.image_output_tokens > 0 {
        return;
    }
    usage.output_tokens = usage.output_tokens.saturating_sub(image_output_tokens);
    usage.image_output_tokens = image_output_tokens;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AudioUsageFallbackError {
    InvalidMetadata,
    AmbiguousCache,
}

fn modality_tokens_if_present(
    metadata: &Value,
    field: &str,
    modality: &str,
) -> Result<Option<u64>, AudioUsageFallbackError> {
    let Some(details) = metadata.get(field) else {
        return Ok(None);
    };
    let details = details
        .as_array()
        .ok_or(AudioUsageFallbackError::InvalidMetadata)?;
    let mut present = false;
    let mut total = 0u64;
    for detail in details {
        if !detail
            .get("modality")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case(modality))
        {
            continue;
        }
        present = true;
        total = total
            .checked_add(
                detail
                    .get("tokenCount")
                    .and_then(Value::as_u64)
                    .ok_or(AudioUsageFallbackError::InvalidMetadata)?,
            )
            .ok_or(AudioUsageFallbackError::InvalidMetadata)?;
    }
    Ok(present.then_some(total))
}

fn modality_tokens_total(metadata: &Value, field: &str) -> Result<u64, AudioUsageFallbackError> {
    let Some(details) = metadata.get(field) else {
        return Ok(0);
    };
    details
        .as_array()
        .ok_or(AudioUsageFallbackError::InvalidMetadata)?
        .iter()
        .try_fold(0u64, |total, detail| {
            total
                .checked_add(
                    detail
                        .get("tokenCount")
                        .and_then(Value::as_u64)
                        .ok_or(AudioUsageFallbackError::InvalidMetadata)?,
                )
                .ok_or(AudioUsageFallbackError::InvalidMetadata)
        })
}

fn append_modality_tokens(
    metadata: &mut Value,
    field: &str,
    modality: &str,
    token_count: u64,
) -> Result<(), AudioUsageFallbackError> {
    let metadata = metadata
        .as_object_mut()
        .ok_or(AudioUsageFallbackError::InvalidMetadata)?;
    let details = metadata
        .entry(field.to_string())
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or(AudioUsageFallbackError::InvalidMetadata)?;
    details.push(json!({"modality": modality, "tokenCount": token_count}));
    Ok(())
}

/// Reconstruct the public AUDIO usage class only when the request gives an exact integral media
/// token count and the provider omitted that modality entirely. Provider-supplied AUDIO details
/// always win. A zero cache count proves all audio is fresh; a full-prompt cache proves all audio
/// is cached; an explicit cache AUDIO row proves the split. Any other partial cache is ambiguous
/// and must fail closed because Flash Preview has different text/audio cache rates.
fn apply_audio_usage_fallback(
    value: &mut Value,
    audio_hint: AudioUsageHint,
) -> Result<(), AudioUsageFallbackError> {
    if audio_hint.tokens == 0 {
        return Ok(());
    }
    let Some(metadata) = value.get_mut("usageMetadata") else {
        // The normal required-usage gate handles a response with no metadata at all.
        return Ok(());
    };
    if modality_tokens_if_present(metadata, "promptTokensDetails", "AUDIO")?.is_some() {
        return Ok(());
    }
    let prompt = metadata
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .ok_or(AudioUsageFallbackError::InvalidMetadata)?;
    if audio_hint.tokens > prompt {
        return Err(AudioUsageFallbackError::InvalidMetadata);
    }
    let cached = metadata
        .get("cachedContentTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if cached > prompt {
        return Err(AudioUsageFallbackError::InvalidMetadata);
    }
    let explicit_cached_audio =
        modality_tokens_if_present(metadata, "cacheTokensDetails", "AUDIO")?;
    let cached_audio = match explicit_cached_audio {
        Some(tokens) if tokens <= cached && tokens <= audio_hint.tokens => tokens,
        Some(_) => return Err(AudioUsageFallbackError::InvalidMetadata),
        None if cached == 0 => 0,
        None if cached == prompt => audio_hint.tokens,
        None => return Err(AudioUsageFallbackError::AmbiguousCache),
    };

    let prompt_detail_total = modality_tokens_total(metadata, "promptTokensDetails")?;
    if prompt_detail_total.saturating_add(audio_hint.tokens) > prompt {
        return Err(AudioUsageFallbackError::InvalidMetadata);
    }
    let cache_detail_total = modality_tokens_total(metadata, "cacheTokensDetails")?;
    let inferred_cached_audio = if explicit_cached_audio.is_none() {
        cached_audio
    } else {
        0
    };
    if cache_detail_total.saturating_add(inferred_cached_audio) > cached {
        return Err(AudioUsageFallbackError::InvalidMetadata);
    }

    append_modality_tokens(metadata, "promptTokensDetails", "AUDIO", audio_hint.tokens)?;
    if explicit_cached_audio.is_none() && cached_audio > 0 {
        append_modality_tokens(metadata, "cacheTokensDetails", "AUDIO", cached_audio)?;
    }
    Ok(())
}

fn settlement_usage_from_response(
    value: &Value,
    image_output_tokens: u64,
) -> Option<metering::GeminiUsage> {
    let mut usage = metering::gemini::usage_from_response_value(value)?;
    apply_image_usage_fallback(
        &mut usage,
        image_output_tokens,
        response_has_inline_image(value),
    );
    Some(usage)
}

fn gemini_tool_calls_in_output(value: &Value) -> Option<bool> {
    let object = value.as_object()?;
    if object.keys().any(|key| {
        !matches!(
            key.as_str(),
            "candidates" | "promptFeedback" | "usageMetadata" | "modelVersion" | "responseId"
        )
    }) {
        return None;
    }
    let Some(candidates) = object.get("candidates") else {
        return Some(false);
    };
    let candidates = candidates.as_array()?;
    let mut saw_call = false;
    for candidate in candidates {
        let candidate = candidate.as_object()?;
        if candidate.keys().any(|key| {
            !matches!(
                key.as_str(),
                "content"
                    | "finishReason"
                    | "safetyRatings"
                    | "citationMetadata"
                    | "tokenCount"
                    | "groundingAttributions"
                    | "groundingMetadata"
                    | "avgLogprobs"
                    | "logprobsResult"
                    | "urlContextMetadata"
                    | "finishMessage"
                    | "index"
            )
        }) {
            return None;
        }
        let Some(content) = candidate.get("content") else {
            continue;
        };
        let content = content.as_object()?;
        if content
            .keys()
            .any(|key| !matches!(key.as_str(), "role" | "parts"))
        {
            return None;
        }
        let Some(parts) = content.get("parts") else {
            continue;
        };
        for part in parts.as_array()? {
            let part = part.as_object()?;
            if part.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "text"
                        | "inlineData"
                        | "functionCall"
                        | "executableCode"
                        | "codeExecutionResult"
                        | "fileData"
                        | "thought"
                        | "thoughtSignature"
                )
            }) {
                return None;
            }
            if let Some(call) = part.get("functionCall") {
                if !call.is_object() {
                    return None;
                }
                saw_call = true;
            }
        }
    }
    Some(saw_call)
}

fn merge_tool_call_evidence(current: Option<bool>, observed: Option<bool>) -> Option<bool> {
    match (current, observed) {
        (Some(true), _) | (_, Some(true)) => Some(true),
        (Some(false), Some(false)) => Some(false),
        _ => None,
    }
}

struct SseTranslator {
    pending: Vec<u8>,
    usage: metering::GeminiUsage,
    provider_error: Option<u16>,
    provider_retry_after: Option<i64>,
    provider_rate_limit_diagnostic: Option<RateLimitDiagnostic>,
    response_id: String,
    public_model: String,
    preserve_upstream_model_version: bool,
    framing: StreamFraming,
    started: bool,
    image_output_tokens: u64,
    image_delivered: bool,
    audio_usage_hint: AudioUsageHint,
    audio_usage_failed: bool,
    tool_calls_in_output: Option<bool>,
    /// Content-free shape of the stream, kept only so a turn that ends without usage can say why in
    /// one journal line instead of leaving the operator with a bare counter. No customer text, no
    /// tool arguments, no identifiers — frame counts and the terminal finish reason.
    shape: StreamShape,
}

/// Counters describing what the upstream actually sent, for diagnosing a turn that produced no
/// usage. Every field is a count or a Google enum name; none of it is customer content.
#[derive(Clone, Debug, Default)]
struct StreamShape {
    frames: u64,
    envelope_only_frames: u64,
    usage_frames: u64,
    countless_usage_frames: u64,
    last_finish_reason: Option<String>,
}

impl std::fmt::Display for StreamShape {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "frames={} envelope_only={} usage_frames={} countless_usage_frames={} finish_reason={}",
            self.frames,
            self.envelope_only_frames,
            self.usage_frames,
            self.countless_usage_frames,
            self.last_finish_reason.as_deref().unwrap_or("none"),
        )
    }
}

impl SseTranslator {
    fn new_with_image_usage(
        framing: StreamFraming,
        public_model: &str,
        image_output_tokens: u64,
        audio_usage_hint: AudioUsageHint,
    ) -> Self {
        Self {
            pending: Vec::new(),
            usage: metering::GeminiUsage::default(),
            provider_error: None,
            provider_retry_after: None,
            provider_rate_limit_diagnostic: None,
            response_id: fresh_response_id(),
            public_model: public_model.to_string(),
            preserve_upstream_model_version: false,
            framing,
            started: false,
            image_output_tokens,
            image_delivered: false,
            audio_usage_hint,
            audio_usage_failed: false,
            tool_calls_in_output: Some(false),
            shape: StreamShape::default(),
        }
    }

    fn with_upstream_model_version(mut self) -> Self {
        self.preserve_upstream_model_version = true;
        self
    }

    /// Record the content-free usage shape of one upstream frame. `usageMetadata` present but
    /// carrying no token counts is the case that used to erase an earlier good snapshot, so it is
    /// counted apart from a frame that reports real counts.
    fn observe_usage_shape(&mut self, wrapper: &Value) {
        for metadata in [
            wrapper.get("usageMetadata"),
            wrapper.pointer("/response/usageMetadata"),
        ]
        .into_iter()
        .flatten()
        {
            self.shape.usage_frames = self.shape.usage_frames.saturating_add(1);
            if metering::gemini::usage_from_metadata_value(metadata).total_tokens() == 0 {
                self.shape.countless_usage_frames =
                    self.shape.countless_usage_frames.saturating_add(1);
            }
        }
        if let Some(reason) = wrapper
            .pointer("/response/candidates/0/finishReason")
            .or_else(|| wrapper.pointer("/candidates/0/finishReason"))
            .and_then(Value::as_str)
        {
            self.shape.last_finish_reason = Some(reason.to_string());
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
        self.shape.frames = self.shape.frames.saturating_add(1);
        self.observe_usage_shape(&wrapper);
        // Settlement reads usage from the envelope as well as from the response it carries. A frame
        // that reports usage next to `response`, or in place of it, is still Google stating what the
        // turn cost, and dropping it as "private" silently downgraded the turn to an unmetered one
        // billed at the preflight hold. Reading is not exposing: the envelope itself is never framed
        // to the client, only the public response fields below are.
        metering::gemini::merge_stream_response_value(&mut self.usage, &wrapper);
        let Some(mut native) = wrapper
            .as_object_mut()
            .and_then(|object| object.remove("response"))
        else {
            self.shape.envelope_only_frames = self.shape.envelope_only_frames.saturating_add(1);
            // A mid-stream upstream error must reach the client as a native error element rather
            // than a clean truncation that looks like success. Genuinely private credit/accounting
            // events carry no `error` and have no public representation, so they stay consumed.
            if let Some(error) = native_stream_error_value(&wrapper) {
                self.provider_retry_after = rate_limit::retry_info_delay(&wrapper);
                self.provider_error = error
                    .pointer("/error/code")
                    .and_then(Value::as_u64)
                    .and_then(|code| u16::try_from(code).ok());
                if self.provider_error == Some(429) {
                    self.provider_rate_limit_diagnostic = Some(
                        RateLimitDiagnostic::from_bounded_value(None, Some(&wrapper), data.len()),
                    );
                }
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
        self.tool_calls_in_output = merge_tool_call_evidence(
            self.tool_calls_in_output,
            gemini_tool_calls_in_output(&native),
        );
        if apply_audio_usage_fallback(&mut native, self.audio_usage_hint).is_err() {
            self.audio_usage_failed = true;
            return Err(());
        }
        if native.as_object().is_none_or(serde_json::Map::is_empty) {
            // Unknown/private response-only events have no public representation.
            return Ok(None);
        }
        self.image_delivered |= response_has_inline_image(&native);
        metering::gemini::merge_stream_response_value(&mut self.usage, &native);
        apply_image_usage_fallback(
            &mut self.usage,
            self.image_output_tokens,
            self.image_delivered,
        );
        // Real Gemini SSE chunks carry a stable responseId for the whole response; mirror it.
        if let Some(object) = native.as_object_mut() {
            if object.contains_key("modelVersion") && !self.preserve_upstream_model_version {
                object.insert("modelVersion".to_string(), json!(&self.public_model));
            }
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
    max_bytes: usize,
    max_chunks: usize,
) -> Result<(), ()> {
    *observed_bytes = observed_bytes.saturating_add(chunk_bytes);
    *observed_chunks = observed_chunks.saturating_add(1);
    if *observed_bytes > max_bytes || *observed_chunks > max_chunks {
        return Err(());
    }
    Ok(())
}

#[derive(Debug)]
enum SendError {
    Token(TokenError),
    Transport(TransportError),
    CalibrationExpired,
}

async fn send_upstream(
    profile: &GeminiProfile,
    url: &str,
    _headers: &HeaderMap,
    body: Bytes,
    rejected_token: Option<&str>,
    user_agent: &str,
    include_antigravity_metadata: bool,
    retry_policy: TransportRetryPolicy,
    token_policy: TokenAcquisitionPolicy,
    calibration_not_after: Option<u64>,
    actual_send_observer: Option<ActualSendObserver>,
) -> Result<(TransportResponse, gemini_credential::SecretString), SendError> {
    let access_token = match rejected_token {
        Some(rejected) => profile.access_token_after_rejection(rejected).await,
        None => profile.access_token_with_policy(token_policy).await,
    }
    .map_err(SendError::Token)?;
    // No customer header is required by Code Assist. Constructing the complete upstream header
    // set locally prevents cookies, trace ids, origins or future identity headers from crossing the
    // provider boundary when a denylist inevitably becomes stale.
    //
    // The Accept header is load-bearing for the streaming method: the Antigravity Cloud Code
    // surface rejects `streamGenerateContent` with a generic INVALID_ARGUMENT unless the request
    // advertises `Accept: text/event-stream`. generateContent accepts the default `*/*`, so only
    // the streaming path needs the explicit SSE accept. Legacy Gemini CLI keeps its JSON accept on
    // its non-streaming calls.
    let accept = if url.contains(":streamGenerateContent") {
        Some("text/event-stream")
    } else if profile.oauth_kind() == OAuthKind::LegacyGeminiCli {
        Some("application/json")
    } else {
        None
    };
    if calibration_not_after.is_some_and(|not_after| {
        u64::try_from(pool::now())
            .ok()
            .is_none_or(|now| now >= not_after)
    }) {
        return Err(SendError::CalibrationExpired);
    }
    let response = profile
        .request(
            url,
            &access_token,
            user_agent,
            include_antigravity_metadata,
            accept,
            "application/json",
            body,
            profile.generation_idle(),
            retry_policy,
            calibration_not_after,
            actual_send_observer,
        )
        .await
        .map_err(|error| match error {
            TransportError::CalibrationExpired => SendError::CalibrationExpired,
            other => SendError::Transport(other),
        })?;
    Ok((response, access_token))
}

async fn read_upstream_body(response: TransportResponse) -> Result<Bytes, TransportError> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if body.len().saturating_add(chunk.len()) > GEMINI_BODY_LIMIT {
            return Err(TransportError::BodyTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

/// Sanitized native GenerateContent input for the common non-stream execution primitive.
///
/// Construction is intentionally private to this module: HTTP admission and a future batch parser
/// must validate/canonicalize first, then call [`prepare_nonstream_generate_request`]. This type
/// cannot carry API authentication, request headers, billing state or a customer reserve.
pub struct GeminiNonstreamGenerateRequest {
    native: Value,
    public_model: String,
    wire_model: String,
    session_id: Option<String>,
    user_prompt_id: String,
    request_id: String,
    image_output_tokens: u64,
    audio_usage_hint: AudioUsageHint,
}

/// Build a batch-callable, transport-ready non-stream request from an already sanitized canonical
/// GenerateContent request. HTTP body parsing/authentication and money admission stay outside.
pub fn prepare_nonstream_generate_request(
    model: &GeminiModel,
    wire_model: impl Into<String>,
    native: Value,
    session_id: Option<String>,
    user_prompt_id: String,
    request_id: String,
) -> GeminiNonstreamGenerateRequest {
    let image_output_tokens = if model.is_image_generation() {
        image_output_tokens(&native)
    } else {
        0
    };
    let audio_usage_hint = if model.id == "gemini-3-flash-preview" {
        flash_preview_audio_usage_hint(&native).unwrap_or_default()
    } else {
        AudioUsageHint::default()
    };
    GeminiNonstreamGenerateRequest {
        native,
        public_model: model.id.clone(),
        wire_model: wire_model.into(),
        session_id,
        user_prompt_id,
        request_id,
        image_output_tokens,
        audio_usage_hint,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiNonstreamTerminalClass {
    Success,
    Auth,
    Quota,
    Client,
    Backend,
    Transport,
    Protocol,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeminiNonstreamTransportEvidence {
    pub status: Option<StatusCode>,
    pub terminal_class: GeminiNonstreamTerminalClass,
    pub response_headers_received: bool,
    pub response_body_complete: bool,
}

pub struct GeminiNonstreamRawResponse {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
    pub usage: Option<metering::GeminiUsage>,
    pub evidence: GeminiNonstreamTransportEvidence,
    rejected_token: gemini_credential::SecretString,
}

#[derive(Debug)]
pub enum GeminiNonstreamProtocolError {
    Malformed,
    AudioUsage,
}

#[derive(Debug)]
pub enum GeminiNonstreamExecuteError {
    Token,
    Transport {
        evidence: GeminiNonstreamTransportEvidence,
    },
    Protocol {
        kind: GeminiNonstreamProtocolError,
        evidence: GeminiNonstreamTransportEvidence,
    },
}

fn nonstream_status_class(status: StatusCode) -> GeminiNonstreamTerminalClass {
    match status.as_u16() {
        200..=299 => GeminiNonstreamTerminalClass::Success,
        401 | 403 => GeminiNonstreamTerminalClass::Auth,
        429 => GeminiNonstreamTerminalClass::Quota,
        400..=499 => GeminiNonstreamTerminalClass::Client,
        _ => GeminiNonstreamTerminalClass::Backend,
    }
}

/// Execute exactly one non-stream GenerateContent attempt on a preselected lease/profile/model.
///
/// This owns only provider execution: Code Assist identity wrapping, OAuth token acquisition,
/// upstream send, bounded body read, public-envelope decoding, authoritative usage parsing and
/// typed terminal evidence. It deliberately performs no HTTP authentication/body parsing, profile
/// selection, affinity, customer reserve, mark-delivering or settlement.
pub async fn execute_nonstream_generate(
    gateway: &GeminiGateway,
    lease: &GeminiLease,
    model: &GeminiModel,
    request: &GeminiNonstreamGenerateRequest,
) -> Result<GeminiNonstreamRawResponse, GeminiNonstreamExecuteError> {
    execute_nonstream_generate_observed(gateway, lease, model, request, None).await
}

pub(crate) async fn execute_nonstream_generate_observed(
    gateway: &GeminiGateway,
    lease: &GeminiLease,
    model: &GeminiModel,
    request: &GeminiNonstreamGenerateRequest,
    actual_send_observer: Option<ActualSendObserver>,
) -> Result<GeminiNonstreamRawResponse, GeminiNonstreamExecuteError> {
    execute_nonstream_generate_with_rejected_token(
        gateway,
        lease,
        model,
        request,
        None,
        actual_send_observer,
    )
    .await
}

async fn execute_nonstream_generate_with_rejected_token(
    gateway: &GeminiGateway,
    lease: &GeminiLease,
    model: &GeminiModel,
    request: &GeminiNonstreamGenerateRequest,
    rejected_token: Option<&str>,
    actual_send_observer: Option<ActualSendObserver>,
) -> Result<GeminiNonstreamRawResponse, GeminiNonstreamExecuteError> {
    let profile = lease.profile();
    let oauth_kind = profile.oauth_kind();
    let project = profile.project_id().await;
    let body = wrap_code_assist_request(
        Operation::Generate,
        oauth_kind,
        &request.wire_model,
        &project,
        &request.native,
        &request.user_prompt_id,
        request.session_id.as_deref(),
        Some(&request.request_id),
    )
    .map_err(|_| GeminiNonstreamExecuteError::Protocol {
        kind: GeminiNonstreamProtocolError::Malformed,
        evidence: GeminiNonstreamTransportEvidence {
            status: None,
            terminal_class: GeminiNonstreamTerminalClass::Protocol,
            response_headers_received: false,
            response_body_complete: false,
        },
    })?;
    let url = format!(
        "{}/v1internal:generateContent",
        gateway.config().generation_upstream_for(
            oauth_kind,
            model.is_image_generation(),
            &request.wire_model,
        )
    );
    let (response, rejected_token) = send_upstream(
        profile,
        &url,
        &HeaderMap::new(),
        body,
        rejected_token,
        &gateway.config().user_agent(oauth_kind, &request.wire_model),
        include_antigravity_metadata(model, profile),
        TransportRetryPolicy::RestartHelperOnce,
        TokenAcquisitionPolicy::Normal,
        None,
        actual_send_observer,
    )
    .await
    .map_err(|error| match error {
        SendError::Token(_) | SendError::CalibrationExpired => GeminiNonstreamExecuteError::Token,
        SendError::Transport(_) => GeminiNonstreamExecuteError::Transport {
            evidence: GeminiNonstreamTransportEvidence {
                status: None,
                terminal_class: GeminiNonstreamTerminalClass::Transport,
                response_headers_received: false,
                response_body_complete: false,
            },
        },
    })?;
    let status = response.status();
    let headers = response.headers().clone();
    let body =
        read_upstream_body(response)
            .await
            .map_err(|_| GeminiNonstreamExecuteError::Transport {
                evidence: GeminiNonstreamTransportEvidence {
                    status: Some(status),
                    terminal_class: GeminiNonstreamTerminalClass::Transport,
                    response_headers_received: true,
                    response_body_complete: false,
                },
            })?;
    if !status.is_success() {
        return Ok(GeminiNonstreamRawResponse {
            status,
            headers,
            body,
            usage: None,
            evidence: GeminiNonstreamTransportEvidence {
                status: Some(status),
                terminal_class: nonstream_status_class(status),
                response_headers_received: true,
                response_body_complete: true,
            },
            rejected_token,
        });
    }
    let body = unwrap_code_assist_response(
        Operation::Generate,
        &body,
        &request.public_model,
        request.audio_usage_hint,
        false,
    )
    .map_err(|decode| GeminiNonstreamExecuteError::Protocol {
        kind: match decode {
            ResponseDecodeError::Malformed => GeminiNonstreamProtocolError::Malformed,
            ResponseDecodeError::AudioUsage => GeminiNonstreamProtocolError::AudioUsage,
        },
        evidence: GeminiNonstreamTransportEvidence {
            status: Some(status),
            terminal_class: GeminiNonstreamTerminalClass::Protocol,
            response_headers_received: true,
            response_body_complete: true,
        },
    })?;
    let usage = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| settlement_usage_from_response(&value, request.image_output_tokens))
        .filter(|usage| !usage.is_zero());
    Ok(GeminiNonstreamRawResponse {
        status,
        headers,
        body,
        usage,
        evidence: GeminiNonstreamTransportEvidence {
            status: Some(status),
            terminal_class: GeminiNonstreamTerminalClass::Success,
            response_headers_received: true,
            response_body_complete: true,
        },
        rejected_token,
    })
}

fn include_antigravity_metadata(model: &GeminiModel, profile: &GeminiProfile) -> bool {
    !(profile.oauth_kind() == OAuthKind::Antigravity && model.id == "gemini-3-flash-preview")
}

async fn stream_response(
    gateway: Arc<GeminiGateway>,
    metrics: Arc<Metrics>,
    profile: Arc<GeminiProfile>,
    lease: GeminiLease,
    admission: GeminiAdmission,
    model: GeminiModel,
    wire_model_id: String,
    rate_limit_request_id: String,
    attempt: usize,
    status: StatusCode,
    headers: HeaderMap,
    mut translator: SseTranslator,
    initial: Vec<Bytes>,
    mut upstream: impl futures_util::Stream<Item = Result<Bytes, TransportError>>
        + Send
        + Unpin
        + 'static,
    post_dispatch_ambiguous: bool,
    calibration_dispatch_ms: Option<u64>,
) -> Result<Response, ApiError> {
    let framing = translator.framing;
    // Register with the shutdown barrier before the durable delivery transition. Otherwise a
    // shutdown can observe zero background tasks, flush billing, and race a late mark/refund from
    // this narrow await window. No downstream byte is exposed until both steps have succeeded.
    let background = gateway
        .track_background_task()
        .map_err(|_| ApiError::unavailable("gemini_shutdown"))
        .map_err(|error| {
            if post_dispatch_ambiguous {
                error.after_dispatch()
            } else {
                error
            }
        })?;
    let delivery_marker_failed = admission.mark_delivering().await.is_err();
    let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
    tokio::spawn(async move {
        let _background = background;
        let _lease = lease;
        // A failed durable delivery transition must not expose a 200 or abandon the private
        // provider stream. Drain and settle authoritative usage in this same actor, but never try
        // the intentionally unreturned receiver and therefore never misclassify it as a client
        // disconnect.
        let mut deliver = !delivery_marker_failed;
        let mut clean_eof = true;
        let mut aborted = false;
        let mut private_bytes = 0usize;
        let mut private_chunks = 0usize;
        let mut malformed = false;
        let mut transport_failed = false;

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
                            Ok(Err(_)) => {
                                admission.record_downstream_disconnect();
                                deliver = false;
                            }
                            Err(_) => deliver = false,
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
                            malformed = !translator.audio_usage_failed;
                            elog::error(
                                "gemini",
                                "gemini stream failed mid-flight: malformed response",
                            );
                            break;
                        }
                    };
                    if translated.is_empty() {
                        if account_stream_start_chunk(
                            &mut private_bytes,
                            &mut private_chunks,
                            chunk_len,
                            STREAM_START_MAX_BYTES,
                            STREAM_START_MAX_CHUNKS,
                        )
                        .is_err()
                        {
                            clean_eof = false;
                            malformed = true;
                            elog::error(
                                "gemini",
                                "gemini stream failed mid-flight: malformed response",
                            );
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
                                Ok(Err(_)) => {
                                    admission.record_downstream_disconnect();
                                    deliver = false;
                                }
                                Err(_) => deliver = false,
                            }
                        }
                    }
                }
                Err(error) => {
                    clean_eof = false;
                    transport_failed = true;
                    Metrics::inc(&metrics.upstream_5xx);
                    Metrics::inc(&metrics.gemini_transport_failures);
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    elog::error(
                        "gemini",
                        format!("gemini stream failed mid-flight: {error}"),
                    );
                    break;
                }
            }
        }
        if clean_eof && !aborted {
            match translator.finish_pending() {
                Ok(chunks) => {
                    for chunk in chunks {
                        if deliver {
                            match tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(chunk))
                                .await
                            {
                                Ok(Ok(())) => {}
                                Ok(Err(_)) => {
                                    admission.record_downstream_disconnect();
                                    deliver = false;
                                }
                                Err(_) => deliver = false,
                            }
                        }
                    }
                }
                Err(()) => {
                    clean_eof = false;
                    malformed = !translator.audio_usage_failed;
                }
            }
        }
        // A JSON-array stream must be closed with `]` (or emitted as `[]` when empty); SSE needs no
        // terminator. Only close on a clean end — a truncated array mirrors a truncated SSE stream.
        if clean_eof && !aborted {
            if let Some(close) = translator.finish_stream() {
                if deliver {
                    match tokio::time::timeout(DOWNSTREAM_SEND_TIMEOUT, sender.send(close)).await {
                        Ok(Ok(())) => {}
                        Ok(Err(_)) => admission.record_downstream_disconnect(),
                        Err(_) => {}
                    }
                }
            }
        }
        let usage = (!translator.audio_usage_failed && !translator.usage.is_zero())
            .then_some(&translator.usage);
        // A metered turn that ends with no usage is not a healthy turn however clean the stream
        // looked: it settles through the fleet unknown-usage policy (zero by default, the measured
        // checkpoint when one was written, the full hold only behind the operator switch) instead
        // of a measured cost. Decide
        // that before classifying the profile, so the model is never credited with a success on the
        // very turn that is about to be recorded as a usage failure.
        let usage_missing = usage.is_none() && admission.requires_usage();
        if !aborted {
            match translator.provider_error {
                Some(401 | 403) => {
                    Metrics::inc(&metrics.upstream_auth);
                    profile.mark_auth_blocked(gateway.config());
                }
                Some(429) => {
                    Metrics::inc(&metrics.upstream_429);
                    let diagnostic = translator
                        .provider_rate_limit_diagnostic
                        .clone()
                        .unwrap_or_else(|| RateLimitDiagnostic::from_value(None, None));
                    let delay = generation_429_cool_secs(
                        translator.provider_retry_after,
                        &diagnostic,
                        profile.quota_reports_remaining(
                            &wire_model_id,
                            gateway.config(),
                            pool::now(),
                        ),
                        gateway.config(),
                    );
                    log_rate_limit_attempt(
                        &rate_limit_request_id,
                        "stream_generate",
                        "stream_midflight",
                        attempt,
                        &model.id,
                        &wire_model_id,
                        profile.id(),
                        profile.oauth_kind(),
                        &diagnostic,
                        delay,
                        &profile.rate_limit_quota_evidence(
                            &wire_model_id,
                            gateway.config(),
                            pool::now(),
                        ),
                    );
                    profile.cool_model_until(&wire_model_id, pool::now() + delay);
                }
                Some(408 | 409 | 425) => {
                    Metrics::inc(&metrics.upstream_5xx);
                    Metrics::inc(&metrics.gemini_transport_failures);
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                }
                Some(500..=599) => {
                    Metrics::inc(&metrics.upstream_5xx);
                    Metrics::inc(&metrics.gemini_backend_failures);
                    profile.mark_model_failure(&wire_model_id, "backend", gateway.config());
                }
                Some(_) if clean_eof && !usage_missing => {
                    profile.mark_model_success(&wire_model_id)
                }
                None if clean_eof && !usage_missing => profile.mark_model_success(&wire_model_id),
                _ if translator.audio_usage_failed => {
                    Metrics::inc(&metrics.gemini_usage_missing);
                    profile.mark_model_failure(&wire_model_id, "usage_metadata", gateway.config());
                }
                _ if malformed => {
                    Metrics::inc(&metrics.upstream_5xx);
                    Metrics::inc(&metrics.gemini_malformed_responses);
                    profile.mark_model_failure(&wire_model_id, "malformed", gateway.config());
                }
                _ => {}
            }
        }
        if usage_missing {
            Metrics::inc(&metrics.gemini_usage_missing);
            if !aborted && !malformed && translator.provider_error.is_none() {
                Metrics::inc(&metrics.gemini_malformed_responses);
                profile.mark_model_failure(&wire_model_id, "usage_metadata", gateway.config());
                // The counter alone could not say whether the upstream reported nothing or we threw
                // its report away, and the settlement it drives is the customer's most expensive
                // one. Name the request and the content-free stream shape so the next occurrence is
                // diagnosable from the journal instead of from a live capture.
                elog::warn(
                    "gemini",
                    format!(
                        "gemini turn settled without usage metadata: request_id={} model={} profile={} {}",
                        admission.request_id(),
                        wire_model_id,
                        profile.id(),
                        translator.shape,
                    ),
                );
            }
        }
        let request_probe = admission.requests_post_turn_probe();
        let (provider_terminal_class, mut delivery_state, tool_calls_in_output) = if aborted {
            (
                ProviderTerminalClass::Unknown,
                DeliveryState::Interrupted,
                None,
            )
        } else if let Some(code) = translator.provider_error {
            (
                StatusCode::from_u16(code)
                    .map(provider_status_class)
                    .unwrap_or(ProviderTerminalClass::Unknown),
                DeliveryState::Interrupted,
                None,
            )
        } else if transport_failed {
            (
                ProviderTerminalClass::Transport,
                DeliveryState::Interrupted,
                None,
            )
        } else if malformed || translator.audio_usage_failed {
            (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            )
        } else if clean_eof && !usage_missing {
            (
                ProviderTerminalClass::Success,
                DeliveryState::Completed,
                translator.tool_calls_in_output,
            )
        } else {
            (
                ProviderTerminalClass::ProtocolError,
                DeliveryState::Interrupted,
                None,
            )
        };
        if delivery_marker_failed {
            delivery_state = DeliveryState::Unknown;
        }
        if let Some(event) = admission.settle_terminal(
            &model,
            usage,
            profile.id(),
            Some(if delivery_marker_failed { 503 } else { 200 }),
            provider_terminal_class,
            delivery_state,
            None,
            true,
            tool_calls_in_output,
        ) {
            profile.record_turn(event);
            if request_probe {
                gateway.request_probe();
            }
        }
    });
    if delivery_marker_failed {
        return Err(ApiError::unavailable("gemini_delivery_marker_failed").after_dispatch());
    }
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
    attach_calibration_dispatch_ms(&mut response, calibration_dispatch_ms);
    Ok(response)
}

async fn record_affinity_success(
    store: &Arc<crate::AffinityStore>,
    input: Option<&AffinityInput>,
    resolution: &mut Option<AffinityResolution>,
    warm_homes: &[String],
    profile_id: &str,
) {
    let Some(input) = input else {
        return;
    };
    let new_cache_root_placement = resolution.is_none() && input.has_cache_root();
    let served_home = store.home_id(profile_id);
    let reused_warm_root = warm_homes.iter().any(|home| home == &served_home);
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
    if new_cache_root_placement {
        store.record_cache_root_placement(input, reused_warm_root);
    }
    store.mark_cache_warm(input, &served_home);
}

pub async fn api(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    if let Some(route) = super::batch_handlers::parse(request.method(), request.uri().path()) {
        if request.method() == Method::OPTIONS {
            return super::batch_handlers::cors();
        }
        if app.gemini_batch.is_none() {
            return (
                StatusCode::NOT_FOUND,
                axum::Json(json!({"error":{"code":404,"status":"NOT_FOUND"}})),
            )
                .into_response();
        }
        return super::batch_handlers::dispatch(app, peer, route, request).await;
    }
    // A browser SDK (@google/genai) issues a CORS preflight before the cross-origin call; the real
    // endpoint answers it without auth. Handle it before routing, which otherwise 404s on OPTIONS.
    if request.method() == Method::OPTIONS {
        return cors_preflight_response();
    }
    let mut fact_guard = None;
    let mut response = match api_inner_observed(app, peer, request, &mut fact_guard).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    };
    apply_native_response_headers(&mut response);
    if let Some(guard) = fact_guard.take() {
        guard.terminal_response(&mut response);
    }
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

#[cfg(test)]
async fn api_inner(
    app: AppState,
    peer: SocketAddr,
    request: axum::extract::Request,
) -> Result<Response, ApiError> {
    let mut fact_guard = None;
    let result = api_inner_observed(app, peer, request, &mut fact_guard).await;
    drop(fact_guard);
    result
}

async fn api_inner_observed(
    app: AppState,
    peer: SocketAddr,
    request: axum::extract::Request,
    fact_guard: &mut Option<GeminiCountTokensFactGuard>,
) -> Result<Response, ApiError> {
    let Some(gateway) = app.gemini.as_ref().cloned() else {
        return Err(ApiError::not_found());
    };
    let route = parse_route(request.method(), request.uri().path())?;
    let pending = begin_admission(&app, request.headers(), &peer).await?;
    if route.operation == Operation::CountTokens {
        let admitted_at = pool::now();
        let seed = pending.request_fact_seed(
            request
                .extensions()
                .get::<crate::execution::LogicalRequestId>(),
            request
                .extensions()
                .get::<crate::execution::ClientAttribution>(),
            request
                .extensions()
                .get::<crate::execution::RequestLifecycleClock>(),
            admitted_at,
        );
        if let (Some(billing), Some(seed)) = (app.billing.as_ref(), seed) {
            *fact_guard = Some(GeminiCountTokensFactGuard::new(
                Arc::clone(billing),
                seed,
                request
                    .extensions()
                    .get::<UniversalCountTokensIntent>()
                    .cloned(),
            ));
        }
    }
    let calibration_target = pending.calibration_target().map(str::to_owned);
    let calibration_not_after = pending.calibration_not_after();

    if route.operation == Operation::Models {
        if calibration_not_after.is_some() {
            return Err(ApiError::unavailable("gemini_calibration_deadline_scope"));
        }
        let page = parse_list_models_query(request.uri().query())?;
        let all = gateway.config().models.iter().collect::<Vec<_>>();
        let start = page.start.min(all.len());
        let end = start.saturating_add(page.size).min(all.len());
        let batch_public = app.gemini_batch.is_some();
        let models = all[start..end]
            .iter()
            .copied()
            .map(|model| model_value(model, batch_public))
            .collect::<Vec<_>>();
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
        if calibration_not_after.is_some() {
            return Err(ApiError::unavailable("gemini_calibration_deadline_scope"));
        }
        // A native GetModel ignores query parameters entirely.
        let _admission = pending.without_reserve();
        return Ok((
            StatusCode::OK,
            axum::Json(model_value(&model, app.gemini_batch.is_some())),
        )
            .into_response());
    }
    // A supplied exact-profile calibration for 3.7 retains the one-shot deadline fence used by
    // the admitted evidence path. Ordinary customer traffic carries neither header and follows
    // the normal retry/reserve/settlement lifecycle. The fleet media matrix carries the same
    // fence on every published model, scoped to the admin calibration headers only.
    let media_matrix_exact = calibration_target.is_some() && calibration_not_after.is_some();
    if model.id == GEMINI_37_MODEL
        && calibration_target.is_some()
        && calibration_not_after.is_none()
    {
        return Err(ApiError::unavailable(
            "gemini_calibration_deadline_required",
        ));
    }
    if model.id != GEMINI_37_MODEL
        && calibration_not_after.is_some()
        && calibration_target.is_none()
    {
        return Err(ApiError::unavailable("gemini_calibration_deadline_scope"));
    }
    let deadline_bound_exact = media_matrix_exact
        || (model.id == GEMINI_37_MODEL
            && calibration_target.is_some()
            && calibration_not_after.is_some());
    let rate_limit_request_id = pending.request_id().to_string();

    // Only the upstream-bound operations carry an alt query; validate it here rather than for the
    // model-metadata routes, which do not reach Code Assist. `framing` decides the downstream wire
    // shape (SSE vs the native JSON array) and is only meaningful for a streaming operation.
    let (query, framing) = parse_stream_query(
        request.uri().query(),
        route.operation == Operation::StreamGenerate,
    )?;

    let request_body_limit = if model.is_image_generation() {
        GEMINI_IMAGE_REQUEST_BODY_LIMIT
    } else {
        GEMINI_TEXT_REQUEST_BODY_LIMIT
    };
    let (parts, body) = request.into_parts();
    let body = to_bytes(body, request_body_limit)
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
    validate_native_request(route.operation, &value, &model, deadline_bound_exact)?;
    if let Some(guard) = fact_guard.as_mut() {
        guard.update_after_native_accept(model_id, &value);
    }
    let wire_model_id = wire_model_for_request(route.operation, &model, &value)?;
    if let Some(guard) = fact_guard.as_mut() {
        guard.resolve_executable_model(&wire_model_id);
    }
    // Billable request facts are admitted only for validated text generation. A typed universal
    // origin replaces the native route semantics rather than suppressing the leaf, so every public
    // adapter still owns exactly one reservation/fact. Counting has its separate terminal producer;
    // image generation, batch and admin remain excluded.
    let mut billable_fact =
        if billable_generation_fact_eligible(route.operation, model.is_image_generation()) {
            let admitted_at = pool::now();
            pending
                .request_fact_seed(
                    parts.extensions.get::<crate::execution::LogicalRequestId>(),
                    parts
                        .extensions
                        .get::<crate::execution::ClientAttribution>(),
                    parts
                        .extensions
                        .get::<crate::execution::RequestLifecycleClock>(),
                    admitted_at,
                )
                .map(|seed| {
                    let executable_model = bounded_request_fact_model(&wire_model_id);
                    let spec = match parts.extensions.get::<UniversalGenerationOrigin>().cloned() {
                        Some(origin) => origin.into_spec(executable_model),
                        None => GeminiBillableRequestSpec::native(
                            bounded_request_fact_model(model_id),
                            executable_model,
                            route.operation == Operation::StreamGenerate,
                            classify_gemini_generate_content(&value),
                        ),
                    };
                    (seed, spec)
                })
        } else {
            None
        };
    let affinity_input = pending.affinity_scope().and_then(|scope| {
        app.affinity
            .infer_gemini(scope, &parts.headers, model_id, &value)
    });
    let mut affinity_resolution = match affinity_input.as_ref() {
        Some(input) => app.affinity.resolve(input).await,
        None => None,
    };
    // Shared system/tools warmth is a soft first-placement hint only. A resolved conversation is
    // stronger and does not consult the root set; a new conversation seeds two profiles before it
    // starts preferring warm copies, matching the Claude and Codex pools.
    let affinity_warm_homes = match (affinity_input.as_ref(), affinity_resolution.as_ref()) {
        (Some(input), None) if input.has_cache_root() => app.affinity.warm_homes(input).await,
        _ => Vec::new(),
    };
    let warm_profile_ids = gateway.profile_ids_for_homes(&app.affinity, &affinity_warm_homes);
    let place_cache_root = affinity_input
        .as_ref()
        .is_some_and(|input| input.has_cache_root())
        && affinity_resolution.is_none();
    let generation = matches!(
        route.operation,
        Operation::Generate | Operation::StreamGenerate
    );
    // Every exact-profile calibration operation is intentionally non-replayable. Admission accepts
    // a calibration target only for Authz::Admin, and request_fact_seed rejects admin, so every
    // one-shot return below is fact-free by construction. `countTokens` is free, but it is the
    // admission fence for the paid turn and therefore must prove exactly one upstream attempt too:
    // no helper restart, OAuth 401 resend, profile rotation or smooth retry.
    // Pre-send token acquisition failures still retain a not-started proof. Paid generation keeps
    // its stricter post-dispatch billing/delivery semantics through `one_shot_generation` below.
    let one_shot_upstream = calibration_target.is_some();
    let one_shot_generation = generation && one_shot_upstream;
    let token_policy = if deadline_bound_exact {
        if generation {
            TokenAcquisitionPolicy::ExactGenerationCachedOnly
        } else {
            TokenAcquisitionPolicy::ExactCount
        }
    } else {
        TokenAcquisitionPolicy::Normal
    };
    let requested_image_output_tokens = if generation && model.is_image_generation() {
        image_output_tokens(&value)
    } else {
        0
    };
    let requested_audio_usage = if generation && model.id == "gemini-3-flash-preview" {
        flash_preview_audio_usage_hint(&value)?
    } else {
        AudioUsageHint::default()
    };
    let upstream_session_id = (generation && !model.is_image_generation()).then(|| {
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
    let upstream_request_id =
        generation.then(|| fresh_antigravity_request_id(model.is_image_generation()));
    // Routing precedes customer reserve, but local in-flight depth never waits or rejects. Every
    // eligible request gets an immediate profile lease; the normal reserve/delivery/settlement
    // lifecycle still runs exactly once across all pre-byte profile retries.
    let generation_budget = generation.then(|| {
        generation_controls(
            &value,
            &model,
            gateway.config().reserve_overhead_tokens,
            requested_audio_usage,
        )
    });
    let mut pending = Some(pending);
    let mut admission: Option<GeminiAdmission> = None;

    let suffix = match route.operation {
        Operation::Generate => "generateContent",
        Operation::StreamGenerate => "streamGenerateContent",
        Operation::CountTokens => "countTokens",
        Operation::Models | Operation::Model => unreachable!(),
    };
    let mut excluded = HashSet::new();
    let mut attempted_profiles = HashSet::new();
    let mut routing_attempts = 0usize;
    let mut retry_failures = 0usize;
    let mut saw_quota = false;
    let mut rate_limit_attempts = 0usize;
    let mut saw_auth = false;
    // A 403 is a verdict about the *request*, not the credential, and is tracked apart from 401.
    let mut saw_permission_denied = false;
    let mut saw_backend = false;
    // Same smooth-wait budget the Anthropic plane has always had. A pool that is momentarily out of
    // capacity — every profile refreshing its token at once, one cooling window about to expire —
    // recovers in well under this budget, and until now the Gemini plane had no way to wait for it:
    // one pass over the profiles and the customer got a 503. Waiting happens before the first public
    // byte, so the response the customer finally sees is still exactly one response.
    let smooth_deadline =
        std::time::Instant::now() + std::time::Duration::from_millis(app.cfg.smooth_wait_ms);
    'smooth: loop {
        let preferred_id = affinity_resolution
            .as_ref()
            .and_then(|resolution| gateway.profile_id_for_home(&app.affinity, &resolution.home));
        let lease = match calibration_target.as_deref() {
            Some(target) => {
                gateway.select_operator_target(&wire_model_id, target, &excluded, generation)
            }
            None => gateway
                .select_routed(
                    &wire_model_id,
                    &excluded,
                    preferred_id.as_deref(),
                    &warm_profile_ids,
                    place_cache_root,
                    generation,
                )
                // A subscription may rest because Google reported its quota is gone. It may not
                // rest because we inferred something about the environment from a 401/403 or a
                // transport fault: that inference steers routing while healthier capacity exists,
                // and stops mattering the moment it would otherwise turn into an empty pool. Only
                // `excluded` bounds this pass, so each profile is still attempted at most once.
                .or_else(|| {
                    gateway.select_routed_ignoring_env_cooling(
                        &wire_model_id,
                        &excluded,
                        preferred_id.as_deref(),
                        &warm_profile_ids,
                        place_cache_root,
                        generation,
                    )
                }),
        };
        let Some(lease) = lease else {
            Metrics::inc(&app.metrics.exhausted);
            elog::warn("gemini", "gemini pool exhausted: no lease");
            if calibration_target.is_some() {
                return Err(ApiError::unavailable(
                    "gemini_calibration_profile_unavailable",
                ));
            }
            // This request is the freshest evidence the pool has that it is out of capacity. Ask
            // for an out-of-band sweep so recovery is bounded by the probe, not by the background
            // cadence — the difference between seconds and the full health interval.
            gateway.request_probe_rate_limited();
            let retry = gateway
                .soonest_ready(&wire_model_id, &HashSet::new(), generation)
                .map(|until| until.saturating_sub(pool::now()).max(1) as u64);
            // Waiting only helps when the pool ran out of *capacity*: cooling expires, a refresh
            // lands, a concurrent request frees its slot. If the round instead collected real
            // provider verdicts — a 401/403 or a backend fault on the profiles it did reach — the
            // fleet already answered, and re-running the rotation would replay the same rejected
            // request across every profile a second time. Google has denied one identical request
            // on all seven profiles before; doubling that traffic is the last thing to do about it.
            // Quota (429) stays retryable: it is time-based and `soonest_ready` bounds the wait.
            let round_saw_provider_verdict =
                saw_auth || saw_permission_denied || saw_backend || retry_failures > 0;
            let remaining = smooth_deadline
                .saturating_duration_since(std::time::Instant::now())
                .as_millis();
            let hint = retry.map(|seconds| seconds as i64).unwrap_or(1);
            if !round_saw_provider_verdict {
                if let Some(step) = crate::proxy::smooth_step(hint, remaining) {
                    tokio::time::sleep(step).await;
                    // A profile skipped a moment ago may be the healthy one now, so the round
                    // starts clean; otherwise the retry would rotate over an empty set.
                    excluded.clear();
                    saw_quota = false;
                    continue 'smooth;
                }
            }
            // Every profile that answered denied this exact request while its credential stayed
            // valid: that is Google refusing the request, not us being unavailable. Return its own
            // verdict so the caller can stop — a synthetic retryable 503 sent clients into an
            // endless retry of something no retry can fix.
            if saw_permission_denied && !saw_quota {
                let delivery = admission
                    .as_ref()
                    .map(GeminiAdmission::observed_failure_delivery)
                    .unwrap_or(DeliveryState::Unknown);
                settle_billable_failure(
                    &mut admission,
                    StatusCode::FORBIDDEN,
                    ProviderTerminalClass::Auth,
                    delivery,
                );
                return Err(ApiError::provider_rejected(StatusCode::FORBIDDEN));
            }
            let delivery = admission
                .as_ref()
                .map(GeminiAdmission::observed_failure_delivery)
                .unwrap_or(DeliveryState::Unknown);
            let (terminal_class, error) = if !gateway.has_authenticated_profiles() {
                (
                    ProviderTerminalClass::Auth,
                    ApiError::unavailable("gemini_profiles_unauthenticated"),
                )
            } else if saw_quota {
                log_rate_limit_exhausted(
                    &rate_limit_request_id,
                    &model.id,
                    &wire_model_id,
                    rate_limit_attempts,
                    routing_attempts,
                    attempted_profiles.len(),
                    retry.unwrap_or(gateway.config().default_rate_limit_cool_secs.max(1) as u64),
                );
                (ProviderTerminalClass::Quota, ApiError::rate_limited(retry))
            } else if saw_auth {
                (
                    ProviderTerminalClass::Auth,
                    ApiError::unavailable("gemini_profiles_unavailable"),
                )
            } else if saw_backend || retry_failures > 0 {
                (
                    ProviderTerminalClass::UpstreamError,
                    ApiError::unavailable("gemini_profiles_unavailable"),
                )
            } else {
                (ProviderTerminalClass::Quota, ApiError::rate_limited(retry))
            };
            settle_billable_failure(&mut admission, error.status, terminal_class, delivery);
            return Err(error);
        };
        if admission.is_none() {
            let pending = pending
                .take()
                .expect("Gemini admission is initialized before the first upstream attempt");
            let ready = if let Some((input, output, _, grounding)) = generation_budget {
                let (admission, effective_output) = pending
                    .reserve(
                        &app,
                        &model,
                        input,
                        output,
                        requested_image_output_tokens,
                        grounding,
                        !model.is_image_generation(),
                        billable_fact.take(),
                    )
                    .await?;
                // Always write the validated ceiling: this also clamps a hostile value above the
                // model limit even when the account can afford the complete request.
                if !model.is_image_generation() {
                    if let Err(error) = cap_generation_output(&mut value, effective_output) {
                        let mut admission = Some(admission);
                        settle_observed_billable_failure(
                            &mut admission,
                            error.status,
                            ProviderTerminalClass::Unknown,
                        );
                        return Err(error);
                    }
                }
                admission
            } else {
                pending.without_reserve()
            };
            admission = Some(ready);
        }
        let admitted_not_after = admission
            .as_ref()
            .and_then(GeminiAdmission::calibration_not_after);
        let profile = lease.profile().clone();
        attempted_profiles.insert(profile.id().to_string());
        routing_attempts = routing_attempts.saturating_add(1);
        let attempt = routing_attempts;
        let oauth_kind = profile.oauth_kind();
        let upstream_user_agent = gateway.config().user_agent(oauth_kind, &wire_model_id);
        // The owned private-route probe served Preview without the older IDE metadata tuple. Keep
        // that minimal identity model-local while the rest of the fleet and background calls
        // retain their live-proven metadata.
        let include_antigravity_metadata =
            !(oauth_kind == OAuthKind::Antigravity && model.id == "gemini-3-flash-preview");
        let mut url = format!(
            "{}/v1internal:{suffix}",
            gateway.config().generation_upstream_for(
                oauth_kind,
                model.is_image_generation(),
                &wire_model_id,
            )
        );
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }
        if route.operation == Operation::Generate && !one_shot_upstream {
            let primitive_request = prepare_nonstream_generate_request(
                &model,
                wire_model_id.clone(),
                value.clone(),
                upstream_session_id.clone(),
                user_prompt_id.clone(),
                upstream_request_id
                    .clone()
                    .expect("generation requests always carry an upstream request id"),
            );
            match execute_nonstream_generate_observed(
                &gateway,
                &lease,
                &model,
                &primitive_request,
                admission
                    .as_ref()
                    .and_then(GeminiAdmission::actual_send_observer),
            )
            .await
            {
                Ok(raw) => {
                    let status = raw.status;
                    let response_headers = raw.headers;
                    let response_body = raw.body;
                    let rejected_token = raw.rejected_token;
                    match status.as_u16() {
                        401 => {
                            Metrics::inc(&app.metrics.upstream_auth);
                            match execute_nonstream_generate_with_rejected_token(
                                &gateway,
                                &lease,
                                &model,
                                &primitive_request,
                                Some(&rejected_token),
                                admission
                                    .as_ref()
                                    .and_then(GeminiAdmission::actual_send_observer),
                            )
                            .await
                            {
                                Ok(retried) if retried.status != StatusCode::UNAUTHORIZED => {
                                    if retried.status.is_success() {
                                        profile.mark_model_success(&wire_model_id);
                                        record_affinity_success(
                                            &app.affinity,
                                            affinity_input.as_ref(),
                                            &mut affinity_resolution,
                                            &affinity_warm_homes,
                                            profile.id(),
                                        )
                                        .await;
                                        let usage = retried.usage;
                                        if admission
                                            .as_ref()
                                            .expect(
                                                "Gemini admission exists after upstream selection",
                                            )
                                            .requires_usage()
                                            && usage.is_none()
                                        {
                                            Metrics::inc(&app.metrics.gemini_usage_missing);
                                            Metrics::inc(&app.metrics.gemini_malformed_responses);
                                            profile.mark_model_failure(
                                                &wire_model_id,
                                                "usage_metadata",
                                                gateway.config(),
                                            );
                                            elog::error(
                                                "gemini",
                                                "gemini request failed: usage metadata missing",
                                            );
                                            settle_observed_billable_failure(
                                                &mut admission,
                                                StatusCode::SERVICE_UNAVAILABLE,
                                                ProviderTerminalClass::ProtocolError,
                                            );
                                            return Err(ApiError::unavailable(
                                                "gemini_usage_metadata_missing",
                                            ));
                                        }
                                        let tool_calls_in_output =
                                            serde_json::from_slice::<Value>(&retried.body)
                                                .ok()
                                                .as_ref()
                                                .and_then(gemini_tool_calls_in_output);
                                        let admission = admission.take().expect(
                                            "Gemini admission exists after upstream selection",
                                        );
                                        let request_probe = admission.requests_post_turn_probe();
                                        if admission.mark_delivering().await.is_err() {
                                            if let Some(event) = admission
                                                .settle_after_delivery_marker_failure(
                                                    &model,
                                                    usage.as_ref(),
                                                    profile.id(),
                                                    tool_calls_in_output,
                                                )
                                            {
                                                profile.record_turn(event);
                                                if request_probe {
                                                    gateway.request_probe();
                                                }
                                            }
                                            return Err(ApiError::unavailable(
                                                "gemini_delivery_marker_failed",
                                            )
                                            .after_dispatch());
                                        }
                                        if let Some(event) = admission.settle_terminal(
                                            &model,
                                            usage.as_ref(),
                                            profile.id(),
                                            Some(200),
                                            ProviderTerminalClass::Success,
                                            DeliveryState::Completed,
                                            None,
                                            true,
                                            tool_calls_in_output,
                                        ) {
                                            profile.record_turn(event);
                                            if request_probe {
                                                gateway.request_probe();
                                            }
                                        }
                                        return Ok(translated_response(
                                            retried.status,
                                            &retried.headers,
                                            retried.body,
                                        ));
                                    }
                                }
                                _ => {}
                            }
                            saw_auth = true;
                            excluded.insert(profile.id().to_string());
                            profile.mark_auth_blocked(gateway.config());
                            continue;
                        }
                        403 => {
                            Metrics::inc(&app.metrics.upstream_auth);
                            excluded.insert(profile.id().to_string());
                            if google_error_status(&response_body).as_deref()
                                == Some("PERMISSION_DENIED")
                            {
                                saw_permission_denied = true;
                            } else {
                                saw_auth = true;
                                profile.mark_auth_blocked(gateway.config());
                            }
                            continue;
                        }
                        429 => {
                            Metrics::inc(&app.metrics.upstream_429);
                            saw_quota = true;
                            rate_limit_attempts = rate_limit_attempts.saturating_add(1);
                            excluded.insert(profile.id().to_string());
                            let diagnostic = RateLimitDiagnostic::from_body(
                                Some(&response_headers),
                                &response_body,
                            );
                            let hint =
                                rate_limit::retry_after_header_delay(Some(&response_headers))
                                    .or_else(|| {
                                        serde_json::from_slice::<Value>(&response_body)
                                            .ok()
                                            .and_then(|value| rate_limit::retry_info_delay(&value))
                                    });
                            let delay = generation_429_cool_secs(
                                hint,
                                &diagnostic,
                                profile.quota_reports_remaining(
                                    &wire_model_id,
                                    gateway.config(),
                                    pool::now(),
                                ),
                                gateway.config(),
                            );
                            log_rate_limit_attempt(
                                &rate_limit_request_id,
                                route.operation.diagnostic_name(),
                                "http_response",
                                attempt,
                                &model.id,
                                &wire_model_id,
                                profile.id(),
                                oauth_kind,
                                &diagnostic,
                                delay,
                                &profile.rate_limit_quota_evidence(
                                    &wire_model_id,
                                    gateway.config(),
                                    pool::now(),
                                ),
                            );
                            profile.cool_model_until(&wire_model_id, pool::now() + delay);
                            continue;
                        }
                        408 | 409 | 425 => {
                            Metrics::inc(&app.metrics.upstream_5xx);
                            Metrics::inc(&app.metrics.gemini_transport_failures);
                            excluded.insert(profile.id().to_string());
                            profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                            retry_failures += 1;
                            if retry_failures > gateway.config().max_transport_retries {
                                elog::error("gemini", "gemini request failed: backend unavailable");
                                settle_billable_failure(
                                    &mut admission,
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    ProviderTerminalClass::UpstreamError,
                                    DeliveryState::Interrupted,
                                );
                                return Err(ApiError::unavailable("gemini_backend_unavailable"));
                            }
                            continue;
                        }
                        500..=599 => {
                            Metrics::inc(&app.metrics.upstream_5xx);
                            Metrics::inc(&app.metrics.gemini_backend_failures);
                            excluded.insert(profile.id().to_string());
                            profile.mark_model_failure(&wire_model_id, "backend", gateway.config());
                            saw_backend = true;
                            retry_failures += 1;
                            if retry_failures > gateway.config().max_transport_retries {
                                elog::error("gemini", "gemini request failed: backend unavailable");
                                settle_billable_failure(
                                    &mut admission,
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    ProviderTerminalClass::UpstreamError,
                                    DeliveryState::Interrupted,
                                );
                                return Err(ApiError::unavailable("gemini_backend_unavailable"));
                            }
                            continue;
                        }
                        _ if status.is_success() => {
                            profile.mark_model_success(&wire_model_id);
                            record_affinity_success(
                                &app.affinity,
                                affinity_input.as_ref(),
                                &mut affinity_resolution,
                                &affinity_warm_homes,
                                profile.id(),
                            )
                            .await;
                            let usage = raw.usage;
                            if admission
                                .as_ref()
                                .expect("Gemini admission exists after upstream selection")
                                .requires_usage()
                                && usage.is_none()
                            {
                                Metrics::inc(&app.metrics.gemini_usage_missing);
                                Metrics::inc(&app.metrics.gemini_malformed_responses);
                                profile.mark_model_failure(
                                    &wire_model_id,
                                    "usage_metadata",
                                    gateway.config(),
                                );
                                elog::error(
                                    "gemini",
                                    "gemini request failed: usage metadata missing",
                                );
                                settle_billable_failure(
                                    &mut admission,
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    ProviderTerminalClass::ProtocolError,
                                    DeliveryState::Interrupted,
                                );
                                return Err(ApiError::unavailable("gemini_usage_metadata_missing"));
                            }
                            let tool_calls_in_output =
                                serde_json::from_slice::<Value>(&response_body)
                                    .ok()
                                    .as_ref()
                                    .and_then(gemini_tool_calls_in_output);
                            let admission = admission
                                .take()
                                .expect("Gemini admission exists after upstream selection");
                            let request_probe = admission.requests_post_turn_probe();
                            if admission.mark_delivering().await.is_err() {
                                if let Some(event) = admission.settle_after_delivery_marker_failure(
                                    &model,
                                    usage.as_ref(),
                                    profile.id(),
                                    tool_calls_in_output,
                                ) {
                                    profile.record_turn(event);
                                    if request_probe {
                                        gateway.request_probe();
                                    }
                                }
                                return Err(ApiError::unavailable("gemini_delivery_marker_failed")
                                    .after_dispatch());
                            }
                            if let Some(event) = admission.settle_terminal(
                                &model,
                                usage.as_ref(),
                                profile.id(),
                                Some(200),
                                ProviderTerminalClass::Success,
                                DeliveryState::Completed,
                                None,
                                true,
                                tool_calls_in_output,
                            ) {
                                profile.record_turn(event);
                                if request_probe {
                                    gateway.request_probe();
                                }
                            }
                            return Ok(translated_response(
                                status,
                                &response_headers,
                                response_body,
                            ));
                        }
                        _ if status.is_client_error() => {
                            profile.mark_authenticated();
                            settle_billable_failure(
                                &mut admission,
                                status,
                                provider_status_class(status),
                                DeliveryState::Interrupted,
                            );
                            return Err(ApiError::provider_rejected(status));
                        }
                        _ => {
                            Metrics::inc(&app.metrics.upstream_5xx);
                            Metrics::inc(&app.metrics.gemini_backend_failures);
                            excluded.insert(profile.id().to_string());
                            profile.mark_model_failure(
                                &wire_model_id,
                                "protocol",
                                gateway.config(),
                            );
                            saw_backend = true;
                            retry_failures += 1;
                            if retry_failures > gateway.config().max_transport_retries {
                                elog::error(
                                    "gemini",
                                    "gemini request failed: backend protocol error",
                                );
                                settle_billable_failure(
                                    &mut admission,
                                    StatusCode::SERVICE_UNAVAILABLE,
                                    ProviderTerminalClass::ProtocolError,
                                    DeliveryState::Interrupted,
                                );
                                return Err(ApiError::unavailable("gemini_backend_protocol_error"));
                            }
                            continue;
                        }
                    }
                }
                Err(GeminiNonstreamExecuteError::Token) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    Metrics::inc(&app.metrics.gemini_transport_failures);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    retry_failures += 1;
                    if retry_failures > gateway.config().max_transport_retries {
                        elog::error("gemini", "gemini request failed: transport unavailable");
                        settle_observed_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::Transport,
                        );
                        return Err(ApiError::unavailable("gemini_transport_unavailable"));
                    }
                    continue;
                }
                Err(GeminiNonstreamExecuteError::Transport { evidence }) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    Metrics::inc(&app.metrics.gemini_transport_failures);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    retry_failures += 1;
                    if retry_failures > gateway.config().max_transport_retries {
                        let (message, reason) = if evidence.response_headers_received {
                            (
                                "gemini request failed: response read failed",
                                "gemini_response_read_failed",
                            )
                        } else {
                            (
                                "gemini request failed: transport unavailable",
                                "gemini_transport_unavailable",
                            )
                        };
                        elog::error("gemini", message);
                        settle_observed_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::Transport,
                        );
                        return Err(ApiError::unavailable(reason));
                    }
                    continue;
                }
                Err(GeminiNonstreamExecuteError::Protocol { kind, .. }) => match kind {
                    GeminiNonstreamProtocolError::AudioUsage => {
                        Metrics::inc(&app.metrics.gemini_usage_missing);
                        profile.mark_model_failure(
                            &wire_model_id,
                            "usage_metadata",
                            gateway.config(),
                        );
                        elog::error(
                            "gemini",
                            "gemini request failed: audio usage metadata missing",
                        );
                        settle_observed_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::ProtocolError,
                        );
                        return Err(ApiError::unavailable("gemini_audio_usage_metadata_missing"));
                    }
                    GeminiNonstreamProtocolError::Malformed => {
                        Metrics::inc(&app.metrics.upstream_5xx);
                        Metrics::inc(&app.metrics.gemini_malformed_responses);
                        excluded.insert(profile.id().to_string());
                        profile.mark_model_failure(&wire_model_id, "malformed", gateway.config());
                        saw_backend = true;
                        retry_failures += 1;
                        if retry_failures > gateway.config().max_transport_retries {
                            elog::error("gemini", "gemini request failed: malformed response");
                            settle_billable_failure(
                                &mut admission,
                                StatusCode::SERVICE_UNAVAILABLE,
                                ProviderTerminalClass::ProtocolError,
                                DeliveryState::Interrupted,
                            );
                            return Err(ApiError::unavailable("gemini_malformed_response"));
                        }
                        continue;
                    }
                },
            }
        }

        let project = profile.project_id().await;
        let upstream_body = match wrap_code_assist_request(
            route.operation,
            oauth_kind,
            &wire_model_id,
            &project,
            &value,
            &user_prompt_id,
            upstream_session_id.as_deref(),
            upstream_request_id.as_deref(),
        ) {
            Ok(body) => body,
            Err(error) => {
                settle_observed_billable_failure(
                    &mut admission,
                    error.status,
                    ProviderTerminalClass::Unknown,
                );
                return Err(error);
            }
        };
        let (mut response, rejected_token) = match send_upstream(
            &profile,
            &url,
            &parts.headers,
            upstream_body.clone(),
            None,
            &upstream_user_agent,
            include_antigravity_metadata,
            if one_shot_upstream {
                TransportRetryPolicy::NeverReplay
            } else {
                TransportRetryPolicy::RestartHelperOnce
            },
            token_policy,
            deadline_bound_exact.then_some(admitted_not_after).flatten(),
            fact_guard
                .as_ref()
                .map(GeminiCountTokensFactGuard::actual_send_observer)
                .or_else(|| {
                    admission
                        .as_ref()
                        .and_then(GeminiAdmission::actual_send_observer)
                }),
        )
        .await
        {
            Ok(response) => response,
            Err(SendError::Token(TokenError::Invalid)) => {
                Metrics::inc(&app.metrics.upstream_auth);
                if one_shot_upstream {
                    return Err(ApiError::unavailable("gemini_calibration_attempt_failed"));
                }
                saw_auth = true;
                excluded.insert(profile.id().to_string());
                profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                continue;
            }
            Err(SendError::Token(TokenError::Temporary | TokenError::Blocked)) => {
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_transport_failures);
                if one_shot_upstream {
                    return Err(ApiError::unavailable("gemini_calibration_attempt_failed"));
                }
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: transport unavailable");
                    settle_observed_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::Transport,
                    );
                    return Err(ApiError::unavailable("gemini_transport_unavailable"));
                }
                continue;
            }
            Err(SendError::Transport(error)) => {
                if let Some(guard) = fact_guard.as_mut() {
                    guard.observe(CountTokensTerminalEvidence::transport(error));
                }
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_transport_failures);
                if one_shot_upstream {
                    return Err(
                        ApiError::unavailable("gemini_calibration_attempt_failed").after_dispatch()
                    );
                }
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: transport unavailable");
                    settle_observed_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::Transport,
                    );
                    return Err(ApiError::unavailable("gemini_transport_unavailable"));
                }
                continue;
            }
            Err(SendError::CalibrationExpired) => {
                settle_observed_billable_failure(
                    &mut admission,
                    StatusCode::SERVICE_UNAVAILABLE,
                    ProviderTerminalClass::Timeout,
                );
                return Err(ApiError::unavailable("gemini_calibration_dispatch_expired"));
            }
        };
        let mut status = response.status();
        if let Some(guard) = fact_guard.as_mut() {
            guard.observe(CountTokensTerminalEvidence::headers(status));
        }

        // A bearer can be revoked before its local expiry. Refresh once on the same profile. The
        // rejected-token compare in the profile mutex ensures a concurrent 401 burst performs one
        // refresh rather than one refresh per request.
        if status == StatusCode::UNAUTHORIZED && !one_shot_upstream {
            Metrics::inc(&app.metrics.upstream_auth);
            match send_upstream(
                &profile,
                &url,
                &parts.headers,
                upstream_body,
                Some(&rejected_token),
                &upstream_user_agent,
                include_antigravity_metadata,
                TransportRetryPolicy::RestartHelperOnce,
                TokenAcquisitionPolicy::Normal,
                None,
                fact_guard
                    .as_ref()
                    .map(GeminiCountTokensFactGuard::actual_send_observer)
                    .or_else(|| {
                        admission
                            .as_ref()
                            .and_then(GeminiAdmission::actual_send_observer)
                    }),
            )
            .await
            {
                Ok((retried, _)) => {
                    response = retried;
                    status = response.status();
                    if let Some(guard) = fact_guard.as_mut() {
                        guard.observe(CountTokensTerminalEvidence::headers(status));
                    }
                }
                Err(SendError::Token(TokenError::Invalid)) => {
                    saw_auth = true;
                    excluded.insert(profile.id().to_string());
                    profile.mark_auth_failed(pool::now() + gateway.config().auth_quarantine_secs);
                    continue;
                }
                Err(
                    error @ (SendError::Token(TokenError::Temporary | TokenError::Blocked)
                    | SendError::Transport(_)
                    | SendError::CalibrationExpired),
                ) => {
                    if let (Some(guard), SendError::Transport(error)) = (fact_guard.as_mut(), error)
                    {
                        guard.observe(CountTokensTerminalEvidence::transport(error));
                    }
                    Metrics::inc(&app.metrics.upstream_5xx);
                    Metrics::inc(&app.metrics.gemini_transport_failures);
                    excluded.insert(profile.id().to_string());
                    profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                    retry_failures += 1;
                    if retry_failures > gateway.config().max_transport_retries {
                        elog::error("gemini", "gemini request failed: token refresh unavailable");
                        settle_observed_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::Transport,
                        );
                        return Err(ApiError::unavailable("gemini_token_refresh_unavailable"));
                    }
                    continue;
                }
            }
        }
        let calibration_dispatch_ms = response.calibration_dispatch_ms();
        if deadline_bound_exact && calibration_dispatch_ms.is_none() {
            settle_observed_billable_failure(
                &mut admission,
                StatusCode::SERVICE_UNAVAILABLE,
                ProviderTerminalClass::ProtocolError,
            );
            return Err(
                ApiError::unavailable("gemini_calibration_dispatch_attestation_missing")
                    .after_dispatch(),
            );
        }
        let response_headers = response.headers().clone();

        if status.is_success() && route.operation == Operation::StreamGenerate {
            let mut stream = response.bytes_stream();
            let mut translator = SseTranslator::new_with_image_usage(
                framing,
                &model.id,
                requested_image_output_tokens,
                requested_audio_usage,
            );
            if deadline_bound_exact {
                translator = translator.with_upstream_model_version();
            }
            let (stream_start_max_bytes, stream_start_max_chunks) = if model.is_image_generation() {
                (GEMINI_BODY_LIMIT, IMAGE_STREAM_START_MAX_CHUNKS)
            } else {
                (STREAM_START_MAX_BYTES, STREAM_START_MAX_CHUNKS)
            };
            // Do not return 200 until at least one public native event exists, because retries are
            // forbidden after delivery. Bound this private prelude independently from per-event
            // framing: an upstream that emits endless credit/accounting events (or empty chunks)
            // must not hold a lease, customer reserve and request lifecycle forever.
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
                                stream_start_max_bytes,
                                stream_start_max_chunks,
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
                Ok(Err(())) if translator.audio_usage_failed => {
                    Metrics::inc(&app.metrics.gemini_usage_missing);
                    profile.mark_model_failure(&wire_model_id, "usage_metadata", gateway.config());
                    elog::error(
                        "gemini",
                        "gemini request failed: audio usage metadata missing",
                    );
                    let error = ApiError::unavailable("gemini_audio_usage_metadata_missing");
                    settle_observed_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::ProtocolError,
                    );
                    return Err(if one_shot_generation {
                        error.after_dispatch()
                    } else {
                        error
                    });
                }
                Ok(Err(())) | Err(_) => {
                    Metrics::inc(&app.metrics.upstream_5xx);
                    Metrics::inc(&app.metrics.gemini_stream_start_failures);
                    excluded.insert(profile.id().to_string());
                    profile.mark_model_failure(&wire_model_id, "stream_start", gateway.config());
                    if one_shot_generation {
                        return Err(ApiError::unavailable("gemini_calibration_attempt_failed")
                            .after_dispatch());
                    }
                    saw_backend = true;
                    retry_failures += 1;
                    if retry_failures > gateway.config().max_transport_retries {
                        elog::error("gemini", "gemini request failed: stream start failed");
                        settle_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::ProtocolError,
                            DeliveryState::Interrupted,
                        );
                        return Err(ApiError::unavailable("gemini_stream_start_failed"));
                    }
                    continue;
                }
            };
            if let Some(code) = translator.provider_error {
                if one_shot_generation && code != 429 {
                    return Err((if (400..500).contains(&code) {
                        ApiError::provider_rejected(
                            StatusCode::from_u16(code).unwrap_or(StatusCode::BAD_REQUEST),
                        )
                    } else {
                        ApiError::unavailable("gemini_calibration_attempt_failed")
                    })
                    .after_dispatch());
                }
                match code {
                    401 | 403 => {
                        Metrics::inc(&app.metrics.upstream_auth);
                        saw_auth = true;
                        excluded.insert(profile.id().to_string());
                        profile.mark_auth_blocked(gateway.config());
                        continue;
                    }
                    429 => {
                        Metrics::inc(&app.metrics.upstream_429);
                        saw_quota = true;
                        rate_limit_attempts = rate_limit_attempts.saturating_add(1);
                        excluded.insert(profile.id().to_string());
                        let diagnostic = translator
                            .provider_rate_limit_diagnostic
                            .as_ref()
                            .cloned()
                            .unwrap_or_else(|| RateLimitDiagnostic::from_value(None, None));
                        let delay = generation_429_cool_secs(
                            translator.provider_retry_after,
                            &diagnostic,
                            profile.quota_reports_remaining(
                                &wire_model_id,
                                gateway.config(),
                                pool::now(),
                            ),
                            gateway.config(),
                        );
                        log_rate_limit_attempt(
                            &rate_limit_request_id,
                            route.operation.diagnostic_name(),
                            "stream_start",
                            attempt,
                            &model.id,
                            &wire_model_id,
                            profile.id(),
                            oauth_kind,
                            &diagnostic,
                            delay,
                            &profile.rate_limit_quota_evidence(
                                &wire_model_id,
                                gateway.config(),
                                pool::now(),
                            ),
                        );
                        profile.cool_model_until(&wire_model_id, pool::now() + delay);
                        if one_shot_generation {
                            return Err(ApiError::rate_limited(Some(
                                u64::try_from(delay).unwrap_or(1),
                            ))
                            .after_dispatch());
                        }
                        continue;
                    }
                    408 | 409 | 425 => {
                        Metrics::inc(&app.metrics.upstream_5xx);
                        Metrics::inc(&app.metrics.gemini_transport_failures);
                        excluded.insert(profile.id().to_string());
                        profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                        retry_failures += 1;
                        if retry_failures > gateway.config().max_transport_retries {
                            elog::error("gemini", "gemini request failed: stream start failed");
                            settle_billable_failure(
                                &mut admission,
                                StatusCode::SERVICE_UNAVAILABLE,
                                ProviderTerminalClass::ProtocolError,
                                DeliveryState::Interrupted,
                            );
                            return Err(ApiError::unavailable("gemini_stream_start_failed"));
                        }
                        continue;
                    }
                    500..=599 => {
                        Metrics::inc(&app.metrics.upstream_5xx);
                        Metrics::inc(&app.metrics.gemini_backend_failures);
                        excluded.insert(profile.id().to_string());
                        profile.mark_model_failure(&wire_model_id, "backend", gateway.config());
                        saw_backend = true;
                        retry_failures += 1;
                        if retry_failures > gateway.config().max_transport_retries {
                            elog::error("gemini", "gemini request failed: stream start failed");
                            settle_billable_failure(
                                &mut admission,
                                StatusCode::SERVICE_UNAVAILABLE,
                                ProviderTerminalClass::ProtocolError,
                                DeliveryState::Interrupted,
                            );
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
                Metrics::inc(&app.metrics.gemini_stream_start_failures);
                excluded.insert(profile.id().to_string());
                profile.mark_model_failure(&wire_model_id, "stream_start", gateway.config());
                if one_shot_upstream {
                    return Err(
                        ApiError::unavailable("gemini_calibration_attempt_failed").after_dispatch()
                    );
                }
                saw_backend = true;
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: stream start failed");
                    settle_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::ProtocolError,
                        DeliveryState::Interrupted,
                    );
                    return Err(ApiError::unavailable("gemini_stream_start_failed"));
                }
                continue;
            }
            profile.mark_model_success(&wire_model_id);
            record_affinity_success(
                &app.affinity,
                affinity_input.as_ref(),
                &mut affinity_resolution,
                &affinity_warm_homes,
                profile.id(),
            )
            .await;
            let admission = admission
                .take()
                .expect("Gemini admission exists after upstream selection");
            return stream_response(
                gateway,
                app.metrics.clone(),
                profile,
                lease,
                admission,
                model,
                wire_model_id,
                rate_limit_request_id.clone(),
                attempt,
                status,
                response_headers,
                translator,
                initial,
                stream,
                one_shot_generation,
                calibration_dispatch_ms,
            )
            .await;
        }

        let response_body = match read_upstream_body(response).await {
            Ok(bytes) => {
                if let Some(guard) = fact_guard.as_mut() {
                    guard.observe(CountTokensTerminalEvidence::body(status));
                }
                bytes
            }
            Err(error) => {
                if let Some(guard) = fact_guard.as_mut() {
                    guard.observe(CountTokensTerminalEvidence::transport(error));
                }
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_transport_failures);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                if one_shot_upstream {
                    return Err(
                        ApiError::unavailable("gemini_calibration_attempt_failed").after_dispatch()
                    );
                }
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: response read failed");
                    settle_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::Transport,
                        DeliveryState::Interrupted,
                    );
                    return Err(ApiError::unavailable("gemini_response_read_failed"));
                }
                continue;
            }
        };
        if one_shot_upstream && !status.is_success() && status != StatusCode::TOO_MANY_REQUESTS {
            return Err((if status.is_client_error() {
                ApiError::provider_rejected(status)
            } else {
                ApiError::unavailable("gemini_calibration_attempt_failed")
            })
            .after_dispatch());
        }
        match status.as_u16() {
            401 => {
                Metrics::inc(&app.metrics.upstream_auth);
                saw_auth = true;
                excluded.insert(profile.id().to_string());
                profile.mark_auth_blocked(gateway.config());
                continue;
            }
            403 => {
                Metrics::inc(&app.metrics.upstream_auth);
                excluded.insert(profile.id().to_string());
                // Google answers 403 for two unrelated things and the HTTP code alone cannot tell
                // them apart. `UNAUTHENTICATED` (and anything unrecognized) is about this project's
                // credential or environment: rotate away and cool it, as before. `PERMISSION_DENIED`
                // means the credential was accepted and the *request* was refused — every profile
                // returns it for the same request, so cooling the fleet punishes the whole customer
                // base for one caller's request and the exponential auth streak makes each repeat
                // worse.
                if google_error_status(&response_body).as_deref() == Some("PERMISSION_DENIED") {
                    saw_permission_denied = true;
                } else {
                    saw_auth = true;
                    profile.mark_auth_blocked(gateway.config());
                }
                continue;
            }
            429 => {
                Metrics::inc(&app.metrics.upstream_429);
                saw_quota = true;
                rate_limit_attempts = rate_limit_attempts.saturating_add(1);
                excluded.insert(profile.id().to_string());
                let diagnostic =
                    RateLimitDiagnostic::from_body(Some(&response_headers), &response_body);
                let hint =
                    rate_limit::retry_after_header_delay(Some(&response_headers)).or_else(|| {
                        serde_json::from_slice::<Value>(&response_body)
                            .ok()
                            .and_then(|value| rate_limit::retry_info_delay(&value))
                    });
                let delay = generation_429_cool_secs(
                    hint,
                    &diagnostic,
                    profile.quota_reports_remaining(&wire_model_id, gateway.config(), pool::now()),
                    gateway.config(),
                );
                log_rate_limit_attempt(
                    &rate_limit_request_id,
                    route.operation.diagnostic_name(),
                    "http_response",
                    attempt,
                    &model.id,
                    &wire_model_id,
                    profile.id(),
                    oauth_kind,
                    &diagnostic,
                    delay,
                    &profile.rate_limit_quota_evidence(
                        &wire_model_id,
                        gateway.config(),
                        pool::now(),
                    ),
                );
                profile.cool_model_until(&wire_model_id, pool::now() + delay);
                if one_shot_upstream {
                    return Err(
                        ApiError::rate_limited(Some(u64::try_from(delay).unwrap_or(1)))
                            .after_dispatch(),
                    );
                }
                continue;
            }
            408 | 409 | 425 => {
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_transport_failures);
                excluded.insert(profile.id().to_string());
                profile.cool_until(pool::now() + gateway.config().transport_cool_secs);
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: backend unavailable");
                    settle_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::UpstreamError,
                        DeliveryState::Interrupted,
                    );
                    return Err(ApiError::unavailable("gemini_backend_unavailable"));
                }
                continue;
            }
            500..=599 => {
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_backend_failures);
                excluded.insert(profile.id().to_string());
                if generation {
                    profile.mark_model_failure(&wire_model_id, "backend", gateway.config());
                }
                saw_backend = true;
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: backend unavailable");
                    settle_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::UpstreamError,
                        DeliveryState::Interrupted,
                    );
                    return Err(ApiError::unavailable("gemini_backend_unavailable"));
                }
                continue;
            }
            _ if status.is_success() => {
                let native_body = match unwrap_code_assist_response(
                    route.operation,
                    &response_body,
                    &model.id,
                    requested_audio_usage,
                    deadline_bound_exact,
                ) {
                    Ok(body) => body,
                    Err(ResponseDecodeError::AudioUsage) => {
                        Metrics::inc(&app.metrics.gemini_usage_missing);
                        profile.mark_model_failure(
                            &wire_model_id,
                            "usage_metadata",
                            gateway.config(),
                        );
                        elog::error(
                            "gemini",
                            "gemini request failed: audio usage metadata missing",
                        );
                        let error = ApiError::unavailable("gemini_audio_usage_metadata_missing");
                        settle_observed_billable_failure(
                            &mut admission,
                            StatusCode::SERVICE_UNAVAILABLE,
                            ProviderTerminalClass::ProtocolError,
                        );
                        return Err(if one_shot_generation {
                            error.after_dispatch()
                        } else {
                            error
                        });
                    }
                    Err(ResponseDecodeError::Malformed) => {
                        if let Some(guard) = fact_guard.as_mut() {
                            guard.observe(CountTokensTerminalEvidence::protocol());
                        }
                        Metrics::inc(&app.metrics.upstream_5xx);
                        Metrics::inc(&app.metrics.gemini_malformed_responses);
                        excluded.insert(profile.id().to_string());
                        if generation {
                            profile.mark_model_failure(
                                &wire_model_id,
                                "malformed",
                                gateway.config(),
                            );
                        }
                        saw_backend = true;
                        if one_shot_upstream {
                            return Err(ApiError::unavailable("gemini_calibration_attempt_failed")
                                .after_dispatch());
                        }
                        retry_failures += 1;
                        if retry_failures > gateway.config().max_transport_retries {
                            elog::error("gemini", "gemini request failed: malformed response");
                            settle_billable_failure(
                                &mut admission,
                                StatusCode::SERVICE_UNAVAILABLE,
                                ProviderTerminalClass::ProtocolError,
                                DeliveryState::Interrupted,
                            );
                            return Err(ApiError::unavailable("gemini_malformed_response"));
                        }
                        continue;
                    }
                };
                if generation {
                    profile.mark_model_success(&wire_model_id);
                } else {
                    profile.mark_authenticated();
                }
                record_affinity_success(
                    &app.affinity,
                    affinity_input.as_ref(),
                    &mut affinity_resolution,
                    &affinity_warm_homes,
                    profile.id(),
                )
                .await;
                let usage = if route.operation == Operation::Generate {
                    match serde_json::from_slice::<Value>(&native_body) {
                        Ok(value) => {
                            settlement_usage_from_response(&value, requested_image_output_tokens)
                                .filter(|usage| !usage.is_zero())
                        }
                        Err(_) => {
                            if !admission
                                .as_ref()
                                .is_some_and(GeminiAdmission::requires_usage)
                            {
                                elog::warn(
                                    "gemini",
                                    "gemini settlement usage unparseable; settling without usage",
                                );
                            }
                            None
                        }
                    }
                } else {
                    None
                };
                if route.operation == Operation::Generate
                    && admission
                        .as_ref()
                        .expect("Gemini admission exists after upstream selection")
                        .requires_usage()
                    && usage.is_none()
                {
                    Metrics::inc(&app.metrics.gemini_usage_missing);
                    Metrics::inc(&app.metrics.gemini_malformed_responses);
                    profile.mark_model_failure(&wire_model_id, "usage_metadata", gateway.config());
                    elog::error("gemini", "gemini request failed: usage metadata missing");
                    let error = ApiError::unavailable("gemini_usage_metadata_missing");
                    settle_observed_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::ProtocolError,
                    );
                    return Err(if one_shot_generation {
                        error.after_dispatch()
                    } else {
                        error
                    });
                }
                let tool_calls_in_output = (route.operation == Operation::Generate)
                    .then(|| serde_json::from_slice::<Value>(&native_body).ok())
                    .flatten()
                    .as_ref()
                    .and_then(gemini_tool_calls_in_output);
                let admission = admission
                    .take()
                    .expect("Gemini admission exists after upstream selection");
                let request_probe = admission.requests_post_turn_probe();
                if admission.mark_delivering().await.is_err() {
                    if let Some(event) = admission.settle_after_delivery_marker_failure(
                        &model,
                        usage.as_ref(),
                        profile.id(),
                        tool_calls_in_output,
                    ) {
                        profile.record_turn(event);
                        if request_probe {
                            gateway.request_probe();
                        }
                    }
                    return Err(
                        ApiError::unavailable("gemini_delivery_marker_failed").after_dispatch()
                    );
                }
                if let Some(event) = admission.settle_terminal(
                    &model,
                    usage.as_ref(),
                    profile.id(),
                    Some(200),
                    ProviderTerminalClass::Success,
                    DeliveryState::Completed,
                    None,
                    true,
                    tool_calls_in_output,
                ) {
                    profile.record_turn(event);
                    if request_probe {
                        gateway.request_probe();
                    }
                }
                let mut response = translated_response(status, &response_headers, native_body);
                attach_calibration_dispatch_ms(&mut response, calibration_dispatch_ms);
                return Ok(response);
            }
            _ if status.is_client_error() => {
                // The private Code Assist error envelope can contain account, project, plan or
                // internal endpoint details. Preserve only the public status class.
                profile.mark_authenticated();
                settle_billable_failure(
                    &mut admission,
                    status,
                    provider_status_class(status),
                    DeliveryState::Interrupted,
                );
                return Err(ApiError::provider_rejected(status));
            }
            _ => {
                Metrics::inc(&app.metrics.upstream_5xx);
                Metrics::inc(&app.metrics.gemini_backend_failures);
                excluded.insert(profile.id().to_string());
                if generation {
                    profile.mark_model_failure(&wire_model_id, "protocol", gateway.config());
                }
                saw_backend = true;
                if one_shot_upstream {
                    return Err(
                        ApiError::unavailable("gemini_calibration_attempt_failed").after_dispatch()
                    );
                }
                retry_failures += 1;
                if retry_failures > gateway.config().max_transport_retries {
                    elog::error("gemini", "gemini request failed: backend protocol error");
                    settle_billable_failure(
                        &mut admission,
                        StatusCode::SERVICE_UNAVAILABLE,
                        ProviderTerminalClass::ProtocolError,
                        DeliveryState::Interrupted,
                    );
                    return Err(ApiError::unavailable("gemini_backend_protocol_error"));
                }
                continue;
            }
        }
    }
}

#[cfg(test)]
mod tests;
