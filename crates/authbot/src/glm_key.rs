//! GLM (Zhipu AI / Z.ai Coding Plan) static API-key validation for the Auth Bot.
//!
//! Pure protocol unit: it probes the plan quota, corroborates the declared plan against the
//! observed window limit, runs one minimal paid admission generation and builds the credential
//! that will be sealed. It owns no Telegram state, no seller job, no payout and no roster
//! publication — the seller wizard arrives as the next dependent change, so this module can be
//! reviewed and tested on its own.
//!
//! Contract: `docs/engine/PROVIDER_ONBOARDING.md` §7; provider facts and their evidence
//! labels: `docs/engine/GLM_PROVIDER.md` §2 (credential/identity), §4 (wire), §7 (acquisition).
//!
//! Unlike KIMI there is no OAuth device flow: the seller buys an individual Coding Plan on
//! their own account, creates a static API key in the console and sends the key to the bot as
//! text. The key is the only credential artifact — validating it, then sealing it, is the
//! whole acquisition.

// The seller wizard arrives as the next dependent change; until then only tests call this.
#![allow(dead_code)]

use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use glm_credential::{
    normalize_base_url, normalize_proxy_url, reviewed_plan_credits, GlmCredential,
    GlmCredentialKind, GlmPlan, GLM_ANTHROPIC_MESSAGES_PATH, GLM_QUOTA_PATH,
};
use serde::Deserialize;

/// Bound on any single provider call. A hung validation must not pin a seller job forever.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Hard ceiling on the single paid admission generation. The paid call is never retried
/// automatically, so this bound is what keeps a stuck upstream from holding a seller job.
pub const GENERATION_DEADLINE: Duration = Duration::from_secs(60);

/// Cheapest served model of the plan trio, used for the paid admission probe
/// (`docs/engine/GLM_PROVIDER.md` §7). The aggregate budget cap of the admission micro-smoke
/// ($0.0001) is enforced by the caller, not here.
pub const ADMISSION_MODEL: &str = "glm-4.7";

/// One plan window exactly as the quota endpoint returned it.
///
/// The unit semantics of `unit`/`number`/`usage`/`currentValue`/`remaining` are **unknown**
/// (`oss-hypothesis`, onWatch `zai_types.go`; `docs/engine/GLM_PROVIDER.md` §5.2/§6): every
/// value is kept raw and is never divided by token prices or otherwise reinterpreted. The
/// only reader is plan corroboration, which takes the smallest `TIME_LIMIT` window total and
/// fails closed on any ambiguity.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaLimit {
    /// Raw `type` discriminator (`TIME_LIMIT`, `TOKENS_LIMIT`, …). Unknown values are
    /// preserved, not rejected.
    pub limit_type: String,
    pub unit: Option<i64>,
    pub number: Option<i64>,
    pub usage: Option<i64>,
    pub current_value: Option<i64>,
    pub remaining: Option<i64>,
    pub percentage: Option<f64>,
    /// Provider-side next reset, epoch milliseconds (raw).
    pub next_reset_time: Option<i64>,
}

/// Per-model usage breakdown entry. Only the documented `modelCode` is captured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QuotaUsageDetail {
    pub model_code: String,
}

/// Raw quota snapshot: immutable provider-side evidence, units uninterpreted.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct QuotaSnapshot {
    pub limits: Vec<QuotaLimit>,
    pub usage_details: Vec<QuotaUsageDetail>,
}

/// Outcome of the free quota probe.
#[derive(Clone, Debug, PartialEq)]
pub enum QuotaProbe {
    /// The key was accepted; the snapshot is raw quota evidence.
    Valid(QuotaSnapshot),
    /// The provider rejected the key. The trap of this endpoint is that rejection arrives as
    /// **HTTP 200 with `code: 401` in the body**, so this is decided on the business code,
    /// never on the HTTP status (`oss-hypothesis`, `docs/engine/GLM_PROVIDER.md` §2.1/§4).
    Invalid,
}

/// Verdict of corroborating the seller-declared plan against the observed quota window.
///
/// The plan has no machine-readable identity (`unknown` per manifest §2.1): it is declared by
/// the offer product and corroborated by the officially published 5-hour window credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanVerdict {
    /// The observed 5-hour window limit equals the declared plan's official credits.
    Confirmed(GlmPlan),
    /// The window is readable as a credits number but contradicts the declared plan —
    /// including a number matching no reviewed tier. Fail closed: the profile stays out of
    /// rotation pending operator review.
    PlanMismatch { declared: GlmPlan, observed_limit: u64 },
    /// The quota shape is not a readable individual credits plan: no `TIME_LIMIT` window
    /// (legacy prompts or Team token mechanics), or an unreadable/ambiguous window total.
    /// Never reinterpreted and never guessed.
    UnsupportedPlanShape,
}

/// Why a key is inadmissible — the input for safe seller guidance. Carries no provider text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidKeyReason {
    /// Business codes 1000–1005: the key does not authenticate at all.
    Auth,
    /// 1113: the call landed outside the plan's served endpoints/balance.
    OutOfPlanBalance,
    /// 1309: the subscription behind the key has expired.
    PlanExpired,
    /// 1311: the admission model is outside the key's plan scope.
    ModelOutOfPlan,
    /// 1313: provider risk-control fair-use flag on the account.
    FairUse,
    /// 1315: the key is bound to enterprise/team scenarios, not an individual plan.
    WrongKeyKind,
}

/// Classified outcome of the single paid admission generation. Typed classes, never a bool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyVerdict {
    /// HTTP 2xx with authoritative, complete usage.
    Valid,
    /// The key must not be published; the reason selects the safe seller guidance.
    Invalid(InvalidKeyReason),
    /// 1308/1310: the key is valid but its plan quota is currently exhausted. **Not** an
    /// invalid key — admission still refuses publication, but the seller guidance is "wait
    /// for the window reset", not "reissue the key".
    QuotaExhausted,
    /// 1316–1321: Team/legacy spend-limit mechanics. Fail closed, never reinterpreted as an
    /// individual credits plan.
    UnsupportedPlanShape,
}

#[derive(Deserialize)]
struct QuotaResponse {
    code: Option<i64>,
    success: Option<bool>,
    data: Option<QuotaData>,
    // `msg` is deliberately not carried: provider text may echo account details and is never
    // surfaced to logs or the seller.
}

#[derive(Deserialize)]
struct QuotaData {
    #[serde(default)]
    limits: Vec<QuotaLimitWire>,
    #[serde(default, rename = "usageDetails")]
    usage_details: Vec<QuotaUsageDetailWire>,
}

#[derive(Deserialize)]
struct QuotaLimitWire {
    #[serde(rename = "type")]
    limit_type: Option<String>,
    unit: Option<i64>,
    number: Option<i64>,
    usage: Option<i64>,
    #[serde(rename = "currentValue")]
    current_value: Option<i64>,
    remaining: Option<i64>,
    percentage: Option<f64>,
    #[serde(rename = "nextResetTime")]
    next_reset_time: Option<i64>,
}

#[derive(Deserialize)]
struct QuotaUsageDetailWire {
    #[serde(rename = "modelCode")]
    model_code: Option<String>,
}

#[derive(Deserialize)]
struct GenerationResponse {
    usage: Option<GenerationUsage>,
}

#[derive(Deserialize)]
struct GenerationUsage {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct GenerationErrorResponse {
    error: Option<GenerationError>,
}

#[derive(Deserialize)]
struct GenerationError {
    code: Option<serde_json::Value>,
}

/// Parse a quota probe response, deciding validity on the **business code** (`oss-hypothesis`
/// wire contract, `docs/engine/GLM_PROVIDER.md` §4.2).
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_quota_probe(status: u16, body: &[u8]) -> Result<QuotaProbe> {
    let parsed: QuotaResponse =
        serde_json::from_slice(body).context("decode GLM quota response")?;
    // The invalid-key trap: HTTP 200 with `code: 401` in the body. Checked before anything
    // else so a proxy that surfaces a real HTTP 401 lands in the same class.
    if parsed.code == Some(401) {
        return Ok(QuotaProbe::Invalid);
    }
    if parsed.success == Some(false) {
        return Ok(QuotaProbe::Invalid);
    }
    // A business code that is neither a rejection nor a known success marker is a contract
    // change: fail closed instead of trusting whatever `data` carries.
    if let Some(code) = parsed.code {
        if code != 0 && code != 200 {
            bail!("GLM quota endpoint returned unrecognised business code {code}");
        }
    }
    if parsed.success != Some(true) {
        bail!("GLM quota response is missing its success flag");
    }
    if status != 200 {
        bail!("GLM quota endpoint returned HTTP {status}");
    }
    let data = parsed
        .data
        .ok_or_else(|| anyhow!("GLM quota response is missing data"))?;
    let snapshot = QuotaSnapshot {
        limits: data
            .limits
            .into_iter()
            .map(|wire| QuotaLimit {
                limit_type: wire.limit_type.unwrap_or_default(),
                unit: wire.unit,
                number: wire.number,
                usage: wire.usage,
                current_value: wire.current_value,
                remaining: wire.remaining,
                percentage: wire.percentage,
                next_reset_time: wire.next_reset_time,
            })
            .collect(),
        usage_details: data
            .usage_details
            .into_iter()
            .filter_map(|wire| wire.model_code)
            .filter(|code| !code.is_empty())
            .map(|model_code| QuotaUsageDetail { model_code })
            .collect(),
    };
    Ok(QuotaProbe::Valid(snapshot))
}

/// The smallest `TIME_LIMIT` window total, or `None` when the shape is not an unambiguous
/// individual credits plan. `number` is the published window total; when absent,
/// `currentValue + usage` reconstructs it. Both forms stay raw observations.
fn observed_smallest_time_window(limits: &[QuotaLimit]) -> Option<u64> {
    let mut smallest: Option<u64> = None;
    for limit in limits {
        if limit.limit_type != "TIME_LIMIT" {
            continue;
        }
        let total = match limit.number {
            Some(number) => u64::try_from(number).ok(),
            None => match (limit.current_value, limit.usage) {
                (Some(current), Some(usage)) => current
                    .checked_add(usage)
                    .and_then(|value| u64::try_from(value).ok()),
                _ => None,
            },
        };
        // A window we cannot read makes the whole shape unreadable: fail closed.
        let total = total?;
        match smallest {
            None => smallest = Some(total),
            Some(previous) if total < previous => smallest = Some(total),
            // Two windows with the same total are ambiguous — the 5-hour window cannot be
            // told from the weekly one, so corroboration fails closed.
            Some(previous) if total == previous => return None,
            Some(_) => {}
        }
    }
    smallest
}

/// Corroborate the declared plan against the observed quota window. Credits plans publish
/// their ladder officially (2 000/12 000/28 000 per 5 hours), so a readable window either
/// confirms the declared tier or contradicts it; anything unreadable is a shape refusal.
pub fn corroborate_plan(snapshot: &QuotaSnapshot, declared: GlmPlan) -> PlanVerdict {
    let Some(observed) = observed_smallest_time_window(&snapshot.limits) else {
        return PlanVerdict::UnsupportedPlanShape;
    };
    let confirmed = reviewed_plan_credits(declared)
        .is_some_and(|credits| credits.per_five_hours == observed);
    if confirmed {
        PlanVerdict::Confirmed(declared)
    } else {
        PlanVerdict::PlanMismatch {
            declared,
            observed_limit: observed,
        }
    }
}

/// Business codes arrive as strings (`"1308"`); tolerate a numeric form without ever treating
/// an unparseable value as a known code.
fn business_code(value: Option<&serde_json::Value>) -> Option<u32> {
    match value? {
        serde_json::Value::String(text) => text.trim().parse().ok(),
        serde_json::Value::Number(number) => number
            .as_u64()
            .and_then(|value| u32::try_from(value).ok()),
        _ => None,
    }
}

/// Classify the paid admission generation. Two-layer error scheme: HTTP status plus business
/// code in `{"error":{"code":"1308",…}}` (`official`, `docs/engine/GLM_PROVIDER.md` §4.2).
///
/// Split out from the HTTP call so the wire contract is testable without a network.
pub fn parse_generation_verdict(status: u16, body: &[u8]) -> Result<KeyVerdict> {
    if (200..300).contains(&status) {
        let parsed: GenerationResponse =
            serde_json::from_slice(body).context("decode GLM admission generation")?;
        let usage = parsed
            .usage
            .ok_or_else(|| anyhow!("GLM admission generation is missing usage"))?;
        // A 2xx without both counters is a contract change, not a success: billing would
        // have nothing authoritative to settle against.
        if usage.input_tokens.is_none() || usage.output_tokens.is_none() {
            bail!("GLM admission generation usage is incomplete");
        }
        return Ok(KeyVerdict::Valid);
    }
    let parsed: GenerationErrorResponse =
        serde_json::from_slice(body).context("decode GLM admission error")?;
    let Some(code) = parsed
        .error
        .as_ref()
        .and_then(|error| business_code(error.code.as_ref()))
    else {
        // No business code: a transport-class answer (gateway page, proxy rejection), never
        // a verdict on the key.
        bail!("GLM admission generation returned HTTP {status}");
    };
    Ok(match code {
        1000..=1005 => KeyVerdict::Invalid(InvalidKeyReason::Auth),
        1113 => KeyVerdict::Invalid(InvalidKeyReason::OutOfPlanBalance),
        1308 | 1310 => KeyVerdict::QuotaExhausted,
        1309 => KeyVerdict::Invalid(InvalidKeyReason::PlanExpired),
        1311 => KeyVerdict::Invalid(InvalidKeyReason::ModelOutOfPlan),
        1313 => KeyVerdict::Invalid(InvalidKeyReason::FairUse),
        1315 => KeyVerdict::Invalid(InvalidKeyReason::WrongKeyKind),
        1316..=1321 => KeyVerdict::UnsupportedPlanShape,
        // Unrecognised codes (rate limit 1302, overload 1305, request validation 1210–1215,
        // anything future) are transport-class: the caller's own bounded policy decides,
        // they never condemn the key.
        _ => bail!("GLM admission generation returned HTTP {status} with business code {code}"),
    })
}

/// Build the credential that will be sealed and published. Base URL and proxy are
/// canonicalized here, so an envelope can only ever carry the reviewed forms.
pub fn credential_from(
    api_key: &str,
    plan: GlmPlan,
    base_url: &str,
    proxy_url: &str,
) -> Result<GlmCredential> {
    let credential = GlmCredential {
        version: 1,
        kind: GlmCredentialKind::PlanKey,
        api_key: api_key.to_string(),
        plan,
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

/// Bounded back-off between read-only quota probe retries: 1s doubling to a 30s cap. Applies
/// only to the free probe — the paid generation is never replayed after an ambiguous
/// transport, because the call may already have consumed quota.
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
            .proxy(reqwest::Proxy::all(proxy_url).context("configure GLM validation proxy")?);
    }
    builder.build().context("build GLM validation client")
}

fn endpoint(base_url: &str, path: &str) -> String {
    format!("{}{path}", base_url.trim_end_matches('/'))
}

/// Free read-only quota probe on the seller's assigned egress. Transport failures are safe to
/// retry with `probe_retry_backoff` — the probe consumes no quota.
pub async fn probe_quota(base_url: &str, api_key: &str, proxy_url: &str) -> Result<QuotaProbe> {
    let response = client(proxy_url)?
        .get(endpoint(base_url, GLM_QUOTA_PATH))
        // No `Bearer` prefix on this endpoint — the raw key is the contract
        // (`oss-hypothesis`, `docs/engine/GLM_PROVIDER.md` §4).
        .header(reqwest::header::AUTHORIZATION, api_key)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .context("probe GLM quota")?;
    let status = response.status().as_u16();
    let body = response.bytes().await.context("read GLM quota")?;
    parse_quota_probe(status, &body)
}

/// One minimal paid generation proving the key serves the Anthropic route with authoritative
/// usage. **Never retried automatically**: after an ambiguous transport failure the call may
/// already have consumed quota, so the caller fails closed instead of replaying it.
pub async fn run_admission_generation(
    base_url: &str,
    api_key: &str,
    proxy_url: &str,
) -> Result<KeyVerdict> {
    let request = serde_json::json!({
        "model": ADMISSION_MODEL,
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "ping"}],
    });
    let response = client(proxy_url)?
        .post(endpoint(base_url, GLM_ANTHROPIC_MESSAGES_PATH))
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await
        .context("run GLM admission generation")?;
    let status = response.status().as_u16();
    let body = response
        .bytes()
        .await
        .context("read GLM admission generation")?;
    parse_generation_verdict(status, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use glm_credential::GLM_BASE_URL_INTERNATIONAL;

    fn time_limit(number: i64) -> QuotaLimit {
        QuotaLimit {
            limit_type: "TIME_LIMIT".into(),
            unit: None,
            number: Some(number),
            usage: None,
            current_value: None,
            remaining: None,
            percentage: None,
            next_reset_time: None,
        }
    }

    #[test]
    fn a_valid_probe_preserves_the_raw_limit_fields() {
        let body = br#"{
            "code": 200, "msg": "success", "success": true,
            "data": {
                "limits": [
                    {"type":"TIME_LIMIT","unit":5,"number":2000,"usage":120,
                     "currentValue":1880,"remaining":1880,"percentage":6.0,
                     "nextResetTime":1800003600000},
                    {"type":"TIME_LIMIT","unit":7,"number":10000,"usage":0,
                     "currentValue":10000,"remaining":10000,"percentage":0,
                     "nextResetTime":1800604800000}
                ],
                "usageDetails": [{"modelCode":"glm-4.7"},{"modelCode":"glm-5.2"}]
            }
        }"#;
        let QuotaProbe::Valid(snapshot) = parse_quota_probe(200, body).unwrap() else {
            panic!("expected a valid probe");
        };
        assert_eq!(snapshot.limits.len(), 2);
        let five_hour = &snapshot.limits[0];
        // Units are unknown (oss-hypothesis): every field survives raw, uninterpreted.
        assert_eq!(five_hour.limit_type, "TIME_LIMIT");
        assert_eq!(five_hour.unit, Some(5));
        assert_eq!(five_hour.number, Some(2000));
        assert_eq!(five_hour.usage, Some(120));
        assert_eq!(five_hour.current_value, Some(1880));
        assert_eq!(five_hour.remaining, Some(1880));
        assert_eq!(five_hour.percentage, Some(6.0));
        assert_eq!(five_hour.next_reset_time, Some(1_800_003_600_000));
        assert_eq!(snapshot.usage_details[0].model_code, "glm-4.7");
        assert_eq!(snapshot.usage_details[1].model_code, "glm-5.2");
    }

    #[test]
    fn http_200_with_business_code_401_is_an_invalid_key() {
        // The trap of this endpoint: rejection arrives as HTTP 200, so the business code
        // decides — never the status.
        let body = br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#;
        assert_eq!(parse_quota_probe(200, body).unwrap(), QuotaProbe::Invalid);
        // The same business code on a real HTTP 401 lands in the same class.
        assert_eq!(parse_quota_probe(401, body).unwrap(), QuotaProbe::Invalid);
    }

    #[test]
    fn success_false_is_an_invalid_key_even_with_http_200() {
        let body = br#"{"code":200,"msg":"rejected","success":false,"data":null}"#;
        assert_eq!(parse_quota_probe(200, body).unwrap(), QuotaProbe::Invalid);
    }

    #[test]
    fn a_non_200_probe_without_a_business_rejection_is_transport() {
        assert!(parse_quota_probe(500, br#"{"code":200,"success":true,"data":{}}"#).is_err());
        assert!(parse_quota_probe(502, b"bad gateway").is_err());
    }

    #[test]
    fn plan_corroboration_matches_the_official_credits_ladder() {
        // Official 5-hour / weekly credits per tier (docs.z.ai/devpack/overview, reviewed
        // 2026-08-03); the weekly window rides along as the larger TIME_LIMIT.
        for (five_hour, weekly, plan) in [
            (2_000, 10_000, GlmPlan::Lite),
            (12_000, 60_000, GlmPlan::Pro),
            (28_000, 140_000, GlmPlan::Max),
        ] {
            let snapshot = QuotaSnapshot {
                limits: vec![time_limit(weekly), time_limit(five_hour)],
                ..QuotaSnapshot::default()
            };
            assert_eq!(
                corroborate_plan(&snapshot, plan),
                PlanVerdict::Confirmed(plan)
            );
        }
    }

    #[test]
    fn current_value_plus_usage_reconstructs_the_window_total() {
        let mut window = time_limit(0);
        window.number = None;
        window.current_value = Some(1_880);
        window.usage = Some(120);
        let snapshot = QuotaSnapshot {
            limits: vec![window],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&snapshot, GlmPlan::Lite),
            PlanVerdict::Confirmed(GlmPlan::Lite)
        );
    }

    #[test]
    fn a_window_limit_contradicting_the_declared_plan_is_a_mismatch() {
        // A readable number matching another tier…
        let snapshot = QuotaSnapshot {
            limits: vec![time_limit(2_000)],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&snapshot, GlmPlan::Pro),
            PlanVerdict::PlanMismatch {
                declared: GlmPlan::Pro,
                observed_limit: 2_000,
            }
        );
        // …or no reviewed tier at all: mismatch, never a guess.
        let snapshot = QuotaSnapshot {
            limits: vec![time_limit(9_999)],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&snapshot, GlmPlan::Pro),
            PlanVerdict::PlanMismatch {
                declared: GlmPlan::Pro,
                observed_limit: 9_999,
            }
        );
    }

    #[test]
    fn legacy_and_team_quota_shapes_fail_closed() {
        // Team token mechanics: TOKENS_LIMIT windows without any TIME_LIMIT credits window.
        let team = QuotaSnapshot {
            limits: vec![QuotaLimit {
                limit_type: "TOKENS_LIMIT".into(),
                number: Some(60_000_000),
                ..time_limit(0)
            }],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&team, GlmPlan::Pro),
            PlanVerdict::UnsupportedPlanShape
        );
        // No windows at all.
        assert_eq!(
            corroborate_plan(&QuotaSnapshot::default(), GlmPlan::Pro),
            PlanVerdict::UnsupportedPlanShape
        );
        // Two equal TIME_LIMIT totals: the 5-hour window cannot be told from the weekly one.
        let ambiguous = QuotaSnapshot {
            limits: vec![time_limit(2_000), time_limit(2_000)],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&ambiguous, GlmPlan::Lite),
            PlanVerdict::UnsupportedPlanShape
        );
        // A TIME_LIMIT window with no readable total.
        let mut unreadable = time_limit(0);
        unreadable.number = None;
        let unreadable = QuotaSnapshot {
            limits: vec![unreadable],
            ..QuotaSnapshot::default()
        };
        assert_eq!(
            corroborate_plan(&unreadable, GlmPlan::Lite),
            PlanVerdict::UnsupportedPlanShape
        );
    }

    #[test]
    fn a_complete_generation_is_admitted() {
        let body = br#"{"id":"msg_1","type":"message","role":"assistant","model":"glm-4.7",
            "stop_reason":"max_tokens",
            "usage":{"input_tokens":12,"output_tokens":1}}"#;
        assert_eq!(
            parse_generation_verdict(200, body).unwrap(),
            KeyVerdict::Valid
        );
    }

    #[test]
    fn a_2xx_without_authoritative_usage_is_not_a_success() {
        // Missing usage or a missing counter is a contract change, never an admission.
        assert!(parse_generation_verdict(200, br#"{"id":"msg_1"}"#).is_err());
        assert!(
            parse_generation_verdict(200, br#"{"usage":{"input_tokens":12}}"#).is_err()
        );
    }

    #[test]
    fn business_error_codes_map_to_verdict_classes() {
        let cases = [
            ("1001", KeyVerdict::Invalid(InvalidKeyReason::Auth)),
            ("1005", KeyVerdict::Invalid(InvalidKeyReason::Auth)),
            ("1113", KeyVerdict::Invalid(InvalidKeyReason::OutOfPlanBalance)),
            ("1308", KeyVerdict::QuotaExhausted),
            ("1310", KeyVerdict::QuotaExhausted),
            ("1309", KeyVerdict::Invalid(InvalidKeyReason::PlanExpired)),
            ("1311", KeyVerdict::Invalid(InvalidKeyReason::ModelOutOfPlan)),
            ("1313", KeyVerdict::Invalid(InvalidKeyReason::FairUse)),
            ("1315", KeyVerdict::Invalid(InvalidKeyReason::WrongKeyKind)),
            ("1316", KeyVerdict::UnsupportedPlanShape),
            ("1321", KeyVerdict::UnsupportedPlanShape),
        ];
        for (code, expected) in cases {
            let body = format!(r#"{{"error":{{"code":"{code}","message":"provider text"}}}}"#);
            assert_eq!(
                parse_generation_verdict(429, body.as_bytes()).unwrap(),
                expected,
                "business code {code}"
            );
        }
    }

    #[test]
    fn unrecognized_codes_and_missing_codes_are_transport_not_verdicts() {
        // Rate limit / overload / request validation never condemn the key.
        for code in ["1302", "1305", "1210"] {
            let body = format!(r#"{{"error":{{"code":"{code}","message":"x"}}}}"#);
            assert!(parse_generation_verdict(429, body.as_bytes()).is_err());
        }
        // A gateway page or proxy rejection carries no business code at all.
        assert!(parse_generation_verdict(502, b"bad gateway").is_err());
    }

    #[test]
    fn the_probe_client_honours_the_assigned_egress_and_fails_closed() {
        assert!(client("socks5://user:pass@egress.example:1080").is_ok());
        // An unparseable proxy URL fails client construction instead of leaking to direct
        // egress. Scheme policy (no file://, no ftp://) is enforced earlier, at
        // `credential_from` canonicalization — asserted in the credential test below.
        assert!(client("not a url").is_err());
    }

    #[test]
    fn credential_from_canonicalizes_and_validates() {
        let credential = credential_from(
            "zai-key-1",
            GlmPlan::Pro,
            "https://api.z.ai/",
            "socks5://user:p%41ss@egress.example:1080",
        )
        .unwrap();
        assert_eq!(credential.kind, GlmCredentialKind::PlanKey);
        assert_eq!(credential.plan, GlmPlan::Pro);
        assert_eq!(credential.base_url, GLM_BASE_URL_INTERNATIONAL);
        assert_eq!(credential.api_key, "zai-key-1");

        // Foreign origins and non-proxy schemes fail closed instead of sealing.
        assert!(credential_from("k", GlmPlan::Pro, "https://glm.example.com", "").is_err());
        assert!(credential_from("k", GlmPlan::Pro, "https://api.z.ai", "file:///etc/passwd").is_err());
        assert!(credential_from("", GlmPlan::Pro, "https://api.z.ai", "").is_err());
        // Sealing and publication belong to glm_roster, which owns the filesystem contract.
    }

    #[test]
    fn probe_backoff_is_bounded_and_the_generation_deadline_is_our_own() {
        assert_eq!(probe_retry_backoff(0), Duration::from_secs(1));
        assert_eq!(probe_retry_backoff(3), Duration::from_secs(8));
        assert_eq!(probe_retry_backoff(5), Duration::from_secs(30));
        // A hostile attempt counter cannot push the back-off past the cap.
        assert_eq!(probe_retry_backoff(100), Duration::from_secs(30));
        assert_eq!(GENERATION_DEADLINE, Duration::from_secs(60));
    }
}
