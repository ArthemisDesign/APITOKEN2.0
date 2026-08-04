//! Per-profile transport to the GLM (Zhipu AI / Z.ai) Coding Plan subscription plane.
//!
//! Contract: `docs/engine/GLM_PROVIDER.md` §4 (wire) and §4.2 (error classes). Three provider
//! facts shape this module:
//!
//! * **The credential is a static API key — there is no refresh, ever.** Unlike KIMI there are
//!   no `RefreshLocks`, no `needs_refresh`, no reseal-on-refresh. A 401 means the key was
//!   revoked or is invalid, so a repeated 401 on the same profile is meaningless: the profile
//!   is dead until the Auth Bot publishes a replacement key. Nothing here may retry-after-401.
//! * **Errors are two-layer.** The provider answers `{"error":{"code":"1308","message":"…"}}`:
//!   the HTTP status and the business code can disagree, and the **business code wins**
//!   (§4.2). The extreme case is the quota endpoint, which answers HTTP 200 with `code: 401`
//!   in the body for an invalid key.
//! * **Risk-control fingerprints the client.** Z.ai detects "SDK-based access" — requests
//!   without the identifying Claude Code tool headers — and throttles or bans the subscription
//!   (oss-hypothesis, manifest §4). Generation traffic must therefore carry the
//!   Claude-Code-compatible identity set from [`GlmTransportConfig::identity`]. The quota
//!   endpoint needs no identity set: it is a monitor surface, not generation.

use std::time::Duration;

use glm_credential::GLM_QUOTA_PATH;
use serde_json::Value;

/// How a request authenticates on the generation route. `Bearer` is the only implemented
/// scheme: it is the official one for the Coding Plan Anthropic endpoint (Claude Code uses it
/// through `ANTHROPIC_AUTH_TOKEN`). Whether the endpoint also accepts `x-api-key` is an open
/// `unknown` (manifest §4/§6), so no alternative is represented until live evidence proves it.
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

/// Authorization header value for the quota endpoint: the **raw key, without the `Bearer`
/// prefix** the generation route uses (oss-hypothesis wire contract, manifest §4). A function
/// rather than a call-site string so the trap — `Bearer` on this endpoint is not the contract —
/// is impossible to miss.
pub fn quota_authorization(api_key: &str) -> &str {
    api_key
}

/// Endpoints the plane is allowed to probe without paying. Deliberately a closed set with one
/// member: a new route must be added here, which makes an ungated or unpriced endpoint a
/// visible decision. There is no generation probe — generation costs quota.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeRoute {
    /// Free, auth-validating quota read (`GET {base}/api/monitor/usage/quota/limit`).
    ///
    /// `unknown` (manifest §6, live gate): whether a passing quota probe proves the
    /// generation route works. Per `docs/engine/PROVIDER_ONBOARDING.md` §8.4 a probe on one
    /// backend path must not rehabilitate another, so readiness treats this as auth/capacity
    /// evidence only, never as generation capability.
    Quota,
}

impl ProbeRoute {
    pub fn path(self) -> &'static str {
        match self {
            Self::Quota => GLM_QUOTA_PATH,
        }
    }
}

/// What an upstream answer means for this profile.
///
/// The axes are deliberately distinct (`docs/engine/PROVIDER_ONBOARDING.md` §8.4): durable
/// account state (dead/suspect), provider quota (wall + reset), model scope and transient
/// transport are different failures and must not share a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamVerdict {
    Ok,
    /// The account is out of rotation until a human/Auth Bot intervention: the static key was
    /// refused (HTTP 401, business 1000–1005, or the quota endpoint's HTTP-200 `code: 401`) or
    /// the plan behind it expired (1309). No retry on this profile can succeed — there is no
    /// refresh token to exchange — so the pool rotates away immediately.
    AccountDead,
    /// Risk-control or account anomaly: fair-use flag (1313), out-of-plan balance (1113),
    /// wrong key kind (1315), Team/legacy spend-limit mechanics (1316–1321), or a business
    /// code this build does not know. Out of rotation but **not dead**: suspect is the
    /// recoverable class and unknown codes fail closed into it, never into dead.
    AccountSuspect,
    /// Provider quota wall: 1308 (rolling 5-hour credits) or 1310 (weekly/monthly credits).
    /// Cool this profile until the parsed reset; rotation must not spend the transport budget.
    QuotaExhausted,
    /// The requested model is outside the key's plan scope (1311). Model-scoped, NOT an
    /// account failure: the same profile may still serve the other plan models.
    ModelIneligible,
    /// Provider-side rate limit/overload (1302/1305), a plain 429/5xx, or a transport failure
    /// before any byte. Bounded rotation within the transport budget.
    Transport,
    /// Deterministic client/request error: request validation (1210–1215), content filter
    /// (1301), access denial (1220) or another plain 4xx. Neither rotate nor blame the profile.
    ClientError,
}

/// Business codes arrive as strings (`"1308"`); tolerate a numeric form without ever treating
/// an unparseable value as a known code (same convention as `authbot::glm_key`).
pub fn error_business_code(payload: &Value) -> Option<u32> {
    match payload.get("error")?.get("code")? {
        Value::String(text) => text.trim().parse().ok(),
        Value::Number(number) => number.as_u64().and_then(|value| u32::try_from(value).ok()),
        _ => None,
    }
}

/// Classify an upstream answer for the GLM plane.
///
/// The **business code has priority over the HTTP class** (manifest §4.2): a 429 carrying
/// 1308 is a quota wall, not a rate limit, and the quota endpoint's HTTP 200 carrying
/// `code: 401` is a dead key, not a success. HTTP-only answers (a proxy page, a WAF block, a
/// gateway timeout) fall through to the status mapping.
///
/// HTTP-only 403 maps to `AccountSuspect`, not to a client error: Z.ai risk-control block
/// pages arrive without a business code, and the documented 403 meaning (1220 access denial)
/// always carries one. Taking the profile out of rotation is recoverable; blaming the
/// customer's request would hide a risk-control strike.
pub fn classify_status(status: u16, business_code: Option<u32>) -> UpstreamVerdict {
    if let Some(code) = business_code {
        match code {
            // Success markers of the quota envelope only confirm Ok on an actual 2xx; a
            // contradictory pair (5xx + code 200) is a lying body, so the HTTP status decides.
            0 | 200 if (200..300).contains(&status) => return UpstreamVerdict::Ok,
            0 | 200 => return classify_http_status(status),
            1000..=1005 => return UpstreamVerdict::AccountDead,
            // The quota endpoint answers HTTP 200 with `code: 401` for an invalid key
            // (oss-hypothesis, manifest §4): the business code makes it auth-dead, never Ok.
            401 => return UpstreamVerdict::AccountDead,
            1309 => return UpstreamVerdict::AccountDead,
            1113 | 1313 | 1316..=1321 => return UpstreamVerdict::AccountSuspect,
            // 1315 means the key is bound to enterprise/team scenarios. The manifest §4.2
            // reaction is "dead"; runtime deliberately lands in the recoverable suspect class
            // instead: whether the binding is truly permanent is unproven, and suspect keeps
            // the profile reviewable without routing traffic to it.
            1315 => return UpstreamVerdict::AccountSuspect,
            1308 | 1310 => return UpstreamVerdict::QuotaExhausted,
            1311 => return UpstreamVerdict::ModelIneligible,
            1302 | 1305 => return UpstreamVerdict::Transport,
            1210..=1215 | 1301 | 1220 => return UpstreamVerdict::ClientError,
            // Unknown code: fail closed into suspect, never into dead (manifest §4.2 decision).
            _ => return UpstreamVerdict::AccountSuspect,
        }
    }
    classify_http_status(status)
}

/// HTTP-only classification: a proxy page, a WAF block or a gateway timeout that carried no
/// usable business code.
fn classify_http_status(status: u16) -> UpstreamVerdict {
    match status {
        200..=299 => UpstreamVerdict::Ok,
        401 => UpstreamVerdict::AccountDead,
        403 => UpstreamVerdict::AccountSuspect,
        408 | 409 | 425 | 429 => UpstreamVerdict::Transport,
        500..=599 => UpstreamVerdict::Transport,
        _ => UpstreamVerdict::ClientError,
    }
}

/// Whether a verdict permits trying another profile.
///
/// A deterministic client error rotates nowhere: the next profile would fail identically.
/// Every account/quota/model refusal rotates to another profile without consuming the
/// transport budget — that budget exists for real upstream outages.
pub fn may_rotate(verdict: UpstreamVerdict) -> bool {
    !matches!(verdict, UpstreamVerdict::Ok | UpstreamVerdict::ClientError)
}

pub fn spends_transport_budget(verdict: UpstreamVerdict) -> bool {
    matches!(verdict, UpstreamVerdict::Transport)
}

/// Claude-Code-compatible identity headers sent on generation traffic.
///
/// Risk-control Z.ai detects SDK-like traffic and bans the subscription over it (manifest §4),
/// so "bare SDK" requests are a direct path to losing the plan. The exact set that passes
/// without throttling is an open `unknown` (manifest §6.7): the defaults below are the parts
/// that identify a Claude Code client, deliberately without behaviour-changing beta flags, and
/// the operator can tune the set once live evidence arrives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmIdentityHeaders {
    /// `User-Agent` of the reviewed Claude Code build (same value the Claude plane fleets).
    pub user_agent: String,
    /// `anthropic-version` header value.
    pub anthropic_version: String,
    /// `anthropic-beta` header value. Default carries only the Claude Code client marker:
    /// behaviour-affecting betas (thinking, cache TTL, context management) stay out until a
    /// live run proves the GLM endpoint tolerates them.
    pub anthropic_beta: String,
}

impl Default for GlmIdentityHeaders {
    fn default() -> Self {
        Self {
            user_agent: "claude-cli/2.1.195 (external, sdk-cli)".to_string(),
            anthropic_version: "2023-06-01".to_string(),
            anthropic_beta: "claude-code-20250219".to_string(),
        }
    }
}

/// Runtime configuration for the plane. `forward` never reads the environment; `server` fills
/// this. There is deliberately **no `base_url` here**: the console origin is per-profile
/// (int keys and CN keys are not interchangeable, manifest §2) and arrives inside the sealed
/// credential, so a fleet-wide override could only misroute a key to the wrong console.
#[derive(Clone, Debug)]
pub struct GlmTransportConfig {
    pub auth_scheme: AuthScheme,
    pub request_timeout: Duration,
    pub identity: GlmIdentityHeaders,
}

impl Default for GlmTransportConfig {
    fn default() -> Self {
        Self {
            auth_scheme: AuthScheme::Bearer,
            request_timeout: Duration::from_secs(120),
            identity: GlmIdentityHeaders::default(),
        }
    }
}

/// Build the absolute URL for a non-billable probe on one profile's console origin.
pub fn probe_url(base_url: &str, route: ProbeRoute) -> String {
    format!("{}{}", base_url.trim_end_matches('/'), route.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn verdict_for(status: u16, code: &str) -> UpstreamVerdict {
        let body = json!({"error": {"code": code, "message": "provider text"}});
        classify_status(status, error_business_code(&body))
    }

    #[test]
    fn the_business_code_wins_over_the_http_class() {
        // A 429 carrying 1308 is a quota wall, not a rate limit: no transport budget may burn.
        assert_eq!(verdict_for(429, "1308"), UpstreamVerdict::QuotaExhausted);
        assert_eq!(verdict_for(400, "1302"), UpstreamVerdict::Transport);
        // The quota endpoint trap: HTTP 200 with code 401 is a dead key, not a success.
        assert_eq!(verdict_for(200, "401"), UpstreamVerdict::AccountDead);
        // …and the same shape through the envelope code path the quota parser also uses.
        assert_eq!(classify_status(200, Some(401)), UpstreamVerdict::AccountDead);
        assert_eq!(
            classify_status(200, Some(1001)),
            UpstreamVerdict::AccountDead
        );
    }

    #[test]
    fn auth_failures_and_plan_expiry_kill_the_account_without_a_retry_path() {
        // 1000–1005: the static key does not authenticate. There is no refresh to attempt, so
        // this is immediately terminal for the profile.
        for code in 1000..=1005 {
            let verdict = classify_status(401, Some(code));
            assert_eq!(verdict, UpstreamVerdict::AccountDead, "code {code}");
            assert!(may_rotate(verdict));
            assert!(!spends_transport_budget(verdict));
        }
        // 1309: the plan behind the key expired — dead until a new key/plan, equally terminal.
        assert_eq!(verdict_for(429, "1309"), UpstreamVerdict::AccountDead);
    }

    #[test]
    fn anomaly_classes_land_in_suspect_and_never_in_dead() {
        // 1113 out-of-plan balance, 1313 fair-use, 1315 wrong key kind, 1316–1321 Team/legacy
        // spend mechanics: all recoverable-by-review, none a proof of a dead account.
        for code in [1113, 1313, 1315, 1316, 1318, 1321] {
            assert_eq!(
                classify_status(429, Some(code)),
                UpstreamVerdict::AccountSuspect,
                "code {code}"
            );
        }
        // An unrecognised code fails closed into suspect — never dead, never silently Ok.
        for code in [1400, 2000, 9999] {
            assert_eq!(
                classify_status(429, Some(code)),
                UpstreamVerdict::AccountSuspect,
                "unknown code {code}"
            );
        }
    }

    #[test]
    fn quota_walls_and_model_scope_have_their_own_axes() {
        assert_eq!(verdict_for(429, "1308"), UpstreamVerdict::QuotaExhausted);
        assert_eq!(verdict_for(429, "1310"), UpstreamVerdict::QuotaExhausted);
        assert!(!spends_transport_budget(UpstreamVerdict::QuotaExhausted));
        // 1311 is model-scoped: it must not condemn the account.
        assert_eq!(verdict_for(429, "1311"), UpstreamVerdict::ModelIneligible);
        assert!(may_rotate(UpstreamVerdict::ModelIneligible));
        assert!(!spends_transport_budget(UpstreamVerdict::ModelIneligible));
    }

    #[test]
    fn rate_limit_and_overload_codes_stay_transport() {
        for code in [1302, 1305] {
            let verdict = classify_status(429, Some(code));
            assert_eq!(verdict, UpstreamVerdict::Transport, "code {code}");
            assert!(spends_transport_budget(verdict));
        }
    }

    #[test]
    fn request_semantics_neither_rotate_nor_blame_the_profile() {
        for code in 1210..=1215 {
            assert_eq!(
                classify_status(400, Some(code)),
                UpstreamVerdict::ClientError,
                "code {code}"
            );
        }
        for code in [1301, 1220] {
            assert_eq!(
                classify_status(400, Some(code)),
                UpstreamVerdict::ClientError,
                "code {code}"
            );
        }
        assert!(!may_rotate(UpstreamVerdict::ClientError));
        assert!(!spends_transport_budget(UpstreamVerdict::ClientError));
    }

    #[test]
    fn http_only_answers_fall_back_to_the_status_mapping() {
        for status in [200, 201, 299] {
            assert_eq!(classify_status(status, None), UpstreamVerdict::Ok);
        }
        assert_eq!(classify_status(401, None), UpstreamVerdict::AccountDead);
        // A codeless 403 is the risk-control block-page shape: suspect, not a client error.
        assert_eq!(classify_status(403, None), UpstreamVerdict::AccountSuspect);
        for status in [408, 409, 425, 429, 500, 502, 503, 504] {
            let verdict = classify_status(status, None);
            assert_eq!(verdict, UpstreamVerdict::Transport, "status {status}");
            assert!(spends_transport_budget(verdict));
        }
        for status in [400, 404, 413, 422] {
            assert_eq!(classify_status(status, None), UpstreamVerdict::ClientError);
        }
    }

    #[test]
    fn success_marker_codes_only_confirm_ok_on_a_real_2xx() {
        assert_eq!(classify_status(200, Some(0)), UpstreamVerdict::Ok);
        assert_eq!(classify_status(200, Some(200)), UpstreamVerdict::Ok);
        // A contradictory pair trusts the HTTP failure, not the lying body.
        assert_eq!(classify_status(500, Some(200)), UpstreamVerdict::Transport);
    }

    #[test]
    fn business_codes_parse_from_strings_or_numbers_only() {
        assert_eq!(
            error_business_code(&json!({"error": {"code": "1308"}})),
            Some(1308)
        );
        assert_eq!(
            error_business_code(&json!({"error": {"code": 1308}})),
            Some(1308)
        );
        assert_eq!(error_business_code(&json!({"error": {"code": "x"}})), None);
        assert_eq!(error_business_code(&json!({"error": {}})), None);
        assert_eq!(error_business_code(&json!({})), None);
    }

    #[test]
    fn the_probe_set_is_quota_only_and_auth_validating() {
        // The closed enum cannot express a generation probe: generation costs quota, so
        // readiness and admission may only ever call the free quota route.
        assert_eq!(ProbeRoute::Quota.path(), "/api/monitor/usage/quota/limit");
        assert!(!ProbeRoute::Quota.path().contains("messages"));
        assert_eq!(
            probe_url("https://api.z.ai/", ProbeRoute::Quota),
            "https://api.z.ai/api/monitor/usage/quota/limit"
        );
        assert_eq!(
            probe_url("https://open.bigmodel.cn", ProbeRoute::Quota),
            "https://open.bigmodel.cn/api/monitor/usage/quota/limit"
        );
    }

    #[test]
    fn the_quota_endpoint_takes_the_raw_key_and_generation_takes_bearer() {
        // The trap is structural: quota without the prefix, generation with it.
        assert_eq!(quota_authorization("zai-key-1"), "zai-key-1");
        assert_eq!(
            AuthScheme::Bearer.header_value("zai-key-1"),
            "Bearer zai-key-1"
        );
        assert_eq!(AuthScheme::Bearer.header_name(), "authorization");
        assert_eq!(GlmTransportConfig::default().auth_scheme, AuthScheme::Bearer);
    }

    #[test]
    fn the_default_identity_set_looks_like_claude_code_without_behavior_betas() {
        let identity = GlmIdentityHeaders::default();
        assert!(identity.user_agent.starts_with("claude-cli/"));
        assert_eq!(identity.anthropic_version, "2023-06-01");
        // The client marker is present; behaviour-changing flags are absent until live proof.
        assert!(identity.anthropic_beta.contains("claude-code-20250219"));
        assert!(!identity.anthropic_beta.contains("interleaved-thinking"));
    }
}
