//! Encrypted, identity-bound Suno (suno.com) subscription session credentials.
//!
//! Pure AEAD envelope handling: no network, no HTTP, no filesystem policy. The contract is
//! `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6, and the provider facts it encodes are
//! recorded in `docs/engine/SUNO_PROVIDER.md` §2.
//!
//! Two properties of this provider shape the module:
//!
//! * **The credential is a subscription web session, not an API key.** Suno has no public
//!   official API; the only working access is session-cookie pooling against internal web
//!   endpoints (oss-hypothesis, `gcui-art/suno-api`, read 2026-08-12 — fail closed until a
//!   live run proves the wire). The sealed envelope holds the full browser cookie string,
//!   whose critical entry is the Clerk `__client` cookie, plus the discovered session id.
//! * **JWT minting and `set-cookie` re-seal are the runtime's concern, not this crate's.**
//!   Short-lived JWTs are minted on demand and never persisted; because a mint response may
//!   rotate the underlying Clerk token, the runtime holds a per-profile single-flight from
//!   JWT mint through envelope re-seal (the KIMI rotating-family discipline: the winner
//!   re-seals before releasing the lock, the loser re-reads). This crate only seals, opens
//!   and validates — it never mints, never merges `set-cookie`, and never calls a host.
//!
//! There is deliberately **no base-url field**: one platform (`suno.com`) with fixed hosts
//! means a host override knob could only smuggle the session to a foreign origin. The hosts
//! and paths below are constants, not credential state.

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
const AAD_PREFIX: &[u8] = b"apitoken/suno-session-credential/v1\0";

/// Clerk authentication host (oss-hypothesis: gcui-art/suno-api, read 2026-08-12; fail
/// closed until proven live).
pub const SUNO_AUTH_BASE_URL: &str = "https://auth.suno.com";
/// Business host serving generation, feed and billing (oss-hypothesis: gcui-art/suno-api,
/// read 2026-08-12; fail closed until proven live).
pub const SUNO_API_BASE_URL: &str = "https://studio-api.prod.suno.com";

/// Clerk session-discovery endpoint on the auth host; answers `last_active_session_id`
/// (oss-hypothesis: gcui-art/suno-api, read 2026-08-12; fail closed until live).
pub const SUNO_CLIENT_PATH: &str = "/v1/client";
/// Clerk JWT-mint endpoint on the auth host. The `{sid}` placeholder is substituted by the
/// caller with the discovered session id (oss-hypothesis: gcui-art/suno-api, read
/// 2026-08-12; fail closed until live).
pub const SUNO_SESSION_TOKENS_PATH: &str = "/v1/client/sessions/{sid}/tokens";
/// Song-generation endpoint on the business host, the only operation admitted in v1
/// (oss-hypothesis: gcui-art/suno-api, read 2026-08-12; fail closed until live).
pub const SUNO_GENERATE_PATH: &str = "/api/generate/v2/";
/// Native quota endpoint (`total_credits_left`, `period`, `monthly_limit`,
/// `monthly_usage`) — semantics unproven, raw counters preserved verbatim (oss-hypothesis:
/// gcui-art/suno-api, read 2026-08-12; fail closed until live).
pub const SUNO_BILLING_INFO_PATH: &str = "/api/billing/info/";
/// hCaptcha gate probe (`{"ctype":"generation"}` → `{"required": bool}`); no CAPTCHA
/// solving is built — a required gate soft-cools the profile (oss-hypothesis:
/// gcui-art/suno-api, read 2026-08-12; fail closed until live).
pub const SUNO_CAPTCHA_CHECK_PATH: &str = "/api/c/check";
/// Generation status/polling feed (oss-hypothesis: gcui-art/suno-api, read 2026-08-12;
/// fail closed until live).
pub const SUNO_FEED_PATH: &str = "/api/feed/v2";
/// Clip metadata/result URLs (oss-hypothesis: gcui-art/suno-api, read 2026-08-12; fail
/// closed until live). Result media is downloaded into our own storage; upstream URLs are
/// never exposed to the customer.
pub const SUNO_CLIP_PATH: &str = "/api/clip";

/// The Clerk cookie entry a session cookie string must carry.
pub const SUNO_REQUIRED_COOKIE: &str = "__client";

/// How a credential authenticates to the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SunoCredentialKind {
    /// A full browser session cookie string of a paid subscription account. The only kind
    /// this provider has.
    SessionCookie,
}

impl SunoCredentialKind {
    fn as_aad(self) -> &'static [u8] {
        match self {
            Self::SessionCookie => b"session-cookie",
        }
    }
}

/// A declared paid Suno plan. The Free tier is excluded by design (no commercial rights, a
/// daily 50-credit drip, an explicit anti-pooling clause — manifest §1). The canonical
/// labels match the `plan` CHECK of
/// `crates/registry/migrations_pg/0050_suno_window_calibration.sql` exactly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SunoPlan {
    Pro,
    Premier,
}

impl SunoPlan {
    /// Canonical label used inside sealed envelopes and roster rows (`Pro`/`Premier`,
    /// matching the calibration schema's `plan IN ('Pro', 'Premier')` CHECK).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pro => "Pro",
            Self::Premier => "Premier",
        }
    }

    /// Parse a declared plan, normalizing case and surrounding whitespace. Free and any
    /// legacy or unknown tier are refused: they are not saleable capacity and fail closed
    /// at onboarding (`docs/engine/SUNO_PROVIDER.md` §1).
    pub fn parse(declared: &str) -> anyhow::Result<Self> {
        match declared.trim().to_ascii_lowercase().as_str() {
            "pro" => Ok(Self::Pro),
            "premier" => Ok(Self::Premier),
            _ => bail!("Suno plan must be one of Pro or Premier"),
        }
    }
}

/// A plan whose published monthly window credits were confirmed against the official
/// pricing page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewedPlan {
    pub plan: SunoPlan,
    /// Published credits per monthly window (no rollover).
    pub credits_per_month: u64,
    /// Date the limits were confirmed against `suno.com/pricing`, `YYYY-MM-DD`.
    pub reviewed_on: &'static str,
}

/// Both paid tiers, with their officially published monthly credits (`suno.com/pricing`
/// embedded plan config, reviewed 2026-08-12). Cohorts merge only by exact plan + the
/// monthly window, so an unlisted plan must stay unrepresentable at [`SunoPlan::parse`].
pub const SUNO_REVIEWED_PLANS: &[ReviewedPlan] = &[
    ReviewedPlan {
        plan: SunoPlan::Pro,
        credits_per_month: 2_500,
        reviewed_on: "2026-08-12",
    },
    ReviewedPlan {
        plan: SunoPlan::Premier,
        credits_per_month: 10_000,
        reviewed_on: "2026-08-12",
    },
];

/// Published monthly window credits for a plan, or `None` for a tier added ahead of its
/// review.
pub fn reviewed_plan_credits(plan: SunoPlan) -> Option<u64> {
    SUNO_REVIEWED_PLANS
        .iter()
        .find(|entry| entry.plan == plan)
        .map(|entry| entry.credits_per_month)
}

/// A Suno subscription session credential. Secrets never reach `Debug` output.
#[derive(Clone, Serialize, Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct SunoCredential {
    #[zeroize(skip)]
    pub version: u8,
    #[zeroize(skip)]
    pub kind: SunoCredentialKind,

    /// Full browser cookie string of the session; the critical entry is the Clerk
    /// `__client` cookie. Bounded and validated for that entry at seal.
    pub cookie: String,

    /// Discovered Clerk session id (`last_active_session_id` from the client endpoint).
    /// Optional because it is rediscoverable from the cookie via [`SUNO_CLIENT_PATH`]; it
    /// is the dedup identity until a `/me`-class endpoint is proven live (manifest §2).
    pub session_id: Option<String>,

    /// Declared paid plan, corroborated by the observed `monthly_limit` at admission.
    #[zeroize(skip)]
    pub plan: SunoPlan,

    /// Optional egress assigned by the Auth Bot. Every provider call, including JWT mints,
    /// goes through this proxy to keep the account geography stable. Never logged.
    pub proxy_url: String,
}

impl std::fmt::Debug for SunoCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SunoCredential")
            .field("version", &self.version)
            .field("kind", &self.kind)
            .field("plan", &self.plan)
            .field("cookie", &"REDACTED")
            .field("session_id", &"REDACTED")
            .field("proxy_url", &"REDACTED")
            .finish()
    }
}

const MAX_COOKIE_LEN: usize = 8192;
const MAX_FIELD_LEN: usize = 512;

impl SunoCredential {
    /// Strict bounded validation. Every rejection is a refusal to route, never a warning.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported Suno credential version");
        }
        if self.cookie.is_empty() || self.cookie.len() > MAX_COOKIE_LEN {
            bail!("Suno credential cookie is missing or oversized");
        }
        // Without the Clerk `__client` entry the material cannot mint a JWT at all, so the
        // absence fails closed at seal instead of burning a live session probe.
        if !cookie_entries(&self.cookie).any(|(name, value)| {
            name == SUNO_REQUIRED_COOKIE && !value.is_empty()
        }) {
            bail!("Suno credential cookie must carry a non-empty __client entry");
        }
        if let Some(session_id) = &self.session_id {
            if session_id.is_empty() || session_id.len() > MAX_FIELD_LEN {
                bail!("Suno credential session id is empty or oversized");
            }
        }
        if !self.proxy_url.is_empty() {
            if self.proxy_url.len() > MAX_FIELD_LEN {
                bail!("Suno credential proxy url is oversized");
            }
            normalize_proxy_url(&self.proxy_url)?;
        }
        Ok(())
    }
}

/// Iterate the `name=value` entries of a cookie string, trimming around each `;`-separated
/// part. Malformed parts without an `=` are skipped rather than trusted.
fn cookie_entries(cookie: &str) -> impl Iterator<Item = (&str, &str)> {
    cookie.split(';').filter_map(|part| {
        let (name, value) = part.split_once('=')?;
        Some((name.trim(), value.trim()))
    })
}

/// Normalize and bound an egress proxy URL. Credentials embedding one must not be able to
/// smuggle arbitrary schemes or fragments into the transport layer.
pub fn normalize_proxy_url(proxy: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(proxy).context("parse Suno proxy url")?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => bail!("unsupported Suno proxy scheme"),
    }
    if parsed.host_str().unwrap_or_default().is_empty() {
        bail!("Suno proxy url has no host");
    }
    if parsed.fragment().is_some() {
        bail!("Suno proxy url must not carry a fragment");
    }
    Ok(parsed.to_string())
}

/// Bound the opaque profile id used as AAD. Path separators and traversal are refused so
/// a profile id can never escape its roster directory.
pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 128 {
        bail!("Suno profile id is missing or oversized");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Suno profile id contains unsupported characters");
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    if key_id.is_empty() || key_id.len() > 64 {
        bail!("Suno credential key id is missing or oversized");
    }
    if !key_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Suno credential key id contains unsupported characters");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("Suno credential key must be 32 bytes of hex");
    }
    let mut key = [0u8; 32];
    for (index, chunk) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).context("decode Suno credential key")?;
        key[index] =
            u8::from_str_radix(text, 16).context("Suno credential key is not valid hex")?;
    }
    Ok(key)
}

/// AAD binds the ciphertext to both the profile it belongs to and the credential kind, so
/// an envelope cannot be moved between profiles or reinterpreted as another kind.
fn associated_data(profile_id: &str, kind: SunoCredentialKind) -> Vec<u8> {
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
    pub kind: SunoCredentialKind,
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
                .ok_or_else(|| anyhow!("Suno credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate Suno credential key id");
            }
        }
        if keys.is_empty() {
            bail!("Suno credential keyring is empty");
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
        credential: &SunoCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active Suno credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode Suno credential")?);
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
            .map_err(|_| anyhow!("seal Suno credential failed"))?;
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
    ) -> anyhow::Result<SunoCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported Suno credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("Suno credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode Suno credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid Suno credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode Suno credential ciphertext")?;
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
                .map_err(|_| anyhow!("Suno credential authentication failed"))?,
        );
        let credential: SunoCredential =
            serde_json::from_slice(&plaintext).context("decode Suno credential")?;
        credential.validate()?;
        // The cleartext kind is an AEAD input, so a mismatch here means the envelope was
        // tampered with in a way that still authenticated — refuse rather than trust it.
        if credential.kind != envelope.kind {
            bail!("Suno credential kind does not match its envelope");
        }
        Ok(credential)
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode Suno credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode Suno credential envelope")
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

    fn session_credential() -> SunoCredential {
        SunoCredential {
            version: CREDENTIAL_VERSION,
            kind: SunoCredentialKind::SessionCookie,
            cookie: "__client=test-clerk-token.9f8c7b6a5d; __session=stale; ajs_id=x".into(),
            session_id: Some("sess_2abcdef0123456789".into()),
            plan: SunoPlan::Pro,
            proxy_url: String::new(),
        }
    }

    #[test]
    fn seal_open_roundtrip_preserves_identity() {
        let ring = keyring();
        let credential = session_credential();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        let opened = ring.open("profile-1", &sealed).unwrap();
        assert_eq!(opened.plan, SunoPlan::Pro);
        assert_eq!(opened.session_id, credential.session_id);
        // The session material is the credential, so it must survive the envelope intact.
        assert_eq!(opened.cookie, credential.cookie);
    }

    #[test]
    fn envelope_cannot_be_moved_to_another_profile() {
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        assert!(ring.open("profile-2", &sealed).is_err());
    }

    #[test]
    fn envelope_kind_substitution_is_refused() {
        // `kind` is cleartext so `open` can build AAD, but it is also an AEAD input and a
        // single-variant enum: substituting another kind string breaks deserialization,
        // and any future second variant would break authentication. Both fail closed.
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        let tampered = String::from_utf8(bytes)
            .unwrap()
            .replace("session_cookie", "oauth");
        assert!(decode_envelope(tampered.as_bytes()).is_err());
    }

    #[test]
    fn old_keys_stay_readable_so_rotation_can_be_online() {
        let ring = keyring();
        let sealed_old = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        let sealed_new = ring.seal(KEY_B, "profile-1", &session_credential()).unwrap();
        assert!(ring.open("profile-1", &sealed_old).is_ok());
        assert!(ring.open("profile-1", &sealed_new).is_ok());
        assert!(ring.contains(KEY_A) && ring.contains(KEY_B));
    }

    #[test]
    fn unknown_key_id_fails_closed() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        sealed.key_id = "zz".into();
        assert!(ring.open("profile-1", &sealed).is_err());
        assert!(ring.seal("zz", "profile-1", &session_credential()).is_err());
    }

    #[test]
    fn tampered_ciphertext_is_refused() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        sealed.ciphertext.push('A');
        assert!(ring.open("profile-1", &sealed).is_err());
    }

    #[test]
    fn cookie_must_carry_a_non_empty_clerk_client_entry() {
        // Whitespace and position do not matter; the entry's presence and value do.
        let mut spaced = session_credential();
        spaced.cookie = " ajs_id=x ; __client=token-value ".into();
        assert!(spaced.validate().is_ok());

        for bad in [
            "ajs_id=x; __session=y",
            "__client=; ajs_id=x",
            "__client = ; ajs_id=x",
            "__clientid=almost-but-not-it",
        ] {
            let mut credential = session_credential();
            credential.cookie = bad.into();
            assert!(credential.validate().is_err(), "accepted {bad:?}");
            let ring = keyring();
            assert!(ring.seal(KEY_A, "profile-1", &credential).is_err());
        }
    }

    #[test]
    fn session_id_is_optional_but_bounded_when_present() {
        let mut rediscoverable = session_credential();
        rediscoverable.session_id = None;
        assert!(rediscoverable.validate().is_ok());

        let mut empty = session_credential();
        empty.session_id = Some(String::new());
        assert!(empty.validate().is_err());

        let mut oversized = session_credential();
        oversized.session_id = Some("s".repeat(MAX_FIELD_LEN + 1));
        assert!(oversized.validate().is_err());
    }

    #[test]
    fn plan_parsing_normalizes_case_and_refuses_unsaleable_tiers() {
        assert_eq!(SunoPlan::parse("pro").unwrap(), SunoPlan::Pro);
        assert_eq!(SunoPlan::parse(" PREMIER ").unwrap(), SunoPlan::Premier);
        for bad in ["", "free", "basic", "pro_20250501", "team", "pro+", "premium"] {
            assert!(SunoPlan::parse(bad).is_err(), "accepted {bad:?}");
        }
        // The canonical label is what the envelope serializes, and it matches the
        // calibration schema's `plan IN ('Pro', 'Premier')` CHECK exactly.
        assert_eq!(serde_json::to_string(&SunoPlan::Pro).unwrap(), "\"Pro\"");
        assert_eq!(SunoPlan::Premier.as_str(), "Premier");
        // An unknown tier is unrepresentable: deserialization of a tampered plan fails
        // closed instead of defaulting.
        assert!(serde_json::from_str::<SunoPlan>("\"Free\"").is_err());
        assert!(serde_json::from_str::<SunoPlan>("\"pro\"").is_err());
    }

    #[test]
    fn official_monthly_credits_match_the_published_ladder() {
        // suno.com/pricing embedded plan config, reviewed 2026-08-12. The published limits
        // are the cohort's native window capacity; no estimation is needed.
        assert_eq!(SUNO_REVIEWED_PLANS.len(), 2);
        for (plan, per_month) in [(SunoPlan::Pro, 2_500), (SunoPlan::Premier, 10_000)] {
            assert_eq!(reviewed_plan_credits(plan), Some(per_month));
        }
        for entry in SUNO_REVIEWED_PLANS {
            assert_eq!(entry.reviewed_on, "2026-08-12");
        }
    }

    #[test]
    fn official_endpoint_constants_match_the_reviewed_contract() {
        assert_eq!(SUNO_AUTH_BASE_URL, "https://auth.suno.com");
        assert_eq!(SUNO_API_BASE_URL, "https://studio-api.prod.suno.com");
        assert_eq!(SUNO_CLIENT_PATH, "/v1/client");
        // The {sid} placeholder is substituted by the caller, never by this crate.
        assert_eq!(SUNO_SESSION_TOKENS_PATH, "/v1/client/sessions/{sid}/tokens");
        assert!(SUNO_SESSION_TOKENS_PATH.contains("{sid}"));
        assert_eq!(SUNO_GENERATE_PATH, "/api/generate/v2/");
        assert_eq!(SUNO_BILLING_INFO_PATH, "/api/billing/info/");
        assert_eq!(SUNO_CAPTCHA_CHECK_PATH, "/api/c/check");
        assert_eq!(SUNO_FEED_PATH, "/api/feed/v2");
        assert_eq!(SUNO_CLIP_PATH, "/api/clip");
    }

    #[test]
    fn bounded_fields_fail_closed() {
        let mut no_cookie = session_credential();
        no_cookie.cookie = String::new();
        assert!(no_cookie.validate().is_err());

        let mut oversized_cookie = session_credential();
        oversized_cookie.cookie = format!("__client={}", "x".repeat(MAX_COOKIE_LEN));
        assert!(oversized_cookie.validate().is_err());

        let mut oversized_proxy = session_credential();
        oversized_proxy.proxy_url =
            format!("http://egress.example:8080/{}", "p".repeat(MAX_FIELD_LEN));
        assert!(oversized_proxy.validate().is_err());

        let mut bad_proxy = session_credential();
        bad_proxy.proxy_url = "file:///etc/passwd".into();
        assert!(bad_proxy.validate().is_err());

        let mut wrong_version = session_credential();
        wrong_version.version = 99;
        assert!(wrong_version.validate().is_err());
    }

    #[test]
    fn debug_never_prints_secrets() {
        let mut credential = session_credential();
        credential.cookie = "__client=super-secret-clerk-token; ajs_id=x".into();
        credential.session_id = Some("sess_secret-session-id".into());
        credential.proxy_url = "http://user:pr0xy-pass@egress.example:8080/".into();
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("super-secret-clerk-token"));
        assert!(!rendered.contains("sess_secret-session-id"));
        assert!(!rendered.contains("pr0xy-pass"));
        assert!(rendered.contains("REDACTED"));
        // Non-secret identity stays visible so operators can diagnose.
        assert!(rendered.contains("Pro"));

        let ring = keyring();
        assert!(!format!("{ring:?}").contains("1111"));

        // Error displays must not echo secrets either: a validation failure carries the
        // reason, never the cookie material or the proxy password.
        let mut bad_proxy = credential.clone();
        bad_proxy.proxy_url = "http://user:pr0xy-pass@egress.example:99999/".into();
        let err = format!("{:#}", bad_proxy.validate().unwrap_err());
        assert!(!err.contains("super-secret-clerk-token"));
        assert!(!err.contains("sess_secret-session-id"));
        assert!(!err.contains("pr0xy-pass"));
    }

    #[test]
    fn profile_ids_cannot_escape_their_directory() {
        assert!(validate_profile_id("suno-01_ab").is_ok());
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
        let sealed = ring.seal(KEY_A, "profile-1", &session_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        assert_eq!(decode_envelope(&bytes).unwrap(), sealed);
    }
}
