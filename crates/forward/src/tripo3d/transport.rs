//! Per-profile transport to the Tripo3D (VAST / Holymolly) prepaid API plane.
//!
//! Contract: `docs/engine/TRIPO3D_PROVIDER.md` §4 (wire) and §4.1 (error classes). Three
//! provider facts shape this module:
//!
//! * **The credential is a static API key — there is no refresh, ever.** A 401 means the key
//!   was revoked or is invalid; nothing here can repair it. Per the pool-must-not-empty
//!   invariant (`docs/engine/PROVIDER_ONBOARDING.md` §8.4) one 401 is still never a verdict:
//!   it lands on the SOFT auth axis (bounded quarantine + probe), because the rejection may
//!   belong to the request path, and a single crafted request must not rest the whole fleet.
//! * **The money boundary is a successful task creation, not a byte.** Tripo3D is a task-based
//!   media API: `POST /task` with `code: 0 + data.task_id` is the point of no return, after
//!   which the task is owned by the creating profile (tasks are per-key isolated, manifest §2)
//!   and rotation becomes impossible by construction.
//! * **Errors are two-layer, and the business code is at the TOP level.** The provider answers
//!   `{"code": int, "message", "suggestion"}` alongside the HTTP status; the documented
//!   decision classes are 429+`2000` (concurrency wall, with `Retry-After`), 429+`1007`
//!   (generic rate limit), 403+`2010` (insufficient balance at task creation) and 401
//!   (invalid key). Everything else maps by HTTP class.

use std::time::Duration;

use serde_json::Value;
use tripo3d_credential::{TRIPO3D_BALANCE_PATH, TRIPO3D_TASK_PATH};

/// How a request authenticates. `Bearer` is the only scheme the platform documents (official,
/// quick-start: `Authorization: Bearer tsk_…` on every request), so it is the only one
/// represented.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    Bearer,
}

impl AuthScheme {
    pub fn header_name(self) -> &'static str {
        match self {
            Self::Bearer => "authorization",
        }
    }

    pub fn header_value(self, api_key: &str) -> String {
        match self {
            Self::Bearer => format!("Bearer {api_key}"),
        }
    }
}

/// Endpoints the plane is allowed to probe without paying. Deliberately a closed set with one
/// member: a new route must be added here, which makes an ungated or unpriced endpoint a
/// visible decision. There is no generation probe — creating a task spends prepaid credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeRoute {
    /// Free, auth-validating balance read (`GET {base}/v2/openapi/user/balance`,
    /// `oss-hypothesis`: SDK-verified, manifest §5.2).
    ///
    /// `unknown` (manifest §6, live gate): whether a passing balance probe proves task
    /// creation works. Per `docs/engine/PROVIDER_ONBOARDING.md` §8.4 a probe on one backend
    /// path must not rehabilitate another, so readiness treats this as auth/capacity evidence
    /// only, never as generation capability.
    Balance,
}

impl ProbeRoute {
    pub fn path(self) -> &'static str {
        match self {
            Self::Balance => TRIPO3D_BALANCE_PATH,
        }
    }
}

/// What an upstream answer means for this profile.
///
/// The axes are deliberately distinct (`docs/engine/PROVIDER_ONBOARDING.md` §8.4): the HARD
/// axis is provider verdicts only (a parsed 429+`Retry-After`, an explicit 403+`2010`
/// insufficient balance); the SOFT axis is everything we inferred (a 401, a transport fault, a
/// timeout, a failed probe). Only the hard axis may deny a request on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamVerdict {
    Ok,
    /// Hard provider wall: HTTP 429 with business code `2000` (concurrency/rate limit), which
    /// the provider documents with a `Retry-After`. Cool this profile for exactly that long and
    /// rotate; the transport budget is NOT spent on a provider verdict.
    RateLimitedHard,
    /// Hard provider verdict: HTTP 403 with business code `2010` — insufficient balance at task
    /// creation. The profile rests until a free balance probe shows funds again; rotation does
    /// not spend the transport budget.
    InsufficientBalance,
    /// The static key was refused (HTTP 401; a `tcli_` Client ID is documented to land here).
    /// SOFT axis: one 401 is never a verdict — the key is static and there is no refresh, but
    /// the rejection can still belong to the request path. Bounded soft quarantine + probe,
    /// exponential backoff, reset on proven success.
    AuthRefused,
    /// Provider-side generic rate limit (429+`1007`), a plain 429/408/5xx, or a transport
    /// failure before task creation. Bounded rotation within the transport budget.
    Transport,
    /// Deterministic client/request error: another 4xx the provider attributes to the request
    /// itself. Neither rotate nor blame the profile.
    ClientError,
    /// A successful HTTP status whose envelope is not the documented `code: 0` success, or a
    /// body that does not parse: a contract change, fail closed. Never rotated as transport —
    /// the answer may already have created the task, and the wire needs review first.
    Protocol,
}

/// Business codes sit at the TOP level of the error envelope (`{"code": …, "message", …}`),
/// unlike the GLM `error.code` nesting. Tolerate a numeric or string form without ever treating
/// an unparseable value as a known code.
pub fn error_business_code(payload: &Value) -> Option<u32> {
    match payload.get("code")? {
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// Classify an upstream answer for the Tripo3D plane.
///
/// The **business code has priority over the HTTP class** (manifest §4.1): a 429 carrying
/// `2000` is a documented concurrency wall, not a generic rate limit. HTTP-only answers (a
/// proxy page, a WAF block, a gateway timeout) fall through to the status mapping. An unknown
/// business code fails closed: on a 2xx it is `Protocol` (the wire changed under us), on an
/// error status it is classified by that status.
pub fn classify_status(status: u16, business_code: Option<u32>) -> UpstreamVerdict {
    if let Some(code) = business_code {
        match code {
            // Success marker only confirms Ok on an actual 2xx; a contradictory pair
            // (5xx + code 0) is a lying body, so the HTTP status decides.
            0 if (200..300).contains(&status) => return UpstreamVerdict::Ok,
            0 => return classify_http_status(status),
            2000 => return UpstreamVerdict::RateLimitedHard,
            1007 => return UpstreamVerdict::Transport,
            2010 => return UpstreamVerdict::InsufficientBalance,
            _ => return classify_http_status(status),
        }
    }
    classify_http_status(status)
}

/// HTTP-only classification: a proxy page, a WAF block or a gateway timeout that carried no
/// usable business code. A 2xx without the documented success envelope is a protocol anomaly,
/// not a success.
fn classify_http_status(status: u16) -> UpstreamVerdict {
    match status {
        200..=299 => UpstreamVerdict::Protocol,
        401 => UpstreamVerdict::AuthRefused,
        // A codeless 403 cannot be told apart from the documented 2010 quota wall, and a
        // codeless 403 may equally be a WAF/risk block: soft, never a hard verdict.
        403 => UpstreamVerdict::AuthRefused,
        408 | 409 | 425 | 429 => UpstreamVerdict::Transport,
        500..=599 => UpstreamVerdict::Transport,
        _ => UpstreamVerdict::ClientError,
    }
}

/// Whether a verdict permits trying another profile BEFORE the money boundary (a successful
/// task creation). A deterministic client error rotates nowhere: the next profile would fail
/// identically. `Protocol` never rotates either: a changed wire under a paid boundary needs
/// review, not a second attempt that might double-create. After a successful creation nothing
/// rotates — encoded by the caller through the attempt phase, not by this predicate.
pub fn may_rotate(verdict: UpstreamVerdict) -> bool {
    matches!(
        verdict,
        UpstreamVerdict::RateLimitedHard
            | UpstreamVerdict::InsufficientBalance
            | UpstreamVerdict::AuthRefused
            | UpstreamVerdict::Transport
    )
}

pub fn spends_transport_budget(verdict: UpstreamVerdict) -> bool {
    matches!(verdict, UpstreamVerdict::Transport)
}

/// Parse a `Retry-After` header value (delta-seconds form only; the HTTP-date form is not
/// documented for this provider and stays unparseable rather than guessed).
pub fn parse_retry_after(value: &str) -> Option<i64> {
    let seconds: i64 = value.trim().parse().ok()?;
    (seconds > 0).then_some(seconds)
}

/// Runtime configuration for the plane. `forward` never reads the environment; `server` fills
/// this. There is deliberately **no `base_url` here**: the platform origin is per-profile
/// (global and CN keys are not interchangeable, manifest §2) and arrives inside the sealed
/// credential, so a fleet-wide override could only misroute a key to the wrong platform.
#[derive(Clone, Debug)]
pub struct Tripo3dTransportConfig {
    pub auth_scheme: AuthScheme,
    pub request_timeout: Duration,
}

impl Default for Tripo3dTransportConfig {
    fn default() -> Self {
        Self {
            auth_scheme: AuthScheme::Bearer,
            // Task creation and polls are short JSON RPCs; the long wait happens between polls,
            // never inside one HTTP read. Artifact downloads get their own bounded reads.
            request_timeout: Duration::from_secs(60),
        }
    }
}

/// Build the absolute URL for a non-billable probe on one profile's platform origin.
pub fn probe_url(base_url: &str, route: ProbeRoute) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), route.path())
}

/// Build the absolute task-creation URL on one profile's platform origin.
pub fn task_create_url(base_url: &str) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), TRIPO3D_TASK_PATH)
}

/// Build the absolute poll URL for an upstream task id. The id is provider-issued; it is
/// interpolated raw, so the caller must pass exactly what `code: 0` returned.
pub fn task_poll_url(base_url: &str, upstream_task_id: &str) -> String {
    format!(
        "{}{}/{}",
        base_url.trim_end_matches('/'),
        TRIPO3D_TASK_PATH,
        upstream_task_id
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn verdict_for(status: u16, code: u32) -> UpstreamVerdict {
        let body = json!({"code": code, "message": "provider text", "suggestion": "…"});
        classify_status(status, error_business_code(&body))
    }

    #[test]
    fn the_business_code_wins_over_the_http_class() {
        // 429 + 2000 is the documented concurrency wall: hard axis, exact Retry-After cooling.
        assert_eq!(verdict_for(429, 2000), UpstreamVerdict::RateLimitedHard);
        // 429 + 1007 is the generic rate limit: bounded transport rotation.
        assert_eq!(verdict_for(429, 1007), UpstreamVerdict::Transport);
        // 403 + 2010 is the documented insufficient-balance verdict: hard axis.
        assert_eq!(verdict_for(403, 2010), UpstreamVerdict::InsufficientBalance);
    }

    #[test]
    fn the_documented_success_envelope_is_required_on_2xx() {
        assert_eq!(verdict_for(200, 0), UpstreamVerdict::Ok);
        // A 2xx without code 0 is a lying or changed wire: protocol, never Ok.
        assert_eq!(classify_status(200, None), UpstreamVerdict::Protocol);
        assert_eq!(verdict_for(200, 2000), UpstreamVerdict::RateLimitedHard);
        assert!(!may_rotate(UpstreamVerdict::Protocol));
    }

    #[test]
    fn a_contradictory_pair_trusts_the_http_failure_not_the_lying_body() {
        assert_eq!(verdict_for(500, 0), UpstreamVerdict::Transport);
    }

    #[test]
    fn auth_refusal_is_soft_and_never_a_verdict_on_its_own() {
        // One 401 is the soft axis (the key is static, there is no refresh, but the rejection
        // may belong to the request path); it rotates without resting the profile durably.
        assert_eq!(classify_status(401, None), UpstreamVerdict::AuthRefused);
        assert_eq!(verdict_for(401, 401), UpstreamVerdict::AuthRefused);
        assert!(may_rotate(UpstreamVerdict::AuthRefused));
        assert!(!spends_transport_budget(UpstreamVerdict::AuthRefused));
        // A codeless 403 cannot be told apart from the 2010 wall and may be a WAF page: soft.
        assert_eq!(classify_status(403, None), UpstreamVerdict::AuthRefused);
    }

    #[test]
    fn hard_verdicts_rotate_without_spending_the_transport_budget() {
        // That budget exists for upstream outages. Spending it on a provider verdict would
        // stop the search before a healthy profile is reached.
        for verdict in [
            UpstreamVerdict::RateLimitedHard,
            UpstreamVerdict::InsufficientBalance,
        ] {
            assert!(may_rotate(verdict));
            assert!(!spends_transport_budget(verdict));
        }
    }

    #[test]
    fn http_only_answers_fall_back_to_the_status_mapping() {
        for status in [408, 409, 425, 429, 500, 502, 503, 504] {
            let verdict = classify_status(status, None);
            assert_eq!(verdict, UpstreamVerdict::Transport, "status {status}");
            assert!(spends_transport_budget(verdict));
        }
        for status in [400, 404, 413, 422] {
            assert_eq!(classify_status(status, None), UpstreamVerdict::ClientError);
        }
        assert!(!may_rotate(UpstreamVerdict::ClientError));
        assert!(!spends_transport_budget(UpstreamVerdict::ClientError));
    }

    #[test]
    fn an_unknown_business_code_falls_back_to_the_status_class() {
        // Unknown codes fail closed onto the HTTP class: they never invent a new axis, and a
        // 2xx with an unknown code is a protocol anomaly rather than a success.
        assert_eq!(verdict_for(429, 9999), UpstreamVerdict::Transport);
        assert_eq!(verdict_for(400, 9999), UpstreamVerdict::ClientError);
        assert_eq!(verdict_for(200, 9999), UpstreamVerdict::Protocol);
    }

    #[test]
    fn business_codes_parse_from_numbers_or_strings_only() {
        assert_eq!(error_business_code(&json!({"code": 2000})), Some(2000));
        assert_eq!(error_business_code(&json!({"code": "2010"})), Some(2010));
        assert_eq!(error_business_code(&json!({"code": "x"})), None);
        assert_eq!(error_business_code(&json!({"code": -1})), None);
        assert_eq!(error_business_code(&json!({})), None);
    }

    #[test]
    fn retry_after_parses_delta_seconds_only() {
        assert_eq!(parse_retry_after("3"), Some(3));
        assert_eq!(parse_retry_after(" 12 "), Some(12));
        assert_eq!(parse_retry_after("0"), None);
        assert_eq!(parse_retry_after("-5"), None);
        assert_eq!(parse_retry_after("Wed, 21 Oct 2015 07:28:00 GMT"), None);
    }

    #[test]
    fn the_probe_set_is_balance_only_and_auth_validating() {
        // The closed enum cannot express a generation probe: task creation spends credits, so
        // readiness and admission may only ever call the free balance route.
        assert_eq!(ProbeRoute::Balance.path(), "/v2/openapi/user/balance");
        assert!(!ProbeRoute::Balance.path().contains("task"));
        assert_eq!(
            probe_url("https://api.tripo3d.ai/", ProbeRoute::Balance),
            "https://api.tripo3d.ai/v2/openapi/user/balance"
        );
        assert_eq!(
            probe_url("https://api.tripo3d.com", ProbeRoute::Balance),
            "https://api.tripo3d.com/v2/openapi/user/balance"
        );
    }

    #[test]
    fn every_request_takes_the_bearer_scheme() {
        // One documented scheme, used identically on task creation, polling and balance.
        assert_eq!(AuthScheme::Bearer.header_value("tsk_k"), "Bearer tsk_k");
        assert_eq!(AuthScheme::Bearer.header_name(), "authorization");
        assert_eq!(
            Tripo3dTransportConfig::default().auth_scheme,
            AuthScheme::Bearer
        );
    }

    #[test]
    fn task_urls_are_built_on_the_profile_origin() {
        assert_eq!(
            task_create_url("https://api.tripo3d.ai/"),
            "https://api.tripo3d.ai/v2/openapi/task"
        );
        assert_eq!(
            task_poll_url("https://api.tripo3d.com", "task-1"),
            "https://api.tripo3d.com/v2/openapi/task/task-1"
        );
    }
}
