//! SHA-pinned Node transport for the private Cloud Code surface.
//!
//! Production requests never approximate Node/OpenSSL through the Claude BoringSSL stack. Every
//! encrypted profile owns one persistent, multiplexed helper process whose exact executable is
//! SHA-256 pinned. Loopback mock upstreams retain an in-process client so the protocol fault matrix
//! remains deterministic without making tests depend on a host Node installation.

use super::config::GeminiConfig;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use bytes::Bytes;
use futures_util::stream::BoxStream;
use futures_util::{Stream, StreamExt, TryStreamExt};
use gemini_credential::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::pin::Pin;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use super::transport_v2::{read_raw_frame, write_raw_frame_locked};

const IPC_PROTOCOL: u8 = 2;
pub(super) const IPC_KIND_CONTROL: u8 = 1;
pub(super) const IPC_KIND_DATA: u8 = 2;
pub(super) const IPC_HEADER_BYTES: usize = 13;
pub(super) const MAX_IPC_CONTROL_BYTES: usize = 1024 * 1024;
pub(super) const MAX_IPC_BODY_BYTES: usize = 256 * 1024 * 1024;
pub(super) const MAX_IPC_DATA_CHUNK_BYTES: usize = 1024 * 1024;
const BODY_CHANNEL_CHUNKS: usize = 256;
const CANCELED_TOMBSTONE_TTL: Duration = Duration::from_secs(300);
const MAX_CANCELED_TOMBSTONES: usize = 8_192;
const HELPER_SOURCE: &str = include_str!("node_transport.cjs");

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigureFrame<'a> {
    r#type: &'static str,
    protocol: u8,
    proxy: &'a str,
    connect_timeout_ms: u64,
    read_timeout_ms: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RequestFrame<'a> {
    r#type: &'static str,
    id: u64,
    method: &'static str,
    url: &'a str,
    headers: &'a [(&'a str, &'a str)],
    body_length: u64,
    /// Ask the pinned helper to return a content-free proof at its final socket submission seam.
    /// Omitted for every non-observed call so the ordinary protocol remains byte-identical.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    observe_actual_send: bool,
    /// Per-request silence bound in milliseconds. Absent means "use the process-wide value from
    /// the configure frame", which keeps other embedders of this helper (authbot) working
    /// unchanged; an explicit `0` means "no deadline", which is what customer generation sends.
    #[serde(skip_serializing_if = "Option::is_none")]
    read_timeout_ms: Option<u64>,
    /// Absolute Unix seconds for the private exact-profile dispatch fence. This is IPC metadata,
    /// never an HTTP header; Node converts it to an exact millisecond boundary at socket handoff.
    #[serde(skip_serializing_if = "Option::is_none")]
    calibration_not_after: Option<u64>,
}

#[derive(Serialize)]
struct CancelFrame {
    r#type: &'static str,
    id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportError {
    Spawn,
    Closed,
    Timeout,
    Network,
    Protocol,
    BodyTooLarge,
    /// The private dispatch fence expired inside Rust or at a Node pre-socket boundary. No
    /// provider HTTP request was emitted, so callers retain an exact not-started proof.
    CalibrationExpired,
}

impl TransportError {
    fn helper_restartable(self) -> bool {
        matches!(self, Self::Spawn | Self::Closed | Self::Protocol)
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Spawn => "Gemini transport could not start",
            Self::Closed => "Gemini transport closed",
            Self::Timeout => "Gemini transport timed out",
            Self::Network => "Gemini transport network failure",
            Self::Protocol => "Gemini transport protocol failure",
            Self::BodyTooLarge => "Gemini transport response exceeded its limit",
            Self::CalibrationExpired => "Gemini calibration dispatch window expired",
        })
    }
}

impl std::error::Error for TransportError {}

pub(crate) struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: BoxStream<'static, Result<Bytes, TransportError>>,
    calibration_dispatch_ms: Option<u64>,
}

impl TransportResponse {
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }

    pub(crate) fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    pub(crate) fn content_length(&self) -> Option<u64> {
        self.headers
            .get("content-length")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
    }

    pub(crate) fn calibration_dispatch_ms(&self) -> Option<u64> {
        self.calibration_dispatch_ms
    }

    pub(crate) fn bytes_stream(self) -> BoxStream<'static, Result<Bytes, TransportError>> {
        self.body
    }

    pub(crate) async fn bytes_limited(mut self, limit: usize) -> Result<Bytes, TransportError> {
        let mut collected = Vec::new();
        while let Some(chunk) = self.body.next().await {
            let chunk = chunk?;
            if collected.len().saturating_add(chunk.len()) > limit {
                return Err(TransportError::BodyTooLarge);
            }
            collected.extend_from_slice(&chunk);
        }
        Ok(Bytes::from(collected))
    }

    pub(crate) async fn bytes_limited_zeroizing(
        mut self,
        limit: usize,
    ) -> Result<Zeroizing<Vec<u8>>, TransportError> {
        let mut collected = Zeroizing::new(Vec::new());
        while let Some(chunk) = self.body.next().await {
            let chunk = chunk?;
            if collected.len().saturating_add(chunk.len()) > limit {
                return Err(TransportError::BodyTooLarge);
            }
            collected.extend_from_slice(&chunk);
        }
        Ok(collected)
    }
}

/// Content-free observer shared only with request-fact instrumentation. Each increment is made at
/// the transport's final actual-submission boundary; overflow is sticky and renders the result
/// unknown rather than publishing a guessed count.
#[derive(Clone, Debug, Default)]
pub struct ActualSendObserver {
    count: Arc<AtomicU64>,
    notify: Arc<tokio::sync::Notify>,
    require_ack: bool,
    acknowledged: Arc<AtomicBool>,
    ack_notify: Arc<tokio::sync::Notify>,
}

const ACTUAL_SEND_OVERFLOW: u64 = u64::MAX;

impl ActualSendObserver {
    pub fn acknowledged() -> Self {
        Self {
            require_ack: true,
            ..Self::default()
        }
    }
    pub(crate) async fn record(&self) {
        self.record_now();
        if self.require_ack && !self.acknowledged.load(Ordering::Acquire) {
            self.ack_notify.notified().await;
        }
    }
    fn record_now(&self) {
        let _ = self
            .count
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                (current != ACTUAL_SEND_OVERFLOW)
                    .then(|| current.checked_add(1).unwrap_or(ACTUAL_SEND_OVERFLOW))
            });
        self.notify.notify_waiters();
    }

    pub async fn observed(&self) {
        if self.count().unwrap_or(0) == 0 {
            self.notify.notified().await;
        }
    }
    pub fn acknowledge(&self) {
        self.acknowledged.store(true, Ordering::Release);
        self.ack_notify.notify_waiters();
    }

    pub fn count(&self) -> Option<u64> {
        match self.count.load(Ordering::Relaxed) {
            ACTUAL_SEND_OVERFLOW => None,
            count => Some(count),
        }
    }
}

pub(crate) struct TransportRequest<'a> {
    pub(crate) url: &'a str,
    pub(crate) headers: Vec<(&'static str, SecretString)>,
    pub(crate) body: Bytes,
    pub(crate) actual_send_observer: Option<ActualSendObserver>,
    /// How long this specific call may stay silent before it is treated as dead, or `None` for no
    /// deadline at all. Generation and token refresh have opposite needs: a reasoning model
    /// legitimately produces nothing for minutes, while a hung token call must fail fast so the
    /// profile can rotate.
    ///
    /// Customer generation passes `None` on purpose. Time cannot answer "is this dead" — that is
    /// what the TCP keepalive probes are for, and they answer it without caring how long the
    /// request has been running. Any wall-clock value here would be a bet on how long a model is
    /// allowed to think, and some customer's task always eventually exceeds the bet.
    pub(crate) idle_timeout: Option<Duration>,
    /// Whether a helper failure before response headers may restart the helper and submit the same
    /// POST again. Exact-target generation is a paid one-shot: helper closure/protocol failure is
    /// ambiguous after the IPC frame is flushed, so that path must never replay. Auxiliary reads,
    /// OAuth refresh and ordinary traffic retain the established single helper restart.
    pub(crate) retry_policy: TransportRetryPolicy,
    /// Absolute Unix seconds accepted only on the private exact-profile lane. It is checked once
    /// more by Rust immediately before this call and then by Node at its final socket boundary.
    pub(crate) calibration_not_after: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TransportRetryPolicy {
    RestartHelperOnce,
    NeverReplay,
}

impl TransportRetryPolicy {
    fn max_helper_attempts(self) -> usize {
        match self {
            Self::RestartHelperOnce => 2,
            Self::NeverReplay => 1,
        }
    }

    fn may_restart_helper(self, attempt: usize, error: TransportError) -> bool {
        self == Self::RestartHelperOnce && attempt == 0 && error.helper_restartable()
    }
}

pub(crate) enum ProfileTransport {
    Loopback(wreq::Client),
    Node(NodeTransport),
}

impl ProfileTransport {
    pub(crate) fn new(cfg: &GeminiConfig, proxy: SecretString) -> anyhow::Result<Self> {
        if cfg.upstream.starts_with("http://") {
            let mut builder = wreq::Client::builder()
                .no_proxy()
                .redirect(wreq::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(cfg.connect_timeout_secs))
                // No client-wide read timeout: every send decides for itself, and customer
                // generation deliberately decides "none". A default here would silently reimpose
                // the ceiling this split exists to remove.
                .pool_idle_timeout(Duration::from_secs(90))
                .tcp_keepalive(Duration::from_secs(60));
            if !proxy.is_empty() {
                builder = builder.proxy(wreq::Proxy::all(proxy.as_str())?);
            }
            return Ok(Self::Loopback(builder.build()?));
        }
        Ok(Self::Node(NodeTransport::new(cfg, proxy)?))
    }

    pub(crate) async fn send(
        &self,
        request: TransportRequest<'_>,
    ) -> Result<TransportResponse, TransportError> {
        if request.calibration_not_after.is_some()
            && request.retry_policy != TransportRetryPolicy::NeverReplay
        {
            return Err(TransportError::Protocol);
        }
        match self {
            Self::Loopback(client) => {
                let mut builder = client.post(request.url);
                if let Some(idle) = request.idle_timeout {
                    builder = builder.read_timeout(idle);
                }
                for (name, value) in request.headers {
                    builder = builder.header(name, value.as_str());
                }
                // Literal loopback is the deterministic mock path and never enters Node. Mirror
                // the same strict boundary immediately before wreq opens its local socket.
                let calibration_dispatch_ms = request
                    .calibration_not_after
                    .map(calibration_dispatch_ms)
                    .transpose()?;
                if let Some(observer) = &request.actual_send_observer {
                    observer.record().await;
                }
                let response = builder
                    .body(request.body)
                    .send()
                    .await
                    .map_err(|_| TransportError::Network)?;
                let status = StatusCode::from_u16(response.status().as_u16())
                    .map_err(|_| TransportError::Protocol)?;
                let mut headers = HeaderMap::new();
                for (name, value) in response.headers() {
                    let name = HeaderName::from_bytes(name.as_str().as_bytes())
                        .map_err(|_| TransportError::Protocol)?;
                    let value = HeaderValue::from_bytes(value.as_bytes())
                        .map_err(|_| TransportError::Protocol)?;
                    headers.append(name, value);
                }
                let body = response
                    .bytes_stream()
                    .map_err(|_| TransportError::Network)
                    .boxed();
                Ok(TransportResponse {
                    status,
                    headers,
                    body,
                    calibration_dispatch_ms,
                })
            }
            Self::Node(transport) => transport.send(request).await,
        }
    }

    pub(crate) async fn shutdown(&self) {
        if let Self::Node(transport) = self {
            transport.shutdown().await;
        }
    }
}

fn unix_epoch_millis() -> Result<u64, TransportError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| TransportError::CalibrationExpired)?
        .as_millis();
    u64::try_from(millis).map_err(|_| TransportError::CalibrationExpired)
}

fn calibration_dispatch_ms(not_after: u64) -> Result<u64, TransportError> {
    let deadline_ms = not_after
        .checked_mul(1_000)
        .ok_or(TransportError::CalibrationExpired)?;
    let dispatch_ms = unix_epoch_millis()?;
    (dispatch_ms > 0 && dispatch_ms < deadline_ms)
        .then_some(dispatch_ms)
        .ok_or(TransportError::CalibrationExpired)
}

/// Validate the exact executable before any profile process can join rotation. Version text alone
/// is insufficient: distribution rebuilds of the same Node release can carry a different OpenSSL
/// and therefore a different TLS fingerprint.
pub(crate) fn attest_node_binary(cfg: &GeminiConfig) -> anyhow::Result<()> {
    let path = Path::new(&cfg.node_binary);
    if !path.is_absolute() || !path.is_file() {
        anyhow::bail!("Gemini Node transport binary must be an absolute regular file");
    }
    if cfg.node_sha256.len() != 64
        || !cfg
            .node_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        anyhow::bail!("Gemini Node transport SHA-256 must be lowercase hexadecimal");
    }
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != cfg.node_sha256 {
        anyhow::bail!("Gemini Node transport binary attestation failed");
    }
    if cfg.node_version.is_empty()
        || cfg.node_version.len() > 64
        || !cfg.node_version.starts_with('v')
        || !cfg
            .node_version
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'v' | b'.' | b'-'))
    {
        anyhow::bail!("Gemini Node transport version is invalid");
    }
    Ok(())
}

pub(crate) struct NodeTransport {
    config: NodeProcessConfig,
    state: tokio::sync::Mutex<Option<Arc<NodeProcess>>>,
    next_request: AtomicU64,
}

struct NodeProcessConfig {
    binary: String,
    version: String,
    proxy: SecretString,
    connect_timeout_ms: u64,
    read_timeout_ms: u64,
}

impl NodeTransport {
    fn new(cfg: &GeminiConfig, proxy: SecretString) -> anyhow::Result<Self> {
        validate_proxy(&proxy)?;
        Ok(Self {
            config: NodeProcessConfig {
                binary: cfg.node_binary.clone(),
                version: cfg.node_version.clone(),
                proxy,
                connect_timeout_ms: cfg.connect_timeout_secs.saturating_mul(1_000),
                read_timeout_ms: cfg.read_timeout_secs.saturating_mul(1_000),
            },
            state: tokio::sync::Mutex::new(None),
            next_request: AtomicU64::new(1),
        })
    }

    async fn process(&self) -> Result<Arc<NodeProcess>, TransportError> {
        let mut state = self.state.lock().await;
        if let Some(process) = state.as_ref().filter(|process| !process.is_closed()) {
            return Ok(process.clone());
        }
        let process = Arc::new(NodeProcess::spawn(&self.config).await?);
        *state = Some(process.clone());
        Ok(process)
    }

    async fn invalidate(&self, process: &Arc<NodeProcess>) {
        let removed = {
            let mut state = self.state.lock().await;
            if state
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, process))
            {
                state.take()
            } else {
                None
            }
        };
        if let Some(process) = removed {
            process.shutdown().await;
        }
    }

    async fn send(
        &self,
        request: TransportRequest<'_>,
    ) -> Result<TransportResponse, TransportError> {
        let id = self.next_request.fetch_add(1, Ordering::Relaxed).max(1);
        for attempt in 0..request.retry_policy.max_helper_attempts() {
            let process = self.process().await?;
            match process.request(id, &request).await {
                Ok(response) => return Ok(response),
                Err(error) if request.retry_policy.may_restart_helper(attempt, error) => {
                    self.invalidate(&process).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(TransportError::Closed)
    }

    async fn shutdown(&self) {
        let process = self.state.lock().await.take();
        if let Some(process) = process {
            process.shutdown().await;
        }
    }
}

fn validate_proxy(proxy: &str) -> anyhow::Result<()> {
    gemini_credential::normalize_proxy_url(proxy)?;
    Ok(())
}

struct PendingRequest {
    headers: Option<oneshot::Sender<Result<ResponseHead, TransportError>>>,
    body: mpsc::Sender<Result<Bytes, TransportError>>,
    calibration_not_after: Option<u64>,
    actual_send_observer: Option<ActualSendObserver>,
}

struct ProcessShared {
    pending: Mutex<HashMap<u64, PendingRequest>>,
    /// A cancellation can race frames that Node already committed to stdout. Keep a bounded,
    /// short-lived tombstone so those frames are ignored without treating the whole multiplexed
    /// helper as corrupt and failing unrelated requests on the same profile.
    canceled: Mutex<HashMap<u64, std::time::Instant>>,
    ready: Mutex<Option<oneshot::Sender<Result<ReadyFrame, TransportError>>>>,
    closed: AtomicBool,
}

impl ProcessShared {
    fn mark_canceled(&self, id: u64) -> bool {
        let now = std::time::Instant::now();
        let mut canceled = self
            .canceled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        canceled.retain(|_, inserted| now.duration_since(*inserted) < CANCELED_TOMBSTONE_TTL);
        if canceled.len() >= MAX_CANCELED_TOMBSTONES {
            elog::warn("gemini", "gemini helper tombstone overflow; closing");
            return false;
        }
        canceled.insert(id, now);
        true
    }

    fn consume_canceled_frame(&self, id: u64, terminal: bool) -> bool {
        let now = std::time::Instant::now();
        let mut canceled = self
            .canceled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        canceled.retain(|_, inserted| now.duration_since(*inserted) < CANCELED_TOMBSTONE_TTL);
        if terminal {
            canceled.remove(&id).is_some()
        } else {
            canceled.contains_key(&id)
        }
    }

    fn close(&self, error: TransportError) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        if let Some(ready) = self
            .ready
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = ready.send(Err(error));
        }
        let pending = std::mem::take(
            &mut *self
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for (_, mut request) in pending {
            if let Some(headers) = request.headers.take() {
                let _ = headers.send(Err(error));
            } else {
                let _ = request.body.try_send(Err(error));
            }
        }
    }
}

struct NodeProcess {
    writer: Arc<tokio::sync::Mutex<ChildStdin>>,
    shared: Arc<ProcessShared>,
    cancel: mpsc::UnboundedSender<u64>,
    kill: Mutex<Option<oneshot::Sender<()>>>,
    exited: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
}

impl NodeProcess {
    async fn spawn(config: &NodeProcessConfig) -> Result<Self, TransportError> {
        let mut command = Command::new(&config.binary);
        command
            .arg("--expose-internals")
            .arg("-e")
            .arg(HELPER_SOURCE)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        isolate_process_group(&mut command);
        let mut child = command.spawn().map_err(|_| TransportError::Spawn)?;
        let pid = child.id();
        let stdin = child.stdin.take().ok_or(TransportError::Spawn)?;
        let stdout = child.stdout.take().ok_or(TransportError::Spawn)?;
        let stderr = child.stderr.take().ok_or(TransportError::Spawn)?;
        let (ready_tx, ready_rx) = oneshot::channel();
        let shared = Arc::new(ProcessShared {
            pending: Mutex::new(HashMap::new()),
            canceled: Mutex::new(HashMap::new()),
            ready: Mutex::new(Some(ready_tx)),
            closed: AtomicBool::new(false),
        });
        tokio::spawn(reader_loop(BufReader::new(stdout), shared.clone()));
        tokio::spawn(drain_stderr(stderr));
        let (kill_tx, kill_rx) = oneshot::channel();
        let exited = Arc::new(AtomicBool::new(false));
        let exit_notify = Arc::new(tokio::sync::Notify::new());
        tokio::spawn(waiter_loop(
            child,
            pid,
            kill_rx,
            shared.clone(),
            exited.clone(),
            exit_notify.clone(),
        ));
        let writer = Arc::new(tokio::sync::Mutex::new(stdin));
        let (cancel, cancel_rx) = mpsc::unbounded_channel();
        tokio::spawn(cancel_writer_loop(
            writer.clone(),
            cancel_rx,
            shared.clone(),
        ));
        let process = Self {
            writer,
            shared,
            cancel,
            kill: Mutex::new(Some(kill_tx)),
            exited,
            exit_notify,
        };
        let configure = ConfigureFrame {
            r#type: "configure",
            protocol: IPC_PROTOCOL,
            proxy: config.proxy.as_str(),
            connect_timeout_ms: config.connect_timeout_ms,
            read_timeout_ms: config.read_timeout_ms,
        };
        process.write_frame(&configure).await?;
        let startup_timeout =
            Duration::from_millis(config.connect_timeout_ms.clamp(1_000, 120_000));
        let ready = tokio::time::timeout(startup_timeout, ready_rx)
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(|_| TransportError::Closed)??;
        if ready.protocol != IPC_PROTOCOL
            || ready.node != config.version
            || ready.platform != "linux"
            || ready.arch != "x64"
            || ready.undici != "node-internal"
        {
            process.shared.close(TransportError::Protocol);
            process.signal_kill();
            return Err(TransportError::Protocol);
        }
        Ok(process)
    }

    fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Acquire)
    }

    async fn request(
        &self,
        id: u64,
        request: &TransportRequest<'_>,
    ) -> Result<TransportResponse, TransportError> {
        if self.is_closed() {
            return Err(TransportError::Closed);
        }
        let (headers_tx, headers_rx) = oneshot::channel();
        let (body_tx, body_rx) = mpsc::channel(BODY_CHANNEL_CHUNKS);
        {
            let mut pending = self
                .shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if pending
                .insert(
                    id,
                    PendingRequest {
                        headers: Some(headers_tx),
                        body: body_tx,
                        calibration_not_after: request.calibration_not_after,
                        actual_send_observer: request.actual_send_observer.clone(),
                    },
                )
                .is_some()
            {
                self.shared.close(TransportError::Protocol);
                return Err(TransportError::Protocol);
            }
        }
        // If the caller disappears before the complete response body, remove the pending IPC
        // request and asynchronously cancel its Node socket. Without this RAII edge, rapid client
        // disconnects or a bounded-body rejection could release Rust admission while leaving an
        // unbounded tail of helper requests alive until their read timeout.
        let cancel_guard = PendingCancelGuard::new(id, self.shared.clone(), self.cancel.clone());
        let headers = request
            .headers
            .iter()
            .map(|(name, value)| (*name, value.as_str()))
            .collect::<Vec<_>>();
        let body_length =
            u64::try_from(request.body.len()).map_err(|_| TransportError::Protocol)?;
        if request.body.len() > MAX_IPC_BODY_BYTES {
            return Err(TransportError::Protocol);
        }
        let frame = RequestFrame {
            r#type: "request",
            id,
            method: "POST",
            url: request.url,
            headers: &headers,
            body_length,
            observe_actual_send: request.actual_send_observer.is_some(),
            read_timeout_ms: Some(request.idle_timeout.map_or(0, |idle| {
                u64::try_from(idle.as_millis()).unwrap_or(u64::MAX)
            })),
            calibration_not_after: request.calibration_not_after,
        };
        if let Err(error) = self.write_request_frame(id, &frame, &request.body).await {
            return Err(error);
        }
        let head = headers_rx.await.map_err(|_| TransportError::Closed)??;
        let body = ResponseBody {
            receiver: body_rx,
            cancel: Some(cancel_guard),
        }
        .boxed();
        Ok(TransportResponse {
            status: head.status,
            headers: head.headers,
            body,
            calibration_dispatch_ms: head.calibration_dispatch_ms,
        })
    }

    async fn write_frame(&self, frame: &(impl Serialize + ?Sized)) -> Result<(), TransportError> {
        let encoded =
            Zeroizing::new(serde_json::to_vec(frame).map_err(|_| TransportError::Protocol)?);
        self.write_raw_frame(IPC_KIND_CONTROL, 0, &encoded).await
    }

    async fn write_request_frame(
        &self,
        id: u64,
        frame: &(impl Serialize + ?Sized),
        body: &[u8],
    ) -> Result<(), TransportError> {
        let encoded =
            Zeroizing::new(serde_json::to_vec(frame).map_err(|_| TransportError::Protocol)?);
        if encoded.len() > MAX_IPC_CONTROL_BYTES || body.len() > MAX_IPC_BODY_BYTES {
            return Err(TransportError::Protocol);
        }
        let mut writer = self.writer.lock().await;
        write_raw_frame_locked(&mut *writer, IPC_KIND_CONTROL, 0, &encoded).await?;
        write_raw_frame_locked(&mut *writer, IPC_KIND_DATA, id, body).await?;
        writer.flush().await.map_err(|_| TransportError::Closed)
    }

    async fn write_raw_frame(
        &self,
        kind: u8,
        id: u64,
        payload: &[u8],
    ) -> Result<(), TransportError> {
        let mut writer = self.writer.lock().await;
        write_raw_frame_locked(&mut *writer, kind, id, payload).await?;
        writer.flush().await.map_err(|_| TransportError::Closed)
    }

    fn signal_kill(&self) {
        if let Some(kill) = self
            .kill
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            let _ = kill.send(());
        }
    }

    async fn shutdown(&self) {
        self.signal_kill();
        loop {
            let notified = self.exit_notify.notified();
            if self.exited.load(Ordering::Acquire) {
                return;
            }
            if tokio::time::timeout(Duration::from_secs(5), notified)
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

struct ResponseBody {
    receiver: mpsc::Receiver<Result<Bytes, TransportError>>,
    cancel: Option<PendingCancelGuard>,
}

impl Stream for ResponseBody {
    type Item = Result<Bytes, TransportError>;

    fn poll_next(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let result = self.receiver.poll_recv(context);
        if matches!(result, Poll::Ready(None)) {
            if let Some(mut cancel) = self.cancel.take() {
                cancel.disarm();
            }
        }
        result
    }
}

struct PendingCancelGuard {
    id: u64,
    shared: Arc<ProcessShared>,
    cancel: mpsc::UnboundedSender<u64>,
    armed: bool,
}

impl PendingCancelGuard {
    fn new(id: u64, shared: Arc<ProcessShared>, cancel: mpsc::UnboundedSender<u64>) -> Self {
        Self {
            id,
            shared,
            cancel,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PendingCancelGuard {
    fn drop(&mut self) {
        if !self.armed || self.shared.closed.load(Ordering::Acquire) {
            return;
        }
        // Publish the tombstone before removing the pending entry. The reader checks tombstones
        // first, so there is no interval in which an already-queued frame can observe neither the
        // pending request nor its cancellation marker and close the multiplexed helper.
        if !self.shared.mark_canceled(self.id) {
            self.shared.close(TransportError::Protocol);
            return;
        }
        let removed = self
            .shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.id)
            .is_some();
        if removed {
            let _ = self.cancel.send(self.id);
        } else {
            self.shared.consume_canceled_frame(self.id, true);
        }
    }
}

impl Drop for NodeProcess {
    fn drop(&mut self) {
        self.signal_kill();
    }
}

struct ResponseHead {
    status: StatusCode,
    headers: HeaderMap,
    calibration_dispatch_ms: Option<u64>,
}

struct ReadyFrame {
    protocol: u8,
    node: String,
    platform: String,
    arch: String,
    undici: String,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
struct InboundFrame {
    #[serde(rename = "type")]
    frame_type: String,
    #[serde(default)]
    #[zeroize(skip)]
    protocol: Option<u8>,
    #[serde(default)]
    node: Option<String>,
    #[serde(default)]
    platform: Option<String>,
    #[serde(default)]
    arch: Option<String>,
    #[serde(default)]
    undici: Option<String>,
    #[serde(default)]
    #[zeroize(skip)]
    id: Option<u64>,
    #[serde(default)]
    #[zeroize(skip)]
    status: Option<u16>,
    #[serde(default)]
    #[zeroize(skip)]
    calibration_dispatch_ms: Option<u64>,
    #[serde(default)]
    #[zeroize(skip)]
    actual_send: Option<bool>,
    #[serde(default)]
    headers: Vec<String>,
    #[serde(default, rename = "kind")]
    error_kind: Option<String>,
}

async fn reader_loop<R>(mut reader: R, shared: Arc<ProcessShared>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    loop {
        let (kind, id, payload) = match read_raw_frame(&mut reader).await {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                elog::error("gemini", "gemini helper reader failed: EOF");
                shared.close(TransportError::Closed);
                return;
            }
            Err(error) => {
                elog::error("gemini", format!("gemini helper reader failed: {error}"));
                shared.close(error);
                return;
            }
        };
        if kind == IPC_KIND_DATA {
            if dispatch_data_frame(&shared, id, Bytes::from(payload))
                .await
                .is_err()
            {
                shared.close(TransportError::Protocol);
                return;
            }
            continue;
        }
        if kind != IPC_KIND_CONTROL || id != 0 {
            shared.close(TransportError::Protocol);
            return;
        }
        let value: InboundFrame = match serde_json::from_slice(&payload) {
            Ok(value) => value,
            Err(_) => {
                elog::error("gemini", "gemini helper reader failed: malformed frame");
                shared.close(TransportError::Protocol);
                return;
            }
        };
        if dispatch_frame(&shared, value).await.is_err() {
            elog::error("gemini", "gemini helper reader failed: dispatch error");
            shared.close(TransportError::Protocol);
            return;
        }
    }
}

async fn dispatch_data_frame(
    shared: &Arc<ProcessShared>,
    id: u64,
    bytes: Bytes,
) -> Result<(), TransportError> {
    if shared.consume_canceled_frame(id, false) {
        return Ok(());
    }
    let sender = {
        let pending = shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let request = pending.get(&id).ok_or(TransportError::Protocol)?;
        if request.headers.is_some() {
            return Err(TransportError::Protocol);
        }
        request.body.clone()
    };
    sender
        .send(Ok(bytes))
        .await
        .map_err(|_| TransportError::Closed)
}

async fn dispatch_frame(
    shared: &Arc<ProcessShared>,
    mut value: InboundFrame,
) -> Result<(), TransportError> {
    if value.frame_type == "ready" {
        let ready = ReadyFrame {
            protocol: value.protocol.ok_or(TransportError::Protocol)?,
            node: value.node.take().ok_or(TransportError::Protocol)?,
            platform: value.platform.take().ok_or(TransportError::Protocol)?,
            arch: value.arch.take().ok_or(TransportError::Protocol)?,
            undici: value.undici.take().ok_or(TransportError::Protocol)?,
        };
        let sender = shared
            .ready
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
            .ok_or(TransportError::Protocol)?;
        let _ = sender.send(Ok(ready));
        return Ok(());
    }
    let id = value.id.ok_or(TransportError::Protocol)?;
    if shared.consume_canceled_frame(id, matches!(value.frame_type.as_str(), "end" | "error")) {
        return Ok(());
    }
    match value.frame_type.as_str() {
        "actual_send" => {
            if value.actual_send != Some(true) {
                return Err(TransportError::Protocol);
            }
            let observer = {
                let pending = shared
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending
                    .get(&id)
                    .and_then(|request| request.actual_send_observer.clone())
                    .ok_or(TransportError::Protocol)?
            };
            observer.record().await;
        }
        "headers" => {
            let status = value
                .status
                .and_then(|status| StatusCode::from_u16(status).ok())
                .ok_or(TransportError::Protocol)?;
            let raw = &value.headers;
            if raw.len() > 512 || raw.len() % 2 != 0 {
                return Err(TransportError::Protocol);
            }
            let mut headers = HeaderMap::new();
            for pair in raw.chunks_exact(2) {
                let name = HeaderName::from_bytes(pair[0].as_bytes())
                    .ok()
                    .ok_or(TransportError::Protocol)?;
                let value = HeaderValue::from_bytes(pair[1].as_bytes())
                    .ok()
                    .ok_or(TransportError::Protocol)?;
                headers.append(name, value);
            }
            let (sender, calibration_not_after) = {
                let mut pending = shared
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let request = pending.get_mut(&id).ok_or(TransportError::Protocol)?;
                let sender = request.headers.take().ok_or(TransportError::Protocol)?;
                (sender, request.calibration_not_after)
            };
            let calibration_dispatch_ms = validate_dispatch_attestation(
                calibration_not_after,
                value.calibration_dispatch_ms,
            )?;
            let _ = sender.send(Ok(ResponseHead {
                status,
                headers,
                calibration_dispatch_ms,
            }));
        }
        "end" => {
            let request = shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id)
                .ok_or(TransportError::Protocol)?;
            if request.headers.is_some() {
                return Err(TransportError::Protocol);
            }
        }
        "error" => {
            let error =
                helper_error_kind(value.error_kind.as_deref()).ok_or(TransportError::Protocol)?;
            let mut request = shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(&id)
                .ok_or(TransportError::Protocol)?;
            if let Some(headers) = request.headers.take() {
                let _ = headers.send(Err(error));
            } else {
                let _ = request.body.send(Err(error)).await;
            }
        }
        _ => return Err(TransportError::Protocol),
    }
    Ok(())
}

fn helper_error_kind(kind: Option<&str>) -> Option<TransportError> {
    match kind {
        Some("timeout") => Some(TransportError::Timeout),
        Some(
            "proxy-timeout" | "proxy-auth" | "proxy-throttle" | "proxy-rejected" | "proxy-upstream"
            | "proxy-connect" | "proxy-eof" | "proxy-protocol" | "tls" | "network",
        ) => Some(TransportError::Network),
        Some("protocol") => Some(TransportError::Protocol),
        Some("calibration-expired") => Some(TransportError::CalibrationExpired),
        _ => None,
    }
}

fn validate_dispatch_attestation(
    not_after: Option<u64>,
    dispatch_ms: Option<u64>,
) -> Result<Option<u64>, TransportError> {
    match (not_after, dispatch_ms) {
        (None, None) => Ok(None),
        (Some(not_after), Some(dispatch_ms)) => {
            let deadline_ms = not_after
                .checked_mul(1_000)
                .ok_or(TransportError::Protocol)?;
            (dispatch_ms > 0 && dispatch_ms < deadline_ms)
                .then_some(Some(dispatch_ms))
                .ok_or(TransportError::Protocol)
        }
        // An attestation is mandatory on the deadline-bound lane and forbidden everywhere else.
        (None, Some(_)) | (Some(_), None) => Err(TransportError::Protocol),
    }
}

async fn drain_stderr<R>(mut stderr: R)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buffer = [0u8; 4 * 1024];
    let mut observed = false;
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) => break,
            Ok(_) => observed = true,
            Err(_) => break,
        }
    }
    if observed {
        elog::warn(
            "gemini",
            "Gemini Node transport emitted redacted diagnostics",
        );
    }
}

async fn cancel_writer_loop(
    writer: Arc<tokio::sync::Mutex<ChildStdin>>,
    mut receiver: mpsc::UnboundedReceiver<u64>,
    shared: Arc<ProcessShared>,
) {
    while let Some(id) = receiver.recv().await {
        if shared.closed.load(Ordering::Acquire) {
            return;
        }
        let encoded = match serde_json::to_vec(&CancelFrame {
            r#type: "cancel",
            id,
        }) {
            Ok(encoded) => Zeroizing::new(encoded),
            Err(_) => {
                shared.close(TransportError::Protocol);
                return;
            }
        };
        let mut writer = writer.lock().await;
        if writer.write_all(&encoded).await.is_err()
            || writer.write_all(b"\n").await.is_err()
            || writer.flush().await.is_err()
        {
            shared.close(TransportError::Closed);
            return;
        }
    }
}

async fn waiter_loop(
    mut child: tokio::process::Child,
    pid: Option<u32>,
    kill: oneshot::Receiver<()>,
    shared: Arc<ProcessShared>,
    exited: Arc<AtomicBool>,
    exit_notify: Arc<tokio::sync::Notify>,
) {
    tokio::select! {
        _ = child.wait() => {}
        _ = kill => {
            kill_process_group(pid);
            let _ = child.start_kill();
            let _ = child.wait().await;
        }
    }
    shared.close(TransportError::Closed);
    exited.store(true, Ordering::Release);
    exit_notify.notify_waiters();
}

fn isolate_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.as_std_mut().process_group(0);
    }
}

fn kill_process_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The per-request silence bound is the whole point of the split: generation and token refresh
    /// travel over the same helper process and must not share one deadline. Absence must stay
    /// representable so other embedders of this helper keep working against the configure-frame
    /// value.
    #[test]
    fn request_frame_carries_an_optional_camel_case_idle_bound() {
        let headers: Vec<(&str, &str)> = vec![];
        let with_bound = serde_json::to_value(RequestFrame {
            r#type: "request",
            id: 7,
            method: "POST",
            url: "https://example.invalid/v1",
            headers: &headers,
            body_length: 0,
            observe_actual_send: false,
            read_timeout_ms: Some(1_800_000),
            calibration_not_after: Some(1_800_000_000),
        })
        .expect("frame serializes");
        assert_eq!(with_bound["readTimeoutMs"], serde_json::json!(1_800_000));
        assert_eq!(
            with_bound["calibrationNotAfter"],
            serde_json::json!(1_800_000_000u64)
        );
        assert_eq!(with_bound["type"], serde_json::json!("request"));

        let without_bound = serde_json::to_value(RequestFrame {
            r#type: "request",
            id: 8,
            method: "POST",
            url: "https://example.invalid/v1",
            headers: &headers,
            body_length: 0,
            observe_actual_send: false,
            read_timeout_ms: None,
            calibration_not_after: None,
        })
        .expect("frame serializes");
        assert!(without_bound.get("readTimeoutMs").is_none());
        assert!(without_bound.get("calibrationNotAfter").is_none());
    }

    #[test]
    fn exact_one_shot_policy_never_restarts_the_helper() {
        assert_eq!(TransportRetryPolicy::NeverReplay.max_helper_attempts(), 1);
        assert_eq!(
            TransportRetryPolicy::RestartHelperOnce.max_helper_attempts(),
            2
        );
        for error in [
            TransportError::Spawn,
            TransportError::Closed,
            TransportError::Protocol,
            TransportError::Timeout,
            TransportError::Network,
            TransportError::BodyTooLarge,
            TransportError::CalibrationExpired,
        ] {
            assert!(!TransportRetryPolicy::NeverReplay.may_restart_helper(0, error));
            assert!(!TransportRetryPolicy::NeverReplay.may_restart_helper(1, error));
        }

        for error in [
            TransportError::Spawn,
            TransportError::Closed,
            TransportError::Protocol,
        ] {
            assert!(TransportRetryPolicy::RestartHelperOnce.may_restart_helper(0, error));
            assert!(!TransportRetryPolicy::RestartHelperOnce.may_restart_helper(1, error));
        }
        for error in [
            TransportError::Timeout,
            TransportError::Network,
            TransportError::BodyTooLarge,
            TransportError::CalibrationExpired,
        ] {
            assert!(!TransportRetryPolicy::RestartHelperOnce.may_restart_helper(0, error));
        }
    }

    #[test]
    fn dispatch_attestation_is_exactly_paired_and_strictly_before_deadline() {
        assert_eq!(validate_dispatch_attestation(None, None), Ok(None));
        assert_eq!(
            validate_dispatch_attestation(Some(1_800_000_000), Some(1_799_999_999_999)),
            Ok(Some(1_799_999_999_999))
        );

        for (not_after, dispatch_ms) in [
            (Some(1_800_000_000), None),
            (None, Some(1_799_999_999_999)),
            (Some(1_800_000_000), Some(0)),
            // Equality is outside the half-open admission interval.
            (Some(1_800_000_000), Some(1_800_000_000_000)),
            (Some(u64::MAX), Some(1)),
        ] {
            assert_eq!(
                validate_dispatch_attestation(not_after, dispatch_ms),
                Err(TransportError::Protocol)
            );
        }
    }

    #[tokio::test]
    async fn helper_header_frame_must_pair_with_the_pending_deadline() {
        fn shared_with_pending(
            not_after: Option<u64>,
        ) -> (
            Arc<ProcessShared>,
            oneshot::Receiver<Result<ResponseHead, TransportError>>,
        ) {
            let (ready, _ready_rx) = oneshot::channel();
            let shared = Arc::new(ProcessShared {
                pending: Mutex::new(HashMap::new()),
                canceled: Mutex::new(HashMap::new()),
                ready: Mutex::new(Some(ready)),
                closed: AtomicBool::new(false),
            });
            let (headers, headers_rx) = oneshot::channel();
            let (body, _body_rx) = mpsc::channel(1);
            shared
                .pending
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(
                    7,
                    PendingRequest {
                        headers: Some(headers),
                        body,
                        calibration_not_after: not_after,
                        actual_send_observer: None,
                    },
                );
            (shared, headers_rx)
        }

        fn header_frame(dispatch_ms: Option<u64>) -> InboundFrame {
            InboundFrame {
                frame_type: "headers".to_string(),
                protocol: None,
                node: None,
                platform: None,
                arch: None,
                undici: None,
                id: Some(7),
                status: Some(200),
                calibration_dispatch_ms: dispatch_ms,
                actual_send: None,
                headers: vec!["content-type".to_string(), "application/json".to_string()],
                error_kind: None,
            }
        }

        let (shared, headers) = shared_with_pending(Some(1_800_000_000));
        dispatch_frame(&shared, header_frame(Some(1_799_999_999_999)))
            .await
            .expect("matching helper attestation");
        let head = headers.await.expect("header sender").expect("valid head");
        assert_eq!(head.calibration_dispatch_ms, Some(1_799_999_999_999));

        let (shared, headers) = shared_with_pending(Some(1_800_000_000));
        assert_eq!(
            dispatch_frame(&shared, header_frame(None)).await,
            Err(TransportError::Protocol)
        );
        assert!(
            headers.await.is_err(),
            "invalid helper head is never published"
        );
    }

    #[test]
    fn proxy_validation_rejects_non_origin_and_non_http_routes() {
        assert!(validate_proxy("http://user:pass@127.0.0.1:8080/").is_ok());
        assert!(validate_proxy("http://user%40mail:p%25ss@127.0.0.1:8080/").is_ok());
        assert!(validate_proxy("https://proxy.example:8443/").is_ok());
        for invalid in [
            "socks5://proxy.example:1080/",
            "http://proxy.example/path",
            "http://proxy.example/?secret=1",
            "http://proxy.example/#secret",
            "http://user%GG:pass@proxy.example/",
            "http://user:pass%@proxy.example/",
        ] {
            assert!(validate_proxy(invalid).is_err(), "accepted {invalid}");
        }
    }

    #[tokio::test]
    async fn canceled_pre_header_request_is_removed_and_forwarded_to_the_helper() {
        let (ready, _ready_rx) = oneshot::channel();
        let shared = Arc::new(ProcessShared {
            pending: Mutex::new(HashMap::new()),
            canceled: Mutex::new(HashMap::new()),
            ready: Mutex::new(Some(ready)),
            closed: AtomicBool::new(false),
        });
        let (headers, _headers_rx) = oneshot::channel();
        let (body, _body_rx) = mpsc::channel(1);
        shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                7,
                PendingRequest {
                    headers: Some(headers),
                    body,
                    calibration_not_after: None,
                    actual_send_observer: None,
                },
            );
        let (cancel, mut canceled) = mpsc::unbounded_channel();
        drop(PendingCancelGuard::new(7, shared.clone(), cancel));
        assert!(!shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&7));
        assert_eq!(canceled.recv().await, Some(7));
    }

    #[tokio::test]
    async fn frames_already_queued_for_a_canceled_request_do_not_kill_other_requests() {
        let (ready, _ready_rx) = oneshot::channel();
        let shared = Arc::new(ProcessShared {
            pending: Mutex::new(HashMap::new()),
            canceled: Mutex::new(HashMap::new()),
            ready: Mutex::new(Some(ready)),
            closed: AtomicBool::new(false),
        });
        let (headers, _headers_rx) = oneshot::channel();
        let (body, _body_rx) = mpsc::channel(1);
        shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                7,
                PendingRequest {
                    headers: Some(headers),
                    body,
                    calibration_not_after: None,
                    actual_send_observer: None,
                },
            );
        let (cancel, _canceled) = mpsc::unbounded_channel();
        drop(PendingCancelGuard::new(7, shared.clone(), cancel));

        let late_data = InboundFrame {
            frame_type: "data".to_string(),
            protocol: None,
            node: None,
            platform: None,
            arch: None,
            undici: None,
            id: Some(7),
            status: None,
            calibration_dispatch_ms: None,
            actual_send: None,
            headers: Vec::new(),
            error_kind: None,
        };
        assert!(dispatch_frame(&shared, late_data).await.is_ok());
        assert!(!shared.closed.load(Ordering::Acquire));

        let late_end = InboundFrame {
            frame_type: "end".to_string(),
            protocol: None,
            node: None,
            platform: None,
            arch: None,
            undici: None,
            id: Some(7),
            status: None,
            calibration_dispatch_ms: None,
            actual_send: None,
            headers: Vec::new(),
            error_kind: None,
        };
        assert!(dispatch_frame(&shared, late_end).await.is_ok());
        assert!(!shared
            .canceled
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(&7));
    }

    #[test]
    fn helper_source_has_no_ambient_proxy_or_tls_impersonation_hooks() {
        for forbidden in ["process.env", "NODE_TLS_REJECT_UNAUTHORIZED", "setCiphers"] {
            assert!(!HELPER_SOURCE.contains(forbidden), "found {forbidden}");
        }
        assert!(HELPER_SOURCE.contains("tls.connect"));
        assert!(HELPER_SOURCE.contains("gzip, deflate, br"));
        assert!(
            HELPER_SOURCE.contains("agent: proxyAgent || (calibration ? directAgent : undefined)")
        );
        assert!(HELPER_SOURCE.contains("request.once('socket', onSocket)"));
        assert!(!HELPER_SOURCE.contains("if (request.socket)"));
    }

    #[test]
    fn helper_proxy_and_tls_failures_keep_runtime_network_policy() {
        assert_eq!(
            helper_error_kind(Some("timeout")),
            Some(TransportError::Timeout)
        );
        for kind in [
            "proxy-timeout",
            "proxy-auth",
            "proxy-throttle",
            "proxy-rejected",
            "proxy-upstream",
            "proxy-connect",
            "proxy-eof",
            "proxy-protocol",
            "tls",
            "network",
        ] {
            assert_eq!(helper_error_kind(Some(kind)), Some(TransportError::Network));
        }
        assert_eq!(
            helper_error_kind(Some("protocol")),
            Some(TransportError::Protocol)
        );
        assert_eq!(
            helper_error_kind(Some("calibration-expired")),
            Some(TransportError::CalibrationExpired)
        );
        assert_eq!(helper_error_kind(Some("secret detail")), None);
        assert_eq!(helper_error_kind(None), None);
    }

    #[tokio::test]
    async fn actual_send_frame_increments_only_the_typed_observer() {
        let (ready, _ready_rx) = oneshot::channel();
        let shared = Arc::new(ProcessShared {
            pending: Mutex::new(HashMap::new()),
            ready: Mutex::new(Some(ready)),
            canceled: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        });
        let (headers, _headers_rx) = oneshot::channel();
        let (body, _body_rx) = mpsc::channel(1);
        let observer = ActualSendObserver::default();
        shared
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                7,
                PendingRequest {
                    headers: Some(headers),
                    body,
                    calibration_not_after: None,
                    actual_send_observer: Some(observer.clone()),
                },
            );
        dispatch_frame(
            &shared,
            InboundFrame {
                frame_type: "actual_send".into(),
                protocol: None,
                node: None,
                platform: None,
                arch: None,
                undici: None,
                id: Some(7),
                status: None,
                calibration_dispatch_ms: None,
                actual_send: Some(true),
                headers: Vec::new(),
                error_kind: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(observer.count(), Some(1));
    }
}
