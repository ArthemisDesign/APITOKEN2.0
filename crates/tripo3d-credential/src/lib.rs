//! Encrypted, identity-bound Tripo3D (VAST / Holymolly) API-platform credentials.
//!
//! Pure AEAD envelope handling: no network, no HTTP, no filesystem policy. The contract is
//! `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6, and the provider facts it encodes are
//! recorded in `docs/engine/TRIPO3D_PROVIDER.md` §2.
//!
//! Two properties of this provider shape the module:
//!
//! * **The credential is a static console API key.** The Tripo3D API platform has no OAuth
//!   flow, no scopes and no refresh family: a `tsk_` key is issued from
//!   `platform.tripo3d.ai/api-keys` and stays valid until it is reissued. There is
//!   deliberately no `rotate`/`is_expired` surface — rotation means issuing a new key in the
//!   console and sealing a fresh envelope through the Auth Bot.
//! * **There is no plan ladder on the API side.** The platform is prepaid credits
//!   ($0.01/credit, top-ups in units of 100); the only cohort axis is the declared top-up
//!   cohort of the offer product (e.g. "Tripo3D API $50"), which the calibration authority
//!   keys on (`crates/registry/migrations_pg/0049_tripo3d_calibration.sql`, `cohort`
//!   column). Unlike GLM there is no published tier list, so there is no reviewed-plan
//!   table here at all.

use std::collections::HashMap;

use anyhow::{anyhow, bail, Context};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, Zeroizing};

const ENVELOPE_VERSION: u8 = 1;
const CREDENTIAL_VERSION: u8 = 1;
const AAD_PREFIX: &[u8] = b"apitoken/tripo3d-api-credential/v1\0";

/// Global Tripo3D API platform origin. Keys are issued per origin and are not
/// interchangeable with the China site (manifest §2).
pub const TRIPO3D_BASE_URL_GLOBAL: &str = "https://api.tripo3d.ai";
/// China Tripo3D API platform origin.
pub const TRIPO3D_BASE_URL_CHINA: &str = "https://api.tripo3d.com";

/// Task creation/polling endpoint served on the platform base URL. Authorization carries
/// the key with a `Bearer` prefix on every request (official, quick-start).
pub const TRIPO3D_TASK_PATH: &str = "/v2/openapi/task";
/// Balance endpoint, the only machine-readable quota surface (`oss-hypothesis`: official
/// Python SDK only; the unit of `balance`/`frozen` is unproven — manifest §5.2/§6).
pub const TRIPO3D_BALANCE_PATH: &str = "/v2/openapi/user/balance";
/// Multipart image-upload endpoint returning an `image_token`. SDK-verified only
/// (`oss-hypothesis`: official Python SDK, not a docs page).
pub const TRIPO3D_UPLOAD_STS_PATH: &str = "/v2/openapi/upload/sts";

/// The documented prefix of an API-platform key. A `tcli_` Client ID is documented to
/// answer 401 and is refused at seal time.
pub const TRIPO3D_API_KEY_PREFIX: &str = "tsk_";

/// How a credential authenticates to the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tripo3dCredentialKind {
    /// Static API-platform key issued from the console. No scopes, no refresh, no expiry.
    /// The only kind this provider has.
    ApiKey,
}

impl Tripo3dCredentialKind {
    fn as_aad(self) -> &'static [u8] {
        match self {
            Self::ApiKey => b"api-key",
        }
    }
}

/// A Tripo3D API-platform credential. Secrets never reach `Debug` output.
#[derive(Clone, Serialize, Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct Tripo3dCredential {
    #[zeroize(skip)]
    pub version: u8,
    #[zeroize(skip)]
    pub kind: Tripo3dCredentialKind,

    /// Static platform API key issued from the console (`tsk_…`). Tripo3D has no
    /// machine-readable subject (no `/me` exists), so the whole key is the dedup identity:
    /// it stays inside the envelope, and the caller (Auth Bot) compares opened envelopes to
    /// detect one key occupying two profiles. This crate deliberately keeps no hashing
    /// dependency.
    pub api_key: String,

    /// Declared top-up cohort of the offer product (e.g. the catalog entry "Tripo3D API
    /// $50"), lowercase-normalized. It is the calibration cohort key and must match the
    /// `cohort` column of `crates/registry/migrations_pg/0049_tripo3d_calibration.sql`.
    #[zeroize(skip)]
    pub cohort: String,

    /// Platform origin the key was issued on, in canonical form: exactly
    /// [`TRIPO3D_BASE_URL_GLOBAL`] or [`TRIPO3D_BASE_URL_CHINA`].
    #[zeroize(skip)]
    pub base_url: String,

    /// Optional egress assigned by the Auth Bot. Never logged.
    pub proxy_url: String,
}

impl std::fmt::Debug for Tripo3dCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tripo3dCredential")
            .field("version", &self.version)
            .field("kind", &self.kind)
            .field("cohort", &self.cohort)
            .field("base_url", &self.base_url)
            .field("api_key", &"REDACTED")
            .field("proxy_url", &"REDACTED")
            .finish()
    }
}

const MAX_KEY_LEN: usize = 8192;
const MAX_COHORT_LEN: usize = 128;
const MAX_FIELD_LEN: usize = 512;

impl Tripo3dCredential {
    /// Strict bounded validation. Every rejection is a refusal to route, never a warning.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported Tripo3D credential version");
        }
        if self.api_key.is_empty() || self.api_key.len() > MAX_KEY_LEN {
            bail!("Tripo3D credential API key is missing or oversized");
        }
        // The prefix is a documented authenticity signal: a `tcli_` Client ID is not a
        // credential and is documented to answer 401, so it fails closed here instead of
        // burning a live request.
        if !self.api_key.starts_with(TRIPO3D_API_KEY_PREFIX) {
            bail!("Tripo3D credential API key must carry the tsk_ prefix");
        }
        // The stored cohort must already be lowercase-normalized, so an envelope can never
        // carry a spelling that would aggregate into a different calibration cohort.
        if normalize_cohort(&self.cohort)? != self.cohort {
            bail!("Tripo3D credential cohort is not in canonical form");
        }
        // The stored base URL must already be canonical, so an envelope can never carry
        // a spelling that parses to an allowed origin but compares unequal to it.
        if normalize_base_url(&self.base_url)? != self.base_url {
            bail!("Tripo3D credential base url is not in canonical form");
        }
        if !self.proxy_url.is_empty() {
            if self.proxy_url.len() > MAX_FIELD_LEN {
                bail!("Tripo3D credential proxy url is oversized");
            }
            normalize_proxy_url(&self.proxy_url)?;
        }
        Ok(())
    }
}

/// Normalize a declared top-up cohort to its canonical form: trimmed, lowercase, bounded
/// and non-empty. The cohort is the calibration aggregation key, so case or whitespace
/// differences must not split one cohort into two.
pub fn normalize_cohort(cohort: &str) -> anyhow::Result<String> {
    let normalized = cohort.trim().to_ascii_lowercase();
    if normalized.is_empty() || normalized.len() > MAX_COHORT_LEN {
        bail!("Tripo3D credential cohort is missing or oversized");
    }
    Ok(normalized)
}

/// Normalize a Tripo3D platform base URL to its canonical origin. Exactly two origins are
/// routable; any other host, a non-root path, a query, a fragment or embedded credentials
/// are refused, because a key is only ever valid against the platform site that issued it.
pub fn normalize_base_url(base_url: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(base_url).context("parse Tripo3D base url")?;
    if parsed.scheme() != "https" && !is_test_loopback(&parsed) {
        bail!("Tripo3D base url must use https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("Tripo3D base url must not carry credentials");
    }
    if parsed.path() != "/" {
        bail!("Tripo3D base url must not carry a path");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        bail!("Tripo3D base url must not carry a query or fragment");
    }
    let origin = parsed.to_string();
    let origin = origin.trim_end_matches('/');
    match origin {
        TRIPO3D_BASE_URL_GLOBAL | TRIPO3D_BASE_URL_CHINA => Ok(origin.to_string()),
        _ if is_test_loopback(&parsed) => Ok(origin.to_string()),
        _ => bail!("Tripo3D base url host is not an official platform origin"),
    }
}

/// Test-only escape hatch: mock upstreams serve plain HTTP on loopback. The feature is enabled
/// exclusively through consumer dev-dependencies, so production binaries keep the strict
/// two-origin https allowlist above.
#[cfg(feature = "test-loopback-base-url")]
fn is_test_loopback(parsed: &url::Url) -> bool {
    parsed.scheme() == "http"
        && matches!(
            parsed.host_str(),
            Some("127.0.0.1") | Some("localhost") | Some("[::1]")
        )
}

#[cfg(not(feature = "test-loopback-base-url"))]
fn is_test_loopback(_parsed: &url::Url) -> bool {
    false
}

/// Normalize and bound an egress proxy URL. Credentials embedding one must not be able to
/// smuggle arbitrary schemes or fragments into the transport layer.
pub fn normalize_proxy_url(proxy: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(proxy).context("parse Tripo3D proxy url")?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => bail!("unsupported Tripo3D proxy scheme"),
    }
    if parsed.host_str().unwrap_or_default().is_empty() {
        bail!("Tripo3D proxy url has no host");
    }
    if parsed.fragment().is_some() {
        bail!("Tripo3D proxy url must not carry a fragment");
    }
    Ok(parsed.to_string())
}

/// Bound the opaque profile id used as AAD. Path separators and traversal are refused so
/// a profile id can never escape its roster directory.
pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 128 {
        bail!("Tripo3D profile id is missing or oversized");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Tripo3D profile id contains unsupported characters");
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    if key_id.is_empty() || key_id.len() > 64 {
        bail!("Tripo3D credential key id is missing or oversized");
    }
    if !key_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Tripo3D credential key id contains unsupported characters");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("Tripo3D credential key must be 32 bytes of hex");
    }
    let mut key = [0u8; 32];
    for (index, chunk) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).context("decode Tripo3D credential key")?;
        key[index] =
            u8::from_str_radix(text, 16).context("Tripo3D credential key is not valid hex")?;
    }
    Ok(key)
}

/// AAD binds the ciphertext to both the profile it belongs to and the credential kind, so
/// an envelope cannot be moved between profiles or reinterpreted as another kind.
fn associated_data(profile_id: &str, kind: Tripo3dCredentialKind) -> Vec<u8> {
    let mut aad = Vec::with_capacity(AAD_PREFIX.len() + profile_id.len() + 16);
    aad.extend_from_slice(AAD_PREFIX);
    aad.extend_from_slice(kind.as_aad());
    aad.push(0);
    aad.extend_from_slice(profile_id.as_bytes());
    aad
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SealedCredential {
    pub version: u8,
    pub key_id: String,
    /// Recorded in cleartext so `open` can select AAD without trial decryption.
    pub kind: Tripo3dCredentialKind,
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
    /// Parse `kid:64-hex-bytes[,kid:64-hex-bytes...]`. Old keys are retained so the
    /// runtime can still open existing envelopes while the Auth Bot seals new ones under
    /// the active key.
    pub fn parse(specification: &str) -> anyhow::Result<Self> {
        let mut keys = HashMap::new();
        for entry in specification
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let (key_id, encoded) = entry
                .split_once(':')
                .ok_or_else(|| anyhow!("Tripo3D credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate Tripo3D credential key id");
            }
        }
        if keys.is_empty() {
            bail!("Tripo3D credential keyring is empty");
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
        credential: &Tripo3dCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active Tripo3D credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode Tripo3D credential")?);
        let mut nonce = [0u8; 24];
        getrandom::fill(&mut nonce).map_err(|_| anyhow!("operating-system CSPRNG unavailable"))?;
        let aad = associated_data(profile_id, credential.kind);
        let ciphertext = XChaCha20Poly1305::new(Key::from_slice(key))
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &plaintext,
                    aad: &aad,
                },
            )
            .map_err(|_| anyhow!("seal Tripo3D credential failed"))?;
        Ok(SealedCredential {
            version: ENVELOPE_VERSION,
            key_id: active_key_id.to_string(),
            kind: credential.kind,
            nonce: URL_SAFE_NO_PAD.encode(nonce),
            ciphertext: URL_SAFE_NO_PAD.encode(ciphertext),
        })
    }

    pub fn open(
        &self,
        profile_id: &str,
        envelope: &SealedCredential,
    ) -> anyhow::Result<Tripo3dCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported Tripo3D credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Tripo3D credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Tripo3D credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Tripo3D credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Tripo3D credential ciphertext")?;
        let aad = associated_data(profile_id, envelope.kind);
        let plaintext = Zeroizing::new(
            XChaCha20Poly1305::new(Key::from_slice(key))
                .decrypt(
                    XNonce::from_slice(&nonce),
                    Payload {
                        msg: &ciphertext,
                        aad: &aad,
                    },
                )
                .map_err(|_| anyhow!("Tripo3D credential authentication failed"))?,
        );
        let credential: Tripo3dCredential =
            serde_json::from_slice(&plaintext).context("decode Tripo3D credential")?;
        credential.validate()?;
        // The cleartext kind is an AEAD input, so a mismatch here means the envelope was
        // tampered with in a way that still authenticated — refuse rather than trust it.
        if credential.kind != envelope.kind {
            bail!("Tripo3D credential kind does not match its envelope");
        }
        Ok(credential)
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode Tripo3D credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode Tripo3D credential envelope")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "a1";
    const KEY_B: &str = "b2";

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!(
            "{KEY_A}:{},{KEY_B}:{}",
            "11".repeat(32),
            "22".repeat(32)
        ))
        .expect("keyring parses")
    }

    fn api_credential() -> Tripo3dCredential {
        Tripo3dCredential {
            version: CREDENTIAL_VERSION,
            kind: Tripo3dCredentialKind::ApiKey,
            api_key: "tsk_test-9f8c7b6a5d.secretpart0123456789".into(),
            cohort: "tripo3d-api-50".into(),
            base_url: TRIPO3D_BASE_URL_GLOBAL.into(),
            proxy_url: String::new(),
        }
    }

    #[test]
    fn seal_open_roundtrip_preserves_identity() {
        let ring = keyring();
        let credential = api_credential();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        let opened = ring.open("profile-1", &sealed).unwrap();
        assert_eq!(opened.cohort, "tripo3d-api-50");
        assert_eq!(opened.base_url, TRIPO3D_BASE_URL_GLOBAL);
        // The whole key is the dedup identity, so it must survive the envelope intact.
        assert_eq!(opened.api_key, credential.api_key);
    }

    #[test]
    fn envelope_cannot_be_moved_to_another_profile() {
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        assert!(ring.open("profile-2", &sealed).is_err());
    }

    #[test]
    fn envelope_kind_substitution_is_refused() {
        // `kind` is cleartext so `open` can build AAD, but it is also an AEAD input and a
        // single-variant enum: substituting another kind string breaks deserialization,
        // and any future second variant would break authentication. Both fail closed.
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        let tampered = String::from_utf8(bytes)
            .unwrap()
            .replace("api_key", "oauth");
        assert!(decode_envelope(tampered.as_bytes()).is_err());
    }

    #[test]
    fn old_keys_stay_readable_so_rotation_can_be_online() {
        let ring = keyring();
        let sealed_old = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        let sealed_new = ring.seal(KEY_B, "profile-1", &api_credential()).unwrap();
        assert!(ring.open("profile-1", &sealed_old).is_ok());
        assert!(ring.open("profile-1", &sealed_new).is_ok());
        assert!(ring.contains(KEY_A) && ring.contains(KEY_B));
    }

    #[test]
    fn unknown_key_id_fails_closed() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        sealed.key_id = "zz".into();
        assert!(ring.open("profile-1", &sealed).is_err());
        assert!(ring.seal("zz", "profile-1", &api_credential()).is_err());
    }

    #[test]
    fn tampered_ciphertext_is_refused() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        sealed.ciphertext.push('A');
        assert!(ring.open("profile-1", &sealed).is_err());
    }

    #[test]
    fn base_url_allowlist_accepts_both_platform_origins() {
        assert_eq!(
            normalize_base_url(TRIPO3D_BASE_URL_GLOBAL).unwrap(),
            TRIPO3D_BASE_URL_GLOBAL
        );
        assert_eq!(
            normalize_base_url(TRIPO3D_BASE_URL_CHINA).unwrap(),
            TRIPO3D_BASE_URL_CHINA
        );
        // A trailing slash from URL serialization normalizes away at intake.
        assert_eq!(
            normalize_base_url("https://api.tripo3d.ai/").unwrap(),
            TRIPO3D_BASE_URL_GLOBAL
        );

        let ring = keyring();
        let mut credential = api_credential();
        credential.base_url = TRIPO3D_BASE_URL_CHINA.into();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        assert_eq!(
            ring.open("profile-1", &sealed).unwrap().base_url,
            TRIPO3D_BASE_URL_CHINA
        );
    }

    #[test]
    fn base_url_rejects_foreign_hosts_paths_and_credentials() {
        for bad in [
            "https://tripo3d.example.com",
            "https://api.tripo3d.ai.evil.com",
            "http://api.tripo3d.ai",
            "https://api.tripo3d.ai:8443",
            "https://api.tripo3d.ai/v2/openapi",
            "https://api.tripo3d.ai/?x=1",
            "https://api.tripo3d.ai/#frag",
            "https://user:pass@api.tripo3d.ai",
            "https://openapi.tripo3d.ai",
            "not-a-url",
        ] {
            assert!(normalize_base_url(bad).is_err(), "accepted {bad}");
        }

        // A stored envelope must carry the canonical form: the caller normalizes at
        // intake, and sealing a non-canonical spelling fails closed.
        let mut credential = api_credential();
        credential.base_url = "https://api.tripo3d.ai/".into();
        assert!(credential.validate().is_err());

        credential.base_url = "https://tripo3d.example.com".into();
        let ring = keyring();
        assert!(ring.seal(KEY_A, "profile-1", &credential).is_err());
    }

    #[cfg(feature = "test-loopback-base-url")]
    #[test]
    fn loopback_base_url_is_accepted_only_with_the_test_feature() {
        assert_eq!(
            normalize_base_url("http://127.0.0.1:9876").unwrap(),
            "http://127.0.0.1:9876"
        );
        assert!(normalize_base_url("http://localhost:9876").is_ok());
        // Loopback never relaxes the rest of the rules.
        assert!(normalize_base_url("http://127.0.0.1:9876/path").is_err());
        assert!(normalize_base_url("http://user:pass@127.0.0.1:9876").is_err());
        assert!(normalize_base_url("http://169.254.1.1:9876").is_err());
    }

    #[cfg(not(feature = "test-loopback-base-url"))]
    #[test]
    fn loopback_base_url_is_refused_without_the_test_feature() {
        assert!(normalize_base_url("http://127.0.0.1:9876").is_err());
        assert!(normalize_base_url("http://localhost:9876").is_err());
    }

    #[test]
    fn api_key_prefix_is_enforced() {
        // A `tcli_` Client ID is documented to answer 401; it must never enter an envelope.
        let mut client_id = api_credential();
        client_id.api_key = "tcli_9f8c7b6a5d0123456789".into();
        assert!(client_id.validate().is_err());

        let mut bare = api_credential();
        bare.api_key = "9f8c7b6a5d0123456789".into();
        assert!(bare.validate().is_err());

        let ring = keyring();
        assert!(ring.seal(KEY_A, "profile-1", &client_id).is_err());
    }

    #[test]
    fn cohort_is_lowercase_normalized_and_bounded() {
        assert_eq!(normalize_cohort(" Tripo3D API $50 ").unwrap(), "tripo3d api $50");
        assert_eq!(normalize_cohort("COHORT-A").unwrap(), "cohort-a");
        for bad in ["", "   ", &"c".repeat(MAX_COHORT_LEN + 1)] {
            assert!(normalize_cohort(bad).is_err(), "accepted {bad:?}");
        }

        // The stored cohort must already be canonical: sealing a non-normalized spelling
        // fails closed so one top-up cohort cannot split into two aggregation keys.
        let mut credential = api_credential();
        credential.cohort = "Tripo3D-API-50".into();
        assert!(credential.validate().is_err());
        credential.cohort = String::new();
        assert!(credential.validate().is_err());
    }

    #[test]
    fn official_endpoint_constants_match_the_reviewed_contract() {
        assert_eq!(TRIPO3D_BASE_URL_GLOBAL, "https://api.tripo3d.ai");
        assert_eq!(TRIPO3D_BASE_URL_CHINA, "https://api.tripo3d.com");
        assert_eq!(TRIPO3D_TASK_PATH, "/v2/openapi/task");
        assert_eq!(TRIPO3D_BALANCE_PATH, "/v2/openapi/user/balance");
        assert_eq!(TRIPO3D_UPLOAD_STS_PATH, "/v2/openapi/upload/sts");
        assert_eq!(TRIPO3D_API_KEY_PREFIX, "tsk_");
    }

    #[test]
    fn bounded_fields_fail_closed() {
        let mut no_key = api_credential();
        no_key.api_key = String::new();
        assert!(no_key.validate().is_err());

        let mut oversized_key = api_credential();
        oversized_key.api_key = format!("{}{}", TRIPO3D_API_KEY_PREFIX, "x".repeat(MAX_KEY_LEN));
        assert!(oversized_key.validate().is_err());

        let mut oversized_proxy = api_credential();
        oversized_proxy.proxy_url =
            format!("http://egress.example:8080/{}", "p".repeat(MAX_FIELD_LEN));
        assert!(oversized_proxy.validate().is_err());

        let mut bad_proxy = api_credential();
        bad_proxy.proxy_url = "file:///etc/passwd".into();
        assert!(bad_proxy.validate().is_err());

        let mut wrong_version = api_credential();
        wrong_version.version = 99;
        assert!(wrong_version.validate().is_err());
    }

    #[test]
    fn debug_never_prints_secrets() {
        let mut credential = api_credential();
        credential.api_key = "tsk_super-secret-key".into();
        credential.proxy_url = "http://user:pr0xy-pass@egress.example:8080/".into();
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("tsk_super-secret-key"));
        assert!(!rendered.contains("pr0xy-pass"));
        assert!(rendered.contains("REDACTED"));
        // Non-secret identity stays visible so operators can diagnose.
        assert!(rendered.contains("tripo3d-api-50"));
        assert!(rendered.contains(TRIPO3D_BASE_URL_GLOBAL));

        let ring = keyring();
        assert!(!format!("{ring:?}").contains("1111"));

        // Error displays must not echo secrets either: a validation failure carries the
        // reason, never the key material or the proxy password.
        let mut bad_base = credential.clone();
        bad_base.base_url = "https://tripo3d.example.com".into();
        let err = format!("{:#}", bad_base.validate().unwrap_err());
        assert!(!err.contains("tsk_super-secret-key"));
        assert!(!err.contains("pr0xy-pass"));

        let mut bad_proxy = credential.clone();
        bad_proxy.proxy_url = "http://user:pr0xy-pass@egress.example:99999/".into();
        let err = format!("{:#}", bad_proxy.validate().unwrap_err());
        assert!(!err.contains("pr0xy-pass"));
    }

    #[test]
    fn profile_ids_cannot_escape_their_directory() {
        assert!(validate_profile_id("tripo3d-01_ab").is_ok());
        for bad in ["", "../escape", "a/b", "a\\b", "a b", &"x".repeat(129)] {
            assert!(validate_profile_id(bad).is_err(), "accepted {bad:?}");
        }
    }

    #[test]
    fn proxy_urls_are_bounded_to_known_schemes() {
        assert!(normalize_proxy_url("http://egress.example:8080").is_ok());
        assert!(normalize_proxy_url("socks5://egress.example:1080").is_ok());
        for bad in [
            "file:///etc/passwd",
            "ftp://egress.example",
            "http://egress.example#frag",
            "not-a-url",
        ] {
            assert!(normalize_proxy_url(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn proxy_userinfo_survives_normalization_reconstruction() {
        // A password carrying a percent-encoded byte and a literal colon must survive the
        // parse → normalize → serialize → re-parse round trip unchanged: the Auth Bot
        // stores the normalized form and the transport re-parses it.
        let original = url::Url::parse("socks5://user:p%41ss:w@egress.example:1080").unwrap();
        let normalized = normalize_proxy_url("socks5://user:p%41ss:w@egress.example:1080").unwrap();
        let reparsed = url::Url::parse(&normalized).unwrap();
        assert_eq!(reparsed.username(), original.username());
        assert_eq!(reparsed.password(), original.password());
        assert_eq!(reparsed.host_str(), original.host_str());
        assert_eq!(
            reparsed.port_or_known_default(),
            original.port_or_known_default()
        );
        // Normalization is idempotent, so storing the normalized form is stable.
        assert_eq!(normalize_proxy_url(&normalized).unwrap(), normalized);
    }

    #[test]
    fn keyring_rejects_malformed_specifications() {
        for bad in [
            "",
            "nokeyid",
            "a1:short",
            &format!("{KEY_A}:{},{KEY_A}:{}", "11".repeat(32), "22".repeat(32)),
        ] {
            assert!(CredentialKeyring::parse(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn envelope_encoding_roundtrips() {
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &api_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        assert_eq!(decode_envelope(&bytes).unwrap(), sealed);
    }
}
