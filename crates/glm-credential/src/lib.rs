//! Encrypted, identity-bound GLM Coding Plan (Zhipu AI / Z.ai) credentials.
//!
//! Pure AEAD envelope handling: no network, no HTTP, no filesystem policy. The contract is
//! `docs/engine/PROVIDER_WIRING_CHECKLIST.md` §6, and the provider facts it encodes are
//! recorded in `docs/engine/GLM_PROVIDER.md`.
//!
//! Two properties of this provider shape the module:
//!
//! * **The credential is a static console API key.** GLM Coding Plan has no OAuth device
//!   flow and no refresh family: a key is issued from the plan console and stays valid
//!   until it is reissued. There is deliberately no `rotate`/`is_expired` surface —
//!   rotation means issuing a new key in the console and sealing a fresh envelope through
//!   the Auth Bot.
//! * **The plan ladder is officially published.** Unlike KIMI, the 5-hour and weekly
//!   window credits of every individual tier are documented
//!   (`docs.z.ai/devpack/overview`, reviewed 2026-08-03), so [`GLM_REVIEWED_PLANS`] lists
//!   all three tiers. Dynamic rate-limit and concurrency differences between tiers are
//!   undocumented and are *not* encoded.

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
const AAD_PREFIX: &[u8] = b"apitoken/glm-plan-credential/v1\0";

/// International GLM Coding Plan console origin. Keys are issued per origin and are not
/// interchangeable with the China site.
pub const GLM_BASE_URL_INTERNATIONAL: &str = "https://api.z.ai";
/// China GLM Coding Plan console origin.
pub const GLM_BASE_URL_CHINA: &str = "https://open.bigmodel.cn";

/// Anthropic-compatible generation endpoint served on the plan base URL. Authorization
/// carries the key with a `Bearer` prefix (official).
pub const GLM_ANTHROPIC_MESSAGES_PATH: &str = "/api/anthropic/v1/messages";
/// Quota endpoint, the only machine-readable plan/quota surface. Authorization carries
/// the raw key **without** a `Bearer` prefix (oss-hypothesis, onWatch `zai_client.go`);
/// an invalid key returns HTTP 200 with `code: 401` in the body.
pub const GLM_QUOTA_PATH: &str = "/api/monitor/usage/quota/limit";
/// OpenAI-compatible generation endpoint, `Bearer` authorized (official). Documented for
/// completeness; the v1 plane serves the Anthropic route only.
pub const GLM_OPENAI_CHAT_PATH: &str = "/api/coding/paas/v4/chat/completions";

/// The three models served on every individual Coding Plan tier (official,
/// devpack/faq). No other model is callable on a plan key, and tier rate-limit
/// differences are deliberately not represented here.
pub const GLM_PLAN_MODELS: &[&str] = &["glm-5.2", "glm-5-turbo", "glm-4.7"];

/// How a credential authenticates to the plan.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlmCredentialKind {
    /// Static API key for an individual GLM Coding Plan, issued from the console. No
    /// refresh, no expiry. The only kind this provider has.
    PlanKey,
}

impl GlmCredentialKind {
    fn as_aad(self) -> &'static [u8] {
        match self {
            Self::PlanKey => b"plan-key",
        }
    }
}

/// A declared individual Coding Plan tier. The plan is declared by the offer the seller
/// bought and is corroborated by the quota endpoint's observed window limit; GLM has no
/// machine-readable `/me`, so an undeclared tier is unrepresentable here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlmPlan {
    Lite,
    Pro,
    Max,
}

impl GlmPlan {
    /// Canonical label used inside sealed envelopes and roster rows.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Pro => "pro",
            Self::Max => "max",
        }
    }

    /// Parse a declared plan, normalizing case and surrounding whitespace. Team and
    /// legacy prompts tiers are refused: they carry a different quota unit and fail
    /// closed at onboarding (`docs/engine/GLM_PROVIDER.md` §1).
    pub fn parse(declared: &str) -> anyhow::Result<Self> {
        match declared.trim().to_ascii_lowercase().as_str() {
            "lite" => Ok(Self::Lite),
            "pro" => Ok(Self::Pro),
            "max" => Ok(Self::Max),
            _ => bail!("GLM plan must be one of lite, pro or max"),
        }
    }
}

/// Official per-plan native window credits (`docs.z.ai/devpack/overview`, reviewed
/// 2026-08-03). Credits are the plan's own quota unit, computed from token usage by the
/// provider's published formula; they never convert to or from USD.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmWindowCredits {
    /// Credits per rolling 5-hour window.
    pub per_five_hours: u64,
    /// Credits per 7-day window counted from the order time.
    pub per_week: u64,
}

/// A tier whose window credits were confirmed against the official documentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewedPlan {
    pub plan: GlmPlan,
    pub credits: GlmWindowCredits,
    /// Date the limits were confirmed against `docs.z.ai/devpack/overview`, `YYYY-MM-DD`.
    pub reviewed_on: &'static str,
}

/// Every individual tier, with its officially published window credits.
///
/// Unlike `KIMI_REVIEWED_PLANS` this list is **not** empty: GLM publishes the ladder. A
/// new tier enters [`GlmPlan`] only together with its published limits and a dated entry
/// here; until then it stays unrepresentable and fails closed at [`GlmPlan::parse`].
pub const GLM_REVIEWED_PLANS: &[ReviewedPlan] = &[
    ReviewedPlan {
        plan: GlmPlan::Lite,
        credits: GlmWindowCredits {
            per_five_hours: 2_000,
            per_week: 10_000,
        },
        reviewed_on: "2026-08-03",
    },
    ReviewedPlan {
        plan: GlmPlan::Pro,
        credits: GlmWindowCredits {
            per_five_hours: 12_000,
            per_week: 60_000,
        },
        reviewed_on: "2026-08-03",
    },
    ReviewedPlan {
        plan: GlmPlan::Max,
        credits: GlmWindowCredits {
            per_five_hours: 28_000,
            per_week: 140_000,
        },
        reviewed_on: "2026-08-03",
    },
];

/// Official window credits for a plan, or `None` for a tier added ahead of its review.
pub fn reviewed_plan_credits(plan: GlmPlan) -> Option<GlmWindowCredits> {
    GLM_REVIEWED_PLANS
        .iter()
        .find(|entry| entry.plan == plan)
        .map(|entry| entry.credits)
}

/// A GLM Coding Plan credential. Secrets never reach `Debug` output.
#[derive(Clone, Serialize, Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct GlmCredential {
    #[zeroize(skip)]
    pub version: u8,
    #[zeroize(skip)]
    pub kind: GlmCredentialKind,

    /// Static plan API key issued from the console. GLM has no machine-readable subject
    /// (`/me` does not exist), so the whole key is the dedup identity: it stays inside
    /// the envelope, and the caller (Auth Bot) compares opened envelopes to detect one
    /// key occupying two profiles. This crate deliberately keeps no hashing dependency.
    pub api_key: String,

    /// Declared individual tier, corroborated by the observed quota window limit.
    #[zeroize(skip)]
    pub plan: GlmPlan,
    /// Console origin the key was issued on, in canonical form: exactly
    /// [`GLM_BASE_URL_INTERNATIONAL`] or [`GLM_BASE_URL_CHINA`].
    #[zeroize(skip)]
    pub base_url: String,

    /// Optional egress assigned by the Auth Bot. Never logged.
    pub proxy_url: String,
}

impl std::fmt::Debug for GlmCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GlmCredential")
            .field("version", &self.version)
            .field("kind", &self.kind)
            .field("plan", &self.plan)
            .field("base_url", &self.base_url)
            .field("api_key", &"REDACTED")
            .field("proxy_url", &"REDACTED")
            .finish()
    }
}

const MAX_KEY_LEN: usize = 8192;
const MAX_FIELD_LEN: usize = 512;

impl GlmCredential {
    /// Strict bounded validation. Every rejection is a refusal to route, never a warning.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported GLM credential version");
        }
        if self.api_key.is_empty() || self.api_key.len() > MAX_KEY_LEN {
            bail!("GLM credential API key is missing or oversized");
        }
        // The stored base URL must already be canonical, so an envelope can never carry
        // a spelling that parses to an allowed origin but compares unequal to it.
        if normalize_base_url(&self.base_url)? != self.base_url {
            bail!("GLM credential base url is not in canonical form");
        }
        if !self.proxy_url.is_empty() {
            if self.proxy_url.len() > MAX_FIELD_LEN {
                bail!("GLM credential proxy url is oversized");
            }
            normalize_proxy_url(&self.proxy_url)?;
        }
        Ok(())
    }
}

/// Normalize a GLM plan base URL to its canonical origin. Exactly two console origins are
/// routable; any other host, a non-root path, a query, a fragment or embedded credentials
/// are refused, because a key is only ever valid against the console that issued it.
pub fn normalize_base_url(base_url: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(base_url).context("parse GLM base url")?;
    if parsed.scheme() != "https" && !is_test_loopback(&parsed) {
        bail!("GLM base url must use https");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("GLM base url must not carry credentials");
    }
    if parsed.path() != "/" {
        bail!("GLM base url must not carry a path");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        bail!("GLM base url must not carry a query or fragment");
    }
    let origin = parsed.to_string();
    let origin = origin.trim_end_matches('/');
    match origin {
        GLM_BASE_URL_INTERNATIONAL | GLM_BASE_URL_CHINA => Ok(origin.to_string()),
        _ if is_test_loopback(&parsed) => Ok(origin.to_string()),
        _ => bail!("GLM base url host is not an official console origin"),
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
    let parsed = url::Url::parse(proxy).context("parse GLM proxy url")?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => bail!("unsupported GLM proxy scheme"),
    }
    if parsed.host_str().unwrap_or_default().is_empty() {
        bail!("GLM proxy url has no host");
    }
    if parsed.fragment().is_some() {
        bail!("GLM proxy url must not carry a fragment");
    }
    Ok(parsed.to_string())
}

/// Bound the opaque profile id used as AAD. Path separators and traversal are refused so
/// a profile id can never escape its roster directory.
pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 128 {
        bail!("GLM profile id is missing or oversized");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("GLM profile id contains unsupported characters");
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    if key_id.is_empty() || key_id.len() > 64 {
        bail!("GLM credential key id is missing or oversized");
    }
    if !key_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("GLM credential key id contains unsupported characters");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("GLM credential key must be 32 bytes of hex");
    }
    let mut key = [0u8; 32];
    for (index, chunk) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).context("decode GLM credential key")?;
        key[index] =
            u8::from_str_radix(text, 16).context("GLM credential key is not valid hex")?;
    }
    Ok(key)
}

/// AAD binds the ciphertext to both the profile it belongs to and the credential kind, so
/// an envelope cannot be moved between profiles or reinterpreted as another kind.
fn associated_data(profile_id: &str, kind: GlmCredentialKind) -> Vec<u8> {
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
    pub kind: GlmCredentialKind,
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
                .ok_or_else(|| anyhow!("GLM credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate GLM credential key id");
            }
        }
        if keys.is_empty() {
            bail!("GLM credential keyring is empty");
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
        credential: &GlmCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active GLM credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode GLM credential")?);
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
            .map_err(|_| anyhow!("seal GLM credential failed"))?;
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
    ) -> anyhow::Result<GlmCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported GLM credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("GLM credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode GLM credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid GLM credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode GLM credential ciphertext")?;
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
                .map_err(|_| anyhow!("GLM credential authentication failed"))?,
        );
        let credential: GlmCredential =
            serde_json::from_slice(&plaintext).context("decode GLM credential")?;
        credential.validate()?;
        // The cleartext kind is an AEAD input, so a mismatch here means the envelope was
        // tampered with in a way that still authenticated — refuse rather than trust it.
        if credential.kind != envelope.kind {
            bail!("GLM credential kind does not match its envelope");
        }
        Ok(credential)
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode GLM credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode GLM credential envelope")
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "a1";
    const KEY_B: &str = "b2";

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("{KEY_A}:{},{KEY_B}:{}", "11".repeat(32), "22".repeat(32)))
            .expect("keyring parses")
    }

    fn plan_credential() -> GlmCredential {
        GlmCredential {
            version: CREDENTIAL_VERSION,
            kind: GlmCredentialKind::PlanKey,
            api_key: "zai-test-key-9f8c7b6a5d.secretpart0123456789".into(),
            plan: GlmPlan::Pro,
            base_url: GLM_BASE_URL_INTERNATIONAL.into(),
            proxy_url: String::new(),
        }
    }

    #[test]
    fn seal_open_roundtrip_preserves_identity() {
        let ring = keyring();
        let credential = plan_credential();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        let opened = ring.open("profile-1", &sealed).unwrap();
        assert_eq!(opened.plan, GlmPlan::Pro);
        assert_eq!(opened.base_url, GLM_BASE_URL_INTERNATIONAL);
        // The whole key is the dedup identity, so it must survive the envelope intact.
        assert_eq!(opened.api_key, credential.api_key);
    }

    #[test]
    fn envelope_cannot_be_moved_to_another_profile() {
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        assert!(ring.open("profile-2", &sealed).is_err());
    }

    #[test]
    fn envelope_kind_substitution_is_refused() {
        // `kind` is cleartext so `open` can build AAD, but it is also an AEAD input and a
        // single-variant enum: substituting another kind string breaks deserialization,
        // and any future second variant would break authentication. Both fail closed.
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        let tampered = String::from_utf8(bytes)
            .unwrap()
            .replace("plan_key", "oauth");
        assert!(decode_envelope(tampered.as_bytes()).is_err());
    }

    #[test]
    fn old_keys_stay_readable_so_rotation_can_be_online() {
        let ring = keyring();
        let sealed_old = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        let sealed_new = ring.seal(KEY_B, "profile-1", &plan_credential()).unwrap();
        assert!(ring.open("profile-1", &sealed_old).is_ok());
        assert!(ring.open("profile-1", &sealed_new).is_ok());
        assert!(ring.contains(KEY_A) && ring.contains(KEY_B));
    }

    #[test]
    fn unknown_key_id_fails_closed() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        sealed.key_id = "zz".into();
        assert!(ring.open("profile-1", &sealed).is_err());
        assert!(ring.seal("zz", "profile-1", &plan_credential()).is_err());
    }

    #[test]
    fn tampered_ciphertext_is_refused() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        sealed.ciphertext.push('A');
        assert!(ring.open("profile-1", &sealed).is_err());
    }

    #[test]
    fn base_url_allowlist_accepts_both_console_origins() {
        assert_eq!(
            normalize_base_url(GLM_BASE_URL_INTERNATIONAL).unwrap(),
            GLM_BASE_URL_INTERNATIONAL
        );
        assert_eq!(
            normalize_base_url(GLM_BASE_URL_CHINA).unwrap(),
            GLM_BASE_URL_CHINA
        );
        // A trailing slash from URL serialization normalizes away at intake.
        assert_eq!(
            normalize_base_url("https://api.z.ai/").unwrap(),
            GLM_BASE_URL_INTERNATIONAL
        );

        let ring = keyring();
        let mut credential = plan_credential();
        credential.base_url = GLM_BASE_URL_CHINA.into();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        assert_eq!(
            ring.open("profile-1", &sealed).unwrap().base_url,
            GLM_BASE_URL_CHINA
        );
    }

    #[test]
    fn base_url_rejects_foreign_hosts_paths_and_credentials() {
        for bad in [
            "https://glm.example.com",
            "https://api.z.ai.evil.com",
            "http://api.z.ai",
            "https://api.z.ai:8443",
            "https://api.z.ai/api/anthropic",
            "https://api.z.ai/?x=1",
            "https://api.z.ai/#frag",
            "https://user:pass@api.z.ai",
            "not-a-url",
        ] {
            assert!(normalize_base_url(bad).is_err(), "accepted {bad}");
        }

        // A stored envelope must carry the canonical form: the caller normalizes at
        // intake, and sealing a non-canonical spelling fails closed.
        let mut credential = plan_credential();
        credential.base_url = "https://api.z.ai/".into();
        assert!(credential.validate().is_err());

        credential.base_url = "https://glm.example.com".into();
        let ring = keyring();
        assert!(ring.seal(KEY_A, "profile-1", &credential).is_err());
    }

    #[test]
    fn plan_parsing_normalizes_case_and_refuses_unknown_tiers() {
        assert_eq!(GlmPlan::parse("lite").unwrap(), GlmPlan::Lite);
        assert_eq!(GlmPlan::parse(" PRO ").unwrap(), GlmPlan::Pro);
        assert_eq!(GlmPlan::parse("Max").unwrap(), GlmPlan::Max);
        for bad in ["", "team", "standard", "premium", "v1", "enterprise", "lite+"] {
            assert!(GlmPlan::parse(bad).is_err(), "accepted {bad:?}");
        }
        // The canonical label is what the envelope serializes.
        assert_eq!(serde_json::to_string(&GlmPlan::Pro).unwrap(), "\"pro\"");
        assert_eq!(GlmPlan::Max.as_str(), "max");
        // An unknown tier is unrepresentable: deserialization of a tampered plan fails
        // closed instead of defaulting.
        assert!(serde_json::from_str::<GlmPlan>("\"team\"").is_err());
    }

    #[test]
    fn official_window_credits_match_the_published_ladder() {
        // docs.z.ai/devpack/overview, reviewed 2026-08-03. Unlike KIMI the ladder is
        // officially published, so the reviewed set covers every individual tier.
        assert_eq!(GLM_REVIEWED_PLANS.len(), 3);
        for (plan, five_hours, week) in [
            (GlmPlan::Lite, 2_000, 10_000),
            (GlmPlan::Pro, 12_000, 60_000),
            (GlmPlan::Max, 28_000, 140_000),
        ] {
            let credits = reviewed_plan_credits(plan).expect("tier is reviewed");
            assert_eq!(credits.per_five_hours, five_hours);
            assert_eq!(credits.per_week, week);
        }
    }

    #[test]
    fn official_endpoint_constants_match_the_reviewed_contract() {
        assert_eq!(GLM_BASE_URL_INTERNATIONAL, "https://api.z.ai");
        assert_eq!(GLM_BASE_URL_CHINA, "https://open.bigmodel.cn");
        // Generation endpoints authorize with `Bearer`; the quota endpoint takes the raw
        // key without the prefix (see the constant docs).
        assert_eq!(GLM_ANTHROPIC_MESSAGES_PATH, "/api/anthropic/v1/messages");
        assert_eq!(GLM_QUOTA_PATH, "/api/monitor/usage/quota/limit");
        assert_eq!(GLM_OPENAI_CHAT_PATH, "/api/coding/paas/v4/chat/completions");
        // All three models are served on every individual tier (official, devpack/faq);
        // tier rate-limit differences are dynamic and deliberately not encoded.
        assert_eq!(GLM_PLAN_MODELS, ["glm-5.2", "glm-5-turbo", "glm-4.7"]);
    }

    #[test]
    fn bounded_fields_fail_closed() {
        let mut no_key = plan_credential();
        no_key.api_key = String::new();
        assert!(no_key.validate().is_err());

        let mut oversized_key = plan_credential();
        oversized_key.api_key = "x".repeat(MAX_KEY_LEN + 1);
        assert!(oversized_key.validate().is_err());

        let mut oversized_proxy = plan_credential();
        oversized_proxy.proxy_url =
            format!("http://egress.example:8080/{}", "p".repeat(MAX_FIELD_LEN));
        assert!(oversized_proxy.validate().is_err());

        let mut bad_proxy = plan_credential();
        bad_proxy.proxy_url = "file:///etc/passwd".into();
        assert!(bad_proxy.validate().is_err());

        let mut wrong_version = plan_credential();
        wrong_version.version = 99;
        assert!(wrong_version.validate().is_err());
    }

    #[test]
    fn debug_never_prints_secrets() {
        let mut credential = plan_credential();
        credential.api_key = "glm-super-secret-key".into();
        credential.proxy_url = "http://user:pr0xy-pass@egress.example:8080/".into();
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("glm-super-secret-key"));
        assert!(!rendered.contains("pr0xy-pass"));
        assert!(rendered.contains("REDACTED"));
        // Non-secret identity stays visible so operators can diagnose.
        assert!(rendered.contains("pro"));
        assert!(rendered.contains(GLM_BASE_URL_INTERNATIONAL));

        let ring = keyring();
        assert!(!format!("{ring:?}").contains("1111"));

        // Error displays must not echo secrets either: a validation failure carries the
        // reason, never the key material or the proxy password.
        let mut bad_base = credential.clone();
        bad_base.base_url = "https://glm.example.com".into();
        let err = format!("{:#}", bad_base.validate().unwrap_err());
        assert!(!err.contains("glm-super-secret-key"));
        assert!(!err.contains("pr0xy-pass"));

        let mut bad_proxy = credential.clone();
        bad_proxy.proxy_url = "http://user:pr0xy-pass@egress.example:99999/".into();
        let err = format!("{:#}", bad_proxy.validate().unwrap_err());
        assert!(!err.contains("pr0xy-pass"));
    }

    #[test]
    fn profile_ids_cannot_escape_their_directory() {
        assert!(validate_profile_id("glm-01_ab").is_ok());
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
        let normalized =
            normalize_proxy_url("socks5://user:p%41ss:w@egress.example:1080").unwrap();
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
        let sealed = ring.seal(KEY_A, "profile-1", &plan_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        assert_eq!(decode_envelope(&bytes).unwrap(), sealed);
    }
}
