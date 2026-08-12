//! Suno (suno.com) subscription-session validation for the Auth Bot.
//!
//! Pure protocol unit: it discovers the Clerk session, mints a short-lived JWT, probes the
//! free billing counters and corroborates the declared plan against the observed monthly
//! limit, then builds the credential that will be sealed. It owns no Telegram state, no
//! seller job, no payout and no roster publication — the seller wizard in `bot.rs` drives it
//! step by step.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §7; provider facts and their evidence
//! labels: `docs/engine/SUNO_PROVIDER.md` §2 (credential/identity), §4 (wire), §5.2 (native
//! quota), §7 (acquisition). Every wire fact is `oss-hypothesis` (gcui-art/suno-api, read
//! 2026-08-12): all parsers fail closed on any schema deviation.
//!
//! The credential artifact is the seller's browser session cookie string (its critical entry
//! is the Clerk `__client` cookie) — a sanctioned one-time artifact of the same class as the
//! Claude `sk-ant-oat01` setup-token, a recorded deviation from the generic "seller never
//! sends a cookie" default (manifest §2): there is no other credential surface. The bot never
//! asks for the account password, 2FA or card data.
//!
//! **There is deliberately no paid admission song here.** One admission song costs 5 credits
//! = $0.02 derived, which exceeds the default $0.0001 admission micro-smoke cap (AGENTS.md);
//! manifest §7 records this as an open admission-budget question, fail closed. Validation
//! therefore stops at the free probes (session discovery, JWT mint, billing info), and this
//! module carries no generation code path at all — adding one requires an explicit
//! operator-approved budget raise first, not a leftover stub.
//!
//! JWT minting is idempotent session keep-alive (manifest §2): all three calls are safe to
//! retry under bounded backoff. Merging a rotated `set-cookie` back into the sealed envelope
//! is the runtime's single-flight concern, not the intake's — the sealed credential carries
//! exactly the cookie string the seller sent.

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use suno_credential::{
    normalize_proxy_url, reviewed_plan_credits, SunoCredential, SunoCredentialKind, SunoPlan,
    SUNO_API_BASE_URL, SUNO_AUTH_BASE_URL, SUNO_BILLING_INFO_PATH, SUNO_CLIENT_PATH,
    SUNO_REQUIRED_COOKIE, SUNO_SESSION_TOKENS_PATH,
};

/// Bound on any single provider call. A hung validation must not pin a seller job forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Clerk version pins of the reviewed session wire (`oss-hypothesis`, gcui-art/suno-api read
/// 2026-08-12; `docs/engine/SUNO_PROVIDER.md` §2). They are part of the reviewed contract:
/// a live run that shows different versions updates this constant deliberately, not silently.
const CLERK_QUERY: &str = "__clerk_api_version=2025-11-10&_clerk_js_version=5.117.0";

/// Bound on the provider-issued identifiers carried further (session id, JWT). Anything
/// longer is a schema deviation and fails closed instead of propagating into a URL path or
/// an envelope.
const MAX_PROVIDER_TOKEN_LEN: usize = 8192;

/// Intake bound on the cookie message. Mirrors the seal-time bound inside
/// `suno_credential::SunoCredential::validate` (8192), so an over-long message is refused
/// here with a seller-readable hint instead of travelling to the provider first.
const MAX_COOKIE_LEN: usize = 8192;

/// Outcome of the Clerk session discovery (`GET /v1/client`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionProbe {
    /// The session material was accepted; the active session id was discovered.
    Active { session_id: String },
    /// The auth host rejected the session (HTTP 401/403): the cookie is dead or revoked.
    Invalid,
}

/// Outcome of the short-lived JWT mint (`POST /v1/client/sessions/{sid}/tokens`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JwtMint {
    /// A fresh JWT was minted. It is used for the billing probe and never persisted.
    Minted { jwt: String },
    /// The auth host rejected the session (HTTP 401/403).
    Invalid,
}

/// Raw billing snapshot: immutable provider-side evidence, semantics unproven
/// (`oss-hypothesis`; `docs/engine/SUNO_PROVIDER.md` §5.2). Every counter is kept raw and
/// nullable; nothing is derived, divided or reinterpreted at intake.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BillingSnapshot {
    pub total_credits_left: Option<i64>,
    pub monthly_limit: Option<i64>,
    pub monthly_usage: Option<i64>,
}

/// Outcome of the free billing probe (`GET /api/billing/info/`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BillingProbe {
    /// The JWT was accepted; the snapshot is raw quota evidence.
    Valid(BillingSnapshot),
    /// The business host rejected the session (HTTP 401/403).
    Invalid,
}

/// Verdict of corroborating the seller-declared plan against the observed monthly window
/// limit. The limits are published per plan (Pro 2 500, Premier 10 000 credits — reviewed
/// 2026-08-12), so a readable limit either confirms the declared tier or contradicts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanVerdict {
    /// The observed `monthly_limit` equals the declared plan's published monthly credits.
    Confirmed(SunoPlan),
    /// The limit is readable as a credits number but contradicts the declared plan —
    /// including a number matching no reviewed tier. Fail closed: the profile stays out of
    /// rotation pending operator review.
    PlanMismatch { declared: SunoPlan, observed_limit: u64 },
    /// `monthly_limit` is absent or unreadable: the snapshot cannot corroborate anything.
    /// Never guessed, never reinterpreted.
    Unreadable,
}

/// Why a session is inadmissible — the input for safe seller guidance. Carries no provider
/// text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidKeyReason {
    /// Any of the three probes answered HTTP 401/403: the session is dead, revoked or was
    /// never logged in. The seller guidance is "copy a fresh cookie from a live session",
    /// not "reissue a key".
    Auth,
}

/// Extract the Clerk `__client` cookie value from a full cookie string. Its presence with a
/// non-empty value is the local authenticity preflight: without it the material cannot mint
/// a JWT at all, so the absence is decided before any network call.
pub fn clerk_client_value(cookie: &str) -> Option<&str> {
    cookie.split(';').filter_map(|part| part.split_once('=')).find_map(
        |(name, value)| {
            (name.trim() == SUNO_REQUIRED_COOKIE && !value.trim().is_empty())
                .then(|| value.trim())
        },
    )
}

/// Local shape preflight of the seller's cookie message: one bounded single line carrying a
/// non-empty `__client` entry. Everything else is decided by the provider probes — there are
/// no local assumptions about the rest of the cookie string.
pub fn cookie_text(text: &str) -> Option<&str> {
    let cookie = text.trim();
    if cookie.is_empty()
        || cookie.len() > MAX_COOKIE_LEN
        || cookie.contains('\n')
        || cookie.contains('\r')
    {
        return None;
    }
    clerk_client_value(cookie)?;
    Some(cookie)
}

#[derive(Deserialize)]
struct ClientResponse {
    response: Option<ClientResponseData>,
}

#[derive(Deserialize)]
struct ClientResponseData {
    last_active_session_id: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    jwt: Option<String>,
}

/// The billing counters are parsed strictly as integers or null: a float or string field is
/// a schema deviation and fails closed at decode time instead of being reinterpreted.
#[derive(Deserialize)]
struct BillingResponse {
    total_credits_left: Option<i64>,
    monthly_limit: Option<i64>,
    monthly_usage: Option<i64>,
}

/// A provider-issued identifier must be bounded and path-safe before it is substituted into
/// a URL or sealed into an envelope.
fn checked_provider_token(raw: &str, what: &str) -> Result<String> {
    let token = raw.trim();
    if token.is_empty() || token.len() > MAX_PROVIDER_TOKEN_LEN {
        bail!("Suno {what} is missing or oversized");
    }
    Ok(token.to_string())
}

/// Parse a session-discovery response.
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_session_discovery(status: u16, body: &[u8]) -> Result<SessionProbe> {
    if status == 401 || status == 403 {
        return Ok(SessionProbe::Invalid);
    }
    if status != 200 {
        bail!("Suno session discovery returned HTTP {status}");
    }
    let parsed: ClientResponse =
        serde_json::from_slice(body).context("decode Suno session discovery")?;
    let session_id = parsed
        .response
        .and_then(|data| data.last_active_session_id)
        .ok_or_else(|| anyhow!("Suno session discovery is missing last_active_session_id"))?;
    Ok(SessionProbe::Active {
        session_id: checked_provider_token(&session_id, "session id")?,
    })
}

/// Parse a JWT-mint response.
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_jwt_mint(status: u16, body: &[u8]) -> Result<JwtMint> {
    if status == 401 || status == 403 {
        return Ok(JwtMint::Invalid);
    }
    if status != 200 {
        bail!("Suno JWT mint returned HTTP {status}");
    }
    let parsed: TokenResponse = serde_json::from_slice(body).context("decode Suno JWT mint")?;
    let jwt = parsed
        .jwt
        .ok_or_else(|| anyhow!("Suno JWT mint is missing the jwt field"))?;
    Ok(JwtMint::Minted {
        jwt: checked_provider_token(&jwt, "jwt")?,
    })
}

/// Parse a billing-probe response. The three counters are kept raw and nullable
/// (`oss-hypothesis`, manifest §5.2): no derivation at intake.
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_billing_probe(status: u16, body: &[u8]) -> Result<BillingProbe> {
    if status == 401 || status == 403 {
        return Ok(BillingProbe::Invalid);
    }
    if status != 200 {
        bail!("Suno billing probe returned HTTP {status}");
    }
    let parsed: BillingResponse =
        serde_json::from_slice(body).context("decode Suno billing info")?;
    Ok(BillingProbe::Valid(BillingSnapshot {
        total_credits_left: parsed.total_credits_left,
        monthly_limit: parsed.monthly_limit,
        monthly_usage: parsed.monthly_usage,
    }))
}

/// Corroborate the declared plan against the observed monthly window limit. The published
/// ladder (Pro 2 500 / Premier 10 000 credits, reviewed 2026-08-12) makes a readable limit
/// either confirm the declared plan or contradict it; anything unreadable fails closed.
pub fn corroborate_plan(snapshot: &BillingSnapshot, declared: SunoPlan) -> PlanVerdict {
    let Some(observed) = snapshot.monthly_limit else {
        return PlanVerdict::Unreadable;
    };
    let Ok(observed) = u64::try_from(observed) else {
        return PlanVerdict::Unreadable;
    };
    if reviewed_plan_credits(declared) == Some(observed) {
        PlanVerdict::Confirmed(declared)
    } else {
        PlanVerdict::PlanMismatch {
            declared,
            observed_limit: observed,
        }
    }
}

/// Build the credential that will be sealed and published. The proxy is canonicalized here,
/// so an envelope can only ever carry the reviewed form. The sealed cookie is exactly the
/// seller's string; any `set-cookie` rotation during the intake probes is the runtime's
/// single-flight concern (manifest §2), not the intake's.
pub fn credential_from(
    cookie: &str,
    session_id: &str,
    plan: SunoPlan,
    proxy_url: &str,
) -> Result<SunoCredential> {
    let credential = SunoCredential {
        version: 1,
        kind: SunoCredentialKind::SessionCookie,
        cookie: cookie.to_string(),
        session_id: Some(checked_provider_token(session_id, "session id")?),
        plan,
        proxy_url: if proxy_url.is_empty() {
            String::new()
        } else {
            normalize_proxy_url(proxy_url)?
        },
    };
    credential.validate()?;
    Ok(credential)
}

/// Bounded back-off between free-probe retries: 1s doubling to a 30s cap. All three intake
/// calls are free and idempotent (JWT minting is session keep-alive, manifest §2), so a
/// transport failure is safe to retry; there is no paid call in this module at all (see the
/// module docs).
pub fn probe_retry_backoff(attempt: u32) -> Duration {
    const CAP: Duration = Duration::from_secs(30);
    let seconds = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
    Duration::from_secs(seconds).min(CAP)
}

fn client(proxy_url: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(HTTP_TIMEOUT);
    if !proxy_url.is_empty() {
        // The seller's assigned egress must be used for the whole validation: opening the
        // account from one IP and probing from another is exactly what trips provider risk
        // control.
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url).context("configure Suno validation proxy")?);
    }
    builder.build().context("build Suno validation client")
}

/// Clerk session discovery on the seller's assigned egress (`oss-hypothesis`,
/// `docs/engine/SUNO_PROVIDER.md` §2). The `__client` cookie value rides as the raw
/// `Authorization` header — that is the reviewed Clerk contract, not a Bearer token.
pub async fn discover_session(cookie: &str, proxy_url: &str) -> Result<SessionProbe> {
    let client_value = clerk_client_value(cookie)
        .ok_or_else(|| anyhow!("Suno cookie carries no non-empty __client entry"))?;
    let url = format!("{SUNO_AUTH_BASE_URL}{SUNO_CLIENT_PATH}?{CLERK_QUERY}");
    let response = client(proxy_url)?
        .get(url)
        .header(reqwest::header::AUTHORIZATION, client_value)
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("discover Suno session")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read Suno session")?;
    parse_session_discovery(status, &body)
}

/// Mint a short-lived JWT for the discovered session on the seller's assigned egress
/// (`oss-hypothesis`, `docs/engine/SUNO_PROVIDER.md` §2). The JWT is used for the billing
/// probe and never persisted; the mint is idempotent keep-alive, safe to retry.
pub async fn mint_jwt(cookie: &str, session_id: &str, proxy_url: &str) -> Result<JwtMint> {
    let client_value = clerk_client_value(cookie)
        .ok_or_else(|| anyhow!("Suno cookie carries no non-empty __client entry"))?;
    // The session id is provider-issued but still crosses into a URL path: bounded and
    // checked before substitution.
    let session_id = checked_provider_token(session_id, "session id")?;
    let url = format!(
        "{SUNO_AUTH_BASE_URL}{}?{CLERK_QUERY}",
        SUNO_SESSION_TOKENS_PATH.replace("{sid}", &session_id)
    );
    let response = client(proxy_url)?
        .post(url)
        .header(reqwest::header::AUTHORIZATION, client_value)
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("mint Suno JWT")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read Suno JWT mint")?;
    parse_jwt_mint(status, &body)
}

/// Free read-only billing probe on the seller's assigned egress (`oss-hypothesis`,
/// `docs/engine/SUNO_PROVIDER.md` §5.2). The minted JWT rides as `Bearer`; the full cookie
/// is re-sent alongside, per the reviewed wire.
pub async fn probe_billing(jwt: &str, cookie: &str, proxy_url: &str) -> Result<BillingProbe> {
    let url = format!("{SUNO_API_BASE_URL}{SUNO_BILLING_INFO_PATH}");
    let response = client(proxy_url)?
        .get(url)
        .bearer_auth(jwt)
        .header(reqwest::header::COOKIE, cookie)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("probe Suno billing info")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read Suno billing info")?;
    parse_billing_probe(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(limit: Option<i64>) -> BillingSnapshot {
        BillingSnapshot {
            total_credits_left: Some(2_400),
            monthly_limit: limit,
            monthly_usage: Some(100),
        }
    }

    #[test]
    fn a_valid_discovery_yields_the_active_session_id() {
        let body = br#"{"response":{"last_active_session_id":"sess_2abcdef0123456789"}}"#;
        assert_eq!(
            parse_session_discovery(200, body).unwrap(),
            SessionProbe::Active {
                session_id: "sess_2abcdef0123456789".into()
            }
        );
    }

    #[test]
    fn discovery_rejections_and_schema_deviations_fail_closed() {
        // A dead or revoked session is a verdict on the material.
        assert_eq!(parse_session_discovery(401, b"{}").unwrap(), SessionProbe::Invalid);
        assert_eq!(parse_session_discovery(403, b"{}").unwrap(), SessionProbe::Invalid);
        // Anything else is transport or contract drift, never a verdict and never a success.
        assert!(parse_session_discovery(500, b"error").is_err());
        assert!(parse_session_discovery(200, br#"{"response":{}}"#).is_err());
        assert!(parse_session_discovery(200, br#"{"response":{"last_active_session_id":""}}"#).is_err());
        assert!(parse_session_discovery(200, b"not json").is_err());
    }

    #[test]
    fn a_valid_mint_yields_a_jwt_and_rejections_are_typed() {
        let body = br#"{"jwt":"header.payload.signature"}"#;
        assert_eq!(
            parse_jwt_mint(200, body).unwrap(),
            JwtMint::Minted {
                jwt: "header.payload.signature".into()
            }
        );
        assert_eq!(parse_jwt_mint(401, b"{}").unwrap(), JwtMint::Invalid);
        assert!(parse_jwt_mint(200, br#"{}"#).is_err());
        assert!(parse_jwt_mint(200, br#"{"jwt":""}"#).is_err());
        assert!(parse_jwt_mint(502, b"bad gateway").is_err());
    }

    #[test]
    fn a_valid_billing_probe_preserves_the_raw_nullable_counters() {
        let body = br#"{"total_credits_left":2400,"period":"2026-08","monthly_limit":2500,
            "monthly_usage":100,"other_future_field":true}"#;
        let BillingProbe::Valid(snapshot) = parse_billing_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(
            snapshot,
            BillingSnapshot {
                total_credits_left: Some(2_400),
                monthly_limit: Some(2_500),
                monthly_usage: Some(100),
            }
        );
        // Nulls stay null — the semantics are unproven (manifest §5.2), nothing is derived.
        let body = br#"{"total_credits_left":null,"monthly_limit":10000,"monthly_usage":null}"#;
        let BillingProbe::Valid(snapshot) = parse_billing_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.total_credits_left, None);
        assert_eq!(snapshot.monthly_limit, Some(10_000));
        assert_eq!(snapshot.monthly_usage, None);
    }

    #[test]
    fn billing_schema_deviations_fail_closed() {
        assert_eq!(parse_billing_probe(401, b"{}").unwrap(), BillingProbe::Invalid);
        assert_eq!(parse_billing_probe(403, b"{}").unwrap(), BillingProbe::Invalid);
        // A float or string where an integer counter was reviewed is a contract change.
        assert!(parse_billing_probe(200, br#"{"monthly_limit":2500.5}"#).is_err());
        assert!(parse_billing_probe(200, br#"{"monthly_limit":"2500"}"#).is_err());
        assert!(parse_billing_probe(500, b"error").is_err());
        assert!(parse_billing_probe(200, b"not json").is_err());
    }

    #[test]
    fn plan_corroboration_matches_the_published_monthly_ladder() {
        for (limit, plan) in [(2_500, SunoPlan::Pro), (10_000, SunoPlan::Premier)] {
            assert_eq!(
                corroborate_plan(&snapshot(Some(limit)), plan),
                PlanVerdict::Confirmed(plan)
            );
        }
    }

    #[test]
    fn a_limit_contradicting_the_declared_plan_is_a_mismatch() {
        // Another tier's number…
        assert_eq!(
            corroborate_plan(&snapshot(Some(2_500)), SunoPlan::Premier),
            PlanVerdict::PlanMismatch {
                declared: SunoPlan::Premier,
                observed_limit: 2_500,
            }
        );
        // …or no reviewed tier at all: mismatch, never a guess.
        assert_eq!(
            corroborate_plan(&snapshot(Some(9_999)), SunoPlan::Pro),
            PlanVerdict::PlanMismatch {
                declared: SunoPlan::Pro,
                observed_limit: 9_999,
            }
        );
    }

    #[test]
    fn an_unreadable_limit_fails_closed() {
        assert_eq!(corroborate_plan(&snapshot(None), SunoPlan::Pro), PlanVerdict::Unreadable);
        assert_eq!(
            corroborate_plan(&snapshot(Some(-1)), SunoPlan::Pro),
            PlanVerdict::Unreadable
        );
    }

    #[test]
    fn the_client_cookie_value_is_extracted_before_any_network_call() {
        assert_eq!(
            clerk_client_value("__client=test-token.9f8c7b; ajs_id=x"),
            Some("test-token.9f8c7b")
        );
        assert_eq!(clerk_client_value("ajs_id=x; __session=y"), None);
        assert_eq!(clerk_client_value("__client=; ajs_id=x"), None);

        for accepted in [
            "__client=tok.abc; ajs_id=x",
            " ajs_id=x ; __client=tok.abc ",
        ] {
            assert!(cookie_text(accepted).is_some(), "{accepted:?}");
        }
        let oversized = format!("__client={}", "x".repeat(8192));
        for rejected in [
            "",
            "  ",
            "ajs_id=x; __session=y",
            "multi\nline __client=tok",
            oversized.as_str(),
        ] {
            assert_eq!(cookie_text(rejected), None, "{rejected:?}");
        }
    }

    #[test]
    fn credential_from_validates_and_bounds() {
        let credential = credential_from(
            "__client=test-token.9f8c7b; ajs_id=x",
            "sess_2abcdef0123456789",
            SunoPlan::Pro,
            "socks5://user:p%41ss@egress.example:1080",
        )
        .unwrap();
        assert_eq!(credential.kind, SunoCredentialKind::SessionCookie);
        assert_eq!(credential.plan, SunoPlan::Pro);
        assert_eq!(
            credential.session_id.as_deref(),
            Some("sess_2abcdef0123456789")
        );

        // A cookie without the Clerk entry, an empty session id and a non-proxy scheme fail
        // closed instead of sealing.
        assert!(credential_from("ajs_id=x", "sess_1", SunoPlan::Pro, "").is_err());
        assert!(credential_from("__client=tok", "", SunoPlan::Pro, "").is_err());
        assert!(
            credential_from("__client=tok", "sess_1", SunoPlan::Pro, "file:///etc/passwd")
                .is_err()
        );
        // Sealing and publication belong to suno_roster, which owns the filesystem contract.
    }

    #[test]
    fn the_probe_client_honours_the_assigned_egress_and_fails_closed() {
        assert!(client("socks5://user:pass@egress.example:1080").is_ok());
        // An unparseable proxy URL fails client construction instead of leaking to direct
        // egress. Scheme policy is enforced earlier, at `credential_from` canonicalization.
        assert!(client("not a url").is_err());
    }

    #[test]
    fn probe_backoff_is_bounded() {
        assert_eq!(probe_retry_backoff(0), Duration::from_secs(1));
        assert_eq!(probe_retry_backoff(3), Duration::from_secs(8));
        assert_eq!(probe_retry_backoff(5), Duration::from_secs(30));
        // A hostile attempt counter cannot push the back-off past the cap.
        assert_eq!(probe_retry_backoff(100), Duration::from_secs(30));
    }
}
