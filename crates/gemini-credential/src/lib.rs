//! Versioned encrypted envelopes shared by the Gemini auth producer and runtime.
//!
//! The roster contains only an opaque profile id and an absolute credential-file path. Google
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
const AAD_PREFIX: &[u8] = b"apitoken/gemini-oauth-credential/v1\0";
const SECRET_AAD_PREFIX: &[u8] = b"apitoken/gemini-oauth-pending-secret/v1\0";

/// Stable, truthful identity shared by Auth Bot and the Gemini runtime. Keeping this value in the
/// common crate prevents OAuth/refresh/generation transports from drifting into different client
/// profiles without impersonating an official Google client.
pub const GEMINI_PROVIDER_USER_AGENT: &str = "apitoken-gemini-provider/1";

/// Heap string that overwrites its buffer on drop. Use for short-lived token/proxy clones outside
/// the long-lived credential envelope.
pub type SecretString = zeroize::Zeroizing<String>;

#[derive(Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
pub struct GeminiCredential {
    pub version: u8,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: i64,
    pub oauth_client_id: String,
    pub oauth_client_secret: String,
    pub token_uri: String,
    pub subject: String,
    pub email: String,
    pub project_id: String,
    pub tier_id: String,
    pub tier_name: String,
    pub plan: String,
    pub proxy: String,
    /// IPRoyal reseller order owning this stable proxy. Zero means a manually supplied proxy whose
    /// lifecycle remains external to Auth Bot.
    #[serde(default)]
    pub proxy_order_id: i64,
    pub issued_at: i64,
}

impl std::fmt::Debug for GeminiCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiCredential")
            .field("version", &self.version)
            .field("expires_at", &self.expires_at)
            .field("plan", &self.plan)
            .field("issued_at", &self.issued_at)
            .field("secrets", &"REDACTED")
            .finish()
    }
}

impl GeminiCredential {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported Gemini credential version");
        }
        bounded_secret(&self.access_token, 8, 16_384, "access token")?;
        bounded_secret(&self.refresh_token, 8, 16_384, "refresh token")?;
        bounded_text(&self.oauth_client_id, 8, 1_024, "OAuth client id")?;
        bounded_secret(&self.oauth_client_secret, 1, 4_096, "OAuth client secret")?;
        let test_loopback = cfg!(feature = "test-loopback-token-uri")
            && (self.token_uri.starts_with("http://127.0.0.1:")
                || self.token_uri.starts_with("http://[::1]:"));
        if self.token_uri != "https://oauth2.googleapis.com/token" && !test_loopback {
            bail!("Gemini token endpoint is not pinned");
        }
        bounded_text(&self.subject, 1, 512, "Google subject")?;
        bounded_text(&self.email, 3, 512, "Google email")?;
        bounded_text(&self.project_id, 1, 512, "Code Assist project")?;
        bounded_text(&self.tier_id, 0, 256, "Gemini tier id")?;
        bounded_text(&self.tier_name, 0, 512, "Gemini tier name")?;
        bounded_text(&self.plan, 1, 128, "Gemini plan")?;
        if !self.proxy.is_empty() {
            bounded_secret(&self.proxy, 8, 4_096, "proxy URL")?;
        }
        if self.proxy_order_id < 0 {
            bail!("invalid Gemini proxy order id");
        }
        Ok(())
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
                .ok_or_else(|| anyhow!("Gemini credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate Gemini credential key id");
            }
        }
        if keys.is_empty() {
            bail!("Gemini credential keyring is empty");
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
        credential: &GeminiCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active Gemini credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode Gemini credential")?);
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
            .map_err(|_| anyhow!("seal Gemini credential failed"))?;
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
    ) -> anyhow::Result<GeminiCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported Gemini credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Gemini credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Gemini credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Gemini credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Gemini credential ciphertext")?;
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
                .map_err(|_| anyhow!("Gemini credential authentication failed"))?,
        );
        let credential: GeminiCredential =
            serde_json::from_slice(&plaintext).context("decode Gemini credential")?;
        credential.validate()?;
        Ok(credential)
    }

    /// Seal a short-lived producer secret (currently PKCE verifier plus account proxy) under a
    /// distinct AEAD domain. `context_id` is the one-use OAuth state, preventing ciphertext from
    /// being moved to another pending callback row.
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
            .ok_or_else(|| anyhow!("active Gemini credential key id is unavailable"))?;
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
            .map_err(|_| anyhow!("seal Gemini pending secret failed"))?;
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
            bail!("unsupported Gemini pending-secret envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Gemini pending-secret key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Gemini pending-secret nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Gemini pending-secret nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Gemini pending-secret ciphertext")?;
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
                .map_err(|_| anyhow!("Gemini pending-secret authentication failed"))?,
        );
        let secret = std::str::from_utf8(&plaintext)
            .context("decode Gemini pending secret")?
            .to_string();
        bounded_secret(&secret, 1, 8_192, "pending secret")?;
        Ok(SecretString::new(secret))
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode Gemini credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode Gemini credential envelope")
}

pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Gemini profile id must match [A-Za-z0-9_-] and be 1..=64 bytes");
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
        bail!("Gemini credential key id must match [A-Za-z0-9_-]");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("Gemini credential keys must be exactly 32 bytes encoded as 64 hex characters");
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
        _ => bail!("Gemini credential key contains non-hex characters"),
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

    fn credential() -> GeminiCredential {
        GeminiCredential {
            version: 1,
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at: 123,
            oauth_client_id: "client.apps.googleusercontent.com".into(),
            oauth_client_secret: "client-secret".into(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
            subject: "google-subject".into(),
            email: "owner@example.com".into(),
            project_id: "managed-project".into(),
            tier_id: "paid-tier".into(),
            tier_name: "Google AI Pro".into(),
            plan: "google_ai_pro".into(),
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
                "oauth_state_01",
                "http://user:pass@127.0.0.1:8080",
            )
            .unwrap();
        let encoded = encode_envelope(&sealed).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("user:pass"));
        assert_eq!(
            ring.open_secret("oauth_state_01", &sealed)
                .unwrap()
                .as_str(),
            "http://user:pass@127.0.0.1:8080"
        );
        assert!(ring.open_secret("oauth_state_02", &sealed).is_err());
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
}
