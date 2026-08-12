//! Tripo3D (VAST / Holymolly) static API-key validation for the Auth Bot.
//!
//! Pure protocol unit: it probes the prepaid balance and corroborates the declared top-up
//! cohort against the observed snapshot, then builds the credential that will be sealed. It
//! owns no Telegram state, no seller job, no payout and no roster publication — the seller
//! wizard in `bot.rs` drives it step by step.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §7; provider facts and their evidence
//! labels: `docs/engine/TRIPO3D_PROVIDER.md` §2 (credential/identity), §4 (wire), §5.2
//! (native quota), §7 (acquisition).
//!
//! Like GLM there is no OAuth device flow: the seller registers on the API platform, tops up
//! exactly the declared amount of the offer product, creates a static `tsk_` key in the
//! console and sends the key to the bot as text. The key is the only credential artifact.
//!
//! **There is deliberately no paid admission task here.** The cheapest paid Tripo3D task
//! costs 5 credits = $0.05, which exceeds the default $0.0001 admission micro-smoke cap
//! (AGENTS.md), and no free zero-cost task is proven to exist; manifest §7 records this as an
//! open admission-budget question, fail closed. Validation therefore stops at the free
//! balance probe, and this module carries no paid code path at all — adding one requires an
//! explicit operator-approved budget raise first, not a leftover stub.

// The seller wizard in `bot.rs` arrives as the dependent follow-up commit; until it lands,
// nothing outside this module's tests calls the intake protocol.
#![allow(dead_code)]

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use serde::Deserialize;
use tripo3d_credential::{
    normalize_base_url, normalize_cohort, normalize_proxy_url, Tripo3dCredential,
    Tripo3dCredentialKind, TRIPO3D_API_KEY_PREFIX, TRIPO3D_BALANCE_PATH,
};

/// Bound on any single provider call. A hung validation must not pin a seller job forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Raw balance snapshot: immutable provider-side evidence, units uninterpreted.
///
/// The unit of `balance`/`frozen` (credits vs dollars) and their decimal semantics are
/// **unknown** (`oss-hypothesis`, official Python SDK only; `docs/engine/TRIPO3D_PROVIDER.md`
/// §5.2/§6): both values are kept as their raw JSON tokens and are never divided by prices
/// or otherwise reinterpreted.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BalanceSnapshot {
    /// Raw `data.balance` token exactly as returned (a JSON number rendered, or the string
    /// contents when the provider sends a string).
    pub balance: Option<String>,
    /// Raw `data.frozen` token, same rules.
    pub frozen: Option<String>,
}

/// Outcome of the free balance probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BalanceProbe {
    /// The key was accepted; the snapshot is raw balance evidence.
    Valid(BalanceSnapshot),
    /// The provider rejected the key: HTTP 401, or a business code 401 carried in the body
    /// (the GLM trap defense — decided on the business code whenever one is present, never
    /// on the HTTP status alone; `official` errors page, `docs/engine/TRIPO3D_PROVIDER.md`
    /// §4.1).
    Invalid,
}

/// Verdict of corroborating the seller-declared top-up cohort against the observed balance.
///
/// Unlike GLM there is no published tier ladder and the balance **unit is unproven**
/// (manifest §5.2), so corroboration cannot compare magnitudes: a declared "$50" top-up
/// cannot be told from 50 credits, 50 dollars or 5 000 millicredits. Until a live run proves
/// the unit, corroboration can only attest that the probe succeeded and both counters parse
/// as non-negative decimals; anything else fails closed. This is the documented fail-closed
/// form of the "declared cohort vs observed balance within tolerance" check of manifest §7.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CohortVerdict {
    /// The probe succeeded and both raw counters are readable non-negative decimals. This is
    /// NOT a proof of the declared amount — it is the strongest statement available while the
    /// balance unit is unknown.
    Consistent,
    /// A counter is missing, negative or not a decimal: the snapshot cannot corroborate
    /// anything. Fail closed — never guessed, never reinterpreted.
    Unreadable,
}

/// Why a key is inadmissible — the input for safe seller guidance. Carries no provider text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidKeyReason {
    /// HTTP 401 (or business code 401): the key does not authenticate at all.
    Auth,
    /// The seller sent a `tcli_` Client ID instead of a `tsk_` API key. The mismatch is
    /// documented to answer 401 (`docs/engine/TRIPO3D_PROVIDER.md` §2/§4.1), so it is
    /// recognized locally instead of burning a live request.
    ClientIdMisuse,
}

/// Local authenticity preflight before any network call. Only the documented `tcli_` Client
/// ID confusion is decided here; every other shape goes to the provider's free probe.
pub fn preflight_key_rejection(api_key: &str) -> Option<InvalidKeyReason> {
    if api_key.starts_with("tcli_") {
        return Some(InvalidKeyReason::ClientIdMisuse);
    }
    // A missing tsk_ prefix is not decided locally: the credential contract refuses it at
    // seal time, and the probe's 401 is the provider's own verdict.
    if !api_key.starts_with(TRIPO3D_API_KEY_PREFIX) {
        return Some(InvalidKeyReason::Auth);
    }
    None
}

#[derive(Deserialize)]
struct BalanceResponse {
    code: Option<i64>,
    data: Option<BalanceData>,
    // `message`/`suggestion` are deliberately not carried: provider text may echo account
    // details and is never surfaced to logs or the seller.
}

#[derive(Deserialize)]
struct BalanceData {
    balance: Option<serde_json::Value>,
    frozen: Option<serde_json::Value>,
}

/// Render one raw quota counter without interpreting it: numbers keep their JSON token,
/// strings keep their contents, anything else is absent. No float arithmetic ever happens —
/// the token is evidence, not money.
fn raw_counter(value: Option<serde_json::Value>) -> Option<String> {
    match value? {
        serde_json::Value::Number(number) => Some(number.to_string()),
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text),
        _ => None,
    }
}

/// A counter corroborates only as a non-negative finite decimal. The magnitude is never
/// compared against the declared top-up: with the unit unproven (manifest §5.2) any numeric
/// tolerance would be invented semantics.
fn is_non_negative_decimal(raw: &str) -> bool {
    raw.trim()
        .parse::<f64>()
        .map(|value| value.is_finite() && value >= 0.0)
        .unwrap_or(false)
}

/// Parse a balance probe response (`official` envelope `{"code":0,"data":{…}}`,
/// `docs/engine/TRIPO3D_PROVIDER.md` §4/§5.2).
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_balance_probe(status: u16, body: &[u8]) -> Result<BalanceProbe> {
    // HTTP 401 is the documented invalid-key verdict (`official` errors page §4.1).
    if status == 401 {
        return Ok(BalanceProbe::Invalid);
    }
    let parsed: BalanceResponse =
        serde_json::from_slice(body).context("decode Tripo3D balance response")?;
    // Defensive mirror of the GLM trap: should a proxy or a future platform build carry the
    // rejection as a business code on another HTTP status, the business code still decides.
    if parsed.code == Some(401) {
        return Ok(BalanceProbe::Invalid);
    }
    match parsed.code {
        Some(0) => {}
        // A business code that is neither success nor the known rejection is a contract
        // change (rate limit 1007/2000, insufficient balance 2010, anything future): the
        // caller's bounded transport policy decides, it never condemns the key.
        Some(code) => bail!("Tripo3D balance endpoint returned unrecognised business code {code}"),
        None => bail!("Tripo3D balance response is missing its business code"),
    }
    if status != 200 {
        bail!("Tripo3D balance endpoint returned HTTP {status}");
    }
    let data = parsed
        .data
        .ok_or_else(|| anyhow!("Tripo3D balance response is missing data"))?;
    Ok(BalanceProbe::Valid(BalanceSnapshot {
        balance: raw_counter(data.balance),
        frozen: raw_counter(data.frozen),
    }))
}

/// Corroborate the declared top-up cohort against the observed balance snapshot.
///
/// `declared` must already be a canonical cohort string (see `credential_from`); an
/// unnormalizable declared cohort is a caller bug and fails closed. While the balance unit
/// is unknown (manifest §5.2/§7) the only available check is that the probe succeeded and
/// both counters parse as non-negative decimals — documented fail-closed corroboration, NOT
/// an amount comparison.
pub fn corroborate_cohort(snapshot: &BalanceSnapshot, declared: &str) -> CohortVerdict {
    if normalize_cohort(declared).is_err() {
        return CohortVerdict::Unreadable;
    }
    let readable = [&snapshot.balance, &snapshot.frozen]
        .into_iter()
        .all(|counter| counter.as_deref().is_some_and(is_non_negative_decimal));
    if readable {
        CohortVerdict::Consistent
    } else {
        CohortVerdict::Unreadable
    }
}

/// Build the credential that will be sealed and published. Cohort, base URL and proxy are
/// canonicalized here, so an envelope can only ever carry the reviewed forms.
pub fn credential_from(
    api_key: &str,
    cohort: &str,
    base_url: &str,
    proxy_url: &str,
) -> Result<Tripo3dCredential> {
    let credential = Tripo3dCredential {
        version: 1,
        kind: Tripo3dCredentialKind::ApiKey,
        api_key: api_key.to_string(),
        cohort: normalize_cohort(cohort)?,
        base_url: normalize_base_url(base_url)?,
        proxy_url: if proxy_url.is_empty() {
            String::new()
        } else {
            normalize_proxy_url(proxy_url)?
        },
    };
    credential.validate()?;
    Ok(credential)
}

/// Bounded back-off between read-only balance probe retries: 1s doubling to a 30s cap. The
/// probe consumes no credits, so a transport failure is safe to retry; there is no paid call
/// in this module at all (see the module docs).
pub fn probe_retry_backoff(attempt: u32) -> Duration {
    const CAP: Duration = Duration::from_secs(30);
    let seconds = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
    Duration::from_secs(seconds).min(CAP)
}

fn client(proxy_url: &str) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder().timeout(HTTP_TIMEOUT);
    if !proxy_url.is_empty() {
        // The seller's assigned egress must be used for the whole validation: probing from a
        // different IP than the account runs on is what trips provider risk control.
        builder = builder
            .proxy(reqwest::Proxy::all(proxy_url).context("configure Tripo3D validation proxy")?);
    }
    builder.build().context("build Tripo3D validation client")
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{path}", base_url.trim_end_matches('/'))
}

/// Free read-only balance probe on the seller's assigned egress
/// (`oss-hypothesis` endpoint, `docs/engine/TRIPO3D_PROVIDER.md` §5.2). Transport failures
/// are safe to retry with `probe_retry_backoff` — the probe consumes no credits.
pub async fn probe_balance(
    base_url: &str,
    api_key: &str,
    proxy_url: &str,
) -> Result<BalanceProbe> {
    let response = client(proxy_url)?
        .get(endpoint(base_url, TRIPO3D_BALANCE_PATH))
        // Every platform request carries the key with a Bearer prefix (official, quick-start).
        .bearer_auth(api_key)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("probe Tripo3D balance")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read Tripo3D balance")?;
    parse_balance_probe(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tripo3d_credential::{TRIPO3D_BASE_URL_CHINA, TRIPO3D_BASE_URL_GLOBAL};

    #[test]
    fn a_valid_probe_preserves_the_raw_counters() {
        let body = br#"{"code":0,"data":{"balance":100.5,"frozen":2}}"#;
        let BalanceProbe::Valid(snapshot) = parse_balance_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        // Units are unknown (oss-hypothesis): both counters survive as their raw JSON
        // tokens, never as binary floats.
        assert_eq!(snapshot.balance.as_deref(), Some("100.5"));
        assert_eq!(snapshot.frozen.as_deref(), Some("2"));
    }

    #[test]
    fn string_counters_survive_as_text() {
        let body = br#"{"code":0,"data":{"balance":"100.00","frozen":"0"}}"#;
        let BalanceProbe::Valid(snapshot) = parse_balance_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.balance.as_deref(), Some("100.00"));
        assert_eq!(snapshot.frozen.as_deref(), Some("0"));
    }

    #[test]
    fn http_401_is_an_invalid_key() {
        // The documented rejection (`official` errors page): decided on the status…
        let body = br#"{"code":401,"message":"invalid api key","suggestion":"check the key"}"#;
        assert_eq!(parse_balance_probe(401, body).unwrap(), BalanceProbe::Invalid);
        // …and the GLM-trap defense: the same business code on another status lands in the
        // same class.
        assert_eq!(parse_balance_probe(200, body).unwrap(), BalanceProbe::Invalid);
    }

    #[test]
    fn other_business_codes_and_statuses_are_transport_not_verdicts() {
        // Rate limit / insufficient balance / future codes never condemn the key.
        for (status, body) in [
            (429, br#"{"code":2000,"message":"rate limited"}"#.as_slice()),
            (429, br#"{"code":1007,"message":"rate limited"}"#.as_slice()),
            (403, br#"{"code":2010,"message":"insufficient balance"}"#.as_slice()),
            (500, br#"{"code":0,"data":{"balance":1,"frozen":0}}"#.as_slice()),
            (502, b"bad gateway".as_slice()),
            (200, br#"{"data":{"balance":1,"frozen":0}}"#.as_slice()),
            (200, br#"{"code":0}"#.as_slice()),
        ] {
            assert!(parse_balance_probe(status, body).is_err(), "{status}");
        }
    }

    #[test]
    fn corroboration_accepts_only_a_readable_non_negative_pair() {
        let snapshot = BalanceSnapshot {
            balance: Some("100.5".into()),
            frozen: Some("2".into()),
        };
        assert_eq!(
            corroborate_cohort(&snapshot, "tripo3d api $50"),
            CohortVerdict::Consistent
        );
        // A missing, negative or non-numeric counter fails closed — never a guess.
        for (balance, frozen) in [
            (None, Some("2".into())),
            (Some("100".into()), None),
            (Some("-1".into()), Some("0".into())),
            (Some("lots".into()), Some("0".into())),
            (None, None),
        ] {
            let snapshot = BalanceSnapshot { balance, frozen };
            assert_eq!(
                corroborate_cohort(&snapshot, "tripo3d api $50"),
                CohortVerdict::Unreadable
            );
        }
        // An uncanonicalizable declared cohort is a caller bug: fail closed.
        assert_eq!(
            corroborate_cohort(&snapshot, ""),
            CohortVerdict::Unreadable
        );
    }

    #[test]
    fn the_tcli_client_id_is_refused_before_any_network_call() {
        assert_eq!(
            preflight_key_rejection("tcli_9f8c7b6a5d0123456789"),
            Some(InvalidKeyReason::ClientIdMisuse)
        );
        assert_eq!(
            preflight_key_rejection("9f8c7b6a5d0123456789"),
            Some(InvalidKeyReason::Auth)
        );
        assert_eq!(preflight_key_rejection("tsk_abcd0123"), None);
    }

    #[test]
    fn credential_from_canonicalizes_and_validates() {
        let credential = credential_from(
            "tsk_test-9f8c7b6a5d",
            " Tripo3D API $50 ",
            "https://api.tripo3d.ai/",
            "socks5://user:p%41ss@egress.example:1080",
        )
        .unwrap();
        assert_eq!(credential.kind, Tripo3dCredentialKind::ApiKey);
        assert_eq!(credential.cohort, "tripo3d api $50");
        assert_eq!(credential.base_url, TRIPO3D_BASE_URL_GLOBAL);
        assert_eq!(credential.api_key, "tsk_test-9f8c7b6a5d");

        let china = credential_from("tsk_test-9f8c7b6a5d", "tripo3d api $50", TRIPO3D_BASE_URL_CHINA, "")
            .unwrap();
        assert_eq!(china.base_url, TRIPO3D_BASE_URL_CHINA);

        // Foreign origins, non-proxy schemes, a Client ID and an empty key fail closed
        // instead of sealing.
        assert!(credential_from("tsk_k", "c", "https://tripo3d.example.com", "").is_err());
        assert!(credential_from("tsk_k", "c", TRIPO3D_BASE_URL_GLOBAL, "file:///etc/passwd").is_err());
        assert!(credential_from("tcli_k", "c", TRIPO3D_BASE_URL_GLOBAL, "").is_err());
        assert!(credential_from("", "c", TRIPO3D_BASE_URL_GLOBAL, "").is_err());
        assert!(credential_from("tsk_k", "", TRIPO3D_BASE_URL_GLOBAL, "").is_err());
        // Sealing and publication belong to tripo3d_roster, which owns the filesystem contract.
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
