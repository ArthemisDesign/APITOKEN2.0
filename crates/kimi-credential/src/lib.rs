//! Encrypted, identity-bound KIMI (Kimi Code) subscription credentials.
//!
//! Pure AEAD envelope handling: no network, no HTTP, no filesystem policy. The contract is
//! `docs/engine/PROVIDER_ONBOARDING.md` §6, and the provider facts it encodes are recorded in
//! `docs/engine/KIMI_PROVIDER.md`.
//!
//! Two properties of this provider shape the module:
//!
//! * **The refresh family rotates.** The official CLI rejects any token response that omits a
//!   `refresh_token`, including refreshes, so every refresh invalidates the previous token. A
//!   caller that reuses a spent refresh token kills the subscription, which is why
//!   [`KimiCredential::rotate`] exists and why the runtime must hold a per-profile single-flight
//!   lock across seal.
//! * **The paid-plan ladder is not verified.** `/me` publishes `user_level_name` machine-readably,
//!   so it is a trustworthy *cohort key*, but which names unlock `k3`, the 1M context window or
//!   the high-speed SKU is documented inconsistently. Capability gating therefore fails closed:
//!   [`reviewed_plan_capabilities`] returns `None` for every unreviewed plan, and an unreviewed
//!   plan gets base capabilities only.

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
const AAD_PREFIX: &[u8] = b"apitoken/kimi-subscription-credential/v1\0";

/// Official Kimi Code OAuth host, device-code grant.
pub const KIMI_OAUTH_HOST: &str = "https://auth.kimi.com";
/// Device authorization endpoint, form-encoded POST with `client_id`.
pub const KIMI_DEVICE_AUTHORIZATION_PATH: &str = "/api/oauth/device_authorization";
/// Token endpoint, used for both the device-code and refresh_token grants.
pub const KIMI_TOKEN_PATH: &str = "/api/oauth/token";
/// Device-code grant type (RFC 8628).
pub const KIMI_DEVICE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Public client id published by the official MIT-licensed Kimi Code CLI.
pub const KIMI_OFFICIAL_OAUTH_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
/// User-Agent the official Kimi Code CLI sends, pinned to the reviewed CLI release.
///
/// The provider identifies clients by this string (the CLI changelog notes it is sent so
/// registries can identify the client version), so a bare/default agent risks looking like an
/// unrelated bot at the subscription endpoint. Version taken from `apps/kimi-code/package.json`
/// at the pinned research SHA `75395f6abb17f83f30d16b51f4e060a639f43622`.
pub const KIMI_CODE_CLI_USER_AGENT: &str = "kimi-code-cli/0.31.1";

/// Canonical subscription API base. The Anthropic-compatible route is served from the same
/// origin, which is why the engine's native protocol can be forwarded unchanged.
pub const KIMI_CODE_BASE_URL: &str = "https://api.kimi.com/coding/v1";
/// Identity endpoint. Publishes the stable subject and the authoritative paid plan.
pub const KIMI_IDENTITY_PATH: &str = "/me";
/// Quota endpoint. The only source of quota for this provider; generation responses carry none.
pub const KIMI_USAGE_PATH: &str = "/usages";

/// Account status that permits routing. Anything else is refused rather than guessed.
pub const KIMI_STATUS_NORMAL: &str = "USER_STATUS_NORMAL";

/// How a credential authenticates to the subscription plane.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KimiCredentialKind {
    /// Device-code OAuth with a rotating refresh family. The Auth Bot path.
    Oauth,
    /// Static API key issued from the Kimi Code console. No refresh, no expiry.
    ConsoleKey,
}

impl KimiCredentialKind {
    fn as_aad(self) -> &'static [u8] {
        match self {
            Self::Oauth => b"oauth",
            Self::ConsoleKey => b"console-key",
        }
    }
}

/// Capabilities a paid plan unlocks. Every field defaults to refused.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KimiPlanCapabilities {
    /// `k3` / `k3-256k` at 256K.
    pub allows_k3: bool,
    /// `k3` at the full 1,048,576-token window.
    pub allows_1m_context: bool,
    /// `kimi-for-coding-highspeed`.
    pub allows_highspeed: bool,
}

/// A paid plan whose capabilities were confirmed on an owned subscription.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewedPlan {
    /// Exact `user_level_name` from `/me`.
    pub plan_name: &'static str,
    pub capabilities: KimiPlanCapabilities,
    /// Date the capability set was confirmed live, `YYYY-MM-DD`.
    pub reviewed_on: &'static str,
}

/// Every documented tier, with the capability set the provider publishes for it.
///
/// This list was empty for a long time, and that was a mistake with a real cost: an empty table
/// does not distinguish a free tier from the most expensive one. Every subscription, whatever its
/// plan, silently collapsed to base capabilities — our own `supports()` refused `k3`/1M/highspeed
/// before any request reached the provider, the pool reported "no profile", and the transparent
/// envelope handed the caller a 429 indistinguishable from an upstream rate limit. Nothing in that
/// chain named the plan, so the cause was invisible from the outside.
///
/// What is genuinely contradictory in the provider's sources is the **price ladder** — the CNY and
/// USD lists carry different names and a mid-2026 split renamed the coding tiers. The
/// **capability → tier mapping** is published (Kimi Code docs, models) and is what this table
/// encodes. `reviewed_on` therefore carries the same meaning as in [`GLM_REVIEWED_PLANS`]: the day
/// the entry was confirmed against official documentation, recorded in
/// `docs/engine/KIMI_PROVIDER.md`.
///
/// [`GLM_REVIEWED_PLANS`]: https://docs.rs/glm-credential
pub const KIMI_REVIEWED_PLANS: &[ReviewedPlan] = &[
    // Entry tiers: every member gets `kimi-for-coding` at 256K and nothing beyond it.
    ReviewedPlan {
        plan_name: "Adagio",
        capabilities: KimiPlanCapabilities {
            allows_k3: false,
            allows_1m_context: false,
            allows_highspeed: false,
        },
        reviewed_on: "2026-08-07",
    },
    ReviewedPlan {
        plan_name: "Andante",
        capabilities: KimiPlanCapabilities {
            allows_k3: false,
            allows_1m_context: false,
            allows_highspeed: false,
        },
        reviewed_on: "2026-08-07",
    },
    // `k3` / `k3-256k` unlock at Moderato; the full 1M window and highspeed do not.
    ReviewedPlan {
        plan_name: "Moderato",
        capabilities: KimiPlanCapabilities {
            allows_k3: true,
            allows_1m_context: false,
            allows_highspeed: false,
        },
        reviewed_on: "2026-08-07",
    },
    // Allegretto and above additionally unlock the 1M context and the highspeed SKU.
    ReviewedPlan {
        plan_name: "Allegretto",
        capabilities: KimiPlanCapabilities {
            allows_k3: true,
            allows_1m_context: true,
            allows_highspeed: true,
        },
        reviewed_on: "2026-08-07",
    },
    ReviewedPlan {
        plan_name: "Allegro",
        capabilities: KimiPlanCapabilities {
            allows_k3: true,
            allows_1m_context: true,
            allows_highspeed: true,
        },
        reviewed_on: "2026-08-07",
    },
    ReviewedPlan {
        plan_name: "Vivace",
        capabilities: KimiPlanCapabilities {
            allows_k3: true,
            allows_1m_context: true,
            allows_highspeed: true,
        },
        reviewed_on: "2026-08-07",
    },
];

/// Capabilities for a plan, or `None` when the plan has not been reviewed.
///
/// `None` does not mean "no access": it means only capabilities documented as available to every
/// member — that is, `kimi-for-coding` at 256K — may be offered. See
/// [`capabilities_or_base`].
///
/// Matching ignores surrounding whitespace and ASCII case. The plan is an operator-visible label,
/// not a secret, and a subscription must not silently lose `k3` because the provider returned
/// `"vivace"` where the docs print `"Vivace"`. A genuinely different tier still misses and still
/// fails closed.
pub fn reviewed_plan_capabilities(plan_name: &str) -> Option<KimiPlanCapabilities> {
    let wanted = plan_name.trim();
    KIMI_REVIEWED_PLANS
        .iter()
        .find(|entry| entry.plan_name.eq_ignore_ascii_case(wanted))
        .map(|entry| entry.capabilities)
}

/// Capabilities to actually serve for a plan: reviewed ones if known, otherwise base only.
pub fn capabilities_or_base(plan_name: &str) -> KimiPlanCapabilities {
    reviewed_plan_capabilities(plan_name).unwrap_or_default()
}

/// A KIMI subscription credential. Secrets never reach `Debug` output.
#[derive(Clone, Serialize, Deserialize, Zeroize)]
#[zeroize(drop)]
pub struct KimiCredential {
    #[zeroize(skip)]
    pub version: u8,
    #[zeroize(skip)]
    pub kind: KimiCredentialKind,

    /// Bearer material. For `Oauth` this is the access token; for `ConsoleKey` the API key.
    pub access_token: String,
    /// Present only for `Oauth`. Rotates on every refresh.
    pub refresh_token: String,
    /// Unix seconds. Zero for `ConsoleKey`, which does not expire.
    #[zeroize(skip)]
    pub expires_at: i64,
    #[zeroize(skip)]
    pub scope: String,

    /// Stable provider subject (`user_id` from `/me`). Quota and dedup authority.
    #[zeroize(skip)]
    pub subject_id: String,
    /// Authoritative paid plan (`user_level_name`). The calibration cohort key.
    #[zeroize(skip)]
    pub plan_name: String,
    #[zeroize(skip)]
    pub plan_level: i64,
    /// `/me` account status. Only [`KIMI_STATUS_NORMAL`] is routable.
    #[zeroize(skip)]
    pub status: String,
    /// `/me` region, retained as inference geography for calibration identity.
    #[zeroize(skip)]
    pub region: String,

    /// Optional egress assigned by the Auth Bot. Never logged.
    pub proxy_url: String,
}

impl std::fmt::Debug for KimiCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KimiCredential")
            .field("version", &self.version)
            .field("kind", &self.kind)
            .field("subject_id", &self.subject_id)
            .field("plan_name", &self.plan_name)
            .field("plan_level", &self.plan_level)
            .field("status", &self.status)
            .field("region", &self.region)
            .field("expires_at", &self.expires_at)
            .field("access_token", &"REDACTED")
            .field("refresh_token", &"REDACTED")
            .field("proxy_url", &"REDACTED")
            .finish()
    }
}

const MAX_TOKEN_LEN: usize = 8192;
const MAX_FIELD_LEN: usize = 512;

impl KimiCredential {
    /// Strict bounded validation. Every rejection is a refusal to route, never a warning.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != CREDENTIAL_VERSION {
            bail!("unsupported KIMI credential version");
        }
        if self.access_token.is_empty() || self.access_token.len() > MAX_TOKEN_LEN {
            bail!("KIMI credential access token is missing or oversized");
        }
        if self.subject_id.is_empty() || self.subject_id.len() > MAX_FIELD_LEN {
            bail!("KIMI credential subject id is missing or oversized");
        }
        // An empty plan would silently collapse distinct cohorts into one, so it is refused
        // rather than defaulted.
        if self.plan_name.is_empty() || self.plan_name.len() > MAX_FIELD_LEN {
            bail!("KIMI credential paid plan is missing or oversized");
        }
        if self.status != KIMI_STATUS_NORMAL {
            bail!("KIMI credential account status is not routable");
        }
        if self.region.len() > MAX_FIELD_LEN {
            bail!("KIMI credential region is oversized");
        }
        if self.scope.len() > MAX_FIELD_LEN {
            bail!("KIMI credential scope is oversized");
        }
        match self.kind {
            KimiCredentialKind::Oauth => {
                if self.refresh_token.is_empty() || self.refresh_token.len() > MAX_TOKEN_LEN {
                    bail!("KIMI OAuth credential refresh token is missing or oversized");
                }
                if self.expires_at <= 0 {
                    bail!("KIMI OAuth credential expiry is missing");
                }
            }
            KimiCredentialKind::ConsoleKey => {
                if !self.refresh_token.is_empty() {
                    bail!("KIMI console key must not carry a refresh token");
                }
                if self.expires_at != 0 {
                    bail!("KIMI console key must not carry an expiry");
                }
            }
        }
        if !self.proxy_url.is_empty() {
            normalize_proxy_url(&self.proxy_url)?;
        }
        Ok(())
    }

    /// Whether the access token is expired at `now_unix`, given a refresh lead time.
    ///
    /// Console keys never expire, so they always report fresh.
    pub fn is_expired(&self, now_unix: i64, lead_secs: i64) -> bool {
        match self.kind {
            KimiCredentialKind::ConsoleKey => false,
            KimiCredentialKind::Oauth => self.expires_at <= now_unix.saturating_add(lead_secs),
        }
    }

    /// Apply a refresh result, replacing **both** halves of the rotating family.
    ///
    /// The provider issues a new refresh token on every refresh and invalidates the old one, so
    /// keeping the previous refresh token would leave a dead credential that fails on next use.
    /// The caller must hold the per-profile single-flight lock across this call and the re-seal
    /// that follows it.
    pub fn rotate(
        &mut self,
        access_token: String,
        refresh_token: String,
        expires_at: i64,
        scope: String,
    ) -> anyhow::Result<()> {
        if self.kind != KimiCredentialKind::Oauth {
            bail!("only a KIMI OAuth credential can be rotated");
        }
        if refresh_token.is_empty() {
            // The official client treats a refresh response without a refresh token as
            // malformed. Accepting one would silently pin us to a spent token.
            bail!("KIMI refresh response did not rotate the refresh token");
        }
        self.access_token = access_token;
        self.refresh_token = refresh_token;
        self.expires_at = expires_at;
        self.scope = scope;
        self.validate()
    }

    /// Capabilities this credential's plan may serve, failing closed on an unreviewed plan.
    pub fn capabilities(&self) -> KimiPlanCapabilities {
        capabilities_or_base(&self.plan_name)
    }
}

/// Normalize and bound an egress proxy URL. Credentials embedding one must not be able to smuggle
/// arbitrary schemes or fragments into the transport layer.
pub fn normalize_proxy_url(proxy: &str) -> anyhow::Result<String> {
    let parsed = url::Url::parse(proxy).context("parse KIMI proxy url")?;
    match parsed.scheme() {
        "http" | "https" | "socks5" | "socks5h" => {}
        _ => bail!("unsupported KIMI proxy scheme"),
    }
    if parsed.host_str().unwrap_or_default().is_empty() {
        bail!("KIMI proxy url has no host");
    }
    if parsed.fragment().is_some() {
        bail!("KIMI proxy url must not carry a fragment");
    }
    Ok(parsed.to_string())
}

/// Bound the opaque profile id used as AAD. Path separators and traversal are refused so a
/// profile id can never escape its roster directory.
pub fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty() || id.len() > 128 {
        bail!("KIMI profile id is missing or oversized");
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("KIMI profile id contains unsupported characters");
    }
    Ok(())
}

fn validate_key_id(key_id: &str) -> anyhow::Result<()> {
    if key_id.is_empty() || key_id.len() > 64 {
        bail!("KIMI credential key id is missing or oversized");
    }
    if !key_id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("KIMI credential key id contains unsupported characters");
    }
    Ok(())
}

fn decode_hex_key(encoded: &str) -> anyhow::Result<[u8; 32]> {
    if encoded.len() != 64 {
        bail!("KIMI credential key must be 32 bytes of hex");
    }
    let mut key = [0u8; 32];
    for (index, chunk) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let text = std::str::from_utf8(chunk).context("decode KIMI credential key")?;
        key[index] =
            u8::from_str_radix(text, 16).context("KIMI credential key is not valid hex")?;
    }
    Ok(key)
}

/// AAD binds the ciphertext to both the profile it belongs to and the credential kind, so an
/// envelope cannot be moved between profiles or reinterpreted as the other kind.
fn associated_data(profile_id: &str, kind: KimiCredentialKind) -> Vec<u8> {
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
    pub kind: KimiCredentialKind,
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
    /// Parse `kid:64-hex-bytes[,kid:64-hex-bytes...]`. Old keys are retained so the runtime can
    /// still open existing envelopes while the Auth Bot seals new ones under the active key.
    pub fn parse(specification: &str) -> anyhow::Result<Self> {
        let mut keys = HashMap::new();
        for entry in specification
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let (key_id, encoded) = entry
                .split_once(':')
                .ok_or_else(|| anyhow!("KIMI credential key entry must be kid:hex"))?;
            validate_key_id(key_id)?;
            let key = decode_hex_key(encoded)?;
            if keys.insert(key_id.to_string(), key).is_some() {
                bail!("duplicate KIMI credential key id");
            }
        }
        if keys.is_empty() {
            bail!("KIMI credential keyring is empty");
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
        credential: &KimiCredential,
    ) -> anyhow::Result<SealedCredential> {
        validate_profile_id(profile_id)?;
        credential.validate()?;
        let key = self
            .keys
            .get(active_key_id)
            .ok_or_else(|| anyhow!("active KIMI credential key id is unavailable"))?;
        let plaintext =
            Zeroizing::new(serde_json::to_vec(credential).context("encode KIMI credential")?);
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
            .map_err(|_| anyhow!("seal KIMI credential failed"))?;
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
    ) -> anyhow::Result<KimiCredential> {
        validate_profile_id(profile_id)?;
        if envelope.version != ENVELOPE_VERSION {
            bail!("unsupported KIMI credential envelope version");
        }
        let key = self
            .keys
            .get(&envelope.key_id)
            .ok_or_else(|| anyhow!("KIMI credential key id is unavailable"))?;
        let nonce = URL_SAFE_NO_PAD
            .decode(&envelope.nonce)
            .context("decode KIMI credential nonce")?;
        if nonce.len() != 24 {
            bail!("invalid KIMI credential nonce");
        }
        let ciphertext = URL_SAFE_NO_PAD
            .decode(&envelope.ciphertext)
            .context("decode KIMI credential ciphertext")?;
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
                .map_err(|_| anyhow!("KIMI credential authentication failed"))?,
        );
        let credential: KimiCredential =
            serde_json::from_slice(&plaintext).context("decode KIMI credential")?;
        credential.validate()?;
        // The cleartext kind is an AEAD input, so a mismatch here means the envelope was
        // tampered with in a way that still authenticated — refuse rather than trust it.
        if credential.kind != envelope.kind {
            bail!("KIMI credential kind does not match its envelope");
        }
        Ok(credential)
    }
}

pub fn encode_envelope(envelope: &SealedCredential) -> anyhow::Result<Vec<u8>> {
    serde_json::to_vec(envelope).context("encode KIMI credential envelope")
}

pub fn decode_envelope(bytes: &[u8]) -> anyhow::Result<SealedCredential> {
    serde_json::from_slice(bytes).context("decode KIMI credential envelope")
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

    fn oauth_credential() -> KimiCredential {
        KimiCredential {
            version: CREDENTIAL_VERSION,
            kind: KimiCredentialKind::Oauth,
            access_token: "access-1".into(),
            refresh_token: "refresh-1".into(),
            expires_at: 2_000_000_000,
            scope: "coding".into(),
            subject_id: "u_123".into(),
            plan_name: "Vivace".into(),
            plan_level: 30,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        }
    }

    fn console_credential() -> KimiCredential {
        KimiCredential {
            version: CREDENTIAL_VERSION,
            kind: KimiCredentialKind::ConsoleKey,
            access_token: "sk-console".into(),
            refresh_token: String::new(),
            expires_at: 0,
            scope: String::new(),
            subject_id: "u_456".into(),
            plan_name: "Moderato".into(),
            plan_level: 10,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        }
    }

    #[test]
    fn seal_open_roundtrip_preserves_identity() {
        let ring = keyring();
        let credential = oauth_credential();
        let sealed = ring.seal(KEY_A, "profile-1", &credential).unwrap();
        let opened = ring.open("profile-1", &sealed).unwrap();
        assert_eq!(opened.subject_id, "u_123");
        assert_eq!(opened.plan_name, "Vivace");
        assert_eq!(opened.access_token, "access-1");
        assert_eq!(opened.refresh_token, "refresh-1");
    }

    #[test]
    fn envelope_cannot_be_moved_to_another_profile() {
        let ring = keyring();
        let sealed = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        assert!(ring.open("profile-2", &sealed).is_err());
    }

    #[test]
    fn envelope_cannot_be_reinterpreted_as_the_other_kind() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        sealed.kind = KimiCredentialKind::ConsoleKey;
        assert!(ring.open("profile-1", &sealed).is_err());
    }

    #[test]
    fn old_keys_stay_readable_so_rotation_can_be_online() {
        let ring = keyring();
        let sealed_old = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        let sealed_new = ring.seal(KEY_B, "profile-1", &oauth_credential()).unwrap();
        assert!(ring.open("profile-1", &sealed_old).is_ok());
        assert!(ring.open("profile-1", &sealed_new).is_ok());
        assert!(ring.contains(KEY_A) && ring.contains(KEY_B));
    }

    #[test]
    fn unknown_key_id_fails_closed() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        sealed.key_id = "zz".into();
        assert!(ring.open("profile-1", &sealed).is_err());
        assert!(ring.seal("zz", "profile-1", &oauth_credential()).is_err());
    }

    #[test]
    fn tampered_ciphertext_is_refused() {
        let ring = keyring();
        let mut sealed = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        sealed.ciphertext.push('A');
        assert!(ring.open("profile-1", &sealed).is_err());
    }

    #[test]
    fn refresh_must_rotate_the_refresh_token() {
        let mut credential = oauth_credential();
        // The provider invalidates the old refresh token on every refresh, so a response that
        // omits a new one would leave a credential that dies on next use.
        let err = credential
            .rotate(
                "access-2".into(),
                String::new(),
                2_100_000_000,
                String::new(),
            )
            .unwrap_err();
        assert!(err.to_string().contains("did not rotate"));
        assert_eq!(credential.refresh_token, "refresh-1");

        credential
            .rotate(
                "access-2".into(),
                "refresh-2".into(),
                2_100_000_000,
                "coding".into(),
            )
            .unwrap();
        assert_eq!(credential.access_token, "access-2");
        assert_eq!(credential.refresh_token, "refresh-2");
    }

    #[test]
    fn console_keys_cannot_be_rotated() {
        let mut credential = console_credential();
        assert!(credential
            .rotate("x".into(), "y".into(), 1, String::new())
            .is_err());
    }

    #[test]
    fn console_keys_never_expire_and_reject_oauth_fields() {
        let credential = console_credential();
        credential.validate().unwrap();
        assert!(!credential.is_expired(i64::MAX, 0));

        let mut bad = console_credential();
        bad.refresh_token = "leaked".into();
        assert!(bad.validate().is_err());

        let mut also_bad = console_credential();
        also_bad.expires_at = 5;
        assert!(also_bad.validate().is_err());
    }

    #[test]
    fn oauth_expiry_respects_the_refresh_lead() {
        let credential = oauth_credential();
        assert!(!credential.is_expired(2_000_000_000 - 120, 60));
        assert!(credential.is_expired(2_000_000_000 - 30, 60));
    }

    #[test]
    fn missing_plan_or_unroutable_status_fails_closed() {
        let mut no_plan = oauth_credential();
        no_plan.plan_name = String::new();
        assert!(no_plan.validate().is_err());

        let mut suspended = oauth_credential();
        suspended.status = "USER_STATUS_BANNED".into();
        assert!(suspended.validate().is_err());
    }

    #[test]
    fn unreviewed_plans_get_base_capabilities_only() {
        // A plan outside the documented ladder is still refused everything tier-gated.
        assert_eq!(reviewed_plan_capabilities("Presto"), None);
        let base = capabilities_or_base("Presto");
        assert!(!base.allows_k3);
        assert!(!base.allows_1m_context);
        assert!(!base.allows_highspeed);
    }

    #[test]
    fn documented_ladder_grants_exactly_the_published_capabilities() {
        // Entry tiers: base only. `k3` opens at Moderato, 1M and highspeed at Allegretto.
        for entry in ["Adagio", "Andante"] {
            let caps = reviewed_plan_capabilities(entry).expect("entry tier is documented");
            assert!(!caps.allows_k3, "{entry} must not grant k3");
            assert!(!caps.allows_1m_context);
            assert!(!caps.allows_highspeed);
        }
        let moderato = reviewed_plan_capabilities("Moderato").expect("documented");
        assert!(moderato.allows_k3);
        assert!(!moderato.allows_1m_context, "1M opens only at Allegretto");
        assert!(!moderato.allows_highspeed, "highspeed opens only at Allegretto");
        for top in ["Allegretto", "Allegro", "Vivace"] {
            let caps = reviewed_plan_capabilities(top).expect("top tier is documented");
            assert!(caps.allows_k3, "{top} must grant k3");
            assert!(caps.allows_1m_context, "{top} must grant the 1M window");
            assert!(caps.allows_highspeed, "{top} must grant highspeed");
        }
    }

    #[test]
    fn plan_lookup_survives_case_and_padding_from_the_provider() {
        // A subscription must not lose k3 because `/me` spelled the tier differently.
        let canonical = reviewed_plan_capabilities("Vivace").expect("documented");
        assert_eq!(reviewed_plan_capabilities("vivace"), Some(canonical));
        assert_eq!(reviewed_plan_capabilities("  VIVACE  "), Some(canonical));
        assert_eq!(reviewed_plan_capabilities("Vivace Plan"), None);
    }

    #[test]
    fn debug_never_prints_secrets() {
        let mut credential = oauth_credential();
        credential.proxy_url = "http://user:pass@egress.example:8080/".into();
        let rendered = format!("{credential:?}");
        assert!(!rendered.contains("access-1"));
        assert!(!rendered.contains("refresh-1"));
        assert!(!rendered.contains("pass"));
        assert!(rendered.contains("REDACTED"));
        // Non-secret identity stays visible so operators can diagnose.
        assert!(rendered.contains("u_123"));

        let ring = keyring();
        assert!(!format!("{ring:?}").contains("1111"));
    }

    #[test]
    fn profile_ids_cannot_escape_their_directory() {
        assert!(validate_profile_id("kimi-01_ab").is_ok());
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
        let sealed = ring.seal(KEY_A, "profile-1", &oauth_credential()).unwrap();
        let bytes = encode_envelope(&sealed).unwrap();
        assert_eq!(decode_envelope(&bytes).unwrap(), sealed);
    }

    #[test]
    fn official_endpoint_constants_match_the_reviewed_contract() {
        assert_eq!(KIMI_OAUTH_HOST, "https://auth.kimi.com");
        assert_eq!(KIMI_CODE_BASE_URL, "https://api.kimi.com/coding/v1");
        assert_eq!(
            KIMI_DEVICE_GRANT_TYPE,
            "urn:ietf:params:oauth:grant-type:device_code"
        );
        assert_eq!(KIMI_USAGE_PATH, "/usages");
    }
}
