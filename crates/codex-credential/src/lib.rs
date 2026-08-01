//! Versioned encrypted envelopes shared by the Codex (ChatGPT) auth producer and runtime.
//!
//! The roster contains only an opaque profile id and an absolute credential-file path. OpenAI
//! identity, OAuth material and authenticated proxy credentials exist only inside this AEAD
//! envelope. The profile id is authenticated as associated data, so swapping two files fails
//! closed instead of silently moving an account to another runtime identity.

use anyhow::{anyhow, bail, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const ENVELOPE_VERSION: u8 = 1;
const CREDENTIAL_VERSION: u8 = 1;
const AAD_PREFIX: &[u8] = b"apitoken/codex-oauth-credential/v1\0";
const SECRET_AAD_PREFIX: &[u8] = b"apitoken/codex-oauth-pending-secret/v1\0";

/// Pinned wire identity of the official Codex client. The native transport presents exactly
/// this identity; the version moves only through a reviewed commit after a live wire probe
/// (`research/CODEX_NATIVE_WIRE.md`).
pub const CODEX_CLI_VERSION: &str = "0.146.0";
pub const CODEX_ORIGINATOR: &str = "codex_cli_rs";
/// Public installed-application identity embedded in the official Codex client. The ChatGPT
/// OAuth client is a public client without a secret; pinning the id prevents a sealed profile
/// from silently switching OAuth application identity at refresh.
pub const CODEX_OFFICIAL_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
pub const CODEX_OFFICIAL_TOKEN_URI: &str = "https://auth.openai.com/oauth/token";
/// Native ChatGPT-backed Codex backend (Responses API over HTTPS/SSE and WebSocket).
pub const CODEX_DEFAULT_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

/// Exact User-Agent shape of the official client: `codex_cli_rs/<version> (<os>; <arch>) <term>`.
pub fn codex_user_agent(version: &str, os: &str, arch: &str, term: &str) -> String {
    format!("{CODEX_ORIGINATOR}/{version} ({os}; {arch}) {term}")
}

/// Heap string that overwrites its buffer on drop. Use for short-lived token/proxy clones outside
/// the long-lived credential envelope.
pub type SecretString = zeroize::Zeroizing<String>;

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct CodexCredential {
    pub version: u8,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub oauth_client_id: String,
    pub token_uri: String,
    /// ChatGPT account id from the id-token `chatgpt_account_id` claim. Sent upstream as the
    /// `ChatGPT-Account-ID` header; it is the subscription's subject, never an email.
    pub account_id: String,
    pub email: String,
    pub plan: String,
    pub proxy: String,
    /// IPRoyal reseller order owning this stable proxy. Zero means a manually supplied proxy whose
    /// lifecycle remains external to Auth Bot.
    #[serde(default)]
    pub proxy_order_id: i64,
    pub issued_at: i64,
}

impl std::fmt::Debug for CodexCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CodexCredential")
            .field("version", &self.version)
            .field("expires_at", &self.expires_at)
            .field("plan", &self.plan)
            .field("issued_at", &self.issued_at)
            .field("secrets", &"REDACTED")
            .finish()
    }
}

impl CodexCredential {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported Codex credential version");
        }
        bounded_secret(&self.access_token, 8, 16_384, "access token")?;
        bounded_secret(&self.refresh_token, 8, 16_384, "refresh token")?;
        if self.oauth_client_id != CODEX_OFFICIAL_OAUTH_CLIENT_ID {
            bail!("Codex OAuth application identity is not the pinned official client");
        }
        let test_loopback = cfg!(feature = "test-loopback-token-uri")
            && (self.token_uri.starts_with("http://127.0.0.1:")
                || self.token_uri.starts_with("http://[::1]:"));
        if self.token_uri != CODEX_OFFICIAL_TOKEN_URI && !test_loopback {
            bail!("Codex token endpoint is not pinned");
        }
        bounded_text(&self.account_id, 4, 512, "ChatGPT account id")?;
        bounded_text(&self.email, 3, 512, "ChatGPT email")?;
        bounded_text(&self.plan, 1, 128, "ChatGPT plan")?;
        if !is_supported_paid_plan(&self.plan) {
            bail!("Codex credential plan is not an approved paid ChatGPT tier");
        }
        if !self.proxy.is_empty() {
            normalize_proxy_url(&self.proxy)?;
        }
        if self.proxy_order_id < 0 {
            bail!("invalid Codex proxy order id");
        }
        Ok(())
    }
}

/// Validate and canonicalize an HTTP(S) proxy origin exactly once for producer publication,
/// runtime duplicate detection and helper admission. Percent-encoded credentials are decoded and
/// encoded again so equivalent spellings cannot represent two rotation identities.
pub fn normalize_proxy_url(proxy: &str) -> anyhow::Result<String> {
    bounded_secret(proxy, 8, 4_096, "proxy URL")?;
    let mut parsed = url::Url::parse(proxy).context("parse Codex proxy URL")?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("Codex proxy must be an HTTP(S) origin");
    }
    let username = decode_proxy_component(parsed.username())?;
    let password = parsed.password().map(decode_proxy_component).transpose()?;
    parsed
        .set_username(&encode_proxy_component(&username))
        .map_err(|_| anyhow!("invalid Codex proxy username"))?;
    parsed
        .set_password(password.as_deref().map(encode_proxy_component).as_deref())
        .map_err(|_| anyhow!("invalid Codex proxy password"))?;
    Ok(parsed.to_string())
}

/// Re-encode a decoded credential into the unreserved set. `Url::set_password` escapes with the
/// userinfo set, which deliberately leaves `%` alone — so a password whose plaintext contains a
/// literal `%41` would be emitted unescaped and decoded back to `A` by the transport helper, and
/// the proxy would then reject a credential the seller supplied correctly. Escaping everything
/// outside the unreserved set makes decode/encode an exact round trip and keeps the canonical form
/// stable under repeated normalization.
fn encode_proxy_component(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(char::from(byte));
            }
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn decode_proxy_component(value: &str) -> anyhow::Result<String> {
    let source = value.as_bytes();
    let mut decoded = Vec::with_capacity(source.len());
    let mut index = 0usize;
    while index < source.len() {
        if source[index] == b'%' {
            let high = source
                .get(index + 1)
                .and_then(|value| proxy_hex(*value))
                .ok_or_else(|| anyhow!("invalid percent encoding in Codex proxy credentials"))?;
            let low = source
                .get(index + 2)
                .and_then(|value| proxy_hex(*value))
                .ok_or_else(|| anyhow!("invalid percent encoding in Codex proxy credentials"))?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(source[index]);
            index += 1;
        }
    }
    let decoded = String::from_utf8(decoded)
        .map_err(|_| anyhow!("Codex proxy credentials are not valid UTF-8"))?;
    if decoded.chars().any(char::is_control) {
        bail!("Codex proxy credentials contain control characters");
    }
    Ok(decoded)
}

fn proxy_hex(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

pub fn is_supported_paid_plan(plan: &str) -> bool {
    matches!(plan, "chatgpt_plus" | "chatgpt_pro" | "chatgpt_business")
}

/// Map only reviewed ChatGPT plan identities (id-token `chatgpt_plan_type` claim) to the internal
/// billing plan. Exact comparisons keep future "Pro"-like trials from being promoted merely
/// because their display name contains a familiar word.
pub fn supported_plan_for_claim(plan_type: &str) -> Option<&'static str> {
    match plan_type.to_ascii_lowercase().as_str() {
        "plus" => Some("chatgpt_plus"),
        "pro" => Some("chatgpt_pro"),
        "business" | "team" | "enterprise" => Some("chatgpt_business"),
        _ => None,
    }
}

fn bounded_text(value: &str, min: usize, max: usize, description: &str) -> anyhow::Result<()> {
    if !(min..=max).contains(&value.len()) || value.chars().any(char::is_control) {
        bail!("invalid {description}");
    }
    Ok(())
}

fn bounded_secret(value: &str, min: usize, max: usize, description: &str) -> anyhow::Result<()> {
    bounded_text(value, min, max, description)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SealedCredential {
    pub version: u8,
    pub key_id: String,
    pub nonce: String,
    pub ciphertext: String,
}

#[derive(Clone)]
pub struct CredentialKeyring {
    keys: HashMap<String, [u8; 32]>,
}

impl Drop for CredentialKeyring {
    fn drop(&mut self) {
        for key in self.keys.values_mut() {
            key.zeroize();
        }
    }
}

impl std::fmt::Debug for CredentialKeyring {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialKeyring")
            .field("key_count", &self.keys.len())
            .field("keys", &"REDACTED")
            .finish()
    }
}

impl CredentialKeyring {
    /// Parse `kid:64-hex-bytes[,kid:64-hex-bytes...]`. Keeping old keys permits online rotation:
    /// authbot seals new profiles with the configured active id while the runtime can still open
    /// existing envelopes until they are republished.
    pub fn parse(specification: &str) -> anyhow::Result<Self> {
        let mut keys = HashMap::new();
        for entry in specification
            .split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            let (key_id, encoded) = entry
                .split_once(':')
                .ok_or_else(|| anyhow!("Codex credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate Codex credential key id");
            }
        }
        if keys.is_empty() {
            bail!("Codex credential keyring is empty");
        }
        Ok(Self { keys })
    }

    pub fn contains(&self, key_id: &str) -> bool {
        self.keys.contains_key(key_id)
    }

    pub fn seal(
        &self,
        active_key_id: &str,
        profile_id: &str,
        credential: &CodexCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active Codex credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode Codex credential")?);
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| anyhow!("operating-system CSPRNG unavailable"))?;
        let aad = associated_data(profile_id);
        let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("seal Codex credential failed"))?;
        Ok(SealedCredential {
            version: ENVELOPE_VERSION,
            key_id: active_key_id.to_string(),
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })
    }

    pub fn open(
        &self,
        profile_id: &str,
        envelope: &SealedCredential,
    ) -> anyhow::Result<CodexCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported Codex credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Codex credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Codex credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Codex credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Codex credential ciphertext")?;
        let aad = associated_data(profile_id);
        let plaintext = Zeroizing::new(
            XChaCha20Poly1305::new(Key::from_slice(key))
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| anyhow!("Codex credential authentication failed"))?,
        );
        let credential: CodexCredential =
            serde_json::from_slice(&plaintext).context("decode Codex credential")?;
        credential.validate()?;
        Ok(credential)
    }

    /// Seal a short-lived producer secret under a distinct AEAD domain. `context_id` is the
    /// one-use flow state, preventing ciphertext from being moved to another pending row.
    pub fn seal_secret(
        &self,
        active_key_id: &str,
        context_id: &str,
        secret: &str,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(context_id)?;
        bounded_secret(secret, 1, 8_192, "pending secret")?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active Codex credential key id is unavailable"))?;
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| anyhow!("operating-system CSPRNG unavailable"))?;
        let aad = associated_secret_data(context_id);
        let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: secret.as_bytes(),
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("seal Codex pending secret failed"))?;
        Ok(SealedCredential {
            version: ENVELOPE_VERSION,
            key_id: active_key_id.to_string(),
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })
    }

    pub fn open_secret(
        &self,
        context_id: &str,
        envelope: &SealedCredential,
    ) -> anyhow::Result<SecretString> {
        validate_profile_id(context_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported Codex pending-secret envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Codex pending-secret key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Codex pending-secret nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Codex pending-secret nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Codex pending-secret ciphertext")?;
        let aad = associated_secret_data(context_id);
        let plaintext = Zeroizing::new(
            XChaCha20Poly1305::new(Key::from_slice(key))
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| anyhow!("Codex pending-secret authentication failed"))?,
        );
        let secret = std::str::from_utf8(&plaintext)
            .context("decode Codex pending secret")?
            .to_string();
        bounded_secret(&secret, 1, 8_192, "pending secret")?;
        Ok(SecretString::new(secret))
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode Codex credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode Codex credential envelope")
}

pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Codex profile id must match [A-Za-z0-9_-] and be 1..=64 bytes");
    }
    Ok(())
}

fn validate_key_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 32
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Codex credential key id must match [A-Za-z0-9_-]");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("Codex credential keys must be exactly 32 bytes encoded as 64 hex characters");
    }
    let mut key = [0u8; 32];
    for (index, output) in key.iter_mut().enumerate() {
        let high = hex(encoded.as_bytes()[index * 2])?;
        let low = hex(encoded.as_bytes()[index * 2 + 1])?;
        *output = (high << 4) | low;
    }
    Ok(key)
}

fn hex(value: u8) -> anyhow::Result<u8> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        b'A'..=b'F' => Ok(value - b'A' + 10),
        _ => bail!("Codex credential key contains non-hex characters"),
    }
}

fn associated_data(profile_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_PREFIX.len() + profile_id.len());
    aad.extend_from_slice(AAD_PREFIX);
    aad.extend_from_slice(profile_id.as_bytes());
    aad
}

fn associated_secret_data(context_id: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(SECRET_AAD_PREFIX.len() + context_id.len());
    aad.extend_from_slice(SECRET_AAD_PREFIX);
    aad.extend_from_slice(context_id.as_bytes());
    aad
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential() -> CodexCredential {
        CodexCredential {
            version: 1,
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at: 123,
            oauth_client_id: CODEX_OFFICIAL_OAUTH_CLIENT_ID.into(),
            token_uri: CODEX_OFFICIAL_TOKEN_URI.into(),
            account_id: "acct_1234567890abcdef".into(),
            email: "owner@example.com".into(),
            plan: "chatgpt_plus".into(),
            proxy: "http://user:pass@127.0.0.1:8080".into(),
            proxy_order_id: 42,
            issued_at: 100,
        }
    }

    #[test]
    fn roundtrip_is_profile_bound_and_contains_no_plaintext() {
        let ring = CredentialKeyring::parse(&format!("current:{}", "11".repeat(32))).unwrap();
        let source = credential();
        let envelope = ring.seal("current", "profile_01", &source).unwrap();
        let encoded = encode_envelope(&envelope).unwrap();
        assert!(!encoded.windows(12).any(|part| part == b"refresh-token"));
        let decoded = decode_envelope(&encoded).unwrap();
        let opened = ring.open("profile_01", &decoded).unwrap();
        assert_eq!(opened.email, "owner@example.com");
        assert!(ring.open("profile_02", &decoded).is_err());
    }

    #[test]
    fn credential_validation_rejects_unreviewed_plan_and_oauth_identity() {
        assert_eq!(supported_plan_for_claim("plus"), Some("chatgpt_plus"));
        assert_eq!(supported_plan_for_claim("Pro"), Some("chatgpt_pro"));
        assert_eq!(supported_plan_for_claim("free"), None);

        let mut candidate = credential();
        candidate.plan = "chatgpt_free".into();
        assert!(candidate.validate().is_err());

        let mut candidate = credential();
        candidate.oauth_client_id = "app_anotherclient".into();
        assert!(candidate.validate().is_err());

        let mut candidate = credential();
        candidate.account_id = String::new();
        assert!(candidate.validate().is_err());
    }

    #[test]
    fn proxy_normalization_collapses_equivalent_credentials_and_default_ports() {
        let plain = normalize_proxy_url("http://user:pass@proxy.example/").unwrap();
        assert_eq!(
            plain,
            normalize_proxy_url("HTTP://u%73er:pa%73s@PROXY.EXAMPLE:80").unwrap()
        );
        for invalid in [
            "socks5://proxy.example:1080/",
            "http://proxy.example/path",
            "http://user%GG:pass@proxy.example/",
            "http://user:%ff@proxy.example/",
            "http://user:%00@proxy.example/",
        ] {
            assert!(normalize_proxy_url(invalid).is_err(), "accepted {invalid}");
        }
    }

    /// Canonicalization decodes credentials, so it must escape everything it decoded. `%` is not in
    /// the userinfo escape set: leaving re-encoding to `Url::set_password` turned a literal `%41`
    /// in a seller password into `A`, and the proxy then rejected the CONNECT with an authentication
    /// failure indistinguishable from an unreachable proxy.
    #[test]
    fn proxy_normalization_round_trips_reserved_credential_bytes() {
        for password in ["pa%41ss", "p%s:s/w@rd#1", "100%", "a b", "%%%"] {
            let encoded = super::encode_proxy_component(password);
            let canonical =
                normalize_proxy_url(&format!("http://user:{encoded}@proxy.example:8080")).unwrap();
            let parsed = url::Url::parse(&canonical).unwrap();
            assert_eq!(
                super::decode_proxy_component(parsed.password().unwrap_or_default()).unwrap(),
                password,
                "credential mangled for {password:?}"
            );
            assert_eq!(
                normalize_proxy_url(&canonical).unwrap(),
                canonical,
                "canonical form is not stable for {password:?}"
            );
        }
    }

    #[test]
    fn old_keys_remain_readable_during_rotation() {
        let ring =
            CredentialKeyring::parse(&format!("new:{},old:{}", "22".repeat(32), "33".repeat(32)))
                .unwrap();
        let envelope = ring.seal("old", "profile", &credential()).unwrap();
        assert!(ring.open("profile", &envelope).is_ok());
        assert!(ring.contains("new"));
    }

    #[test]
    fn pending_secrets_are_context_bound_and_not_plaintext() {
        let ring = CredentialKeyring::parse(&format!("current:{}", "66".repeat(32))).unwrap();
        let sealed = ring
            .seal_secret(
                "current",
                "device_state_01",
                "http://user:pass@127.0.0.1:8080",
            )
            .unwrap();
        let encoded = encode_envelope(&sealed).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("user:pass"));
        assert_eq!(
            ring.open_secret("device_state_01", &sealed)
                .unwrap()
                .as_str(),
            "http://user:pass@127.0.0.1:8080"
        );
        assert!(ring.open_secret("device_state_02", &sealed).is_err());
    }

    #[test]
    fn keyring_and_debug_never_expose_secrets() {
        let raw = format!("active:{}", "44".repeat(32));
        let ring = CredentialKeyring::parse(&raw).unwrap();
        let debug = format!("{ring:?} {:?}", credential());
        assert!(!debug.contains("444444"));
        assert!(!debug.contains("refresh-token"));
        assert!(!debug.contains("owner@example.com"));
    }

    #[test]
    fn user_agent_matches_official_shape() {
        assert_eq!(
            codex_user_agent("0.145.0", "Ubuntu 22.04", "x86_64", "xterm-256color"),
            "codex_cli_rs/0.145.0 (Ubuntu 22.04; x86_64) xterm-256color"
        );
    }
}
