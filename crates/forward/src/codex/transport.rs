//! Native HTTPS transport to the ChatGPT-backed Codex backend.
//!
//! One profile = one persistent wreq client pinned to the official client identity
//! (`codex_cli_rs` originator/User-Agent, per-profile proxy, BoringSSL ClientHello). Generation
//! is a single `POST /responses` SSE stream; this module translates the upstream `response.*`
//! events into the exact internal notification vocabulary the runner consumes, so the public
//! streaming contract is unchanged from the app-server era.
//!
//! Authentication never leaves this module in plaintext: the bearer token is passed as a
//! zeroizing secret, is never logged, and is refreshed only through the pinned OAuth token
//! endpoint owned by `codex-credential`.

use super::runner::bounded_cache_key;
use super::CodexConfig;
use codex_credential::SecretString;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::{mpsc, watch, Mutex};

const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
/// 256 MiB payload plus JSON/SSE framing overhead. Public OpenAI body is 256 MiB.
const MAX_SSE_LINE_BYTES: usize = 384 * 1024 * 1024;

fn sse_chunk_exceeds_bound(buffered: usize, chunk_len: usize) -> bool {
    buffered.saturating_add(chunk_len) > MAX_SSE_LINE_BYTES
}
const TURN_EVENT_QUEUE: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    Disabled,
    InvalidConfig(String),
    Closed,
    Timeout(&'static str),
    Protocol(String),
    ContextWindowExceeded,
    UsageLimitExceeded {
        retry_after: Option<u64>,
    },
    BadRequest,
    /// The provider rejected the request for a safety/alignment policy reason. This is a client
    /// terminal error, not evidence that the OAuth profile is invalid or that the account is dead.
    PolicyViolation,
    /// The provider completed execution but omitted authoritative terminal usage. Do not turn this
    /// into a successful zero-usage turn; settlement applies the explicit unknown-usage policy.
    MissingAuthoritativeUsage,
    AuthenticationRequired,
    SubscriptionRequired,
    /// A ClaudeStore request was started after the local pool became terminal, but the external
    /// turn did not produce an authoritative successful result. Keep the original local error so
    /// the public status remains stable, while callers can remove the `not_started` proof: once an
    /// external send begins, execution is ambiguous even when no public response byte was emitted.
    ExternalFallbackFailed {
        local: Box<ProcessError>,
    },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("Codex provider is disabled"),
            Self::InvalidConfig(message) => write!(f, "invalid Codex configuration: {message}"),
            Self::Closed => f.write_str("Codex upstream closed its transport"),
            Self::Timeout(phase) => write!(f, "Codex upstream timed out during {phase}"),
            Self::Protocol(message) => write!(f, "Codex upstream protocol error: {message}"),
            Self::ContextWindowExceeded => f.write_str("model context window exceeded"),
            Self::UsageLimitExceeded { .. } => {
                f.write_str("ChatGPT subscription usage limit exceeded")
            }
            Self::BadRequest => f.write_str("model rejected the request"),
            Self::PolicyViolation => f.write_str("request blocked by provider policy"),
            Self::MissingAuthoritativeUsage => {
                f.write_str("Codex provider completed without authoritative usage")
            }
            Self::AuthenticationRequired => f.write_str("Codex profile is not authenticated"),
            Self::SubscriptionRequired => {
                f.write_str("Codex profile is not authenticated with a ChatGPT subscription")
            }
            Self::ExternalFallbackFailed { local } => {
                write!(
                    f,
                    "ClaudeStore fallback failed after local terminal result: {local}"
                )
            }
        }
    }
}

impl std::error::Error for ProcessError {}

impl ProcessError {
    pub(crate) fn diagnostic_class(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::InvalidConfig(_) => "invalid_config",
            Self::Closed => "closed",
            Self::Timeout(_) => "timeout",
            Self::Protocol(_) => "protocol",
            Self::ContextWindowExceeded => "context_window_exceeded",
            Self::UsageLimitExceeded { .. } => "usage_limit_exceeded",
            Self::BadRequest => "bad_request",
            Self::PolicyViolation => "policy_violation",
            Self::MissingAuthoritativeUsage => "missing_authoritative_usage",
            Self::AuthenticationRequired => "authentication_required",
            Self::SubscriptionRequired => "subscription_required",
            Self::ExternalFallbackFailed { .. } => "external_fallback_failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimitWindow {
    /// Canonical provider utilisation in 10^-8 fraction units. This is parsed from the wire
    /// decimal without binary floating point and is the calibration/routing source of truth.
    pub used_fraction_units: i64,
    /// Rounded whole-percent compatibility projection for older status/health consumers.
    pub used_percent: i64,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}

pub const RATE_LIMIT_FRACTION_SCALE: i64 = 100_000_000;
const RATE_LIMIT_PERCENT_SCALE: i64 = RATE_LIMIT_FRACTION_SCALE / 100;

impl CodexRateLimitWindow {
    pub fn used_fraction(&self) -> f64 {
        self.used_fraction_units as f64 / RATE_LIMIT_FRACTION_SCALE as f64
    }

    pub fn used_percent_value(&self) -> f64 {
        self.used_fraction_units as f64 / RATE_LIMIT_PERCENT_SCALE as f64
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimits {
    pub primary: Option<CodexRateLimitWindow>,
    pub secondary: Option<CodexRateLimitWindow>,
    pub reached: bool,
    pub observed_at: i64,
}

/// One authenticated profile's live model catalogue.
///
/// Model availability and Fast capability are kept separately: the official catalogue advertises
/// Fast as the `priority` service tier (with legacy `additional_speed_tiers: ["fast"]` on older
/// payloads), while actual entitlement is learned only from completed generation responses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CodexModelCatalog {
    pub models: HashSet<String>,
    pub fast_models: HashSet<String>,
    /// Provider-authored presentation name. It is intentionally optional: startup fallback and
    /// older provider payloads must not synthesize one from the model id.
    pub display_names: HashMap<String, String>,
    /// Provider-published total context window for one model. This is deliberately distinct from
    /// the locally reviewed output ceiling: `/models.max_context_window` is the authority for the
    /// subscription rollout, with legacy `/models.context_window` used only when no maximum is
    /// present; the configured model contract remains the authority for output/admission.
    pub input_token_limits: HashMap<String, u64>,
}

impl CodexRateLimits {
    fn windows(&self) -> impl Iterator<Item = &CodexRateLimitWindow> {
        self.primary.iter().chain(self.secondary.iter())
    }

    /// Highest utilisation across the reported windows. `None` means no window was reported and the
    /// caller must treat utilisation as unknown rather than as zero-or-full.
    pub fn max_used_percent(&self) -> Option<i64> {
        self.windows().map(|window| window.used_percent).max()
    }

    /// Soonest reset among the windows at or above `threshold_percent`. This is what a client
    /// should wait for before the provider can serve again.
    pub fn soonest_reset_at_or_above(&self, threshold_percent: i64) -> Option<i64> {
        self.windows()
            .filter(|window| window.used_percent >= threshold_percent)
            .filter_map(|window| window.resets_at)
            .min()
    }
}

#[derive(Debug, Clone)]
pub enum AppServerEvent {
    Notification { method: String, params: Value },
}

pub(crate) struct TurnEvents {
    receiver: mpsc::Receiver<AppServerEvent>,
    closed: watch::Receiver<Option<ProcessError>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl TurnEvents {
    /// Start the shared Responses-SSE decoder for an upstream that does not own a local ChatGPT
    /// quota snapshot. ClaudeStore uses the same public `response.*` framing, but its rate limits
    /// must never be attributed to a subscription home.
    pub(crate) fn from_external_response(response: wreq::Response) -> Self {
        let (sender, receiver) = mpsc::channel(TURN_EVENT_QUEUE);
        let (closed_sender, closed) = watch::channel(None);
        let task = tokio::spawn(read_sse_stream(
            response,
            sender,
            closed_sender,
            Arc::new(Mutex::new(None)),
        ));
        Self {
            receiver,
            closed,
            task: Some(task),
        }
    }

    /// Receive the next turn event while observing transport closure out of band.
    ///
    /// The per-turn event queue is deliberately bounded and EOF is carried out of band, so closure
    /// can never be blocked on a full queue. Events already accepted from the transport must still
    /// be delivered before EOF: otherwise a final model delta followed immediately by a stream cut
    /// can be hidden from the runner, which would incorrectly classify the attempt as replaceable
    /// and retry it on another subscription after output has already reached the client.
    pub(crate) async fn recv(&mut self) -> Result<AppServerEvent, ProcessError> {
        loop {
            match self.receiver.try_recv() {
                Ok(event) => return Ok(event),
                Err(mpsc::error::TryRecvError::Empty) => {}
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    return Err(self.closed.borrow().clone().unwrap_or(ProcessError::Closed));
                }
            }
            if let Some(error) = self.closed.borrow().clone() {
                return Err(error);
            }
            tokio::select! {
                biased;
                event = self.receiver.recv() => {
                    return match event {
                        Some(event) => Ok(event),
                        None => Err(self.closed.borrow().clone().unwrap_or(ProcessError::Closed)),
                    };
                }
                changed = self.closed.changed() => {
                    if changed.is_err() {
                        return Err(ProcessError::Closed);
                    }
                    // Re-check the queue before returning the terminal error. The transport reader
                    // routes accepted events before it publishes EOF, and that ordering is part of
                    // the public streaming contract.
                }
            }
        }
    }
}

impl Drop for TurnEvents {
    fn drop(&mut self) {
        // Cancelling the consumer cancels the upstream read: the producer task owns the response
        // body, and aborting it closes the connection instead of letting a dead stream linger.
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// Bearer material for one upstream call. The token stays zeroizing; nothing here is `Debug`.
pub(crate) struct AuthContext {
    pub access_token: SecretString,
    pub account_id: String,
}

pub(crate) enum ImageDispatchError {
    /// No image request could have reached the provider.
    PreDispatch(ProcessError),
    /// The request may have reached the provider; execution cannot be ruled out.
    Ambiguous(ProcessError),
}

/// Result of one OAuth refresh against the pinned token endpoint.
pub(crate) struct TokenRefresh {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: i64,
}

#[derive(Debug)]
struct WireIdentity {
    session_id: String,
    thread_id: String,
    turn_id: String,
    window_id: String,
    turn_metadata: String,
}

/// Request-local identity for the ordered Responses SSE protocol.
///
/// The upstream contract numbers every event and every output item. Keep those protocol keys at
/// the transport boundary so a replayed event cannot become a second public delta and a transient
/// item-id omission/drift cannot split one logical output into two downstream messages.
#[derive(Debug, Default)]
struct SseProtocolState {
    last_sequence_number: Option<u64>,
    output_item_ids: HashMap<u64, String>,
    duplicate_sequence_reported: bool,
    identity_drift_reported: bool,
}

impl SseProtocolState {
    fn accept_sequence(&mut self, payload: &Value) -> bool {
        let Some(sequence_number) = payload.get("sequence_number").and_then(Value::as_u64) else {
            // Older private-backend events did not always carry the public sequence field. Keep
            // them compatible; only events with an authoritative protocol identity are deduped.
            return true;
        };
        if self
            .last_sequence_number
            .is_some_and(|last| sequence_number <= last)
        {
            if !self.duplicate_sequence_reported {
                elog::warn("codex", "Codex duplicate SSE sequence suppressed");
                self.duplicate_sequence_reported = true;
            }
            return false;
        }
        self.last_sequence_number = Some(sequence_number);
        true
    }

    fn canonical_item_id(
        &mut self,
        output_index: Option<u64>,
        incoming_item_id: Option<&str>,
        prefix: &str,
    ) -> String {
        let incoming_item_id = incoming_item_id.filter(|value| !value.is_empty());
        let Some(output_index) = output_index else {
            return incoming_item_id
                .map(str::to_string)
                .unwrap_or_else(|| format!("{prefix}_unknown"));
        };
        if let Some(canonical) = self.output_item_ids.get(&output_index).cloned() {
            if incoming_item_id != Some(canonical.as_str()) {
                self.report_identity_drift();
            }
            return canonical;
        }

        let canonical = incoming_item_id
            .map(str::to_string)
            .unwrap_or_else(|| format!("{prefix}_stream_{output_index}"));
        if incoming_item_id.is_none() {
            self.report_identity_drift();
        }
        self.output_item_ids.insert(output_index, canonical.clone());
        canonical
    }

    fn event_item_id(&mut self, payload: &Value, prefix: &str) -> String {
        self.canonical_item_id(
            payload.get("output_index").and_then(Value::as_u64),
            payload.get("item_id").and_then(Value::as_str),
            prefix,
        )
    }

    fn normalize_output_item(&mut self, payload: &Value) -> Option<Value> {
        let mut item = payload.get("item")?.clone();
        let prefix = match item.get("type").and_then(Value::as_str) {
            Some("message") => "msg",
            Some("reasoning") => "rs",
            Some("function_call") => "fc",
            Some("custom_tool_call") => "ctc",
            _ => "item",
        };
        let canonical = self.canonical_item_id(
            payload.get("output_index").and_then(Value::as_u64),
            item.get("id").and_then(Value::as_str),
            prefix,
        );
        if let Some(object) = item.as_object_mut() {
            object.insert("id".to_string(), Value::String(canonical));
        }
        Some(item)
    }

    fn report_identity_drift(&mut self) {
        if !self.identity_drift_reported {
            elog::warn("codex", "Codex output item identity drift normalized");
            self.identity_drift_reported = true;
        }
    }
}

/// Per-profile native client. Owns the connection pool and the pinned wire identity; it has no
/// credential state of its own — every call receives its bearer explicitly. Clone is an Arc bump:
/// the pool replaces the whole value when a wedged transport generation is recycled.
#[derive(Clone)]
pub(crate) struct ProfileTransport {
    cfg: Arc<CodexConfig>,
    client: wreq::Client,
    image_client: wreq::Client,
    installation_id: String,
}

fn build_client(
    cfg: &CodexConfig,
    proxy: Option<&str>,
    retry: Option<wreq::retry::Policy>,
) -> Result<wreq::Client, ProcessError> {
    let mut builder = wreq::Client::builder()
        // BoringSSL ClientHello with ALPN=http/1.1, same attested profile the Claude fleet
        // presents (see nodetls). ChatGPT's backend is fronted by the same edge stack.
        .emulation(crate::nodetls::bun_emulation())
        .connect_timeout(std::time::Duration::from_millis(
            cfg.request_timeout_ms.max(1).min(30_000),
        ))
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(60))
        // Silence between reads bounds a connected-but-mute stream; SSE activity resets it.
        .read_timeout(std::time::Duration::from_millis(
            cfg.turn_silence_timeout_ms.max(30_000),
        ));
    if let Some(retry) = retry {
        builder = builder.retry(retry);
    }
    if let Some(proxy) = proxy {
        builder = builder.proxy(
            wreq::Proxy::all(proxy)
                .map_err(|error| ProcessError::InvalidConfig(format!("profile proxy: {error}")))?,
        );
    }
    builder
        .build()
        .map_err(|error| ProcessError::InvalidConfig(format!("http client: {error}")))
}

impl ProfileTransport {
    pub(crate) fn new(cfg: Arc<CodexConfig>, proxy: Option<&str>) -> Result<Self, ProcessError> {
        // Per-profile proxy from the sealed credential wins; the standard proxy environment is the
        // fallback for profiles sealed without one (the official client honours it the same way).
        let proxy = proxy
            .filter(|value| !value.trim().is_empty())
            .map(str::to_string)
            .or_else(|| {
                cfg.default_proxy_env
                    .get("HTTPS_PROXY")
                    .or_else(|| cfg.default_proxy_env.get("https_proxy"))
                    .or_else(|| cfg.default_proxy_env.get("ALL_PROXY"))
                    .or_else(|| cfg.default_proxy_env.get("all_proxy"))
                    .cloned()
            });
        let client = build_client(&cfg, proxy.as_deref(), None)?;
        // Images are paid, non-idempotent operations. Isolate their no-retry policy instead of
        // changing the established default retry behavior of text, usage, models and OAuth calls.
        let image_client =
            build_client(&cfg, proxy.as_deref(), Some(wreq::retry::Policy::never()))?;
        // The first-party installation id survives client restarts and account switches. Derive an
        // equally opaque stable UUID from the configured roster location instead of rotating it
        // whenever a profile transport generation is recycled.
        let installation_id = stable_uuid("installation", &cfg.profiles_file);
        Ok(Self {
            cfg,
            client,
            image_client,
            installation_id,
        })
    }

    fn request_identity(&self, cache_key: &str) -> WireIdentity {
        // A root Codex thread uses the same UUID for session and thread. The window remains stable
        // across pool rotation for this tenant-scoped cache key; every user turn gets its own id.
        let thread_id = stable_uuid("thread", cache_key);
        let turn_id = uuid();
        let window_id = stable_uuid("window", cache_key);
        let turn_metadata = json!({
            "installation_id": self.installation_id,
            "session_id": thread_id,
            "thread_id": thread_id,
            "turn_id": turn_id,
            "window_id": window_id,
            "request_kind": "turn",
        })
        .to_string();
        WireIdentity {
            session_id: thread_id.clone(),
            thread_id,
            turn_id,
            window_id,
            turn_metadata,
        }
    }

    fn attach_client_metadata(&self, body: &mut Value, identity: &WireIdentity) {
        body["client_metadata"] = json!({
            "x-codex-installation-id": self.installation_id,
            "session_id": identity.session_id,
            "thread_id": identity.thread_id,
            "turn_id": identity.turn_id,
            "x-codex-window-id": identity.window_id,
            "x-codex-turn-metadata": identity.turn_metadata,
        });
    }

    fn wire_headers(&self, auth: &AuthContext) -> wreq::header::HeaderMap {
        let mut headers = wreq::header::HeaderMap::new();
        headers.insert(
            wreq::header::AUTHORIZATION,
            wreq::header::HeaderValue::from_str(&format!("Bearer {}", auth.access_token.as_str()))
                .unwrap_or_else(|_| wreq::header::HeaderValue::from_static("invalid")),
        );
        for (name, value) in [
            ("chatgpt-account-id", auth.account_id.as_str()),
            ("originator", codex_credential::CODEX_ORIGINATOR),
            ("user-agent", self.cfg.user_agent().as_str()),
            // The official OpenAI provider sends the package version as a standalone provider
            // header in addition to the version embedded in User-Agent. Keep both pinned to the
            // same reviewed value: backend feature rollout (including service tiers) must see the
            // exact client version identity rather than only the catalogue query parameter.
            ("version", self.cfg.cli_version.as_str()),
            ("accept", "text/event-stream"),
        ] {
            if let Ok(value) = wreq::header::HeaderValue::from_str(value) {
                headers.insert(
                    wreq::header::HeaderName::from_bytes(name.as_bytes())
                        .unwrap_or_else(|_| wreq::header::HeaderName::from_static("x-never")),
                    value,
                );
            }
        }
        headers
    }

    fn turn_headers(&self, auth: &AuthContext, identity: &WireIdentity) -> wreq::header::HeaderMap {
        let mut headers = self.wire_headers(auth);
        for (name, value) in [
            ("session-id", identity.session_id.as_str()),
            ("thread-id", identity.thread_id.as_str()),
            ("x-client-request-id", identity.thread_id.as_str()),
            ("x-codex-window-id", identity.window_id.as_str()),
            ("x-codex-turn-metadata", identity.turn_metadata.as_str()),
        ] {
            if let Ok(value) = wreq::header::HeaderValue::from_str(value) {
                headers.insert(
                    wreq::header::HeaderName::from_bytes(name.as_bytes())
                        .unwrap_or_else(|_| wreq::header::HeaderName::from_static("x-never")),
                    value,
                );
            }
        }
        headers
    }

    /// Dispatch one image JSON request through this profile's pinned no-retry client.
    ///
    /// Receiving headers proves dispatch. Before headers, wreq can reliably identify local builder
    /// faults and connector/proxy-connector failures; every other request error remains ambiguous.
    pub(crate) async fn run_image_request(
        &self,
        auth: &AuthContext,
        url: String,
        body: &Value,
        image_turn_id: &str,
        deadline: Option<tokio::time::Instant>,
    ) -> Result<wreq::Response, ImageDispatchError> {
        let mut headers = self.wire_headers(auth);
        headers.insert(
            wreq::header::ACCEPT,
            wreq::header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            wreq::header::CONTENT_TYPE,
            wreq::header::HeaderValue::from_static("application/json"),
        );
        let turn_id = wreq::header::HeaderValue::from_str(image_turn_id).map_err(|_| {
            ImageDispatchError::PreDispatch(ProcessError::InvalidConfig(
                "image turn id header".to_string(),
            ))
        })?;
        headers.insert(
            wreq::header::HeaderName::from_static("x-codex-image-turn-id"),
            turn_id,
        );
        let send = self
            .image_client
            .post(url)
            .headers(headers)
            .json(body)
            .send();
        let response = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, send).await.map_err(|_| {
                ImageDispatchError::Ambiguous(ProcessError::Timeout("image response headers"))
            })?,
            None => send.await,
        };
        response.map_err(|error| {
            let process_error = if error.is_timeout() {
                ProcessError::Timeout("image response headers")
            } else {
                ProcessError::Closed
            };
            if error.is_builder() || error.is_connect() || error.is_proxy_connect() {
                ImageDispatchError::PreDispatch(process_error)
            } else {
                ImageDispatchError::Ambiguous(process_error)
            }
        })
    }

    /// Run one native generation, translating the SSE stream into runner notifications.
    ///
    /// The returned handle streams events as they arrive; HTTP-layer failures (status, connect,
    /// timeout before the first event) are returned synchronously so the pool can rotate before
    /// any byte reached the client. Upstream cache stickiness is deliberate: `session_id` carries
    /// the tenant-scoped cache digest (stable for the whole conversation), matching what the
    /// official client sends for one continuous session — a random per-request id would read as
    /// a brand-new session on every call and waste the warm prefix.
    pub(crate) async fn run_turn(
        &self,
        auth: &AuthContext,
        mut body: Value,
        prompt_cache_key: Option<&str>,
        rate_limits: Arc<Mutex<Option<CodexRateLimits>>>,
        attempts: Option<&super::CodexAttemptObserver>,
    ) -> Result<TurnEvents, ProcessError> {
        let cache_key = prompt_cache_key.map(bounded_cache_key).unwrap_or_else(uuid);
        let identity = self.request_identity(&cache_key);
        self.attach_client_metadata(&mut body, &identity);
        let headers = self.turn_headers(auth, &identity);
        // Count only the actual generation transport submission, immediately before `.send()`.
        if let Some(attempts) = attempts {
            attempts.record_send();
        }
        let response = self
            .client
            .post(self.cfg.responses_url())
            .headers(headers)
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProcessError::Timeout("turn start")
                } else {
                    ProcessError::Closed
                }
            })?;
        let status = response.status();
        note_header_rate_limits(response.headers(), &rate_limits).await;
        if !status.is_success() {
            return Err(classify_http_error(
                status,
                response.headers().clone(),
                bounded_body(response).await,
            ));
        }
        let (sender, receiver) = mpsc::channel(TURN_EVENT_QUEUE);
        let (closed_sender, closed) = watch::channel(None);
        let task = tokio::spawn(read_sse_stream(
            response,
            sender,
            closed_sender,
            rate_limits,
        ));
        Ok(TurnEvents {
            receiver,
            closed,
            task: Some(task),
        })
    }

    /// Read the account's plan and window utilisation without running a generation.
    pub(crate) async fn fetch_usage(
        &self,
        auth: &AuthContext,
    ) -> Result<CodexRateLimits, ProcessError> {
        let response = self
            .client
            .get(self.cfg.usage_url())
            .headers(self.wire_headers(auth))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProcessError::Timeout("usage probe")
                } else {
                    ProcessError::Closed
                }
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(classify_http_error(
                status,
                response.headers().clone(),
                None,
            ));
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| ProcessError::Protocol("usage response body unreadable".to_string()))?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| ProcessError::Protocol("usage response is not JSON".to_string()))?;
        parse_usage_response(&value)
            .ok_or_else(|| ProcessError::Protocol("usage response omitted rate limits".to_string()))
    }

    /// Best-effort live model availability. The public catalog intersects this with the reviewed
    /// billing catalog; a failure keeps the last-good snapshot.
    pub(crate) async fn fetch_models(
        &self,
        auth: &AuthContext,
    ) -> Result<CodexModelCatalog, ProcessError> {
        let response = self
            .client
            .get(format!(
                "{}?client_version={}",
                self.cfg.models_url(),
                self.cfg.cli_version
            ))
            .headers(self.wire_headers(auth))
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProcessError::Timeout("model catalog")
                } else {
                    ProcessError::Closed
                }
            })?;
        if !response.status().is_success() {
            return Err(classify_http_error(
                response.status(),
                response.headers().clone(),
                None,
            ));
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| ProcessError::Protocol("models response body unreadable".to_string()))?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| ProcessError::Protocol("models response is not JSON".to_string()))?;
        parse_model_catalog(&value)
    }

    /// Refresh the profile's OAuth material against the pinned token endpoint. The caller owns the
    /// credential lock (single-flight) and durable persistence of the rotated refresh token.
    pub(crate) async fn refresh(
        &self,
        token_uri: &str,
        client_id: &str,
        refresh_token: &str,
    ) -> Result<TokenRefresh, ProcessError> {
        let form = serde_urlencoded::to_string([
            ("client_id", client_id),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ])
        .map_err(|_| ProcessError::Protocol("refresh form unencodable".to_string()))?;
        let response = self
            .client
            .post(token_uri)
            .header(
                "content-type",
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .header("user-agent", self.cfg.user_agent())
            .header("originator", codex_credential::CODEX_ORIGINATOR)
            .header("accept", "application/json")
            .body(form)
            .send()
            .await
            .map_err(|error| {
                if error.is_timeout() {
                    ProcessError::Timeout("token refresh")
                } else {
                    ProcessError::Closed
                }
            })?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                400 | 401 | 403 => ProcessError::AuthenticationRequired,
                429 => ProcessError::UsageLimitExceeded {
                    retry_after: retry_after_seconds(response.headers()),
                },
                _ => ProcessError::Timeout("token refresh"),
            });
        }
        let body = response
            .bytes()
            .await
            .map_err(|_| ProcessError::Protocol("refresh response body unreadable".to_string()))?;
        let value: Value = serde_json::from_slice(&body)
            .map_err(|_| ProcessError::Protocol("refresh response is not JSON".to_string()))?;
        let access_token = value
            .get("access_token")
            .and_then(Value::as_str)
            .filter(|token| (8..=16_384).contains(&token.len()))
            .map(str::to_string)
            .ok_or_else(|| {
                ProcessError::Protocol("refresh response omitted access_token".into())
            })?;
        let expires_in = value
            .get("expires_in")
            .and_then(Value::as_i64)
            .filter(|seconds| (60..=86_400).contains(seconds))
            .unwrap_or(3_600);
        Ok(TokenRefresh {
            access_token,
            refresh_token: value
                .get("refresh_token")
                .and_then(Value::as_str)
                .filter(|token| !token.is_empty() && token.len() <= 16_384)
                .map(str::to_string),
            expires_in,
        })
    }
}

async fn bounded_body(response: wreq::Response) -> Option<Vec<u8>> {
    let body = response.bytes().await.ok()?;
    Some(body[..body.len().min(MAX_ERROR_BODY_BYTES)].to_vec())
}

pub(crate) fn retry_after_seconds(headers: &wreq::header::HeaderMap) -> Option<u64> {
    headers
        .get(wreq::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds > 0)
}

fn parse_model_catalog(value: &Value) -> Result<CodexModelCatalog, ProcessError> {
    let data = value
        .get("models")
        .or_else(|| value.get("data"))
        .and_then(Value::as_array)
        .ok_or_else(|| ProcessError::Protocol("models response omitted models".to_string()))?;
    let mut catalog = CodexModelCatalog::default();
    for model in data {
        let Some(id) = model.as_str().or_else(|| {
            ["id", "model", "slug"]
                .iter()
                .find_map(|key| model.get(*key).and_then(Value::as_str))
        }) else {
            continue;
        };
        catalog.models.insert(id.to_string());
        if let Some(display_name) = model
            .get("display_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| {
                !name.is_empty() && name.len() <= 256 && !name.chars().any(char::is_control)
            })
        {
            catalog
                .display_names
                .insert(id.to_string(), display_name.to_string());
        }
        // The authenticated subscription catalog can expose both the currently configured
        // `context_window` and the provider-approved `max_context_window`. OpenAI's public model
        // contract defines the latter as the total context window; reserve the reviewed output
        // ceiling separately when publishing the maximum usable input.
        if let Some(context_window) = model
            .get("max_context_window")
            .or_else(|| model.get("context_window"))
            .and_then(Value::as_u64)
            .filter(|limit| *limit > 0)
        {
            catalog
                .input_token_limits
                .insert(id.to_string(), context_window);
        }
        let has_fast_tier = ["service_tiers", "additional_speed_tiers"]
            .iter()
            .filter_map(|key| model.get(*key).and_then(Value::as_array))
            .flatten()
            .filter_map(|tier| {
                tier.as_str()
                    .or_else(|| tier.get("id").and_then(Value::as_str))
            })
            .any(|tier| matches!(tier, "priority" | "fast"));
        if has_fast_tier {
            catalog.fast_models.insert(id.to_string());
        }
    }
    Ok(catalog)
}

/// Map an upstream HTTP failure onto the pool's blame vocabulary. Status classification mirrors
/// the app-server-era `codexErrorInfo` mapping: quota and auth failures belong to the account,
/// 4xx request faults belong to the client, and everything else is an upstream fault.
fn classify_http_error(
    status: wreq::StatusCode,
    headers: wreq::header::HeaderMap,
    body: Option<Vec<u8>>,
) -> ProcessError {
    let code = body.as_deref().and_then(|body| {
        let value: Value = serde_json::from_slice(body).ok()?;
        value
            .pointer("/error/code")
            .or_else(|| value.pointer("/error/type"))
            .or_else(|| value.get("code"))
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if code.as_deref() == Some("misalignment_policy_violation") {
        return ProcessError::PolicyViolation;
    }
    match status.as_u16() {
        400 | 404 | 409 | 422 => match code.as_deref() {
            Some("context_length_exceeded" | "context_window_exceeded") => {
                ProcessError::ContextWindowExceeded
            }
            _ => ProcessError::BadRequest,
        },
        401 => ProcessError::AuthenticationRequired,
        403 => match code.as_deref() {
            Some("unsupported_country_region_territory" | "account_deactivated") => {
                ProcessError::SubscriptionRequired
            }
            _ => ProcessError::AuthenticationRequired,
        },
        429 => ProcessError::UsageLimitExceeded {
            retry_after: retry_after_seconds(&headers),
        },
        408 | 425 => ProcessError::Timeout("upstream request"),
        _ => ProcessError::Timeout("upstream"),
    }
}

/// Harvest a rate-limit snapshot from response headers. Header names are the best-known official
/// set; the live wire probe (`research/CODEX_NATIVE_WIRE.md`) pins the exact spelling, and every
/// candidate is optional so an absent header never breaks a turn.
async fn note_header_rate_limits(
    headers: &wreq::header::HeaderMap,
    rate_limits: &Arc<Mutex<Option<CodexRateLimits>>>,
) {
    let get = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };
    let window = |prefix: &str| -> Option<CodexRateLimitWindow> {
        let used_fraction_units = get(&format!("x-codex-{prefix}-used-percent"))
            .and_then(|value| parse_used_percent_units(&value))?;
        Some(CodexRateLimitWindow {
            used_fraction_units,
            used_percent: rounded_percent(used_fraction_units),
            window_duration_mins: get(&format!("x-codex-{prefix}-window-minutes"))
                .and_then(|value| value.parse::<i64>().ok()),
            resets_at: get(&format!("x-codex-{prefix}-reset-at"))
                .and_then(|value| value.parse::<i64>().ok())
                .or_else(|| {
                    get(&format!("x-codex-{prefix}-reset-after-seconds"))
                        .or_else(|| get(&format!("x-codex-{prefix}-resets-in-seconds")))
                        .and_then(|value| value.parse::<i64>().ok())
                        .map(|seconds| pool::now().saturating_add(seconds))
                }),
        })
    };
    let primary = window("primary");
    let secondary = window("secondary");
    if primary.is_none() && secondary.is_none() {
        return;
    }
    let reached = get("x-codex-rate-limit-reached")
        .map(|value| value == "true" || value == "1")
        .unwrap_or(false);
    let mut guard = rate_limits.lock().await;
    *guard = Some(CodexRateLimits {
        primary,
        secondary,
        reached,
        observed_at: pool::now(),
    });
}

/// Parse the `/wham/usage` account response (verified live shape):
/// `rate_limit: {allowed, limit_reached, primary_window: {used_percent, limit_window_seconds,
/// reset_after_seconds, reset_at}, secondary_window|null}`. Legacy spellings
/// (`rate_limits`, `primary`/`secondary` with `window_minutes`/`resets_in_seconds`) remain as
/// fallback; unknown extra fields (`additional_rate_limits`, credits, spend control) are ignored
/// for now — none of the pinned billing models maps to a separate metered family.
fn parse_usage_response(value: &Value) -> Option<CodexRateLimits> {
    let limits = value
        .get("rate_limit")
        .or_else(|| value.get("rate_limits"))?;
    let window_of = |window: &Value| -> Option<CodexRateLimitWindow> {
        let used_fraction_units = window
            .get("used_percent")?
            .as_number()
            .and_then(|number| parse_used_percent_units(&number.to_string()))?;
        Some(CodexRateLimitWindow {
            used_fraction_units,
            used_percent: rounded_percent(used_fraction_units),
            window_duration_mins: window
                .get("limit_window_seconds")
                .and_then(Value::as_i64)
                .map(|seconds| seconds / 60)
                .or_else(|| {
                    window
                        .get("window_minutes")
                        .or_else(|| window.get("window_duration_mins"))
                        .and_then(Value::as_i64)
                }),
            resets_at: window
                .get("reset_at")
                .or_else(|| window.get("resets_at"))
                .and_then(Value::as_i64)
                .or_else(|| {
                    window
                        .get("reset_after_seconds")
                        .or_else(|| window.get("resets_in_seconds"))
                        .and_then(Value::as_i64)
                        .map(|seconds| pool::now().saturating_add(seconds))
                }),
        })
    };
    let mut primary = limits.get("primary_window").and_then(window_of);
    let mut secondary = limits.get("secondary_window").and_then(window_of);
    if primary.is_none() && secondary.is_none() {
        primary = limits.get("primary").and_then(window_of);
        secondary = limits.get("secondary").and_then(window_of);
    }
    if primary.is_none() && secondary.is_none() {
        return None;
    }
    // The provider's own verdict: a window at 100% with `allowed: true` still serves (verified
    // live), so only an explicit limit verdict or a false allowance hard-excludes a home.
    let reached = limits
        .get("limit_reached")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || matches!(limits.get("allowed").and_then(Value::as_bool), Some(false));
    Some(CodexRateLimits {
        primary,
        secondary,
        reached,
        observed_at: pool::now(),
    })
}

/// Parse a provider percentage into 10^-8 fraction units (10^-6 percentage points) without f64.
/// More precise decimals are rounded half-up to the durable resolution. Scientific notation is
/// accepted because serde_json canonically renders small valid JSON numbers that way; hostile
/// leading signs, malformed digits and values outside 0..=100 fail closed instead of being clamped.
fn parse_used_percent_units(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with(['+', '-']) {
        return None;
    }
    let mut exponent_parts = raw.split(['e', 'E']);
    let mantissa = exponent_parts.next()?;
    let exponent = exponent_parts
        .next()
        .map_or(Some(0), parse_decimal_exponent)?;
    if exponent_parts.next().is_some() {
        return None;
    }

    let mut parts = mantissa.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let coefficient = format!("{whole}{fraction}").parse::<i128>().ok()?;
    let fraction_digits = i32::try_from(fraction.len()).ok()?;
    let shift = exponent.checked_add(6)?.checked_sub(fraction_digits)?;
    let units = if shift >= 0 {
        if coefficient == 0 {
            0
        } else {
            coefficient.checked_mul(10i128.checked_pow(shift as u32)?)?
        }
    } else {
        let divisor_power = shift.unsigned_abs();
        if divisor_power > 38 {
            0
        } else {
            let divisor = 10i128.checked_pow(divisor_power)?;
            let quotient = coefficient / divisor;
            let remainder = coefficient % divisor;
            quotient.checked_add(i128::from(remainder >= divisor / 2))?
        }
    };
    i64::try_from(units)
        .ok()
        .filter(|units| (0..=RATE_LIMIT_FRACTION_SCALE).contains(units))
}

fn parse_decimal_exponent(raw: &str) -> Option<i32> {
    let (negative, digits) = if let Some(rest) = raw.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = raw.strip_prefix('+') {
        (false, rest)
    } else {
        (false, raw)
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<i32>().ok()?;
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

fn rounded_percent(used_fraction_units: i64) -> i64 {
    used_fraction_units.saturating_add(RATE_LIMIT_PERCENT_SCALE / 2) / RATE_LIMIT_PERCENT_SCALE
}

/// Convert upstream snake_case usage into the camelCase shape the runner's `CodexUsage` parser
/// already owns. Kept in one place so the runner never learns wire spelling.
fn translate_usage(usage: &Value) -> Value {
    let details = |root: &Value, key: &str| root.get(key).and_then(Value::as_u64).unwrap_or(0);
    let input_details = usage
        .get("input_tokens_details")
        .cloned()
        .unwrap_or(Value::Null);
    let output_details = usage
        .get("output_tokens_details")
        .cloned()
        .unwrap_or(Value::Null);
    // Native deployments have used both spellings. They are aliases for one subset, never two
    // additive buckets; prefer the current public spelling when both appear during a rollout.
    let cache_write_tokens = input_details
        .get("cache_write_tokens")
        .and_then(Value::as_u64)
        .or_else(|| {
            input_details
                .get("cache_creation_tokens")
                .and_then(Value::as_u64)
        })
        .unwrap_or(0);
    json!({
        "inputTokens": details(usage, "input_tokens"),
        "cachedInputTokens": details(&input_details, "cached_tokens"),
        "cacheWriteInputTokens": cache_write_tokens,
        "outputTokens": details(usage, "output_tokens"),
        "reasoningOutputTokens": details(&output_details, "reasoning_tokens"),
        "totalTokens": details(usage, "total_tokens"),
    })
}

/// Map an upstream error code onto the app-server-era `codexErrorInfo` vocabulary the runner's
/// turn-error classifier already consumes.
fn codex_error_info(code: Option<&str>) -> Value {
    match code {
        Some("rate_limit_exceeded" | "usage_limit_exceeded" | "insufficient_quota") => {
            Value::String("usageLimitExceeded".to_string())
        }
        Some("context_length_exceeded" | "context_window_exceeded") => {
            Value::String("contextWindowExceeded".to_string())
        }
        Some("misalignment_policy_violation") => {
            Value::String("misalignmentPolicyViolation".to_string())
        }
        Some("invalid_api_key" | "unauthorized" | "authentication_error") => {
            Value::String("unauthorized".to_string())
        }
        Some("invalid_request_error" | "bad_request") => Value::String("badRequest".to_string()),
        _ => Value::Null,
    }
}

/// Read one SSE stream, translating `response.*` events into runner notifications. Terminal
/// events (`response.completed`/`response.failed`/`response.incomplete`) close the turn; a
/// truncated stream publishes `Closed` out of band so a cut mid-delta is never mistaken for a
/// completed turn.
async fn read_sse_stream(
    response: wreq::Response,
    sender: mpsc::Sender<AppServerEvent>,
    closed: watch::Sender<Option<ProcessError>>,
    rate_limits: Arc<Mutex<Option<CodexRateLimits>>>,
) {
    let terminal = drive_sse_stream(response, &sender, &rate_limits).await;
    // Deliver everything accepted so far before publishing the terminal state: `recv` re-checks
    // the queue after observing `closed`, so this ordering is what keeps a final delta visible.
    drop(sender);
    // Publish the terminal state in BOTH outcomes. Without this a finished producer silently
    // drops the watch sender, and `recv`'s changed() arm fails the turn with a spurious Closed
    // even though the terminal event already reached the queue.
    match terminal {
        Ok(()) => {
            closed.send_replace(None);
        }
        Err(error) => {
            closed.send_replace(Some(error));
        }
    }
}

async fn drive_sse_stream(
    response: wreq::Response,
    sender: &mpsc::Sender<AppServerEvent>,
    rate_limits: &Arc<Mutex<Option<CodexRateLimits>>>,
) -> Result<(), ProcessError> {
    use futures_util::StreamExt;
    let mut stream = response.bytes_stream();
    let mut buffer: Vec<u8> = Vec::with_capacity(16_384);
    let mut event_name = String::new();
    let mut data_lines: Vec<String> = Vec::new();
    let mut protocol = SseProtocolState::default();
    loop {
        let chunk = match stream.next().await {
            Some(Ok(chunk)) => chunk,
            Some(Err(error)) => {
                return Err(if error.is_timeout() {
                    ProcessError::Timeout("turn silence")
                } else {
                    ProcessError::Closed
                });
            }
            None => {
                // EOF with a pending event: a well-formed backend ends with a blank line, but a
                // trailing terminal event without it is still a valid turn boundary.
                if !data_lines.is_empty() {
                    let data = data_lines.join("\n");
                    data_lines.clear();
                    let name = std::mem::take(&mut event_name);
                    if dispatch_sse_event(&name, &data, sender, rate_limits, &mut protocol).await? {
                        return Ok(());
                    }
                }
                return Err(ProcessError::Closed);
            }
        };
        if sse_chunk_exceeds_bound(buffer.len(), chunk.len()) {
            return Err(ProcessError::Protocol(
                "SSE event exceeded bound".to_string(),
            ));
        }
        buffer.extend_from_slice(&chunk);
        while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = buffer.drain(..=newline).collect();
            let line = std::str::from_utf8(&line)
                .map_err(|_| ProcessError::Protocol("SSE line is not UTF-8".to_string()))?
                .trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !data_lines.is_empty() {
                    let data = data_lines.join("\n");
                    data_lines.clear();
                    let name = std::mem::take(&mut event_name);
                    if dispatch_sse_event(&name, &data, sender, rate_limits, &mut protocol).await? {
                        // A completed turn's stream is done: the official backend closes after
                        // the terminal event, and waiting out an idle keep-alive would only
                        // delay settlement.
                        return Ok(());
                    }
                }
                continue;
            }
            if let Some(comment) = line.strip_prefix(':') {
                let _ = comment;
                continue;
            }
            if let Some(value) = line.strip_prefix("event:") {
                event_name = value.trim().to_string();
            } else if let Some(value) = line.strip_prefix("data:") {
                data_lines.push(value.trim_start().to_string());
            }
        }
    }
}

/// Translate one SSE event. Returns `true` when the turn reached a terminal state.
async fn dispatch_sse_event(
    event: &str,
    data: &str,
    sender: &mpsc::Sender<AppServerEvent>,
    rate_limits: &Arc<Mutex<Option<CodexRateLimits>>>,
    protocol: &mut SseProtocolState,
) -> Result<bool, ProcessError> {
    let payload: Value = match serde_json::from_str(data) {
        Ok(payload) => payload,
        Err(_) => {
            if event.is_empty() {
                elog::warn("codex", "codex upstream SSE event not JSON; skipped");
            } else {
                elog::warn(
                    "codex",
                    format!("codex upstream SSE event {event} not JSON; skipped"),
                );
            }
            return Ok(false);
        }
    };
    if !protocol.accept_sequence(&payload) {
        return Ok(false);
    }
    let kind = payload.get("type").and_then(Value::as_str).unwrap_or(event);
    macro_rules! notify {
        ($method:expr, $params:expr $(,)?) => {
            let _ = sender
                .send(AppServerEvent::Notification {
                    method: $method.to_string(),
                    params: $params,
                })
                .await;
        };
    }
    match kind {
        "response.output_text.delta" => {
            let item_id = protocol.event_item_id(&payload, "msg");
            let delta = payload.get("delta").and_then(Value::as_str).unwrap_or("");
            notify!(
                "item/agentMessage/delta",
                json!({"itemId": item_id, "delta": delta}),
            );
        }
        "response.reasoning_summary_part.added" => {
            let item_id = protocol.event_item_id(&payload, "rs");
            notify!(
                "item/reasoning/summaryPartAdded",
                json!({
                    "itemId": item_id,
                    "summaryIndex": payload.get("summary_index").and_then(Value::as_u64).unwrap_or(0),
                }),
            );
        }
        "response.reasoning_summary_text.delta" => {
            let item_id = protocol.event_item_id(&payload, "rs");
            notify!(
                "item/reasoning/summaryTextDelta",
                json!({
                    "itemId": item_id,
                    "summaryIndex": payload.get("summary_index").and_then(Value::as_u64).unwrap_or(0),
                    "delta": payload.get("delta").and_then(Value::as_str).unwrap_or(""),
                }),
            );
        }
        "response.reasoning_text.delta" => {
            // Raw reasoning text is hidden chain-of-thought; the runner ignores this method.
            let item_id = protocol.event_item_id(&payload, "rs");
            notify!(
                "item/reasoning/textDelta",
                json!({
                    "itemId": item_id,
                    "delta": payload.get("delta").and_then(Value::as_str).unwrap_or(""),
                }),
            );
        }
        "response.output_item.added" => {
            // The public event precedes content deltas and therefore owns the first canonical
            // output-index identity even though the runner does not otherwise consume it.
            let _ = protocol.normalize_output_item(&payload);
        }
        "response.output_item.done" => {
            if let Some(item) = protocol.normalize_output_item(&payload) {
                notify!("rawResponseItem/completed", json!({"item": item}));
            }
        }
        "response.completed" => {
            let Some(usage) = payload.pointer("/response/usage").filter(|v| !v.is_null()) else {
                return Err(ProcessError::MissingAuthoritativeUsage);
            };
            notify!(
                "rawResponse/completed",
                json!({"usage": translate_usage(usage)})
            );
            // Preserve the completed-response tier as a wire diagnostic. ChatGPT-authenticated
            // Codex commonly reports `default` for measurably accelerated Fast turns, so the
            // runner deliberately derives the effective tier from the accepted request.
            let provider_reported_tier = payload
                .pointer("/response/service_tier")
                .and_then(Value::as_str)
                .unwrap_or("default");
            notify!(
                "turn/completed",
                json!({"turn": {"status": "completed", "serviceTier": provider_reported_tier}})
            );
            return Ok(true);
        }
        "response.failed" | "response.incomplete" | "error" => {
            let error = payload
                .pointer("/response/error")
                .or_else(|| payload.get("error"))
                .cloned()
                .unwrap_or(Value::Null);
            let code = error.get("code").and_then(Value::as_str);
            if code == Some("misalignment_policy_violation") {
                return Err(ProcessError::PolicyViolation);
            }
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("model turn failed");
            notify!(
                "error",
                json!({
                    "willRetry": false,
                    "error": {"message": message, "codexErrorInfo": codex_error_info(code)},
                }),
            );
            notify!(
                "turn/completed",
                json!({"turn": {"status": "failed", "error": {"message": message,
                    "codexErrorInfo": codex_error_info(code)}}}),
            );
            return Ok(true);
        }
        "codex.rate_limits" => {
            if let Some(limits) = parse_usage_response(&payload) {
                *rate_limits.lock().await = Some(limits);
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Stable opaque UUIDs for the official session/thread/window metadata. They are derived from the
/// already tenant-scoped cache key, so rotation keeps one conversation identity without exposing
/// any customer key or raw session value to the provider.
fn stable_uuid(scope: &str, cache_key: &str) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"claude-api/codex-wire-identity/v1\0");
    hasher.update(scope.as_bytes());
    hasher.update(b"\0");
    hasher.update(cache_key.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format_uuid(bytes)
}

/// Random UUID for request-scoped identities when no tenant cache key exists.
fn uuid() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        return format!("session-{}", crate::upstream::fresh_request_id());
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format_uuid(bytes)
}

fn format_uuid(bytes: [u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (index, byte) in bytes.iter().enumerate() {
        if matches!(index, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_translation_keeps_the_runner_vocabulary() {
        let upstream = json!({
            "input_tokens": 100,
            "input_tokens_details": {"cached_tokens": 40, "cache_write_tokens": 11},
            "output_tokens": 20,
            "output_tokens_details": {"reasoning_tokens": 5},
            "total_tokens": 120,
        });
        let translated = translate_usage(&upstream);
        assert_eq!(translated["inputTokens"], 100);
        assert_eq!(translated["cachedInputTokens"], 40);
        assert_eq!(translated["cacheWriteInputTokens"], 11);
        assert_eq!(translated["outputTokens"], 20);
        assert_eq!(translated["reasoningOutputTokens"], 5);
        assert_eq!(translated["totalTokens"], 120);
    }

    #[test]
    fn usage_translation_accepts_legacy_cache_creation_without_double_counting_aliases() {
        let legacy = translate_usage(&json!({
            "input_tokens": 10,
            "input_tokens_details": {"cache_creation_tokens": 3},
            "output_tokens": 1,
        }));
        assert_eq!(legacy["cacheWriteInputTokens"], 3);

        let overlap = translate_usage(&json!({
            "input_tokens": 10,
            "input_tokens_details": {
                "cache_write_tokens": 4,
                "cache_creation_tokens": 99
            },
            "output_tokens": 1,
        }));
        assert_eq!(overlap["cacheWriteInputTokens"], 4);
    }

    #[test]
    fn rate_limit_percent_parser_is_exact_bounded_and_fixed_point() {
        assert_eq!(parse_used_percent_units("0"), Some(0));
        assert_eq!(parse_used_percent_units("42.1256784"), Some(42_125_678));
        assert_eq!(parse_used_percent_units("42.1256785"), Some(42_125_679));
        assert_eq!(parse_used_percent_units("99.9999999"), Some(100_000_000));
        assert_eq!(parse_used_percent_units("100.000001"), None);
        assert_eq!(parse_used_percent_units("1e-6"), Some(1));
        assert_eq!(parse_used_percent_units("5e-7"), Some(1));
        assert_eq!(parse_used_percent_units("4e-7"), Some(0));
        assert_eq!(parse_used_percent_units("1e2"), Some(100_000_000));
        let smallest_durable_json_number = json!(0.000001)
            .as_number()
            .expect("JSON number")
            .to_string();
        assert_eq!(
            parse_used_percent_units(&smallest_durable_json_number),
            Some(1)
        );
        for invalid in [
            "",
            "-1",
            "+1",
            "1e",
            "e1",
            "1e999999999999",
            "NaN",
            "101",
            ".5",
            "1.2.3",
        ] {
            assert_eq!(parse_used_percent_units(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn model_catalog_preserves_current_and_legacy_fast_capabilities() {
        let catalog = parse_model_catalog(&json!({
            "models": [
                {
                    "slug": "gpt-current",
                    "display_name": "GPT Current",
                    "context_window": 272000,
                    "max_context_window": 1050000,
                    "service_tiers": [{"id": "priority", "name": "Fast"}],
                    "additional_speed_tiers": []
                },
                {
                    "model": "gpt-legacy",
                    "service_tiers": [],
                    "additional_speed_tiers": ["fast"]
                },
                {"id": "gpt-standard", "service_tiers": [{"id": "default"}]},
                "gpt-string"
            ]
        }))
        .unwrap();
        assert_eq!(catalog.models.len(), 4);
        assert!(catalog.fast_models.contains("gpt-current"));
        assert!(catalog.fast_models.contains("gpt-legacy"));
        assert!(!catalog.fast_models.contains("gpt-standard"));
        assert!(!catalog.fast_models.contains("gpt-string"));
        assert_eq!(catalog.input_token_limits["gpt-current"], 1_050_000);
        assert_eq!(catalog.display_names["gpt-current"], "GPT Current");
        assert!(!catalog.input_token_limits.contains_key("gpt-standard"));
    }

    #[test]
    fn model_catalog_omits_non_positive_or_non_integer_context_metadata() {
        let catalog = parse_model_catalog(&json!({
            "models": [
                {"slug": "valid", "context_window": 1},
                {"slug": "valid-maximum", "context_window": 272000, "max_context_window": 1050000},
                {"slug": "zero", "context_window": 0},
                {"slug": "negative", "context_window": -1},
                {"slug": "string", "context_window": "272000"}
            ]
        }))
        .unwrap();
        assert_eq!(catalog.input_token_limits.len(), 2);
        assert_eq!(catalog.input_token_limits["valid"], 1);
        assert_eq!(catalog.input_token_limits["valid-maximum"], 1_050_000);
    }

    #[test]
    fn model_catalog_omits_unsafe_display_names() {
        let catalog = parse_model_catalog(&json!({
            "models": [
                {"slug": "empty", "display_name": "   "},
                {"slug": "control", "display_name": "GPT\nInjected"},
                {"slug": "too-long", "display_name": "x".repeat(257)},
                {"slug": "valid", "display_name": "  GPT Valid  "}
            ]
        }))
        .unwrap();
        assert_eq!(catalog.display_names.len(), 1);
        assert_eq!(catalog.display_names["valid"], "GPT Valid");
    }

    #[test]
    fn model_catalog_requires_a_models_array() {
        assert!(matches!(
            parse_model_catalog(&json!({"object": "list"})),
            Err(ProcessError::Protocol(_))
        ));
    }

    #[test]
    fn usage_response_parses_both_reset_spellings() {
        let value = json!({
            "plan_type": "plus",
            "rate_limit": {
                "primary": {"used_percent": 42.1256785, "window_minutes": 300, "resets_in_seconds": 60},
                "secondary": {"used_percent": 6.0, "window_minutes": 10080, "resets_at": 4_102_444_800i64}
            }
        });
        let limits = parse_usage_response(&value).unwrap();
        assert_eq!(limits.primary.as_ref().unwrap().used_percent, 42);
        assert_eq!(
            limits.primary.as_ref().unwrap().used_fraction_units,
            42_125_679
        );
        assert!(limits.primary.as_ref().unwrap().resets_at.unwrap() > 0);
        assert_eq!(
            limits.secondary.as_ref().unwrap().resets_at,
            Some(4_102_444_800)
        );
        assert!(!limits.reached);
    }

    #[test]
    fn usage_response_parses_the_verified_live_shape() {
        // Exact shape recorded by the 2026-07-31 live probe on a ChatGPT Pro profile.
        let value = json!({
            "plan_type": "pro",
            "rate_limit": {
                "allowed": true,
                "limit_reached": false,
                "primary_window": {
                    "used_percent": 100,
                    "limit_window_seconds": 604800,
                    "reset_after_seconds": 383348,
                    "reset_at": 4_102_444_800i64
                },
                "secondary_window": null
            },
            "additional_rate_limits": [],
            "credits": {"balance": "0"}
        });
        let limits = parse_usage_response(&value).unwrap();
        let primary = limits.primary.as_ref().unwrap();
        assert_eq!(primary.used_percent, 100);
        assert_eq!(primary.window_duration_mins, Some(10_080));
        assert_eq!(primary.resets_at, Some(4_102_444_800));
        assert!(limits.secondary.is_none());
        // 100% with allowed: true must NOT mark the home as reached (it still serves).
        assert!(!limits.reached);
        let reached = parse_usage_response(&json!({
            "rate_limit": {"allowed": false, "limit_reached": true,
                "primary_window": {"used_percent": 100, "limit_window_seconds": 604800,
                    "reset_at": 4_102_444_800i64}}
        }))
        .unwrap();
        assert!(reached.reached);
    }

    #[test]
    fn http_error_classification_keeps_blame_axes() {
        let headers = wreq::header::HeaderMap::new();
        assert_eq!(
            classify_http_error(wreq::StatusCode::UNAUTHORIZED, headers.clone(), None),
            ProcessError::AuthenticationRequired
        );
        assert!(matches!(
            classify_http_error(wreq::StatusCode::TOO_MANY_REQUESTS, headers.clone(), None),
            ProcessError::UsageLimitExceeded { .. }
        ));
        assert_eq!(
            classify_http_error(wreq::StatusCode::BAD_REQUEST, headers.clone(), None),
            ProcessError::BadRequest
        );
        let context =
            serde_json::to_vec(&json!({"error": {"code": "context_length_exceeded"}})).unwrap();
        assert_eq!(
            classify_http_error(
                wreq::StatusCode::BAD_REQUEST,
                headers.clone(),
                Some(context)
            ),
            ProcessError::ContextWindowExceeded
        );
        let policy = serde_json::to_vec(&json!({
            "error": {"code": "misalignment_policy_violation"}
        }))
        .unwrap();
        assert_eq!(
            classify_http_error(
                wreq::StatusCode::FORBIDDEN,
                headers.clone(),
                Some(policy.clone())
            ),
            ProcessError::PolicyViolation
        );
        assert_eq!(
            classify_http_error(wreq::StatusCode::BAD_REQUEST, headers.clone(), Some(policy)),
            ProcessError::PolicyViolation
        );
        assert!(matches!(
            classify_http_error(wreq::StatusCode::INTERNAL_SERVER_ERROR, headers, None),
            ProcessError::Timeout(_)
        ));
    }

    #[test]
    fn codex_error_info_maps_only_reviewed_codes() {
        assert_eq!(
            codex_error_info(Some("misalignment_policy_violation")),
            Value::String("misalignmentPolicyViolation".to_string())
        );
        assert_eq!(
            codex_error_info(Some("rate_limit_exceeded")),
            Value::String("usageLimitExceeded".to_string())
        );
        assert_eq!(
            codex_error_info(Some("context_length_exceeded")),
            Value::String("contextWindowExceeded".to_string())
        );
        assert_eq!(codex_error_info(Some("something_new")), Value::Null);
        assert_eq!(codex_error_info(None), Value::Null);
    }

    /// Minimal HTTP/1.1 mock of the native backend: serves canned SSE for `POST …/responses`
    /// and a usage JSON for `GET …/wham/usage`. One connection per request.
    async fn mock_upstream<F>(respond: F) -> String
    where
        F: Fn(&str, &str) -> (u16, String, Vec<(&'static str, String)>) + Send + Sync + 'static,
    {
        let respond = std::sync::Arc::new(respond);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let respond = respond.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut head: Vec<u8> = Vec::new();
                    // Read the request head (tests keep requests small).
                    loop {
                        if head.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                        let mut chunk = [0u8; 4096];
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            break;
                        }
                        head.extend_from_slice(&chunk[..n]);
                        if head.len() > 1_000_000 {
                            return;
                        }
                    }
                    let request = String::from_utf8_lossy(&head).to_string();
                    let first = request.lines().next().unwrap_or("").to_string();
                    let (status, body, extra) = respond(&first, &request);
                    let reason = match status {
                        200 => "OK",
                        400 => "Bad Request",
                        401 => "Unauthorized",
                        429 => "Too Many Requests",
                        500 => "Internal Server Error",
                        _ => "Status",
                    };
                    let mut response = format!(
                        "HTTP/1.1 {status} {reason}\r\ncontent-length: {}\r\nconnection: close\r\n",
                        body.len()
                    );
                    for (name, value) in extra {
                        response.push_str(&format!("{name}: {value}\r\n"));
                    }
                    response.push_str("\r\n");
                    response.push_str(&body);
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    fn test_config(base: &str) -> Arc<CodexConfig> {
        Arc::new(CodexConfig {
            smooth_wait_ms: 0,
            enabled: true,
            base_url: format!("{base}/codex"),
            profiles_file: "/tmp/roster.json".to_string(),
            credential_keys: codex_credential::CredentialKeyring::parse(&format!(
                "current:{}",
                "11".repeat(32)
            ))
            .unwrap(),
            cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
            request_timeout_ms: 5_000,
            turn_timeout_ms: 5_000,
            turn_silence_timeout_ms: 5_000,
            health_probe_interval_secs: 300,
            reserve_5h: 0.02,
            reserve_7d: 0.03,
            reserve_jitter: 0.0,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: None,
            history_redis_timeout_ms: 10,
            default_proxy_env: Default::default(),
            models: Vec::new(),
        })
    }

    fn test_auth() -> AuthContext {
        AuthContext {
            access_token: codex_credential::SecretString::new("test-access".to_string()),
            account_id: "acct_test_1".to_string(),
        }
    }

    #[test]
    fn wire_identity_matches_the_pinned_official_request_shape() {
        let cfg = test_config("http://127.0.0.1:1");
        let transport = ProfileTransport::new(cfg.clone(), None).unwrap();
        let identity = transport.request_identity("session-test");
        let repeated = transport.request_identity("session-test");
        let headers = transport.turn_headers(&test_auth(), &identity);
        let mut body = json!({"model": "gpt-5.6-sol"});
        transport.attach_client_metadata(&mut body, &identity);

        assert_eq!(
            headers.get("version").and_then(|value| value.to_str().ok()),
            Some(cfg.cli_version.as_str())
        );
        assert!(headers
            .get("user-agent")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains(cfg.cli_version.as_str())));
        assert_eq!(identity.session_id, repeated.session_id);
        assert_eq!(identity.thread_id, repeated.thread_id);
        assert_eq!(identity.window_id, repeated.window_id);
        assert_eq!(identity.session_id, identity.thread_id);
        assert_ne!(identity.turn_id, repeated.turn_id);
        for (header, expected) in [
            ("session-id", identity.session_id.as_str()),
            ("thread-id", identity.thread_id.as_str()),
            ("x-client-request-id", identity.thread_id.as_str()),
            ("x-codex-window-id", identity.window_id.as_str()),
            ("x-codex-turn-metadata", identity.turn_metadata.as_str()),
        ] {
            assert_eq!(
                headers.get(header).and_then(|value| value.to_str().ok()),
                Some(expected),
                "header {header}"
            );
        }
        assert_eq!(
            body["client_metadata"],
            json!({
                "x-codex-installation-id": transport.installation_id,
                "session_id": identity.session_id,
                "thread_id": identity.thread_id,
                "turn_id": identity.turn_id,
                "x-codex-window-id": identity.window_id,
                "x-codex-turn-metadata": identity.turn_metadata,
            })
        );
        assert!(headers.get("session_id").is_none());
        assert!(headers.get("openai-beta").is_none());
        assert!(headers.get("x-codex-installation-id").is_none());
        let turn_metadata: Value = serde_json::from_str(
            body["client_metadata"]["x-codex-turn-metadata"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(turn_metadata["request_kind"], "turn");
        assert_eq!(turn_metadata["session_id"], identity.session_id);
        assert_eq!(turn_metadata["thread_id"], identity.thread_id);
        assert_eq!(turn_metadata["turn_id"], identity.turn_id);
        assert_eq!(turn_metadata["window_id"], identity.window_id);
        assert_eq!(turn_metadata["installation_id"], transport.installation_id);
    }

    fn sse_body(events: &[(&str, &str)]) -> String {
        events
            .iter()
            .map(|(event, data)| format!("event: {event}\ndata: {data}\n\n"))
            .collect()
    }

    async fn collect(events: &mut TurnEvents) -> Vec<(String, Value)> {
        let mut out = Vec::new();
        loop {
            match events.recv().await {
                Ok(AppServerEvent::Notification { method, params }) => out.push((method, params)),
                Err(_) => break,
            }
        }
        out
    }

    #[tokio::test]
    async fn response_completed_without_usage_is_rejected_before_turn_completion() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[(
                "response.completed",
                r#"{"response":{"status":"completed"}}"#,
            )]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let error = events.recv().await.expect_err("missing usage must fail");
        assert_eq!(error, ProcessError::MissingAuthoritativeUsage);
    }

    #[tokio::test]
    async fn response_failed_misalignment_is_typed_and_non_retryable() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[(
                "response.failed",
                r#"{"response":{"error":{"code":"misalignment_policy_violation","message":"blocked"}}}"#,
            )]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let error = events.recv().await.expect_err("policy failure must be typed");
        assert_eq!(error, ProcessError::PolicyViolation);
    }

    #[tokio::test]
    async fn turn_streams_deltas_items_usage_and_closes() {
        let base = mock_upstream(|first, _| {
            assert!(first.starts_with("POST /codex/responses"));
            let body = sse_body(&[
                ("response.output_text.delta", r#"{"item_id":"msg_1","delta":"hel"}"#),
                ("response.output_text.delta", r#"{"item_id":"msg_1","delta":"lo"}"#),
                ("response.output_item.done", r#"{"item":{"type":"message","id":"msg_1","role":"assistant","content":[{"type":"output_text","text":"hello"}]}}"#),
                ("response.completed", r#"{"response":{"usage":{"input_tokens":11,"input_tokens_details":{"cached_tokens":3},"output_tokens":5,"output_tokens_details":{"reasoning_tokens":2},"total_tokens":16}}}"#),
            ]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let limits = Arc::new(Mutex::new(None));
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                limits,
                None,
            )
            .await
            .unwrap();
        let seen = collect(&mut events).await;
        let methods: Vec<&str> = seen.iter().map(|(method, _)| method.as_str()).collect();
        assert_eq!(
            methods,
            vec![
                "item/agentMessage/delta",
                "item/agentMessage/delta",
                "rawResponseItem/completed",
                "rawResponse/completed",
                "turn/completed",
            ]
        );
        let usage = &seen[3].1["usage"];
        assert_eq!(usage["inputTokens"], 11);
        assert_eq!(usage["cachedInputTokens"], 3);
        assert_eq!(usage["reasoningOutputTokens"], 2);
    }

    #[tokio::test]
    async fn duplicate_sequence_and_item_id_drift_produce_one_logical_message() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[
                (
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","sequence_number":10,"output_index":0,"delta":"only "}"#,
                ),
                (
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","sequence_number":11,"output_index":0,"item_id":"msg_changed","delta":"once"}"#,
                ),
                // A transport replay keeps the protocol sequence number. It must not become a
                // second public delta.
                (
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","sequence_number":11,"output_index":0,"item_id":"msg_changed","delta":"once"}"#,
                ),
                (
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","sequence_number":12,"output_index":0,"item":{"type":"message","id":"msg_final","role":"assistant","content":[{"type":"output_text","text":"only once"}]}}"#,
                ),
                (
                    "response.completed",
                    r#"{"type":"response.completed","sequence_number":13,"response":{"usage":{"input_tokens":1,"output_tokens":2,"total_tokens":3}}}"#,
                ),
            ]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let seen = collect(&mut events).await;
        let deltas = seen
            .iter()
            .filter(|(method, _)| method == "item/agentMessage/delta")
            .map(|(_, params)| {
                (
                    params["itemId"].as_str().unwrap(),
                    params["delta"].as_str().unwrap(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(
            deltas,
            vec![("msg_stream_0", "only "), ("msg_stream_0", "once")]
        );
        assert_eq!(
            deltas.iter().map(|(_, delta)| *delta).collect::<String>(),
            "only once"
        );
        let completed_item = seen
            .iter()
            .find(|(method, _)| method == "rawResponseItem/completed")
            .unwrap();
        assert_eq!(completed_item.1["item"]["id"], "msg_stream_0");
    }

    #[tokio::test]
    async fn equal_text_with_distinct_sequences_is_preserved() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[
                (
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","sequence_number":1,"output_index":0,"item_id":"msg_1","delta":"ha"}"#,
                ),
                (
                    "response.output_text.delta",
                    r#"{"type":"response.output_text.delta","sequence_number":2,"output_index":0,"item_id":"msg_1","delta":"ha"}"#,
                ),
                (
                    "response.output_item.done",
                    r#"{"type":"response.output_item.done","sequence_number":3,"output_index":0,"item":{"type":"message","id":"msg_1","role":"assistant","content":[{"type":"output_text","text":"haha"}]}}"#,
                ),
                (
                    "response.completed",
                    r#"{"type":"response.completed","sequence_number":4,"response":{"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#,
                ),
            ]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let seen = collect(&mut events).await;
        let text = seen
            .iter()
            .filter(|(method, _)| method == "item/agentMessage/delta")
            .filter_map(|(_, params)| params["delta"].as_str())
            .collect::<String>();
        assert_eq!(text, "haha");
    }

    #[tokio::test]
    async fn events_survive_arbitrary_chunk_splits() {
        let base = mock_upstream(|_, _| {
            // No explicit chunking needed: the mock writes the whole body at once, the transport
            // must still parse regardless of how TCP framed it. A second case splits mid-event.
            let body = sse_body(&[
                ("response.output_text.delta", r#"{"item_id":"m","delta":"x"}"#),
                ("response.completed", r#"{"response":{"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#),
            ]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let seen = collect(&mut events).await;
        assert_eq!(seen.len(), 3);
        assert_eq!(seen[2].1["turn"]["status"], "completed");
    }

    #[tokio::test]
    async fn http_failures_classify_before_any_byte_is_sent() {
        let base = mock_upstream(|first, _| {
            if first.starts_with("POST /codex/responses") {
                (
                    429,
                    r#"{"error":{"type":"rate_limit_exceeded","message":"slow down"}}"#.to_string(),
                    vec![("retry-after", "17".to_string())],
                )
            } else {
                (
                    401,
                    r#"{"error":{"code":"invalid_api_key"}}"#.to_string(),
                    vec![],
                )
            }
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let outcome = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await;
        assert!(matches!(
            outcome.err(),
            Some(ProcessError::UsageLimitExceeded {
                retry_after: Some(17)
            })
        ));
        assert_eq!(
            transport.fetch_usage(&test_auth()).await.err(),
            Some(ProcessError::AuthenticationRequired)
        );
    }

    #[tokio::test]
    async fn usage_probe_and_stream_events_feed_one_snapshot_cell() {
        let base = mock_upstream(|first, _| {
            if first.starts_with("GET /wham/usage") {
                (
                    200,
                    r#"{"plan_type":"plus","rate_limit":{"primary":{"used_percent":42.1256785,"window_minutes":300,"resets_in_seconds":3600},"secondary":{"used_percent":6.0,"window_minutes":10080,"resets_at":4102444800}}}"#.to_string(),
                    vec![("content-type", "application/json".to_string())],
                )
            } else {
                let body = sse_body(&[
                    ("codex.rate_limits", r#"{"rate_limit":{"primary":{"used_percent":55.0,"window_minutes":300,"resets_in_seconds":1800}}}"#),
                    ("response.completed", r#"{"response":{"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#),
                ]);
                (200, body, vec![("content-type", "text/event-stream".to_string())])
            }
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let limits = transport.fetch_usage(&test_auth()).await.unwrap();
        assert_eq!(limits.primary.as_ref().unwrap().used_percent, 42);
        assert_eq!(
            limits.primary.as_ref().unwrap().used_fraction_units,
            42_125_679
        );
        assert_eq!(
            limits.secondary.as_ref().unwrap().resets_at,
            Some(4_102_444_800)
        );

        let cell = Arc::new(Mutex::new(None));
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                cell.clone(),
                None,
            )
            .await
            .unwrap();
        let _ = collect(&mut events).await;
        let streamed = cell.lock().await.clone().unwrap();
        assert_eq!(streamed.primary.as_ref().unwrap().used_percent, 55);
    }

    #[tokio::test]
    async fn rate_limit_response_headers_update_the_snapshot_cell() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[(
                "response.completed",
                r#"{"response":{"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#,
            )]);
            (
                200,
                body,
                vec![
                    ("content-type", "text/event-stream".to_string()),
                    ("x-codex-primary-used-percent", "61.000001".to_string()),
                    ("x-codex-primary-window-minutes", "300".to_string()),
                    ("x-codex-primary-reset-at", "4102444800".to_string()),
                ],
            )
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let cell = Arc::new(Mutex::new(None));
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                cell.clone(),
                None,
            )
            .await
            .unwrap();
        let _ = collect(&mut events).await;
        let limits = cell.lock().await.clone().unwrap();
        assert_eq!(limits.primary.as_ref().unwrap().used_percent, 61);
        assert_eq!(
            limits.primary.as_ref().unwrap().used_fraction_units,
            61_000_001
        );
        assert_eq!(
            limits.primary.as_ref().unwrap().resets_at,
            Some(4_102_444_800)
        );
    }

    #[tokio::test]
    async fn completed_turn_carries_the_provider_reported_service_tier() {
        let base = mock_upstream(|_, _| {
            let body = sse_body(&[(
                "response.completed",
                r#"{"response":{"service_tier":"default","usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2}}}"#,
            )]);
            (200, body, vec![("content-type", "text/event-stream".to_string())])
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let mut events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5", "service_tier": "priority"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let seen = collect(&mut events).await;
        let completed = seen
            .iter()
            .find(|(method, _)| method == "turn/completed")
            .expect("turn must complete");
        assert_eq!(completed.1["turn"]["serviceTier"], "default");
    }

    #[tokio::test]
    async fn dropping_the_stream_aborts_the_upstream_read() {
        let base = mock_upstream(|_, _| {
            // Never terminates by itself: only the client abort can end this request.
            (
                200,
                "event: response.output_text.delta\ndata: {\"item_id\":\"m\",\"delta\":\"x\"}\n\n"
                    .to_string(),
                vec![("content-type", "text/event-stream".to_string())],
            )
        })
        .await;
        let transport = ProfileTransport::new(test_config(&base), None).unwrap();
        let events = transport
            .run_turn(
                &test_auth(),
                json!({"model": "gpt-5.5"}),
                None,
                Arc::new(Mutex::new(None)),
                None,
            )
            .await
            .unwrap();
        let handle = events.task.as_ref().unwrap().abort_handle();
        drop(events);
        // abort() schedules cancellation; the producer must observe it promptly.
        for _ in 0..100 {
            if handle.is_finished() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("producer task did not finish after the consumer was dropped");
    }

    #[test]
    fn sse_line_bound_rejects_384_mib_plus_one_before_allocation() {
        assert_eq!(MAX_SSE_LINE_BYTES, 384 * 1024 * 1024);
        assert!(!sse_chunk_exceeds_bound(0, MAX_SSE_LINE_BYTES));
        assert!(sse_chunk_exceeds_bound(0, MAX_SSE_LINE_BYTES + 1));
        assert!(sse_chunk_exceeds_bound(MAX_SSE_LINE_BYTES, 1));
        assert!(!sse_chunk_exceeds_bound(MAX_SSE_LINE_BYTES - 1, 1));
        assert!(sse_chunk_exceeds_bound(usize::MAX, 1));
    }
}
