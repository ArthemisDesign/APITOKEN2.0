//! Exact official-Node HTTP transport for one Gemini OAuth publication transaction.
//!
//! Auth Bot performs token exchange, userinfo and first Code Assist onboarding only once per
//! subscription, so it owns a short-lived sequential helper instead of the runtime's persistent
//! per-profile multiplexer. Both evaluate the same dependency-free helper source and attest the
//! same Node/OpenSSL executable before a bearer or proxy credential crosses IPC.

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const PROTOCOL: u8 = 1;
const MAX_LINE_BYTES: usize = 48 * 1024 * 1024;
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const OAUTH_CONNECT_TIMEOUT_MS: u64 = 30_000;
const OAUTH_READ_TIMEOUT_MS: u64 = 90_000;
const HELPER_SOURCE: &str = include_str!("../../forward/src/gemini/node_transport.cjs");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestFailureKind {
    Timeout,
    ProxyTimeout,
    ProxyAuth,
    ProxyThrottle,
    ProxyRejected,
    ProxyUpstream,
    ProxyConnect,
    ProxyEof,
    ProxyProtocol,
    Tls,
    Network,
    Protocol,
    Helper,
}

impl RequestFailureKind {
    fn from_helper(kind: Option<&str>) -> Self {
        match kind {
            Some("timeout") => Self::Timeout,
            Some("proxy-timeout") => Self::ProxyTimeout,
            Some("proxy-auth") => Self::ProxyAuth,
            Some("proxy-throttle") => Self::ProxyThrottle,
            Some("proxy-rejected") => Self::ProxyRejected,
            Some("proxy-upstream") => Self::ProxyUpstream,
            Some("proxy-connect") => Self::ProxyConnect,
            Some("proxy-eof") => Self::ProxyEof,
            Some("proxy-protocol") => Self::ProxyProtocol,
            Some("tls") => Self::Tls,
            Some("network") => Self::Network,
            Some("protocol") => Self::Protocol,
            _ => Self::Helper,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::ProxyTimeout => "proxy_timeout",
            Self::ProxyAuth => "proxy_auth",
            Self::ProxyThrottle => "proxy_throttle",
            Self::ProxyRejected => "proxy_rejected",
            Self::ProxyUpstream => "proxy_upstream",
            Self::ProxyConnect => "proxy_connect",
            Self::ProxyEof => "proxy_eof",
            Self::ProxyProtocol => "proxy_protocol",
            Self::Tls => "tls",
            Self::Network => "network",
            Self::Protocol => "protocol",
            Self::Helper => "helper",
        }
    }

    /// These failures happen before the target TLS request can carry an OAuth code. Retrying the
    /// exact token exchange is therefore safe; a generic read timeout/network failure is not.
    pub(crate) fn safe_to_retry_before_target(self) -> bool {
        matches!(
            self,
            Self::ProxyTimeout
                | Self::ProxyThrottle
                | Self::ProxyUpstream
                | Self::ProxyConnect
                | Self::ProxyEof
                | Self::Tls
        )
    }

    /// Once an access token exists, every authbot control-plane operation is replay-safe. Keep
    /// malformed CONNECT responses and explicit auth/rejection failures fail-closed, while all
    /// transient transport classes receive bounded recovery.
    pub(crate) fn retryable_control_plane(self) -> bool {
        matches!(
            self,
            Self::Timeout
                | Self::ProxyTimeout
                | Self::ProxyThrottle
                | Self::ProxyUpstream
                | Self::ProxyConnect
                | Self::ProxyEof
                | Self::Tls
                | Self::Network
        )
    }
}

#[derive(Debug)]
struct RequestFailure(RequestFailureKind);

impl std::fmt::Display for RequestFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "Gemini OAuth transport {}", self.0.as_str())
    }
}

impl std::error::Error for RequestFailure {}

/// Return only a bounded, credential-free operator diagnostic. Raw helper errors and proxy URLs
/// are deliberately never surfaced to journalctl.
pub fn diagnostic_kind(error: &anyhow::Error) -> &'static str {
    error
        .downcast_ref::<RequestFailure>()
        .map(|failure| failure.0.as_str())
        .unwrap_or("startup_or_ipc")
}

pub(crate) fn failure_kind(error: &anyhow::Error) -> Option<RequestFailureKind> {
    error
        .downcast_ref::<RequestFailure>()
        .map(|failure| failure.0)
}

#[derive(Clone, Copy)]
pub enum Method {
    Get,
    Post,
}

impl Method {
    fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
        }
    }
}

pub struct Response {
    pub status: u16,
    pub body: Zeroizing<Vec<u8>>,
}

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    request_timeout: Duration,
}

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
    #[serde(skip_serializing_if = "Option::is_none")]
    wire_profile: Option<&'static str>,
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
    data: Option<String>,
    #[serde(default)]
    kind: Option<String>,
}

impl Client {
    pub async fn connect(proxy: &str) -> anyhow::Result<Self> {
        Self::connect_with_timeouts(
            proxy,
            OAUTH_CONNECT_TIMEOUT_MS,
            OAUTH_READ_TIMEOUT_MS,
            REQUEST_TIMEOUT,
        )
        .await
    }

    async fn connect_with_timeouts(
        proxy: &str,
        connect_timeout_ms: u64,
        read_timeout_ms: u64,
        request_timeout: Duration,
    ) -> anyhow::Result<Self> {
        attest_node_binary()?;
        let mut child = Command::new(gemini_credential::GEMINI_NODE_BINARY)
            .arg("--expose-internals")
            .arg("-e")
            .arg(HELPER_SOURCE)
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Gemini OAuth Node stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Gemini OAuth Node stdout unavailable"))?;
        let mut client = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 1,
            request_timeout,
        };
        client
            .write_frame(&ConfigureFrame {
                r#type: "configure",
                protocol: PROTOCOL,
                proxy,
                connect_timeout_ms,
                read_timeout_ms,
            })
            .await?;
        let ready = tokio::time::timeout(
            Duration::from_millis(connect_timeout_ms),
            client.read_frame(),
        )
        .await??;
        if ready.frame_type != "ready"
            || ready.protocol != Some(PROTOCOL)
            || ready.node.as_deref() != Some(gemini_credential::GEMINI_NODE_VERSION)
            || ready.platform.as_deref() != Some("linux")
            || ready.arch.as_deref() != Some("x64")
            || ready.undici.as_deref() != Some("node-internal")
        {
            anyhow::bail!("Gemini OAuth Node attestation handshake failed");
        }
        Ok(client)
    }

    pub async fn request(
        &mut self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> anyhow::Result<Response> {
        self.request_profile(method, url, headers, body, None).await
    }

    /// Independently attested global-fetch path used only for verified Google userinfo.
    pub async fn fetch_userinfo(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> anyhow::Result<Response> {
        self.request_profile(Method::Get, url, headers, &[], Some("undici-fetch"))
            .await
    }

    async fn request_profile(
        &mut self,
        method: Method,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        wire_profile: Option<&'static str>,
    ) -> anyhow::Result<Response> {
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("Gemini OAuth request id exhausted"))?;
        let encoded_body = Zeroizing::new(BASE64.encode(body));
        self.write_frame(&RequestFrame {
            r#type: "request",
            id,
            method: method.as_str(),
            url,
            headers,
            body: encoded_body.as_str(),
            wire_profile,
        })
        .await?;

        let mut status = None;
        let mut response_body = Zeroizing::new(Vec::new());
        let deadline = tokio::time::Instant::now() + self.request_timeout;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(RequestFailure(RequestFailureKind::Timeout).into());
            }
            let frame = match tokio::time::timeout(remaining, self.read_frame()).await {
                Ok(frame) => frame?,
                Err(_) => return Err(RequestFailure(RequestFailureKind::Timeout).into()),
            };
            if frame.id != Some(id) {
                anyhow::bail!("Gemini OAuth Node returned an unexpected request id");
            }
            match frame.frame_type.as_str() {
                "headers" if status.is_none() => {
                    status = frame.status.filter(|value| (100..=599).contains(value));
                    if status.is_none() {
                        anyhow::bail!("Gemini OAuth Node returned an invalid status");
                    }
                }
                "data" if status.is_some() => {
                    let encoded = frame.data.as_deref().ok_or_else(|| {
                        anyhow::anyhow!("Gemini OAuth Node returned invalid data")
                    })?;
                    let chunk = Zeroizing::new(BASE64.decode(encoded)?);
                    if response_body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                        anyhow::bail!("Gemini OAuth response exceeded its limit");
                    }
                    response_body.extend_from_slice(&chunk);
                }
                "end" if status.is_some() => {
                    return Ok(Response {
                        status: status.unwrap_or(500),
                        body: response_body,
                    });
                }
                "error" => {
                    return Err(RequestFailure(RequestFailureKind::from_helper(
                        frame.kind.as_deref(),
                    ))
                    .into());
                }
                _ => anyhow::bail!("Gemini OAuth Node protocol failure"),
            }
        }
    }

    async fn write_frame(&mut self, frame: &impl Serialize) -> anyhow::Result<()> {
        let encoded = Zeroizing::new(serde_json::to_vec(frame)?);
        if encoded.len() > MAX_LINE_BYTES {
            anyhow::bail!("Gemini OAuth Node IPC frame exceeded its limit");
        }
        self.stdin.write_all(&encoded).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn read_frame(&mut self) -> anyhow::Result<InboundFrame> {
        let mut line = Zeroizing::new(Vec::new());
        loop {
            let available = self.stdout.fill_buf().await?;
            if available.is_empty() {
                anyhow::bail!("Gemini OAuth Node closed unexpectedly");
            }
            if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
                if line.len().saturating_add(newline) > MAX_LINE_BYTES {
                    anyhow::bail!("Gemini OAuth Node IPC line exceeded its limit");
                }
                line.extend_from_slice(&available[..newline]);
                self.stdout.consume(newline + 1);
                break;
            }
            if line.len().saturating_add(available.len()) > MAX_LINE_BYTES {
                anyhow::bail!("Gemini OAuth Node IPC line exceeded its limit");
            }
            let len = available.len();
            line.extend_from_slice(available);
            self.stdout.consume(len);
        }
        Ok(serde_json::from_slice(&line)?)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn attest_node_binary() -> anyhow::Result<()> {
    let mut file = std::fs::File::open(gemini_credential::GEMINI_NODE_BINARY)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    if format!("{:x}", hasher.finalize()) != gemini_credential::GEMINI_NODE_SHA256 {
        anyhow::bail!("Gemini OAuth Node binary attestation failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_and_runtime_share_one_helper_and_attestation_tuple() {
        assert!(HELPER_SOURCE.contains("class GeminiProxyAgent"));
        assert!(HELPER_SOURCE.contains("internal/deps/undici/undici"));
        assert!(HELPER_SOURCE.contains("undici-fetch"));
        assert!(HELPER_SOURCE.contains("stdoutBlocked"));
        for kind in [
            "proxy-timeout",
            "proxy-auth",
            "proxy-throttle",
            "proxy-rejected",
            "proxy-upstream",
            "proxy-connect",
            "proxy-eof",
            "proxy-protocol",
        ] {
            assert!(HELPER_SOURCE.contains(kind), "missing helper kind {kind}");
        }
        assert_eq!(gemini_credential::GEMINI_CLI_VERSION, "0.53.0");
        assert_eq!(gemini_credential::GEMINI_NODE_VERSION, "v24.18.0");
        assert_eq!(gemini_credential::GEMINI_NODE_SHA256.len(), 64);
    }

    #[test]
    fn helper_failures_become_bounded_secret_free_diagnostics() {
        for (input, expected) in [
            (Some("timeout"), "timeout"),
            (Some("proxy-timeout"), "proxy_timeout"),
            (Some("proxy-auth"), "proxy_auth"),
            (Some("proxy-throttle"), "proxy_throttle"),
            (Some("proxy-rejected"), "proxy_rejected"),
            (Some("proxy-upstream"), "proxy_upstream"),
            (Some("proxy-connect"), "proxy_connect"),
            (Some("proxy-eof"), "proxy_eof"),
            (Some("proxy-protocol"), "proxy_protocol"),
            (Some("tls"), "tls"),
            (Some("network"), "network"),
            (Some("protocol"), "protocol"),
            (Some("proxy-password"), "helper"),
            (None, "helper"),
        ] {
            let error: anyhow::Error =
                RequestFailure(RequestFailureKind::from_helper(input)).into();
            assert_eq!(diagnostic_kind(&error), expected);
        }
        assert_eq!(
            diagnostic_kind(&anyhow::anyhow!("internal detail")),
            "startup_or_ipc"
        );
    }

    #[test]
    fn oauth_recovery_never_replays_an_ambiguous_token_exchange() {
        for kind in [
            RequestFailureKind::ProxyTimeout,
            RequestFailureKind::ProxyThrottle,
            RequestFailureKind::ProxyUpstream,
            RequestFailureKind::ProxyConnect,
            RequestFailureKind::ProxyEof,
            RequestFailureKind::Tls,
        ] {
            assert!(kind.safe_to_retry_before_target(), "kind={kind:?}");
        }
        for kind in [
            RequestFailureKind::Timeout,
            RequestFailureKind::Network,
            RequestFailureKind::ProxyAuth,
            RequestFailureKind::ProxyRejected,
            RequestFailureKind::ProxyProtocol,
            RequestFailureKind::Protocol,
            RequestFailureKind::Helper,
        ] {
            assert!(!kind.safe_to_retry_before_target(), "kind={kind:?}");
        }
    }
}
