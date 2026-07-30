//! Attested official-Node transport for Gemini CLI's Code Assist surface.
//!
//! Production requests never approximate Node/OpenSSL through the Claude BoringSSL stack. Every
//! encrypted profile owns one persistent, multiplexed helper process whose exact executable is
//! SHA-256 pinned. Loopback mock upstreams retain an in-process client so the protocol fault matrix
//! remains deterministic without making tests depend on a host Node installation.

use super::config::GeminiConfig;
use axum::http::{HeaderMap, HeaderName, HeaderValue, StatusCode};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
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
use std::time::Duration;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, Command};
use tokio::sync::{mpsc, oneshot};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const IPC_PROTOCOL: u8 = 1;
const MAX_IPC_LINE_BYTES: usize = 48 * 1024 * 1024;
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
struct RequestFrame<'a> {
    r#type: &'static str,
    id: u64,
    method: &'static str,
    url: &'a str,
    headers: &'a [(&'a str, &'a str)],
    body: &'a str,
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
        })
    }
}

impl std::error::Error for TransportError {}

pub(crate) struct TransportResponse {
    status: StatusCode,
    headers: HeaderMap,
    body: BoxStream<'static, Result<Bytes, TransportError>>,
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

pub(crate) struct TransportRequest<'a> {
    pub(crate) url: &'a str,
    pub(crate) headers: Vec<(&'static str, SecretString)>,
    pub(crate) body: Bytes,
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
                .read_timeout(Duration::from_secs(cfg.read_timeout_secs))
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
        match self {
            Self::Loopback(client) => {
                let mut builder = client.post(request.url);
                for (name, value) in request.headers {
                    builder = builder.header(name, value.as_str());
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
        for attempt in 0..=1 {
            let process = self.process().await?;
            match process.request(id, &request).await {
                Ok(response) => return Ok(response),
                Err(error) if attempt == 0 && error.helper_restartable() => {
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
        let encoded_body = Zeroizing::new(BASE64.encode(&request.body));
        let frame = RequestFrame {
            r#type: "request",
            id,
            method: "POST",
            url: request.url,
            headers: &headers,
            body: encoded_body.as_str(),
        };
        if let Err(error) = self.write_frame(&frame).await {
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
        })
    }

    async fn write_frame(&self, frame: &(impl Serialize + ?Sized)) -> Result<(), TransportError> {
        let encoded =
            Zeroizing::new(serde_json::to_vec(frame).map_err(|_| TransportError::Protocol)?);
        if encoded.len() > MAX_IPC_LINE_BYTES {
            return Err(TransportError::Protocol);
        }
        let mut writer = self.writer.lock().await;
        writer
            .write_all(&encoded)
            .await
            .map_err(|_| TransportError::Closed)?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|_| TransportError::Closed)?;
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
    headers: Vec<String>,
    #[serde(default)]
    data: Option<String>,
    #[serde(default, rename = "kind")]
    error_kind: Option<String>,
}

async fn reader_loop<R>(mut reader: R, shared: Arc<ProcessShared>)
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Zeroizing::new(Vec::new());
    loop {
        line.zeroize();
        match read_bounded_line(&mut reader, &mut line).await {
            Ok(true) => {}
            Ok(false) => {
                shared.close(TransportError::Closed);
                return;
            }
            Err(error) => {
                shared.close(error);
                return;
            }
        }
        let value: InboundFrame = match serde_json::from_slice(&line) {
            Ok(value) => value,
            Err(_) => {
                shared.close(TransportError::Protocol);
                return;
            }
        };
        if dispatch_frame(&shared, value).await.is_err() {
            shared.close(TransportError::Protocol);
            return;
        }
    }
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
            let sender = {
                let mut pending = shared
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                pending
                    .get_mut(&id)
                    .and_then(|request| request.headers.take())
                    .ok_or(TransportError::Protocol)?
            };
            let _ = sender.send(Ok(ResponseHead { status, headers }));
        }
        "data" => {
            let encoded = value.data.as_deref().ok_or(TransportError::Protocol)?;
            let decoded = Zeroizing::new(
                BASE64
                    .decode(encoded)
                    .map_err(|_| TransportError::Protocol)?,
            );
            let bytes = Bytes::copy_from_slice(&decoded);
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
            let _ = sender.send(Ok(bytes)).await;
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
            let error = match value.error_kind.as_deref() {
                Some("timeout") => TransportError::Timeout,
                Some("network") => TransportError::Network,
                Some("protocol") => TransportError::Protocol,
                _ => return Err(TransportError::Protocol),
            };
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

async fn read_bounded_line<R>(reader: &mut R, line: &mut Vec<u8>) -> Result<bool, TransportError>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader
            .fill_buf()
            .await
            .map_err(|_| TransportError::Closed)?;
        if available.is_empty() {
            return Ok(!line.is_empty());
        }
        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > MAX_IPC_LINE_BYTES {
                return Err(TransportError::Protocol);
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            return Ok(true);
        }
        if line.len().saturating_add(available.len()) > MAX_IPC_LINE_BYTES {
            return Err(TransportError::Protocol);
        }
        let len = available.len();
        line.extend_from_slice(available);
        reader.consume(len);
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
        eprintln!("Gemini Node transport emitted redacted diagnostics");
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
            headers: Vec::new(),
            data: Some("bGF0ZQ==".to_string()),
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
            headers: Vec::new(),
            data: None,
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
    }
}
