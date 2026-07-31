//! Multiplexed JSON-RPC transport for `codex app-server`.
//!
//! The legacy owner uses newline-delimited JSON over a private stdio child. Blue-green gateway
//! slots use the official websocket control protocol through `codex app-server proxy`, leaving the
//! separately supervised Unix-socket daemon as the only owner of each authenticated home.

use super::discovery;
use super::{CodexConfig, CodexHomeSpec, CodexTransport};
use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
    ReadBuf,
};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{client_async_with_config, WebSocketStream};

const MAX_JSONRPC_LINE_BYTES: usize = 32 * 1024 * 1024;
const MAX_ORPHAN_EVENTS_PER_THREAD: usize = 64;
const APP_SERVER_RUNTIME_DIR: &str = "/run/apitoken/codex-app-servers";
const APP_SERVER_READY_MARKER: &str = "openai-codex-client-v1";
/// Sentinel of the cross-generation busy marker. A separate word from the ready marker so a file
/// of one kind can never be read as the other after a partial upgrade.
const APP_SERVER_BUSY_MARKER: &str = "openai-codex-client-busy-v1";
const PROCESS_GROUP_REAP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
const PROCESS_GROUP_REAP_POLL: std::time::Duration = std::time::Duration::from_millis(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    Disabled,
    HomeInUse,
    HomeLockUnavailable,
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
            Self::HomeInUse => {
                f.write_str("Codex homes are already owned by another provider process")
            }
            Self::HomeLockUnavailable => f.write_str("Codex home ownership lock is unavailable"),
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
            Self::HomeInUse => "home_in_use",
            Self::HomeLockUnavailable => "home_lock_unavailable",
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
}

pub(crate) struct TurnEvents {
    receiver: mpsc::Receiver<AppServerEvent>,
    closed: watch::Receiver<Option<ProcessError>>,
}

/// Bidirectional stdio of the official byte-preserving proxy. The bytes carried here are a normal
/// websocket HTTP upgrade followed by websocket frames; JSONL is never written directly to it.
struct ChildProxyIo {
    stdout: ChildStdout,
    stdin: ChildStdin,
}

impl AsyncRead for ChildProxyIo {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stdout).poll_read(cx, buffer)
    }
}

impl AsyncWrite for ChildProxyIo {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.get_mut().stdin).poll_write(cx, buffer)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stdin).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().stdin).poll_shutdown(cx)
    }
}

enum AppServerWriter {
    JsonLines(ChildStdin),
    WebSocket(SplitSink<WebSocketStream<ChildProxyIo>, Message>),
}

impl TurnEvents {
    /// Receive the next turn event while observing transport closure out of band.
    ///
    /// The per-turn event queue is deliberately bounded and EOF is carried out of band, so closure
    /// can never be blocked on a full queue. Events already accepted from the transport must still
    /// be delivered before EOF: otherwise a final model delta followed immediately by process exit
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
                    return event.ok_or(ProcessError::Closed);
                }
                changed = self.closed.changed() => {
                    if changed.is_err() {
                        return Err(ProcessError::Closed);
                    }
                    // Re-check the queue before returning the terminal error. The transport reader
                    // routes stdout frames before it publishes EOF, and that ordering is part of
                    // the public streaming contract.
                }
            }
        }
    }
}

struct ProcessShared {
    live: AtomicBool,
    pending: Mutex<HashMap<u64, oneshot::Sender<Result<Value, ProcessError>>>>,
    turns: Mutex<HashMap<String, mpsc::Sender<AppServerEvent>>>,
    orphan_events: Mutex<HashMap<String, VecDeque<AppServerEvent>>>,
    rate_limits: Mutex<Option<CodexRateLimits>>,
    closed: watch::Sender<Option<ProcessError>>,
}

impl ProcessShared {
    fn new() -> Self {
        let (closed, _receiver) = watch::channel(None);
        Self {
            live: AtomicBool::new(true),
            pending: Mutex::new(HashMap::new()),
            turns: Mutex::new(HashMap::new()),
            orphan_events: Mutex::new(HashMap::new()),
            rate_limits: Mutex::new(None),
            closed,
        }
    }

    async fn close(&self, error: ProcessError) {
        if !self.live.swap(false, Ordering::AcqRel) {
            return;
        }
        self.closed.send_replace(Some(error.clone()));
        for (_, sender) in self.pending.lock().await.drain() {
            let _ = sender.send(Err(error.clone()));
        }
        self.turns.lock().await.clear();
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
    home: String,
    writer: Mutex<AppServerWriter>,
    shared: Arc<ProcessShared>,
    ready: AtomicBool,
    next_id: AtomicU64,
    child: Mutex<ChildLifecycle>,
    /// A root-validated lease used by the daemon roller. It appears only after initialization and
    /// authentication succeed, and disappears only after the proxy process has been reaped.
    ready_marker: Option<PathBuf>,
}

/// Child ownership must survive cancellation of a request/invalidation future. Once shutdown
/// starts, a detached reaper owns the OS child and publishes one cloneable terminal result. A
/// later shutdown call therefore waits for the same reaper instead of either spawning beside an
/// unreaped home owner or abandoning a zombie until the gateway process exits.
enum ChildLifecycle {
    Running(Child),
    Reaping(watch::Receiver<Option<Result<(), ProcessError>>>),
}

impl CodexProcess {
    /// Launch one supervised child without awaiting protocol initialization after the OS process
    /// exists. The caller can therefore publish this generation before `start` reaches a
    /// cancellation point.
    pub async fn launch(cfg: Arc<CodexConfig>, spec: &CodexHomeSpec) -> Result<Self, ProcessError> {
        validate_config(&cfg, &spec.path)?;
        verify_binary_digest(&cfg).await?;
        verify_version(&cfg, &spec.path).await?;

        let mut command = child_command(&cfg, spec);
        let client_lease = match cfg.transport {
            CodexTransport::OwnedChild => None,
            CodexTransport::SharedDaemonProxy => {
                let lease = new_client_lease()?;
                command.env("CLAUDE_API_CODEX_CLIENT_LEASE", &lease);
                Some(lease)
            }
        };
        isolate_process_group(&mut command);
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|error| ProcessError::Spawn(error.to_string()))?;
        let ready_marker = match cfg.transport {
            CodexTransport::OwnedChild => None,
            CodexTransport::SharedDaemonProxy => {
                let proxy_pid = child.id().ok_or_else(|| {
                    ProcessError::Spawn("shared proxy process id was unavailable".to_string())
                })?;
                Some(shared_client_ready_marker(
                    &spec.path,
                    std::process::id(),
                    proxy_pid,
                    client_lease.as_deref().ok_or_else(|| {
                        ProcessError::Spawn("shared client lease was unavailable".to_string())
                    })?,
                ))
            }
        };
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
        tokio::spawn(stderr_loop(BufReader::new(stderr)));
        let writer = match cfg.transport {
            CodexTransport::OwnedChild => {
                tokio::spawn(reader_loop(BufReader::new(stdout), shared.clone()));
                AppServerWriter::JsonLines(stdin)
            }
            CodexTransport::SharedDaemonProxy => {
                let proxy = ChildProxyIo { stdout, stdin };
                let websocket_config = WebSocketConfig::default()
                    .max_message_size(Some(MAX_JSONRPC_LINE_BYTES))
                    .max_frame_size(Some(MAX_JSONRPC_LINE_BYTES));
                let connect =
                    client_async_with_config("ws://localhost/", proxy, Some(websocket_config));
                let (websocket, _response) = tokio::time::timeout(
                    std::time::Duration::from_millis(cfg.startup_timeout_ms.max(1)),
                    connect,
                )
                .await
                .map_err(|_| ProcessError::Timeout("shared app-server websocket handshake"))?
                .map_err(|error| {
                    ProcessError::Protocol(format!(
                        "shared app-server websocket handshake failed: {error}"
                    ))
                })?;
                let (writer, reader) = websocket.split();
                tokio::spawn(websocket_reader_loop(
                    reader,
                    shared.clone(),
                    ready_marker.clone(),
                ));
                AppServerWriter::WebSocket(writer)
            }
        };

        let process = Self {
            cfg,
            home: spec.path.clone(),
            writer: Mutex::new(writer),
            shared,
            ready: AtomicBool::new(false),
            next_id: AtomicU64::new(1),
            child: Mutex::new(ChildLifecycle::Running(child)),
            ready_marker,
        };
        Ok(process)
    }

    /// Complete startup only after the owning home has published this generation.
    pub async fn start(&self) -> Result<(), ProcessError> {
        self.initialize().await?;
        self.require_subscription().await?;
        if let Err(error) = self.refresh_rate_limits().await {
            // The class alone says the provider answered and refused, which is not enough to act
            // on: "the method is not supported for this account", "the parameters are wrong" and
            // "this subscription has no window yet" are three different fixes behind one label.
            // The numeric code discriminates them. Only the code is logged — the accompanying
            // message is upstream free text and stays out of our logs.
            let code = match &error {
                ProcessError::Rpc { code, .. } => *code,
                _ => 0,
            };
            eprintln!(
                "Codex home {} rate-limit snapshot unavailable [{}] rpc_code={code}",
                discovery::home_id(&self.home),
                error.diagnostic_class()
            );
        }
        self.ready.store(true, Ordering::Release);
        if let Err(error) = self.publish_ready_marker().await {
            self.ready.store(false, Ordering::Release);
            return Err(error);
        }
        if !self.is_live() {
            self.ready.store(false, Ordering::Release);
            let _ = self.remove_ready_marker().await;
            return Err(ProcessError::Closed);
        }
        Ok(())
    }

    pub fn is_live(&self) -> bool {
        self.shared.live.load(Ordering::Acquire)
    }

    pub(crate) fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire) && self.is_live()
    }

    /// Poison one transport generation before a replacement can use the same `CODEX_HOME`.
    pub(crate) async fn shutdown(&self) -> Result<(), ProcessError> {
        self.ready.store(false, Ordering::Release);
        let mut reaper = {
            let mut lifecycle = self.child.lock().await;
            match &*lifecycle {
                ChildLifecycle::Reaping(receiver) => receiver.clone(),
                ChildLifecycle::Running(_) => {
                    let (finished, receiver) = watch::channel(None);
                    let previous = std::mem::replace(
                        &mut *lifecycle,
                        ChildLifecycle::Reaping(receiver.clone()),
                    );
                    let ChildLifecycle::Running(mut child) = previous else {
                        unreachable!("running child changed while lifecycle lock was held")
                    };
                    let process_group = child.id();
                    let group_result = kill_process_group(process_group);
                    let _ = child.start_kill();
                    let marker = self.ready_marker.clone();
                    tokio::spawn(async move {
                        let result = reap_child(child, process_group, marker, group_result).await;
                        finished.send_replace(Some(result));
                    });
                    receiver
                }
            }
        };
        self.shared.close(ProcessError::Closed).await;
        loop {
            if let Some(result) = reaper.borrow().clone() {
                return result;
            }
            if reaper.changed().await.is_err() {
                return Err(ProcessError::Spawn(
                    "Codex child reaper exited without a result".to_string(),
                ));
            }
        }
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
        let closed = self.shared.closed.subscribe();
        if !self.is_live() {
            self.shared.turns.lock().await.remove(thread_id);
            return Err(ProcessError::Closed);
        }
        Ok(TurnEvents { receiver, closed })
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
        let response = self
            .request_with_timeout("initialize", params, self.cfg.startup_timeout_ms)
            .await?;
        if self.cfg.transport == CodexTransport::SharedDaemonProxy {
            let actual_home = response.get("codexHome").and_then(Value::as_str);
            if actual_home != Some(self.home.as_str()) {
                return Err(ProcessError::Protocol(
                    "shared app-server answered for an unexpected Codex home".to_string(),
                ));
            }
        }
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
            //
            // Naming the home is what makes this line actionable. Without it the pool could not
            // distinguish "one endpoint blipped once" from "this subscription's quota has not been
            // refreshed in hours", which is a real state: `account/read` keeps answering, so the
            // home stays healthy and routable while its quota reading is frozen at whatever it last
            // said. Production has a home in exactly that state right now and the log could not say
            // which one — it had to be inferred from a gauge that had stopped moving.
            //
            // The class alone still does not say what to fix. Startup already reports the numeric
            // code, and this path — the one that actually repeats, every sweep, on several homes at
            // once — did not, so the recurring failure could not be told apart from an unsupported
            // method or a subscription with no window yet. Only the code is logged; the upstream
            // message is free text and stays out of our logs.
            let code = match &error {
                ProcessError::Rpc { code, .. } => *code,
                _ => 0,
            };
            eprintln!(
                "Codex home {} rate-limit snapshot unavailable [{}] rpc_code={code}",
                discovery::home_id(&self.home),
                error.diagnostic_class()
            );
        }
        if !self.is_live() {
            let _ = self.remove_ready_marker().await;
            return Err(ProcessError::Closed);
        }
        self.publish_ready_marker().await?;
        if !self.is_live() {
            let _ = self.remove_ready_marker().await;
            return Err(ProcessError::Closed);
        }
        Ok(())
    }

    async fn refresh_rate_limits(&self) -> Result<(), ProcessError> {
        let response = self
            .request_with_timeout(
                "account/rateLimits/read",
                json!({}),
                self.cfg.request_timeout_ms,
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
    async fn publish_ready_marker(&self) -> Result<(), ProcessError> {
        let Some(marker) = self.ready_marker.clone() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || publish_ready_marker(&marker))
            .await
            .map_err(|error| ProcessError::Spawn(format!("ready-marker task failed: {error}")))?
    }

    async fn remove_ready_marker(&self) -> Result<(), ProcessError> {
        remove_ready_marker_path(self.ready_marker.clone()).await
    }

    /// Send one JSON-RPC frame, bounded in time.
    ///
    /// Both awaits below can block forever against a peer that is gone without closing: the shared
    /// writer lock, and the write itself once the socket buffer stops draining. Neither was covered
    /// by the request deadline, which only bounds *waiting for a reply*, so a single stuck send
    /// pinned the writer lock and every later request on that home queued behind it with no
    /// deadline at all — turns parked forever, and their load slots were never released.
    ///
    /// A send that misses the deadline may have left a partial frame on the wire, which
    /// desynchronises the stream for every sibling turn on this generation. The generation is
    /// therefore poisoned rather than reused, and the deadline surfaces as a normal transport
    /// deadline that the admission policy already knows how to act on.
    async fn write_value(&self, value: &Value) -> Result<(), ProcessError> {
        if !self.is_live() {
            return Err(ProcessError::Closed);
        }
        let encoded = serde_json::to_string(value)
            .map_err(|error| ProcessError::Protocol(error.to_string()))?;
        if encoded.len() > MAX_JSONRPC_LINE_BYTES {
            return Err(ProcessError::Protocol(
                "outgoing JSON-RPC frame exceeded 32 MiB".to_string(),
            ));
        }
        let deadline = std::time::Duration::from_millis(self.cfg.request_timeout_ms.max(1));
        match tokio::time::timeout(deadline, self.write_encoded(encoded)).await {
            Ok(result) => result,
            Err(_) => {
                let error = ProcessError::Timeout("app-server write");
                self.shared.close(error.clone()).await;
                Err(error)
            }
        }
    }

    async fn write_encoded(&self, encoded: String) -> Result<(), ProcessError> {
        let mut writer = self.writer.lock().await;
        match &mut *writer {
            AppServerWriter::JsonLines(writer) => {
                writer
                    .write_all(encoded.as_bytes())
                    .await
                    .map_err(|error| {
                        ProcessError::Protocol(format!("failed to write app-server stdin: {error}"))
                    })?;
                writer.write_all(b"\n").await.map_err(|error| {
                    ProcessError::Protocol(format!("failed to write app-server stdin: {error}"))
                })?;
                writer.flush().await.map_err(|error| {
                    ProcessError::Protocol(format!("failed to flush app-server stdin: {error}"))
                })
            }
            AppServerWriter::WebSocket(writer) => writer
                .send(Message::Text(encoded.into()))
                .await
                .map_err(|error| {
                    ProcessError::Protocol(format!("failed to write app-server websocket: {error}"))
                }),
        }
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

/// Paths whose digest this process has already verified.
///
/// The pinned binary is immutable and its path carries its own digest, so a second full hash of the
/// same file can only ever produce the same answer. Recomputing it was not free: it re-read the
/// whole multi-hundred-megabyte binary on every bridge (re)establish, and that happens on the
/// RECOVERY path — once per sweep, per home, for as long as a home fails to come back, and now
/// potentially on several homes at once. The attestation must stay, but it belongs to the first
/// launch of a given binary, not to every attempt to reconnect while the pool is already degraded.
static VERIFIED_BINARIES: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
    std::sync::OnceLock::new();

fn binary_already_verified(path: &str) -> bool {
    VERIFIED_BINARIES
        .get_or_init(Default::default)
        .lock()
        .map(|seen| seen.contains(path))
        .unwrap_or(false)
}

fn remember_verified_binary(path: &str) {
    if let Ok(mut seen) = VERIFIED_BINARIES.get_or_init(Default::default).lock() {
        seen.insert(path.to_string());
    }
}

async fn verify_binary_digest(cfg: &CodexConfig) -> Result<(), ProcessError> {
    if binary_already_verified(&cfg.binary) {
        return Ok(());
    }
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
    // Only a verified digest is remembered, so a mismatch is re-checked every time rather than
    // being cached into silence.
    remember_verified_binary(&cfg.binary);
    Ok(())
}

async fn verify_version(cfg: &CodexConfig, home: &str) -> Result<(), ProcessError> {
    let mut command = Command::new(&cfg.binary);
    command
        .arg("--version")
        .env_clear()
        .env("CODEX_HOME", home)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    isolate_process_group(&mut command);
    let mut child = command
        .spawn()
        .map_err(|error| ProcessError::Spawn(error.to_string()))?;
    let mut process_group = ProcessGroupGuard::new(child.id());
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| ProcessError::Spawn("version stdout pipe was unavailable".to_string()))?;
    let status = match tokio::time::timeout(
        std::time::Duration::from_millis(cfg.startup_timeout_ms.max(1)),
        child.wait(),
    )
    .await
    {
        Ok(status) => status.map_err(|error| ProcessError::Spawn(error.to_string()))?,
        Err(_) => {
            let group_result = process_group.kill();
            let _ = child.start_kill();
            let wait_result = child.wait().await.map_err(|error| {
                ProcessError::Spawn(format!("failed to reap timed-out version check: {error}"))
            });
            group_result?;
            wait_result?;
            return Err(ProcessError::Timeout("version check"));
        }
    };
    process_group.kill()?;
    if !status.success() {
        return Err(ProcessError::Spawn(format!(
            "version check exited with {}",
            status
        )));
    }
    let mut output = Vec::new();
    stdout
        .read_to_end(&mut output)
        .await
        .map_err(|error| ProcessError::Spawn(error.to_string()))?;
    let actual = String::from_utf8_lossy(&output).trim().to_string();
    if actual != cfg.expected_version {
        return Err(ProcessError::VersionMismatch {
            expected: cfg.expected_version.clone(),
            actual,
        });
    }
    Ok(())
}

struct ProcessGroupGuard {
    pid: Option<u32>,
}

impl ProcessGroupGuard {
    fn new(pid: Option<u32>) -> Self {
        Self { pid }
    }

    fn kill(&mut self) -> Result<(), ProcessError> {
        kill_process_group(self.pid)?;
        self.pid = None;
        Ok(())
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        let _ = kill_process_group(self.pid);
    }
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
}

fn kill_process_group(pid: Option<u32>) -> Result<(), ProcessError> {
    #[cfg(unix)]
    {
        let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return Ok(());
        };
        // Every Codex command starts a fresh process group whose id is its direct child's pid.
        // Killing the group prevents helpers that inherited CODEX_HOME or stdio from outliving it.
        if unsafe { libc::kill(-pid, libc::SIGKILL) } == 0 {
            return Ok(());
        }
        let error = std::io::Error::last_os_error();
        if error.raw_os_error() == Some(libc::ESRCH) {
            return Ok(());
        }
        return Err(ProcessError::Spawn(format!(
            "failed to kill Codex process group: {error}"
        )));
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(())
    }
}

async fn reap_child(
    mut child: Child,
    process_group: Option<u32>,
    ready_marker: Option<PathBuf>,
    group_result: Result<(), ProcessError>,
) -> Result<(), ProcessError> {
    let wait_result = child
        .wait()
        .await
        .map_err(|error| ProcessError::Spawn(format!("failed to reap Codex child: {error}")));
    // `Child::wait` reaps only the app-server itself. A killed helper can remain briefly as an
    // orphaned zombie until the host reaper collects it; during that window `kill -0` still sees
    // the process group. Do not publish completion while any descendant is observable.
    let reap_result = wait_for_process_group_exit(process_group).await;
    let marker_result = remove_ready_marker_path(ready_marker).await;
    group_result?;
    wait_result?;
    reap_result?;
    marker_result
}

async fn remove_ready_marker_path(marker: Option<PathBuf>) -> Result<(), ProcessError> {
    let Some(marker) = marker else {
        return Ok(());
    };
    tokio::task::spawn_blocking(move || remove_ready_marker(&marker))
        .await
        .map_err(|error| {
            ProcessError::Spawn(format!("ready-marker cleanup task failed: {error}"))
        })?
}

async fn wait_for_process_group_exit(pid: Option<u32>) -> Result<(), ProcessError> {
    #[cfg(unix)]
    {
        let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) else {
            return Ok(());
        };
        let wait = async {
            loop {
                if unsafe { libc::kill(-pid, 0) } == -1 {
                    let error = std::io::Error::last_os_error();
                    match error.raw_os_error() {
                        Some(libc::ESRCH) => return Ok(()),
                        // The group still exists but belongs to another uid. This should not occur
                        // for a Codex child; treating it as present keeps shutdown fail-closed.
                        Some(libc::EPERM) => {}
                        Some(libc::EINTR) => continue,
                        _ => {
                            return Err(ProcessError::Spawn(format!(
                                "failed to observe Codex process group: {error}"
                            )))
                        }
                    }
                }
                tokio::time::sleep(PROCESS_GROUP_REAP_POLL).await;
            }
        };
        return tokio::time::timeout(PROCESS_GROUP_REAP_TIMEOUT, wait)
            .await
            .map_err(|_| ProcessError::Spawn("Codex process group was not reaped".to_string()))?;
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        Ok(())
    }
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
    match cfg.transport {
        CodexTransport::OwnedChild => {
            match &spec.proxy {
                // A dedicated egress for this account. One address serving every ChatGPT profile
                // is itself a fleet signal, exactly as it is on the Claude side.
                Some(proxy) => {
                    for name in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"] {
                        command.env(name, proxy);
                    }
                    for name in ["http_proxy", "https_proxy", "all_proxy"] {
                        command.env(name, proxy);
                    }
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
            // Keep model-visible context as close to a normal API call as app-server permits. The
            // pinned build additionally disables built-in native tools for this client name.
            for override_value in app_server_config_overrides() {
                command.arg("--config").arg(override_value);
            }
            command.arg("app-server").arg("--listen").arg("stdio://");
        }
        CodexTransport::SharedDaemonProxy => {
            // Network egress and the authenticated store belong exclusively to the separately
            // supervised daemon. This child is only the official byte-preserving websocket/UDS
            // bridge; it inherits neither proxy credentials nor any gateway secret.
            let socket = shared_daemon_socket(&spec.path);
            command
                .arg("app-server")
                .arg("proxy")
                .arg("--sock")
                .arg(socket);
        }
    }
    command
}

fn shared_daemon_socket(home: &str) -> std::path::PathBuf {
    Path::new(APP_SERVER_RUNTIME_DIR).join(format!("{}.sock", shared_daemon_id(home)))
}

fn shared_daemon_id(home: &str) -> String {
    let name = Path::new(home)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(home);
    let digest = Sha256::digest(name.as_bytes());
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn new_client_lease() -> Result<String, ProcessError> {
    let mut random = [0u8; 16];
    getrandom::fill(&mut random)
        .map_err(|_| ProcessError::Spawn("could not generate shared client lease".to_string()))?;
    Ok(random.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn shared_client_ready_marker(
    home: &str,
    gateway_pid: u32,
    proxy_pid: u32,
    lease: &str,
) -> PathBuf {
    Path::new(APP_SERVER_RUNTIME_DIR).join(format!(
        "{}.client.{gateway_pid}.{proxy_pid}.{lease}.ready",
        shared_daemon_id(home)
    ))
}

/// Where this gateway process advertises that it has work in flight on `home`'s shared daemon.
///
/// Keyed by gateway pid alone, with neither the proxy pid nor the client lease: busy-ness is a
/// property of the gateway process rather than of one transport generation, and a reader in another
/// generation has to be able to name the file without knowing either.
fn shared_client_busy_marker(home: &str, gateway_pid: u32) -> PathBuf {
    Path::new(APP_SERVER_RUNTIME_DIR).join(format!(
        "{}.client.{gateway_pid}.busy",
        shared_daemon_id(home)
    ))
}

fn publish_ready_marker(marker: &Path) -> Result<(), ProcessError> {
    publish_marker(marker, APP_SERVER_READY_MARKER, "ready.tmp", true)
}

/// Publish or refresh this gateway's busy marker for a home.
///
/// Unlike the ready marker this never reuses an existing valid file: the modification time is the
/// evidence of freshness a reader in another generation checks, so republishing has to move it.
fn publish_busy_marker(marker: &Path) -> Result<(), ProcessError> {
    publish_marker(marker, APP_SERVER_BUSY_MARKER, "busy.tmp", false)
}

fn publish_marker(
    marker: &Path,
    sentinel: &str,
    temporary_extension: &str,
    reuse_valid: bool,
) -> Result<(), ProcessError> {
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;

    let parent = marker.parent().ok_or_else(|| {
        ProcessError::Spawn("shared ready-marker parent was unavailable".to_string())
    })?;
    let parent_metadata = std::fs::symlink_metadata(parent).map_err(|error| {
        ProcessError::Spawn(format!(
            "shared ready-marker directory was unavailable: {error}"
        ))
    })?;
    if !parent_metadata.is_dir() || parent_metadata.file_type().is_symlink() {
        return Err(ProcessError::Spawn(
            "shared ready-marker directory was unsafe".to_string(),
        ));
    }
    if reuse_valid && marker_is_valid(marker, sentinel)? {
        return Ok(());
    }
    remove_marker(marker)?;
    let temporary = marker.with_extension(temporary_extension);
    remove_marker(&temporary)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(&temporary).map_err(|error| {
        ProcessError::Spawn(format!("could not create shared ready marker: {error}"))
    })?;
    if let Err(error) = writeln!(file, "{sentinel}")
        .and_then(|()| file.sync_all())
        .and_then(|()| std::fs::rename(&temporary, marker))
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(ProcessError::Spawn(format!(
            "could not publish shared ready marker: {error}"
        )));
    }
    Ok(())
}

fn marker_is_valid(marker: &Path, sentinel: &str) -> Result<bool, ProcessError> {
    let metadata = match std::fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => {
            return Err(ProcessError::Spawn(format!(
                "could not inspect shared ready marker: {error}"
            )))
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProcessError::Spawn(
            "shared ready-marker path was unsafe".to_string(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o777 != 0o600 {
            return Err(ProcessError::Spawn(
                "shared ready-marker permissions were unsafe".to_string(),
            ));
        }
    }
    let contents = std::fs::read_to_string(marker).map_err(|error| {
        ProcessError::Spawn(format!("could not read shared ready marker: {error}"))
    })?;
    if contents != format!("{sentinel}\n") {
        return Err(ProcessError::Spawn(
            "shared ready-marker contents were invalid".to_string(),
        ));
    }
    Ok(true)
}

fn remove_ready_marker(marker: &Path) -> Result<(), ProcessError> {
    remove_marker(marker)
}

fn remove_marker(marker: &Path) -> Result<(), ProcessError> {
    let metadata = match std::fs::symlink_metadata(marker) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(ProcessError::Spawn(format!(
                "could not inspect shared ready marker: {error}"
            )))
        }
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ProcessError::Spawn(
            "shared ready-marker path was unsafe".to_string(),
        ));
    }
    std::fs::remove_file(marker).map_err(|error| {
        ProcessError::Spawn(format!("could not remove shared ready marker: {error}"))
    })
}

/// Whether a gateway generation *other than this one* currently has work in flight on the shared
/// daemon that owns `home`.
///
/// At a blue/green cutover the candidate generation has nothing of its own in flight while the old
/// generation is still serving turns over the same serialized daemon. The candidate's control probe
/// queues behind those turns and misses its deadline, and a missed deadline reads as a dead
/// transport. The in-process `inflight` counter cannot see this — it counts only our own turns, so
/// the candidate measures zero and concludes the home has gone silent. This marker is the only
/// signal that crosses generations.
///
/// A marker counts only while it is provably current: the gateway that wrote it must still exist,
/// the file must be ours and safe, and it must have been refreshed recently. Everything else is
/// reaped on sight, because a leaked marker that silently suppressed recycling forever would be a
/// worse failure than the one this prevents.
fn shared_daemon_busy(
    dir: &Path,
    home: &str,
    self_gateway_pid: u32,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
) -> bool {
    let prefix = format!("{}.client.", shared_daemon_id(home));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    let mut busy = false;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some(rest) = name.strip_prefix(&prefix) else {
            continue;
        };
        let Some(owner) = rest.strip_suffix(".busy") else {
            continue;
        };
        let Ok(owner) = owner.parse::<u32>() else {
            continue;
        };
        // Our own load is already known exactly from the in-process counter. The marker exists to
        // report the generations we cannot otherwise see, so counting ourselves here would only
        // blur which of the two signals actually fired.
        if owner == self_gateway_pid {
            continue;
        }
        if busy_marker_is_current(&entry.path(), owner, now, max_age) {
            busy = true;
        } else {
            let _ = remove_marker(&entry.path());
        }
    }
    busy
}

fn busy_marker_is_current(
    marker: &Path,
    owner: u32,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
) -> bool {
    if !gateway_process_is_alive(owner) {
        return false;
    }
    let Ok(metadata) = std::fs::symlink_metadata(marker) else {
        return false;
    };
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return false;
    }
    let Ok(modified) = metadata.modified() else {
        return false;
    };
    // A timestamp in the future can only come from clock skew, and the owner is known to be alive;
    // reading it as stale would recycle a home that is very probably serving.
    if let Ok(age) = now.duration_since(modified) {
        if age > max_age {
            return false;
        }
    }
    marker_is_valid(marker, APP_SERVER_BUSY_MARKER).unwrap_or(false)
}

/// Whether a bare pid still names a live process. Signal 0 runs the existence and permission checks
/// without delivering anything; `EPERM` means the process is there but owned by another uid.
fn gateway_process_is_alive(pid: u32) -> bool {
    #[cfg(unix)]
    {
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        if pid <= 0 {
            return false;
        }
        if unsafe { libc::kill(pid, 0) } == 0 {
            return true;
        }
        matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        )
    }
    #[cfg(not(unix))]
    {
        let _ = pid;
        false
    }
}

/// Announce that this gateway has work in flight on `home`'s shared daemon. Idempotent, and each
/// call moves the marker's timestamp forward so a reader can tell a working generation from one
/// that leaked a file and went away.
pub(crate) fn publish_shared_busy_marker(home: &str) -> Result<(), ProcessError> {
    publish_busy_marker(&shared_client_busy_marker(home, std::process::id()))
}

/// Withdraw this gateway's claim on `home`. Best-effort: a marker we fail to remove is reaped by
/// the next reader once it goes stale.
pub(crate) fn clear_shared_busy_marker(home: &str) {
    let _ = remove_marker(&shared_client_busy_marker(home, std::process::id()));
}

/// Whether another gateway generation is serving turns on `home` right now.
pub(crate) fn shared_daemon_busy_elsewhere(home: &str, max_age: std::time::Duration) -> bool {
    shared_daemon_busy(
        Path::new(APP_SERVER_RUNTIME_DIR),
        home,
        std::process::id(),
        std::time::SystemTime::now(),
        max_age,
    )
}

pub(crate) fn app_server_config_overrides() -> [&'static str; 10] {
    [
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
    ]
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
        if let Err(error) = decode_and_dispatch(&shared, &line).await {
            shared.close(error).await;
            return;
        }
    }
}

async fn websocket_reader_loop(
    mut reader: SplitStream<WebSocketStream<ChildProxyIo>>,
    shared: Arc<ProcessShared>,
    ready_marker: Option<PathBuf>,
) {
    loop {
        let message = match reader.next().await {
            Some(Ok(message)) => message,
            Some(Err(error)) => {
                close_shared_websocket(
                    &shared,
                    ProcessError::Protocol(format!("failed to read app-server websocket: {error}")),
                    ready_marker.clone(),
                )
                .await;
                return;
            }
            None => {
                close_shared_websocket(&shared, ProcessError::Closed, ready_marker.clone()).await;
                return;
            }
        };
        match message {
            Message::Text(payload) => {
                if payload.len() > MAX_JSONRPC_LINE_BYTES {
                    close_shared_websocket(
                        &shared,
                        ProcessError::Protocol(
                            "incoming JSON-RPC frame exceeded 32 MiB".to_string(),
                        ),
                        ready_marker.clone(),
                    )
                    .await;
                    return;
                }
                if let Err(error) = decode_and_dispatch(&shared, payload.as_bytes()).await {
                    close_shared_websocket(&shared, error, ready_marker.clone()).await;
                    return;
                }
            }
            Message::Binary(_) => {
                close_shared_websocket(
                    &shared,
                    ProcessError::Protocol(
                        "app-server websocket emitted a binary JSON-RPC frame".to_string(),
                    ),
                    ready_marker.clone(),
                )
                .await;
                return;
            }
            Message::Close(_) => {
                close_shared_websocket(&shared, ProcessError::Closed, ready_marker.clone()).await;
                return;
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
        }
    }
}

async fn close_shared_websocket(
    shared: &ProcessShared,
    error: ProcessError,
    ready_marker: Option<PathBuf>,
) {
    // Host-side rolling decisions must stop counting this authenticated client as soon as its
    // control channel dies, even before the next health pass invalidates and reaps the proxy.
    shared.close(error).await;
    let _ = remove_ready_marker_path(ready_marker).await;
}

async fn decode_and_dispatch(shared: &ProcessShared, payload: &[u8]) -> Result<(), ProcessError> {
    let value = serde_json::from_slice(payload)
        .map_err(|error| ProcessError::Protocol(format!("invalid JSON-RPC frame: {error}")))?;
    dispatch_value(shared, value).await
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
            .unwrap_or(false);
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
    use std::collections::BTreeMap;

    struct LiveTempTree(PathBuf);

    impl Drop for LiveTempTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn command_config(transport: CodexTransport) -> CodexConfig {
        CodexConfig {
            enabled: true,
            transport,
            ownership_lock_file: "/run/apitoken/codex-home.lock".to_string(),
            binary: "/opt/codex".to_string(),
            binary_sha256: "0".repeat(64),
            expected_version: "codex-cli test".to_string(),
            homes: vec!["/srv/codex/home-a".to_string()],
            homes_dir: None,
            work_dir: "/srv/codex/work".to_string(),
            startup_timeout_ms: 1_000,
            request_timeout_ms: 1_000,
            turn_timeout_ms: 1_000,
            turn_silence_timeout_ms: 1_000,
            health_probe_interval_secs: 300,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 60,
            history_local_cap: 16,
            history_redis_url: None,
            history_secret: None,
            history_redis_timeout_ms: 100,
            child_proxy_env: BTreeMap::from([
                ("HTTPS_PROXY".to_string(), "http://fleet-proxy".to_string()),
                ("NO_PROXY".to_string(), "127.0.0.1".to_string()),
            ]),
            models: Vec::new(),
        }
    }

    #[test]
    fn shared_daemon_transport_uses_only_the_official_websocket_proxy() {
        let spec = CodexHomeSpec {
            path: "/srv/codex/home-a".to_string(),
            id: "home-a".to_string(),
            proxy: Some("http://account-proxy".to_string()),
        };
        let command = child_command(&command_config(CodexTransport::SharedDaemonProxy), &spec);
        let command = command.as_std();
        let args = command
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert_eq!(args[..3], ["app-server", "proxy", "--sock"]);
        assert_eq!(args[3], shared_daemon_socket(&spec.path).to_string_lossy());
        assert_eq!(
            args[3],
            "/run/apitoken/codex-app-servers/746e810f65f2551f.sock"
        );
        assert!(args[3].starts_with("/run/apitoken/codex-app-servers/"));
        assert!(args[3].ends_with(".sock"));
        assert!(args[3].len() < 80);
        assert_eq!(
            shared_client_ready_marker(
                &spec.path,
                123,
                456,
                "0123456789abcdef0123456789abcdef"
            ),
            Path::new(
                "/run/apitoken/codex-app-servers/746e810f65f2551f.client.123.456.0123456789abcdef0123456789abcdef.ready"
            )
        );
        let env = command
            .get_envs()
            .filter_map(|(name, value)| {
                value.map(|value| (name.to_string_lossy(), value.to_string_lossy()))
            })
            .collect::<Vec<_>>();
        assert!(!env.iter().any(|(name, _)| name.contains("PROXY")));
    }

    #[test]
    fn shared_ready_marker_is_atomic_idempotent_and_removable() {
        let suffix = new_client_lease().expect("unique marker-test suffix");
        let root = std::env::temp_dir().join(format!("codex-ready-{suffix}"));
        std::fs::create_dir(&root).expect("create marker-test root");
        let _cleanup = LiveTempTree(root.clone());
        let marker = root.join("client.ready");

        publish_ready_marker(&marker).expect("publish marker");
        assert!(marker_is_valid(&marker, APP_SERVER_READY_MARKER).expect("validate marker"));
        publish_ready_marker(&marker).expect("idempotent publish");
        remove_ready_marker(&marker).expect("remove marker");
        assert!(!marker_is_valid(&marker, APP_SERVER_READY_MARKER).expect("marker absent"));
    }

    /// A live process id other than this one. `init` is always present, and the aliveness check
    /// counts `EPERM` — "exists, owned by somebody else" — as alive, so this works unprivileged.
    const LIVE_FOREIGN_PID: u32 = 1;

    fn busy_test_root(label: &str) -> (PathBuf, LiveTempTree) {
        let suffix = new_client_lease().expect("unique marker-test suffix");
        let root = std::env::temp_dir().join(format!("codex-{label}-{suffix}"));
        std::fs::create_dir(&root).expect("create marker-test root");
        let cleanup = LiveTempTree(root.clone());
        (root, cleanup)
    }

    fn busy_marker_in(root: &Path, home: &str, owner: u32) -> PathBuf {
        root.join(format!("{}.client.{owner}.busy", shared_daemon_id(home)))
    }

    fn a_moment() -> std::time::Duration {
        std::time::Duration::from_secs(60)
    }

    #[test]
    fn a_busy_marker_is_named_for_the_gateway_that_owns_it() {
        assert_eq!(
            shared_client_busy_marker("/srv/codex/homes/alpha", 4321),
            Path::new(&format!(
                "/run/apitoken/codex-app-servers/{}.client.4321.busy",
                shared_daemon_id("/srv/codex/homes/alpha")
            ))
        );
    }

    #[test]
    fn a_live_foreign_gateway_marks_the_daemon_busy() {
        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-live");
        let marker = busy_marker_in(&root, home, LIVE_FOREIGN_PID);
        publish_busy_marker(&marker).expect("publish busy marker");

        assert!(shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            std::time::SystemTime::now(),
            a_moment()
        ));
        assert!(marker.exists(), "a current claim must survive being read");
    }

    #[test]
    fn a_busy_marker_from_a_departed_gateway_is_not_evidence_and_is_reaped() {
        // A pid that has certainly been released: spawn a process and reap it before asking.
        let mut child = std::process::Command::new("/usr/bin/true")
            .spawn()
            .expect("spawn a short-lived child");
        let departed = child.id();
        child.wait().expect("reap the child");

        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-dead");
        let marker = busy_marker_in(&root, home, departed);
        publish_busy_marker(&marker).expect("publish busy marker");

        assert!(!shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            std::time::SystemTime::now(),
            a_moment()
        ));
        assert!(
            !marker.exists(),
            "a claim whose owner is gone must be reaped, not left to suppress recycling"
        );
    }

    #[test]
    fn a_busy_marker_that_stopped_being_refreshed_is_not_evidence_and_is_reaped() {
        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-stale");
        let marker = busy_marker_in(&root, home, LIVE_FOREIGN_PID);
        publish_busy_marker(&marker).expect("publish busy marker");

        // Ask from far enough in the future that the claim has aged out of its refresh window.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(3_600);
        assert!(!shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            later,
            a_moment()
        ));
        assert!(
            !marker.exists(),
            "a claim nobody refreshes must not defer a recycle forever"
        );
    }

    #[test]
    fn a_busy_marker_with_foreign_contents_fails_closed_and_is_reaped() {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-foreign");
        let marker = busy_marker_in(&root, home, LIVE_FOREIGN_PID);
        std::fs::write(&marker, "unexpected\n").expect("write foreign marker");
        #[cfg(unix)]
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600))
            .expect("restrict foreign marker");

        assert!(!shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            std::time::SystemTime::now(),
            a_moment()
        ));
        assert!(!marker.exists(), "a file we did not write is not a claim");
    }

    #[test]
    fn a_busy_marker_belonging_to_another_daemon_is_ignored() {
        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-other");
        let marker = busy_marker_in(&root, "/srv/codex/homes/beta", LIVE_FOREIGN_PID);
        publish_busy_marker(&marker).expect("publish busy marker");

        assert!(!shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            std::time::SystemTime::now(),
            a_moment()
        ));
        assert!(
            marker.exists(),
            "another daemon's claim is none of this home's business"
        );
    }

    #[test]
    fn our_own_busy_marker_says_nothing_about_other_generations() {
        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-self");
        let marker = busy_marker_in(&root, home, std::process::id());
        publish_busy_marker(&marker).expect("publish busy marker");

        // Our own load is already known exactly from the in-process counter; reporting it here
        // would make the two signals indistinguishable in the journal.
        assert!(!shared_daemon_busy(
            &root,
            home,
            std::process::id(),
            std::time::SystemTime::now(),
            a_moment()
        ));
        assert!(marker.exists(), "our own claim must not be reaped by us");
    }

    #[test]
    fn refreshing_a_busy_marker_moves_its_timestamp_forward() {
        let home = "/srv/codex/homes/alpha";
        let (root, _cleanup) = busy_test_root("busy-refresh");
        let marker = busy_marker_in(&root, home, LIVE_FOREIGN_PID);
        publish_busy_marker(&marker).expect("publish busy marker");
        let first = std::fs::metadata(&marker)
            .and_then(|metadata| metadata.modified())
            .expect("first timestamp");

        std::thread::sleep(std::time::Duration::from_millis(20));
        publish_busy_marker(&marker).expect("refresh busy marker");
        let second = std::fs::metadata(&marker)
            .and_then(|metadata| metadata.modified())
            .expect("second timestamp");

        // Freshness is the whole guard against a leaked claim, so a refresh that reused the
        // existing file — as the ready marker deliberately does — would silently disable it.
        assert!(
            second > first,
            "a refresh must move the timestamp that proves the claim is live"
        );
    }

    #[tokio::test]
    async fn shared_transport_closure_revokes_its_authenticated_client_marker() {
        let suffix = new_client_lease().expect("unique marker-test suffix");
        let root = std::env::temp_dir().join(format!("codex-close-{suffix}"));
        std::fs::create_dir(&root).expect("create marker-test root");
        let _cleanup = LiveTempTree(root.clone());
        let marker = root.join("client.ready");
        publish_ready_marker(&marker).expect("publish marker");
        let shared = ProcessShared::new();

        close_shared_websocket(&shared, ProcessError::Closed, Some(marker.clone())).await;

        assert!(!shared.live.load(Ordering::Acquire));
        assert!(!marker.exists());
    }

    /// Exact-protocol smoke for a locally installed official Codex binary. It is manual because CI
    /// does not carry that platform binary; pin upgrades run it with both variables set.
    #[cfg(unix)]
    #[tokio::test]
    #[ignore = "requires CODEX_LIVE_BIN and a short real CODEX_LIVE_TMP_ROOT"]
    async fn live_official_proxy_speaks_the_websocket_control_protocol() {
        use std::os::unix::fs::PermissionsExt;

        let binary =
            PathBuf::from(std::env::var_os("CODEX_LIVE_BIN").expect("CODEX_LIVE_BIN must be set"));
        let temporary_parent = PathBuf::from(
            std::env::var_os("CODEX_LIVE_TMP_ROOT")
                .expect("CODEX_LIVE_TMP_ROOT must be a short real directory"),
        );
        let suffix = new_client_lease().expect("unique live-test suffix");
        let root = temporary_parent.join(format!("codex-ws-{}", &suffix[..8]));
        std::fs::create_dir(&root).expect("create live-test root");
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
            .expect("secure live-test root");
        let _cleanup = LiveTempTree(root.clone());
        let home = root.join("home");
        std::fs::create_dir(&home).expect("create live-test home");
        std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o700))
            .expect("secure live-test home");
        let socket = root.join("a.sock");

        let mut server_command = Command::new(&binary);
        server_command
            .env_clear()
            .env("CODEX_HOME", &home)
            .env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
            .current_dir(&home)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for value in app_server_config_overrides() {
            server_command.arg("--config").arg(value);
        }
        server_command
            .arg("app-server")
            .arg("--listen")
            .arg(format!("unix://{}", socket.display()));
        let mut server = server_command.spawn().expect("start official app-server");
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while !socket.exists() {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("official app-server socket did not appear");

        let mut proxy_command = Command::new(&binary);
        proxy_command
            .env_clear()
            .env("CODEX_HOME", &home)
            .env("PATH", "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin")
            .arg("app-server")
            .arg("proxy")
            .arg("--sock")
            .arg(&socket)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut proxy = proxy_command.spawn().expect("start official stdio proxy");
        let proxy_io = ChildProxyIo {
            stdout: proxy.stdout.take().expect("proxy stdout"),
            stdin: proxy.stdin.take().expect("proxy stdin"),
        };
        let websocket_config = WebSocketConfig::default()
            .max_message_size(Some(MAX_JSONRPC_LINE_BYTES))
            .max_frame_size(Some(MAX_JSONRPC_LINE_BYTES));
        let (mut websocket, _) = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client_async_with_config("ws://localhost/", proxy_io, Some(websocket_config)),
        )
        .await
        .expect("official proxy websocket handshake timed out")
        .expect("official proxy websocket handshake failed");
        websocket
            .send(Message::Text(
                json!({
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "clientInfo": {
                            "name": "apitoken_openai_compat",
                            "title": "API Token live transport test",
                            "version": env!("CARGO_PKG_VERSION")
                        },
                        "capabilities": {"experimentalApi": true}
                    }
                })
                .to_string()
                .into(),
            ))
            .await
            .expect("send initialize");
        let response = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let message = websocket
                    .next()
                    .await
                    .expect("official websocket closed")
                    .expect("official websocket read failed");
                if let Message::Text(payload) = message {
                    let value: Value = serde_json::from_str(payload.as_ref()).expect("JSON-RPC");
                    if value.get("id").and_then(Value::as_u64) == Some(1) {
                        return value;
                    }
                }
            }
        })
        .await
        .expect("initialize response timed out");
        assert_eq!(
            response
                .pointer("/result/codexHome")
                .and_then(Value::as_str),
            home.to_str()
        );
        websocket
            .send(Message::Text(
                json!({"method": "initialized"}).to_string().into(),
            ))
            .await
            .expect("send initialized");
        let _ = websocket.close(None).await;
        let _ = proxy.start_kill();
        let _ = proxy.wait().await;
        let _ = server.start_kill();
        let _ = server.wait().await;
    }

    #[test]
    fn owned_child_keeps_the_hardened_app_server_configuration() {
        let spec = CodexHomeSpec {
            path: "/srv/codex/home-a".to_string(),
            id: "home-a".to_string(),
            proxy: None,
        };
        let command = child_command(&command_config(CodexTransport::OwnedChild), &spec);
        let args = command
            .as_std()
            .get_args()
            .map(|value| value.to_string_lossy().to_string())
            .collect::<Vec<_>>();
        assert!(args
            .windows(3)
            .any(|args| args == ["app-server", "--listen", "stdio://"]));
        for expected in app_server_config_overrides() {
            assert!(args.iter().any(|arg| arg == expected));
        }
    }

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

    #[test]
    fn one_hundred_percent_without_provider_reached_flag_is_observational() {
        let limits = parse_rate_limits(&json!({
            "primary": {
                "usedPercent": 100,
                "windowDurationMins": 300,
                "resetsAt": 4_102_444_800_i64
            },
            "secondary": null,
            "rateLimitReachedType": null,
            "spendControlReached": false
        }))
        .unwrap();
        assert_eq!(limits.primary.unwrap().used_percent, 100);
        assert!(!limits.reached);
    }

    #[tokio::test]
    async fn transport_closure_preserves_events_that_preceded_eof() {
        let shared = ProcessShared::new();
        let (sender, receiver) = mpsc::channel(1);
        sender
            .try_send(AppServerEvent::Notification {
                method: "test/noisy".to_string(),
                params: json!({}),
            })
            .unwrap();
        shared
            .turns
            .lock()
            .await
            .insert("thread-1".to_string(), sender);
        let mut events = TurnEvents {
            receiver,
            closed: shared.closed.subscribe(),
        };

        shared.close(ProcessError::Closed).await;

        assert!(matches!(
            events.recv().await,
            Ok(AppServerEvent::Notification { method, .. }) if method == "test/noisy"
        ));
        assert!(matches!(events.recv().await, Err(ProcessError::Closed)));
    }
}
