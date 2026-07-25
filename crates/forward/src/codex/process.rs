//! Multiplexed newline-delimited JSON-RPC transport for `codex app-server`.

use super::{CodexConfig, CodexHomeSpec};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{mpsc, oneshot, Mutex};

const MAX_JSONRPC_LINE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ORPHAN_EVENTS_PER_THREAD: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    Disabled,
    Busy,
    InvalidConfig(String),
    VersionMismatch { expected: String, actual: String },
    DigestMismatch { expected: String, actual: String },
    Spawn(String),
    Closed,
    Timeout(&'static str),
    Protocol(String),
    Rpc { code: i64, message: String },
    ContextWindowExceeded,
    UsageLimitExceeded { retry_after: Option<u64> },
    BadRequest,
    AuthenticationRequired,
    SubscriptionRequired,
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("Codex provider is disabled"),
            Self::Busy => f.write_str("Codex provider concurrency limit reached"),
            Self::InvalidConfig(message) => write!(f, "invalid Codex configuration: {message}"),
            Self::VersionMismatch { expected, actual } => {
                write!(
                    f,
                    "Codex version mismatch: expected {expected:?}, got {actual:?}"
                )
            }
            Self::DigestMismatch { expected, actual } => {
                write!(
                    f,
                    "Codex binary digest mismatch: expected {expected}, got {actual}"
                )
            }
            Self::Spawn(message) => write!(f, "failed to start Codex app-server: {message}"),
            Self::Closed => f.write_str("Codex app-server closed its transport"),
            Self::Timeout(phase) => write!(f, "Codex app-server timed out during {phase}"),
            Self::Protocol(message) => write!(f, "Codex app-server protocol error: {message}"),
            Self::Rpc { code, message } => {
                write!(f, "Codex app-server JSON-RPC error {code}: {message}")
            }
            Self::ContextWindowExceeded => f.write_str("model context window exceeded"),
            Self::UsageLimitExceeded { .. } => {
                f.write_str("ChatGPT subscription usage limit exceeded")
            }
            Self::BadRequest => f.write_str("model rejected the request"),
            Self::AuthenticationRequired => f.write_str("Codex profile is not authenticated"),
            Self::SubscriptionRequired => {
                f.write_str("Codex profile is not authenticated with a ChatGPT subscription")
            }
        }
    }
}

impl std::error::Error for ProcessError {}

impl ProcessError {
    pub(crate) fn diagnostic_class(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Busy => "busy",
            Self::InvalidConfig(_) => "invalid_config",
            Self::VersionMismatch { .. } => "version_mismatch",
            Self::DigestMismatch { .. } => "digest_mismatch",
            Self::Spawn(_) => "spawn",
            Self::Closed => "closed",
            Self::Timeout(_) => "timeout",
            Self::Protocol(_) => "protocol",
            Self::Rpc { .. } => "rpc",
            Self::ContextWindowExceeded => "context_window_exceeded",
            Self::UsageLimitExceeded { .. } => "usage_limit_exceeded",
            Self::BadRequest => "bad_request",
            Self::AuthenticationRequired => "authentication_required",
            Self::SubscriptionRequired => "subscription_required",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimitWindow {
    pub used_percent: i64,
    pub window_duration_mins: Option<i64>,
    pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexRateLimits {
    pub primary: Option<CodexRateLimitWindow>,
    pub secondary: Option<CodexRateLimitWindow>,
    pub reached: bool,
    pub observed_at: i64,
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
    Notification {
        method: String,
        params: Value,
    },
    ServerRequest {
        id: Value,
        method: String,
        params: Value,
    },
    Closed(ProcessError),
}

pub(crate) struct TurnEvents {
    pub(crate) receiver: mpsc::Receiver<AppServerEvent>,
}

struct ProcessShared {
    live: AtomicBool,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, ProcessError>>>>,
    turns: Mutex<HashMap<String, mpsc::Sender<AppServerEvent>>>,
    orphan_events: Mutex<HashMap<String, VecDeque<AppServerEvent>>>,
    rate_limits: Mutex<Option<CodexRateLimits>>,
}

impl ProcessShared {
    fn new() -> Self {
        Self {
            live: AtomicBool::new(true),
            pending: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
            orphan_events: Mutex::new(HashMap::new()),
            rate_limits: Mutex::new(None),
        }
    }

    async fn close(&self, error: ProcessError) {
        if !self.live.swap(false, Ordering::AcqRel) {
            return;
        }
        for (_, sender) in self.pending.lock().await.drain() {
            let _ = sender.send(Err(error.clone()));
        }
        for (_, sender) in self.turns.lock().await.drain() {
            let _ = sender.send(AppServerEvent::Closed(error.clone())).await;
        }
        self.orphan_events.lock().await.clear();
    }

    async fn route_event(&self, thread_id: String, event: AppServerEvent) {
        if let Some(sender) = self.turns.lock().await.get(&thread_id).cloned() {
            if sender.send(event).await.is_err() {
                self.turns.lock().await.remove(&thread_id);
            }
            return;
        }
        let mut orphans = self.orphan_events.lock().await;
        let queue = orphans.entry(thread_id).or_default();
        if queue.len() == MAX_ORPHAN_EVENTS_PER_THREAD {
            queue.pop_front();
        }
        queue.push_back(event);
    }
}

pub struct CodexProcess {
    cfg: Arc<CodexConfig>,
    writer: Mutex<ChildStdin>,
    shared: Arc<ProcessShared>,
    next_id: AtomicU64,
    // `kill_on_drop(true)` makes dropping an invalidated generation terminate its child.
    _child: Mutex<Child>,
}

impl CodexProcess {
    /// Start one supervised child bound to exactly one authenticated `CODEX_HOME`. Every home in
    /// the pool runs its own attested child; they share nothing but the pinned binary.
    pub async fn spawn(cfg: Arc<CodexConfig>, spec: &CodexHomeSpec) -> Result<Self, ProcessError> {
        validate_config(&cfg, &spec.path)?;
        verify_binary_digest(&cfg).await?;
        verify_version(&cfg, &spec.path).await?;

        let mut command = child_command(&cfg, spec);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| ProcessError::Spawn(error.to_string()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProcessError::Spawn("stdin pipe was unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProcessError::Spawn("stdout pipe was unavailable".to_string()))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| ProcessError::Spawn("stderr pipe was unavailable".to_string()))?;

        let shared = Arc::new(ProcessShared::new());
        tokio::spawn(reader_loop(BufReader::new(stdout), shared.clone()));
        tokio::spawn(stderr_loop(BufReader::new(stderr)));

        let process = Self {
            cfg,
            writer: Mutex::new(stdin),
            shared,
            next_id: AtomicU64::new(1),
            _child: Mutex::new(child),
        };
        process.initialize().await?;
        process.require_subscription().await?;
        if let Err(error) = process.refresh_rate_limits().await {
            eprintln!(
                "Codex rate-limit snapshot unavailable [{}]",
                error.diagnostic_class()
            );
        }
        Ok(process)
    }

    pub fn is_live(&self) -> bool {
        self.shared.live.load(Ordering::Acquire)
    }

    pub async fn request(
        &self,
        method: &'static str,
        params: Value,
    ) -> Result<Value, ProcessError> {
        self.request_with_timeout(method, params, self.cfg.request_timeout_ms)
            .await
    }

    pub async fn request_with_timeout(
        &self,
        method: &'static str,
        params: Value,
        timeout_ms: u64,
    ) -> Result<Value, ProcessError> {
        if !self.is_live() {
            return Err(ProcessError::Closed);
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = oneshot::channel();
        self.shared.pending.lock().await.insert(id, sender);
        let payload = json!({"id": id, "method": method, "params": params});
        if let Err(error) = self.write_value(&payload).await {
            self.shared.pending.lock().await.remove(&id);
            self.shared.close(error.clone()).await;
            return Err(error);
        }
        match tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms.max(1)),
            receiver,
        )
        .await
        {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(ProcessError::Closed),
            Err(_) => {
                self.shared.pending.lock().await.remove(&id);
                Err(ProcessError::Timeout(method))
            }
        }
    }

    pub async fn notify(
        &self,
        method: &'static str,
        params: Option<Value>,
    ) -> Result<(), ProcessError> {
        let mut payload = json!({"method": method});
        if let Some(params) = params {
            payload["params"] = params;
        }
        self.write_value(&payload).await
    }

    pub async fn respond_error(
        &self,
        id: Value,
        code: i64,
        message: &str,
    ) -> Result<(), ProcessError> {
        self.write_value(&json!({
            "id": id,
            "error": {"code": code, "message": message}
        }))
        .await
    }

    pub async fn register_turn(&self, thread_id: &str) -> Result<TurnEvents, ProcessError> {
        if !self.is_live() {
            return Err(ProcessError::Closed);
        }
        let (sender, receiver) = mpsc::channel(512);
        if self
            .shared
            .turns
            .lock()
            .await
            .insert(thread_id.to_string(), sender.clone())
            .is_some()
        {
            return Err(ProcessError::Protocol(format!(
                "duplicate live thread registration {thread_id}"
            )));
        }
        if let Some(mut pending) = self.shared.orphan_events.lock().await.remove(thread_id) {
            while let Some(event) = pending.pop_front() {
                if sender.send(event).await.is_err() {
                    break;
                }
            }
        }
        Ok(TurnEvents { receiver })
    }

    pub async fn unregister_turn(&self, thread_id: &str) {
        self.shared.turns.lock().await.remove(thread_id);
        self.shared.orphan_events.lock().await.remove(thread_id);
    }

    pub async fn rate_limits(&self) -> Option<CodexRateLimits> {
        self.shared.rate_limits.lock().await.clone()
    }

    pub async fn usage_limit_retry_after(&self) -> Option<u64> {
        let limits = self.rate_limits().await?;
        let now = pool::now();
        limits
            .primary
            .iter()
            .chain(limits.secondary.iter())
            .filter(|window| window.used_percent >= 100)
            .filter_map(|window| window.resets_at)
            .map(|reset| reset.saturating_sub(now).clamp(1, 7 * 24 * 3600) as u64)
            .max()
    }

    async fn initialize(&self) -> Result<(), ProcessError> {
        let params = json!({
            "clientInfo": {
                "name": "apitoken_openai_compat",
                "title": "API Token OpenAI-compatible gateway",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {
                "experimentalApi": true,
                "requestAttestation": false,
                "mcpServerOpenaiFormElicitation": false
            }
        });
        self.request_with_timeout("initialize", params, self.cfg.startup_timeout_ms)
            .await?;
        self.notify("initialized", None).await
    }

    async fn require_subscription(&self) -> Result<(), ProcessError> {
        let response = self
            .request_with_timeout(
                "account/read",
                json!({"refreshToken": false}),
                self.cfg.startup_timeout_ms,
            )
            .await?;
        validate_subscription_account(&response)
    }

    /// Read-only liveness of the authenticated profile, used by the background health loop.
    ///
    /// A device login expires without any traffic touching it, so the pool must learn about a dead
    /// home before a customer request does. The check reuses the same `account/read` semantics as
    /// startup and refreshes the rate-limit snapshot the admission gate reads.
    pub(crate) async fn probe(&self) -> Result<(), ProcessError> {
        self.require_subscription().await?;
        if let Err(error) = self.refresh_rate_limits().await {
            // The snapshot is observational. A profile that answered `account/read` is usable even
            // when this endpoint is briefly unavailable.
            eprintln!(
                "Codex rate-limit snapshot unavailable [{}]",
                error.diagnostic_class()
            );
        }
        Ok(())
    }

    async fn refresh_rate_limits(&self) -> Result<(), ProcessError> {
        let response = self
            .request_with_timeout(
                "account/rateLimits/read",
                json!({}),
                self.cfg.startup_timeout_ms,
            )
            .await?;
        let limits = response.get("rateLimits").ok_or_else(|| {
            ProcessError::Protocol(
                "account/rateLimits/read response omitted rateLimits".to_string(),
            )
        })?;
        let parsed = parse_rate_limits(limits).ok_or_else(|| {
            ProcessError::Protocol(
                "account/rateLimits/read returned invalid rateLimits".to_string(),
            )
        })?;
        *self.shared.rate_limits.lock().await = Some(parsed);
        Ok(())
    }
}

fn validate_subscription_account(response: &Value) -> Result<(), ProcessError> {
    // `requiresOpenaiAuth=true` means the selected provider is an OpenAI-authenticated provider;
    // it does NOT mean credentials are missing. The official ChatGPT example reports both
    // `account.type=chatgpt` and `requiresOpenaiAuth=true`.
    let requires_openai_auth = response
        .get("requiresOpenaiAuth")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    match response.pointer("/account/type").and_then(Value::as_str) {
        Some("chatgpt") => Ok(()),
        Some(_) => Err(ProcessError::SubscriptionRequired),
        None if requires_openai_auth => Err(ProcessError::AuthenticationRequired),
        None => Err(ProcessError::SubscriptionRequired),
    }
}

impl CodexProcess {
    async fn write_value(&self, value: &Value) -> Result<(), ProcessError> {
        if !self.is_live() {
            return Err(ProcessError::Closed);
        }
        let mut encoded =
            serde_json::to_vec(value).map_err(|error| ProcessError::Protocol(error.to_string()))?;
        if encoded.len() > MAX_JSONRPC_LINE_BYTES {
            return Err(ProcessError::Protocol(
                "outgoing JSON-RPC frame exceeded 32 MiB".to_string(),
            ));
        }
        encoded.push(b'\n');
        let mut writer = self.writer.lock().await;
        writer.write_all(&encoded).await.map_err(|error| {
            ProcessError::Protocol(format!("failed to write app-server stdin: {error}"))
        })?;
        writer.flush().await.map_err(|error| {
            ProcessError::Protocol(format!("failed to flush app-server stdin: {error}"))
        })
    }
}

fn validate_config(cfg: &CodexConfig, home: &str) -> Result<(), ProcessError> {
    if !cfg.enabled {
        return Err(ProcessError::Disabled);
    }
    for (name, value) in [
        ("binary", cfg.binary.as_str()),
        ("codex_home", home),
        ("work_dir", cfg.work_dir.as_str()),
    ] {
        if value.is_empty() || !Path::new(value).is_absolute() {
            return Err(ProcessError::InvalidConfig(format!(
                "{name} must be a non-empty absolute path"
            )));
        }
    }
    if cfg.expected_version.trim().is_empty() {
        return Err(ProcessError::InvalidConfig(
            "expected_version must be set".to_string(),
        ));
    }
    if cfg.binary_sha256.len() != 64
        || !cfg
            .binary_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err(ProcessError::InvalidConfig(
            "binary_sha256 must be 64 lowercase hexadecimal characters".to_string(),
        ));
    }
    if cfg.models.is_empty() {
        return Err(ProcessError::InvalidConfig(
            "at least one model must be advertised".to_string(),
        ));
    }
    Ok(())
}

async fn verify_binary_digest(cfg: &CodexConfig) -> Result<(), ProcessError> {
    let path = cfg.binary.clone();
    let actual = tokio::task::spawn_blocking(move || {
        let mut file =
            std::fs::File::open(path).map_err(|error| ProcessError::Spawn(error.to_string()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|error| ProcessError::Spawn(error.to_string()))?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }
        Ok::<_, ProcessError>(format!("{:x}", hasher.finalize()))
    })
    .await
    .map_err(|error| ProcessError::Spawn(error.to_string()))??;
    if actual != cfg.binary_sha256 {
        return Err(ProcessError::DigestMismatch {
            expected: cfg.binary_sha256.clone(),
            actual,
        });
    }
    Ok(())
}

async fn verify_version(cfg: &CodexConfig, home: &str) -> Result<(), ProcessError> {
    let output = tokio::time::timeout(
        std::time::Duration::from_millis(cfg.startup_timeout_ms.max(1)),
        Command::new(&cfg.binary)
            .arg("--version")
            .env_clear()
            .env("CODEX_HOME", home)
            .output(),
    )
    .await
    .map_err(|_| ProcessError::Timeout("version check"))?
    .map_err(|error| ProcessError::Spawn(error.to_string()))?;
    if !output.status.success() {
        return Err(ProcessError::Spawn(format!(
            "version check exited with {}",
            output.status
        )));
    }
    let actual = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if actual != cfg.expected_version {
        return Err(ProcessError::VersionMismatch {
            expected: cfg.expected_version.clone(),
            actual,
        });
    }
    Ok(())
}

fn child_command(cfg: &CodexConfig, spec: &CodexHomeSpec) -> Command {
    let mut command = Command::new(&cfg.binary);
    command
        .env_clear()
        .env("CODEX_HOME", &spec.path)
        .env("PATH", "/usr/local/bin:/usr/bin:/bin")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb")
        .current_dir(&cfg.work_dir);
    match &spec.proxy {
        // A dedicated egress for this account. One address serving every ChatGPT profile is itself
        // a fleet signal, exactly as it is on the Claude side. Only the standard proxy variables
        // are set — no TLS or user-agent impersonation is added, so this changes where the official
        // client connects from and nothing about how it identifies itself.
        Some(proxy) => {
            for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
                command.env(name, proxy);
            }
            for name in ["http_proxy", "https_proxy", "all_proxy"] {
                command.env(name, proxy);
            }
            // The shared NO_PROXY still applies: it carves out destinations, it does not select one.
            if let Some(no_proxy) = cfg.child_proxy_env.get("NO_PROXY") {
                command.env("NO_PROXY", no_proxy);
                command.env("no_proxy", no_proxy);
            }
        }
        None => {
            for (name, value) in &cfg.child_proxy_env {
                command.env(name, value);
            }
        }
    }
    // Keep model-visible context as close to a normal API call as app-server permits. The pinned
    // build additionally disables built-in native tools for this client name; customer functions
    // supplied as dynamic tools remain available.
    for override_value in [
        "include_permissions_instructions=false",
        "include_apps_instructions=false",
        "include_collaboration_mode_instructions=false",
        "include_environment_context=false",
        "skills.include_instructions=false",
        "features.plugins=false",
        "features.apps=false",
        "features.multi_agent_v2=false",
        "project_doc_max_bytes=0",
        "mcp_servers={}",
    ] {
        command.arg("--config").arg(override_value);
    }
    command.arg("app-server").arg("--listen").arg("stdio://");
    command
}

async fn reader_loop<R>(mut reader: R, shared: Arc<ProcessShared>)
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        line.clear();
        let has_frame = match read_bounded_jsonrpc_line(&mut reader, &mut line).await {
            Ok(has_frame) => has_frame,
            Err(error) => {
                shared.close(error).await;
                return;
            }
        };
        if !has_frame {
            shared.close(ProcessError::Closed).await;
            return;
        }
        let value: Value = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(error) => {
                shared
                    .close(ProcessError::Protocol(format!(
                        "invalid JSON-RPC frame: {error}"
                    )))
                    .await;
                return;
            }
        };
        if let Err(error) = dispatch_value(&shared, value).await {
            shared.close(error).await;
            return;
        }
    }
}

/// Read one newline-delimited frame without allowing `read_until` to allocate past the protocol
/// cap. The delimiter is consumed but excluded from `line`, so the byte limit applies to JSON.
async fn read_bounded_jsonrpc_line<R>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> Result<bool, ProcessError>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await.map_err(|error| {
            ProcessError::Protocol(format!("failed to read app-server stdout: {error}"))
        })?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > MAX_JSONRPC_LINE_BYTES {
                return Err(ProcessError::Protocol(
                    "incoming JSON-RPC frame exceeded 32 MiB".to_string(),
                ));
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            return Ok(true);
        }
        if line.len().saturating_add(available.len()) > MAX_JSONRPC_LINE_BYTES {
            return Err(ProcessError::Protocol(
                "incoming JSON-RPC frame exceeded 32 MiB".to_string(),
            ));
        }
        let consumed = available.len();
        line.extend_from_slice(available);
        reader.consume(consumed);
    }
}

async fn dispatch_value(shared: &ProcessShared, value: Value) -> Result<(), ProcessError> {
    if let Some(id) = value.get("id").and_then(Value::as_u64) {
        if value.get("method").is_none() {
            if let Some(sender) = shared.pending.lock().await.remove(&id) {
                if let Some(error) = value.get("error") {
                    let code = error.get("code").and_then(Value::as_i64).unwrap_or(-32000);
                    let message = error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown JSON-RPC error")
                        .to_string();
                    let _ = sender.send(Err(ProcessError::Rpc { code, message }));
                } else {
                    let _ = sender.send(Ok(value.get("result").cloned().unwrap_or(Value::Null)));
                }
            }
            return Ok(());
        }
    }

    let Some(method) = value.get("method").and_then(Value::as_str) else {
        // Responses for a timed-out request are intentionally ignored.
        return Ok(());
    };
    let params = value.get("params").cloned().unwrap_or(Value::Null);
    if method == "account/rateLimits/updated" {
        if let Some(limits) = params.get("rateLimits").and_then(parse_rate_limits) {
            *shared.rate_limits.lock().await = Some(limits);
        }
        return Ok(());
    }
    let thread_id = params
        .get("threadId")
        .and_then(Value::as_str)
        .map(str::to_string);
    let event = if let Some(id) = value.get("id").cloned() {
        AppServerEvent::ServerRequest {
            id,
            method: method.to_string(),
            params,
        }
    } else {
        AppServerEvent::Notification {
            method: method.to_string(),
            params,
        }
    };
    if let Some(thread_id) = thread_id {
        shared.route_event(thread_id, event).await;
    }
    Ok(())
}

fn parse_rate_limits(value: &Value) -> Option<CodexRateLimits> {
    let object = value.as_object()?;
    let primary = object.get("primary").and_then(parse_rate_limit_window);
    let secondary = object.get("secondary").and_then(parse_rate_limit_window);
    let reached = object
        .get("rateLimitReachedType")
        .is_some_and(|value| !value.is_null())
        || object
            .get("spendControlReached")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        || primary
            .iter()
            .chain(secondary.iter())
            .any(|window| window.used_percent >= 100);
    Some(CodexRateLimits {
        primary,
        secondary,
        reached,
        observed_at: pool::now(),
    })
}

fn parse_rate_limit_window(value: &Value) -> Option<CodexRateLimitWindow> {
    let object = value.as_object()?;
    Some(CodexRateLimitWindow {
        used_percent: object.get("usedPercent")?.as_i64()?.clamp(0, 100),
        window_duration_mins: object.get("windowDurationMins").and_then(Value::as_i64),
        resets_at: object.get("resetsAt").and_then(Value::as_i64),
    })
}

async fn stderr_loop<R>(mut reader: R)
where
    R: AsyncRead + Unpin,
{
    let mut buffer = [0u8; 8 * 1024];
    let mut reported = false;
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(read) => {
                // App-server diagnostics are not a stable public contract and may contain request
                // text, account metadata, paths, or credentials. Bound reads, discard bytes and
                // emit only one presence signal per child to avoid both disclosure and log floods.
                if !reported {
                    eprintln!(
                        "codex app-server emitted redacted diagnostics (first chunk: {read} bytes)"
                    );
                    reported = true;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_read_accepts_official_chatgpt_requires_auth_semantics() {
        assert_eq!(
            validate_subscription_account(&json!({
                "account": {"type": "chatgpt"},
                "requiresOpenaiAuth": true
            })),
            Ok(())
        );
        assert_eq!(
            validate_subscription_account(&json!({
                "account": {"type": "apiKey"},
                "requiresOpenaiAuth": true
            })),
            Err(ProcessError::SubscriptionRequired)
        );
        assert_eq!(
            validate_subscription_account(&json!({
                "account": null,
                "requiresOpenaiAuth": true
            })),
            Err(ProcessError::AuthenticationRequired)
        );
        assert_eq!(
            validate_subscription_account(&json!({
                "account": null,
                "requiresOpenaiAuth": false
            })),
            Err(ProcessError::SubscriptionRequired)
        );
    }

    #[tokio::test]
    async fn dispatches_responses_notifications_and_server_requests() {
        let shared = ProcessShared::new();
        let (response_tx, response_rx) = oneshot::channel();
        shared.pending.lock().await.insert(7, response_tx);
        dispatch_value(&shared, json!({"id": 7, "result": {"ok": true}}))
            .await
            .unwrap();
        assert_eq!(response_rx.await.unwrap().unwrap(), json!({"ok": true}));

        let (turn_tx, mut turn_rx) = mpsc::channel(8);
        shared
            .turns
            .lock()
            .await
            .insert("thread-1".to_string(), turn_tx);
        dispatch_value(
            &shared,
            json!({
                "method": "item/agentMessage/delta",
                "params": {"threadId": "thread-1", "turnId": "turn-1", "delta": "hi"}
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            turn_rx.recv().await,
            Some(AppServerEvent::Notification { method, .. })
                if method == "item/agentMessage/delta"
        ));

        dispatch_value(
            &shared,
            json!({
                "id": "server-1",
                "method": "item/tool/call",
                "params": {"threadId": "thread-1", "turnId": "turn-1"}
            }),
        )
        .await
        .unwrap();
        assert!(matches!(
            turn_rx.recv().await,
            Some(AppServerEvent::ServerRequest { method, .. }) if method == "item/tool/call"
        ));
    }

    #[tokio::test]
    async fn jsonrpc_line_reader_enforces_the_cap_before_growing() {
        let mut valid = BufReader::new(
            &br#"{"id":1}
"#[..],
        );
        let mut line = Vec::new();
        assert!(read_bounded_jsonrpc_line(&mut valid, &mut line)
            .await
            .unwrap());
        assert_eq!(line, br#"{"id":1}"#);

        let oversized = vec![b'x'; MAX_JSONRPC_LINE_BYTES + 1];
        let mut oversized = BufReader::new(oversized.as_slice());
        line.clear();
        assert!(matches!(
            read_bounded_jsonrpc_line(&mut oversized, &mut line).await,
            Err(ProcessError::Protocol(message)) if message.contains("exceeded 32 MiB")
        ));
        assert!(line.len() <= MAX_JSONRPC_LINE_BYTES);
    }

    #[tokio::test]
    async fn buffers_bounded_events_until_thread_registration() {
        let shared = ProcessShared::new();
        for index in 0..(MAX_ORPHAN_EVENTS_PER_THREAD + 4) {
            dispatch_value(
                &shared,
                json!({
                    "method": "test/event",
                    "params": {"threadId": "late", "index": index}
                }),
            )
            .await
            .unwrap();
        }
        let pending = shared.orphan_events.lock().await;
        let queue = pending.get("late").unwrap();
        assert_eq!(queue.len(), MAX_ORPHAN_EVENTS_PER_THREAD);
        let first = match queue.front().unwrap() {
            AppServerEvent::Notification { params, .. } => {
                params.get("index").and_then(Value::as_u64)
            }
            _ => None,
        };
        assert_eq!(first, Some(4));
    }

    #[tokio::test]
    async fn captures_global_rate_limit_updates_without_routing_customer_events() {
        let shared = ProcessShared::new();
        dispatch_value(
            &shared,
            json!({
                "method": "account/rateLimits/updated",
                "params": {
                    "rateLimits": {
                        "primary": {
                            "usedPercent": 100,
                            "windowDurationMins": 300,
                            "resetsAt": 4_102_444_800_i64
                        },
                        "secondary": null,
                        "rateLimitReachedType": "rate_limit_reached"
                    }
                }
            }),
        )
        .await
        .unwrap();
        let limits = shared.rate_limits.lock().await.clone().unwrap();
        assert!(limits.reached);
        assert_eq!(limits.primary.unwrap().used_percent, 100);
        assert!(shared.orphan_events.lock().await.is_empty());
    }
}
