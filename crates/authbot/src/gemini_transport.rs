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
const HELPER_SOURCE: &str = include_str!("../../forward/src/gemini/node_transport.cjs");

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
}

impl Client {
    pub async fn connect(proxy: &str) -> anyhow::Result<Self> {
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
        };
        client
            .write_frame(&ConfigureFrame {
                r#type: "configure",
                protocol: PROTOCOL,
                proxy,
                connect_timeout_ms: 30_000,
                read_timeout_ms: 90_000,
            })
            .await?;
        let ready = tokio::time::timeout(Duration::from_secs(30), client.read_frame()).await??;
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

    /// Exact global-fetch path used only by Gemini CLI's fetchAndCacheUserInfo().
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
        let deadline = tokio::time::Instant::now() + REQUEST_TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                anyhow::bail!("Gemini OAuth Node request timed out");
            }
            let frame = tokio::time::timeout(remaining, self.read_frame()).await??;
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
                "error" => anyhow::bail!("Gemini OAuth Node request failed"),
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
        assert_eq!(gemini_credential::GEMINI_CLI_VERSION, "0.53.0");
        assert_eq!(gemini_credential::GEMINI_NODE_VERSION, "v24.18.0");
        assert_eq!(gemini_credential::GEMINI_NODE_SHA256.len(), 64);
    }
}
