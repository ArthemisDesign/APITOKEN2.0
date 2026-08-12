//! Per-profile transport to the Suno (suno.com) subscription session-pool plane.
//!
//! Contract: `docs/engine/SUNO_PROVIDER.md` §4 (wire) and §4.1 (error classes). Three provider
//! facts shape this module:
//!
//! * **The credential is a session, and the JWT is minted on demand.** There is no static key:
//!   the Clerk `__client` cookie discovers the session and mints short-lived JWTs
//!   (`Authorization: Bearer {jwt}` on the business host), and a mint answer may rotate the
//!   underlying cookie material via `set-cookie`. A 401/403 after a successful mint is still
//!   never a verdict on its own: per the pool-must-not-empty invariant
//!   (`docs/engine/PROVIDER_ONBOARDING.md` §8.4) it lands on the SOFT auth axis (bounded
//!   quarantine + probe), because the rejection may belong to the request path.
//! * **The money boundary is a successful generation creation, not a byte.** Suno is a
//!   task-based media API: a successful `POST /api/generate/v2/` (or its extend/lyrics/stems
//!   siblings) is the point of no return, after which the generation belongs to the creating
//!   profile's account and rotation becomes impossible by construction.
//! * **No error reference exists.** No official error catalogue is published (`unknown`,
//!   manifest §4.1), so classification is conservative and HTTP-only: 401/403 after a mint →
//!   soft auth axis; 429 → hard quota/rate wall with cooling; 5xx/timeouts pre-result →
//!   bounded transport rotation; every other 4xx → deterministic client error. An unexpected
//!   success shape fails closed as `Protocol`, never a guessed success.

use std::time::Duration;

use suno_credential::{
    SUNO_API_BASE_URL, SUNO_AUTH_BASE_URL, SUNO_BILLING_INFO_PATH, SUNO_CAPTCHA_CHECK_PATH,
    SUNO_CLIENT_PATH, SUNO_CLIP_PATH, SUNO_FEED_PATH, SUNO_GENERATE_PATH,
    SUNO_SESSION_TOKENS_PATH,
};

/// Clerk version pins of the reviewed session wire (`oss-hypothesis`, gcui-art/suno-api read
/// 2026-08-12; manifest §2). Part of the reviewed contract: a live run that shows different
/// versions updates this constant deliberately, not silently.
pub const CLERK_QUERY: &str = "__clerk_api_version=2025-11-10&_clerk_js_version=5.117.0";

/// Extend/concat generation endpoint on the business host (`oss-hypothesis`, manifest §4).
pub const SUNO_CONCAT_PATH: &str = "/api/generate/concat/v2/";
/// Lyrics generation endpoint on the business host (`oss-hypothesis`, manifest §4).
pub const SUNO_LYRICS_PATH: &str = "/api/generate/lyrics/";
/// Stems split endpoint on the business host; `{song_id}` is substituted by the caller
/// (`oss-hypothesis`, manifest §4).
pub const SUNO_STEMS_PATH: &str = "/api/edit/stems";

/// Endpoints the plane is allowed to probe without paying. Deliberately a closed set with one
/// member: a new route must be added here, which makes an ungated or unpriced endpoint a
/// visible decision. There is no generation probe — creating a generation spends subscription
/// credits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeRoute {
    /// Free, auth-validating quota read (`GET /api/billing/info/`, `oss-hypothesis`:
    /// gcui-art/suno-api, manifest §5.2).
    ///
    /// `unknown` (manifest §6, live gate): whether a passing billing probe proves generation
    /// works. Per `docs/engine/PROVIDER_ONBOARDING.md` §8.4 a probe on one backend path must
    /// not rehabilitate another, so readiness treats this as auth/quota evidence only, never
    /// as generation capability.
    BillingInfo,
}

impl ProbeRoute {
    pub fn path(self) -> &'static str {
        match self {
            Self::BillingInfo => SUNO_BILLING_INFO_PATH,
        }
    }
}

/// What an upstream answer means for this profile.
///
/// The axes are deliberately distinct (`docs/engine/PROVIDER_ONBOARDING.md` §8.4): the HARD
/// axis is provider verdicts only (a 429 rate wall, an explicit quota exhaustion visible via
/// the billing probe zeroing); the SOFT axis is everything we inferred (a 401/403 after a
/// successful mint, a CAPTCHA-required pre-check answer, a transport fault, a timeout, a
/// failed probe). Only the hard axis may deny a request on its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamVerdict {
    Ok,
    /// Hard provider wall: HTTP 429. No `Retry-After` contract is documented for this
    /// provider, so the cool-down is the caller's bounded guess, and the transport budget is
    /// NOT spent on a provider verdict.
    RateLimitedHard,
    /// Hard provider verdict: explicit quota exhaustion — observed as the billing probe's
    /// remaining credits zeroing below the reserve, or an upstream answer the gateway has
    /// proven to mean "no credits". Never emitted by [`classify_status`]: without an official
    /// error reference no HTTP status alone can carry this meaning, so only the gateway's
    /// quota evidence constructs it.
    QuotaExhausted,
    /// The session was refused (HTTP 401/403 after a successful JWT mint). SOFT axis: one
    /// rejection is never a verdict — the mint succeeded, so the refusal may belong to the
    /// request path. Bounded soft quarantine + probe, exponential backoff, reset on proven
    /// success.
    AuthRefused,
    /// The hCaptcha pre-check answered `required: true` (`oss-hypothesis`, manifest §4). No
    /// CAPTCHA solving exists by design, so the profile soft-cools and the attempt rotates.
    /// Never emitted by [`classify_status`]: the gateway constructs it from the pre-check.
    CaptchaRequired,
    /// A plain 408/5xx or a transport failure before generation creation. Bounded rotation
    /// within the transport budget.
    Transport,
    /// Deterministic client/request error: another 4xx the provider attributes to the request
    /// itself. Neither rotate nor blame the profile.
    ClientError,
    /// A successful HTTP status whose body is not the documented success shape, or a body
    /// that does not parse: a contract change, fail closed. Never rotated as transport — the
    /// answer may already have created the generation, and the wire needs review first.
    Protocol,
}

/// Classify an upstream answer for the Suno plane.
///
/// There is no documented business-code layer (`unknown`, manifest §4.1), so classification
/// is HTTP-only and conservative: a 2xx is `Ok` at the transport level (the parse layer still
/// fails closed on a lying body), 401/403 are the soft auth axis, 429 is the hard rate wall,
/// 408/5xx are transport, and every other 4xx is a deterministic client error.
pub fn classify_status(status: u16) -> UpstreamVerdict {
    match status {
        200..=299 => UpstreamVerdict::Ok,
        401 | 403 => UpstreamVerdict::AuthRefused,
        429 => UpstreamVerdict::RateLimitedHard,
        408 | 500..=599 => UpstreamVerdict::Transport,
        _ => UpstreamVerdict::ClientError,
    }
}

/// Whether a verdict permits trying another profile BEFORE the money boundary (a successful
/// generation creation). A deterministic client error rotates nowhere: the next profile would
/// fail identically. `Protocol` never rotates either: a changed wire under a paid boundary
/// needs review, not a second attempt that might double-create. After a successful creation
/// nothing rotates — encoded by the caller through the attempt phase, not by this predicate.
pub fn may_rotate(verdict: UpstreamVerdict) -> bool {
    matches!(
        verdict,
        UpstreamVerdict::RateLimitedHard
            | UpstreamVerdict::QuotaExhausted
            | UpstreamVerdict::AuthRefused
            | UpstreamVerdict::CaptchaRequired
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
/// this. There is deliberately **no `base_url` here**: the provider has one platform with fixed
/// official hosts (constants in `suno_credential`), so a fleet-wide override could only smuggle
/// a session to a foreign origin.
#[derive(Clone, Debug)]
pub struct SunoTransportConfig {
    pub request_timeout: Duration,
}

/// The host pair a profile talks to. Production constructs exactly one value — the fixed
/// official hosts — via [`SunoHosts::official`]; the gateway's tests inject a loopback mock
/// through the crate-internal constructor. This is NOT an operator knob: no env or config path
/// reaches it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SunoHosts {
    pub auth_base: String,
    pub api_base: String,
}

impl SunoHosts {
    pub(crate) fn official() -> Self {
        Self {
            auth_base: SUNO_AUTH_BASE_URL.to_string(),
            api_base: SUNO_API_BASE_URL.to_string(),
        }
    }

    #[cfg(test)]
    pub(crate) fn loopback(auth_base: String, api_base: String) -> Self {
        Self { auth_base, api_base }
    }

    fn auth(&self, path: &str) -> String {
        format!("{}{}", self.auth_base.trim_end_matches('/'), path)
    }

    fn api(&self, path: &str) -> String {
        format!("{}{}", self.api_base.trim_end_matches('/'), path)
    }

    pub(crate) fn session_discovery_url(&self) -> String {
        format!("{}?{CLERK_QUERY}", self.auth(SUNO_CLIENT_PATH))
    }

    pub(crate) fn jwt_mint_url(&self, session_id: &str) -> String {
        format!(
            "{}?{CLERK_QUERY}",
            self.auth(&SUNO_SESSION_TOKENS_PATH.replace("{sid}", session_id))
        )
    }

    pub(crate) fn captcha_check_url(&self) -> String {
        self.api(SUNO_CAPTCHA_CHECK_PATH)
    }

    pub(crate) fn generate_song_url(&self) -> String {
        self.api(SUNO_GENERATE_PATH)
    }

    pub(crate) fn generate_concat_url(&self) -> String {
        self.api(SUNO_CONCAT_PATH)
    }

    pub(crate) fn lyrics_create_url(&self) -> String {
        self.api(SUNO_LYRICS_PATH)
    }

    pub(crate) fn lyrics_status_url(&self, lyrics_id: &str) -> String {
        self.api(&format!("{SUNO_LYRICS_PATH}{lyrics_id}"))
    }

    pub(crate) fn stems_url(&self, song_id: &str) -> String {
        self.api(&format!("{SUNO_STEMS_PATH}/{song_id}"))
    }

    pub(crate) fn feed_url(&self, clip_ids: &[&str]) -> String {
        self.api(&format!("{SUNO_FEED_PATH}?ids={}", clip_ids.join(",")))
    }

    pub(crate) fn clip_url(&self, clip_id: &str) -> String {
        self.api(&format!("{SUNO_CLIP_PATH}/{clip_id}"))
    }

    pub(crate) fn billing_info_url(&self) -> String {
        self.api(SUNO_BILLING_INFO_PATH)
    }
}

impl Default for SunoTransportConfig {
    fn default() -> Self {
        Self {
            // Generation creation and polls are short JSON RPCs; the long wait happens between
            // polls, never inside one HTTP read. Artifact downloads get their own bounded reads.
            request_timeout: Duration::from_secs(60),
        }
    }
}

// ── URL builders on the fixed official hosts (no override exists by design) ──

/// Clerk session-discovery URL on the auth host (`GET`, `oss-hypothesis`, manifest §2).
pub fn session_discovery_url() -> String {
    format!("{SUNO_AUTH_BASE_URL}{SUNO_CLIENT_PATH}?{CLERK_QUERY}")
}

/// Clerk JWT-mint URL for one discovered session id (`POST`, `oss-hypothesis`, manifest §2).
/// The session id is provider-issued and bounded by the caller before substitution.
pub fn jwt_mint_url(session_id: &str) -> String {
    format!(
        "{SUNO_AUTH_BASE_URL}{}?{CLERK_QUERY}",
        SUNO_SESSION_TOKENS_PATH.replace("{sid}", session_id)
    )
}

/// hCaptcha gate probe URL (`POST {"ctype":"generation"}`, `oss-hypothesis`, manifest §4).
pub fn captcha_check_url() -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_CAPTCHA_CHECK_PATH}")
}

/// Song-generation URL (`POST /api/generate/v2/`, `oss-hypothesis`, manifest §4).
pub fn generate_song_url() -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_GENERATE_PATH}")
}

/// Extend/concat generation URL (`POST /api/generate/concat/v2/`, `oss-hypothesis`,
/// manifest §4).
pub fn generate_concat_url() -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_CONCAT_PATH}")
}

/// Lyrics creation URL (`POST /api/generate/lyrics/`, `oss-hypothesis`, manifest §4).
pub fn lyrics_create_url() -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_LYRICS_PATH}")
}

/// Lyrics status URL for a provider-issued lyrics id (`GET`, `oss-hypothesis`, manifest §4).
pub fn lyrics_status_url(lyrics_id: &str) -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_LYRICS_PATH}{lyrics_id}")
}

/// Stems split URL for one song (`POST /api/edit/stems/{song_id}`, `oss-hypothesis`,
/// manifest §4). The song id is provider-issued and bounded by the caller before substitution.
pub fn stems_url(song_id: &str) -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_STEMS_PATH}/{song_id}")
}

/// Feed/status poll URL for a set of clip ids (`GET /api/feed/v2?ids=…`, `oss-hypothesis`,
/// manifest §4).
pub fn feed_url(clip_ids: &[&str]) -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_FEED_PATH}?ids={}", clip_ids.join(","))
}

/// Clip metadata URL (`GET /api/clip/{clipId}`, `oss-hypothesis`, manifest §4).
pub fn clip_url(clip_id: &str) -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_CLIP_PATH}/{clip_id}")
}

/// Native quota URL (`GET /api/billing/info/`, `oss-hypothesis`, manifest §5.2).
pub fn billing_info_url() -> String {
    format!("{SUNO_API_BASE_URL}{SUNO_BILLING_INFO_PATH}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_only_classification_is_conservative() {
        assert_eq!(classify_status(200), UpstreamVerdict::Ok);
        // 401/403 after a successful mint is the SOFT axis, never a verdict on its own.
        assert_eq!(classify_status(401), UpstreamVerdict::AuthRefused);
        assert_eq!(classify_status(403), UpstreamVerdict::AuthRefused);
        // 429 is the documented-conservative hard rate wall.
        assert_eq!(classify_status(429), UpstreamVerdict::RateLimitedHard);
        for status in [408, 500, 502, 503, 504] {
            let verdict = classify_status(status);
            assert_eq!(verdict, UpstreamVerdict::Transport, "status {status}");
            assert!(spends_transport_budget(verdict));
        }
        for status in [400, 404, 413, 422] {
            assert_eq!(classify_status(status), UpstreamVerdict::ClientError);
        }
        assert!(!may_rotate(UpstreamVerdict::ClientError));
        assert!(!spends_transport_budget(UpstreamVerdict::ClientError));
    }

    #[test]
    fn rotation_and_budget_rules_keep_the_axes_distinct() {
        // Hard provider verdicts and soft faults rotate without spending the transport budget:
        // that budget exists for upstream outages, and spending it on a provider verdict would
        // stop the search before a healthy profile is reached.
        for verdict in [
            UpstreamVerdict::RateLimitedHard,
            UpstreamVerdict::QuotaExhausted,
            UpstreamVerdict::AuthRefused,
            UpstreamVerdict::CaptchaRequired,
        ] {
            assert!(may_rotate(verdict), "{verdict:?}");
            assert!(!spends_transport_budget(verdict), "{verdict:?}");
        }
        // A protocol anomaly never rotates into a possible double-create on a paid boundary.
        assert!(!may_rotate(UpstreamVerdict::Protocol));
        assert!(!spends_transport_budget(UpstreamVerdict::Protocol));
    }

    #[test]
    fn quota_exhaustion_and_captcha_are_constructed_not_classified() {
        // Without an official error reference no HTTP status alone may mean "no credits" or
        // "solve a CAPTCHA": only the gateway's quota evidence / pre-check constructs these.
        for status in [200, 400, 401, 403, 409, 422, 429, 500, 503] {
            let verdict = classify_status(status);
            assert_ne!(verdict, UpstreamVerdict::QuotaExhausted, "status {status}");
            assert_ne!(verdict, UpstreamVerdict::CaptchaRequired, "status {status}");
        }
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
    fn the_probe_set_is_billing_only_and_auth_validating() {
        // The closed enum cannot express a generation probe: generation spends credits, so
        // readiness and admission may only ever call the free billing route.
        assert_eq!(ProbeRoute::BillingInfo.path(), "/api/billing/info/");
        assert!(!ProbeRoute::BillingInfo.path().contains("generate"));
        assert_eq!(
            format!("{SUNO_API_BASE_URL}{}", ProbeRoute::BillingInfo.path()),
            "https://studio-api.prod.suno.com/api/billing/info/"
        );
    }

    #[test]
    fn urls_are_built_on_the_fixed_official_hosts() {
        assert_eq!(
            session_discovery_url(),
            "https://auth.suno.com/v1/client?__clerk_api_version=2025-11-10&_clerk_js_version=5.117.0"
        );
        assert_eq!(
            jwt_mint_url("sess_1"),
            "https://auth.suno.com/v1/client/sessions/sess_1/tokens?__clerk_api_version=2025-11-10&_clerk_js_version=5.117.0"
        );
        assert_eq!(
            captcha_check_url(),
            "https://studio-api.prod.suno.com/api/c/check"
        );
        assert_eq!(
            generate_song_url(),
            "https://studio-api.prod.suno.com/api/generate/v2/"
        );
        assert_eq!(
            generate_concat_url(),
            "https://studio-api.prod.suno.com/api/generate/concat/v2/"
        );
        assert_eq!(
            lyrics_create_url(),
            "https://studio-api.prod.suno.com/api/generate/lyrics/"
        );
        assert_eq!(
            lyrics_status_url("lyr_1"),
            "https://studio-api.prod.suno.com/api/generate/lyrics/lyr_1"
        );
        assert_eq!(
            stems_url("song_1"),
            "https://studio-api.prod.suno.com/api/edit/stems/song_1"
        );
        assert_eq!(
            feed_url(&["c1", "c2"]),
            "https://studio-api.prod.suno.com/api/feed/v2?ids=c1,c2"
        );
        assert_eq!(
            clip_url("c1"),
            "https://studio-api.prod.suno.com/api/clip/c1"
        );
        assert_eq!(
            billing_info_url(),
            "https://studio-api.prod.suno.com/api/billing/info/"
        );
    }
}
