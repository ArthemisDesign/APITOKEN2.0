//! Per-profile transport to the KIMI (Kimi Code) subscription plane.
//!
//! Contract: `docs/engine/KIMI_PROVIDER.md` §4 and §4.2. Three provider facts shape this module:
//!
//! * **The subscription serves an Anthropic-compatible endpoint.** The engine's native protocol is
//!   forwarded unchanged, so there is no translation layer here — only auth, egress and error
//!   classification.
//! * **The refresh family rotates.** Every refresh invalidates its predecessor, so refreshes are
//!   single-flight per profile: the winner re-seals before releasing the lock, and losers reuse
//!   the winner's token instead of spending a token that is already dead.
//! * **`/v1/models` is ungated.** It answers 200 for an invalid key while generation then returns
//!   403, so it must never be used as a health probe. [`ProbeRoute`] encodes that as a type rather
//!   than a comment.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use kimi_credential::{KimiCredential, KIMI_CODE_BASE_URL, KIMI_IDENTITY_PATH, KIMI_USAGE_PATH};
use tokio::sync::Mutex;

/// How a request authenticates. The official CLI proves `Bearer`; Claude Code's documented
/// `ANTHROPIC_API_KEY` path implies `x-api-key`. Which one the Anthropic route accepts is an open
/// `unknown` in the manifest, so it stays configurable rather than guessed in code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthScheme {
    /// Proven by the official MIT-licensed Kimi Code CLI for `/me`, `/usages` and chat.
    Bearer,
    /// Documented for the Claude Code integration path.
    ApiKeyHeader,
}

impl AuthScheme {
    pub fn header_name(self) -> &'static str {
        match self {
            Self::Bearer => "authorization",
            Self::ApiKeyHeader => "x-api-key",
        }
    }

    pub fn header_value(self, token: &str) -> String {
        match self {
            Self::Bearer => format!("Bearer {token}"),
            Self::ApiKeyHeader => token.to_string(),
        }
    }
}

/// Endpoints the plane is allowed to call. Deliberately a closed set: a new route must be added
/// here, which makes an ungated or unpriced endpoint a visible decision.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeRoute {
    /// Non-billable identity check. Valid readiness probe.
    Identity,
    /// Non-billable quota read. Valid readiness probe and the only source of quota.
    Usage,
}

impl ProbeRoute {
    pub fn path(self) -> &'static str {
        match self {
            Self::Identity => KIMI_IDENTITY_PATH,
            Self::Usage => KIMI_USAGE_PATH,
        }
    }
}

/// What an upstream status means for this profile.
///
/// The provider itself separates "engine overloaded, retrying helps" from "account quota, retrying
/// is pointless", which is exactly the split between the transport and quota health axes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpstreamVerdict {
    Ok,
    /// Auth failed, or the plan does not grant the requested capability. First occurrence forces a
    /// refresh and one same-profile retry; a repeat quarantines the profile.
    Auth,
    /// Membership verification hiccup (402). Documented as usually temporary, so it is a transport
    /// fault rather than an account death sentence.
    MembershipTemporary,
    /// Quota wall. Cool this profile until reset; rotation must not spend transport budget.
    QuotaExhausted,
    /// Provider-side overload or transport failure. Bounded rotation.
    Transport,
    /// Deterministic client error. Neither rotate nor blame the profile.
    ClientError,
}

/// Classify an upstream status for the KIMI plane.
///
/// 403 is deliberately conservative. The provider uses it both for "your tier lacks this
/// capability" and for "your quota is exhausted", and the manifest records that the two are not
/// reliably distinguishable without parsing the body. Treating an ambiguous 403 as a quota wall
/// takes the profile out of rotation until reset instead of marking a healthy paid subscription
/// dead, which is the cheaper mistake.
pub fn classify_status(status: u16) -> UpstreamVerdict {
    match status {
        200..=299 => UpstreamVerdict::Ok,
        401 => UpstreamVerdict::Auth,
        402 => UpstreamVerdict::MembershipTemporary,
        403 => UpstreamVerdict::QuotaExhausted,
        408 | 409 | 425 | 429 => UpstreamVerdict::Transport,
        500..=599 => UpstreamVerdict::Transport,
        _ => UpstreamVerdict::ClientError,
    }
}

/// Whether a verdict permits trying another profile.
///
/// Quota and auth are the account's fault, so they rotate without consuming the transport budget
/// reserved for real upstream outages. A deterministic client error rotates nowhere: the next
/// profile would fail identically.
pub fn spends_transport_budget(verdict: UpstreamVerdict) -> bool {
    matches!(
        verdict,
        UpstreamVerdict::Transport | UpstreamVerdict::MembershipTemporary
    )
}

pub fn may_rotate(verdict: UpstreamVerdict) -> bool {
    !matches!(verdict, UpstreamVerdict::Ok | UpstreamVerdict::ClientError)
}

/// Runtime configuration for the plane. `forward` never reads the environment; `server` fills this.
#[derive(Clone, Debug)]
pub struct KimiTransportConfig {
    pub base_url: String,
    pub auth_scheme: AuthScheme,
    pub request_timeout: Duration,
    /// How long before expiry a token is refreshed proactively.
    pub refresh_lead: Duration,
}

impl Default for KimiTransportConfig {
    fn default() -> Self {
        Self {
            base_url: KIMI_CODE_BASE_URL.to_string(),
            auth_scheme: AuthScheme::Bearer,
            request_timeout: Duration::from_secs(120),
            refresh_lead: Duration::from_secs(120),
        }
    }
}

/// Per-profile single-flight refresh locks.
///
/// The provider rotates the refresh family on every exchange, so two concurrent refreshes for one
/// profile would spend the same refresh token twice and kill the subscription. The lock is per
/// profile, never global: one profile refreshing must not stall traffic on the others.
#[derive(Default)]
pub struct RefreshLocks {
    locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
}

impl RefreshLocks {
    pub async fn for_profile(&self, profile_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        Arc::clone(
            locks
                .entry(profile_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }
}

/// Whether this credential needs refreshing before use.
pub fn needs_refresh(
    credential: &KimiCredential,
    now_unix: i64,
    config: &KimiTransportConfig,
) -> bool {
    credential.is_expired(now_unix, config.refresh_lead.as_secs() as i64)
}

/// Build the absolute URL for a non-billable probe.
pub fn probe_url(config: &KimiTransportConfig, route: ProbeRoute) -> String {
    format!("{}{}", config.base_url.trim_end_matches('/'), route.path())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kimi_credential::{KimiCredentialKind, KIMI_STATUS_NORMAL};

    fn oauth_credential(expires_at: i64) -> KimiCredential {
        KimiCredential {
            version: 1,
            kind: KimiCredentialKind::Oauth,
            access_token: "access".into(),
            refresh_token: "refresh".into(),
            expires_at,
            scope: "coding".into(),
            subject_id: "u_1".into(),
            plan_name: "Moderato".into(),
            plan_level: 10,
            status: KIMI_STATUS_NORMAL.into(),
            region: "REGION_CN".into(),
            proxy_url: String::new(),
        }
    }

    #[test]
    fn the_probe_route_set_cannot_express_the_ungated_models_endpoint() {
        // /v1/models answers 200 for an invalid key and generation then returns 403, so a health
        // probe built on it reports a dead subscription as healthy. The closed enum makes that
        // impossible to write by accident.
        for route in [ProbeRoute::Identity, ProbeRoute::Usage] {
            assert!(!route.path().contains("models"));
        }
        let config = KimiTransportConfig::default();
        assert_eq!(
            probe_url(&config, ProbeRoute::Identity),
            "https://api.kimi.com/coding/v1/me"
        );
        assert_eq!(
            probe_url(&config, ProbeRoute::Usage),
            "https://api.kimi.com/coding/v1/usages"
        );
    }

    #[test]
    fn quota_and_auth_failures_do_not_spend_the_transport_budget() {
        // That budget exists for real upstream outages. Burning it on an account-level refusal
        // would stop the rotation before a healthy profile is reached.
        assert!(!spends_transport_budget(UpstreamVerdict::QuotaExhausted));
        assert!(!spends_transport_budget(UpstreamVerdict::Auth));
        assert!(spends_transport_budget(UpstreamVerdict::Transport));
        assert!(spends_transport_budget(
            UpstreamVerdict::MembershipTemporary
        ));
    }

    #[test]
    fn an_ambiguous_403_is_treated_as_a_quota_wall_not_a_dead_account() {
        // The provider uses 403 both for "tier lacks capability" and "quota exhausted". Cooling
        // until reset is recoverable; marking a paid subscription dead is not.
        assert_eq!(classify_status(403), UpstreamVerdict::QuotaExhausted);
        assert!(may_rotate(UpstreamVerdict::QuotaExhausted));
        assert!(!spends_transport_budget(UpstreamVerdict::QuotaExhausted));
    }

    #[test]
    fn membership_verification_is_temporary_rather_than_fatal() {
        assert_eq!(classify_status(402), UpstreamVerdict::MembershipTemporary);
    }

    #[test]
    fn deterministic_client_errors_neither_rotate_nor_blame_the_profile() {
        for status in [400, 404, 413, 422] {
            assert_eq!(classify_status(status), UpstreamVerdict::ClientError);
            assert!(!may_rotate(UpstreamVerdict::ClientError));
        }
    }

    #[test]
    fn overload_and_network_classes_rotate_within_the_transport_budget() {
        for status in [408, 409, 425, 429, 500, 502, 503, 504] {
            let verdict = classify_status(status);
            assert_eq!(verdict, UpstreamVerdict::Transport, "status {status}");
            assert!(may_rotate(verdict));
            assert!(spends_transport_budget(verdict));
        }
    }

    #[test]
    fn success_never_rotates() {
        for status in [200, 201, 299] {
            assert_eq!(classify_status(status), UpstreamVerdict::Ok);
            assert!(!may_rotate(UpstreamVerdict::Ok));
        }
    }

    #[test]
    fn both_documented_auth_schemes_are_representable_without_guessing() {
        assert_eq!(AuthScheme::Bearer.header_name(), "authorization");
        assert_eq!(AuthScheme::Bearer.header_value("t"), "Bearer t");
        assert_eq!(AuthScheme::ApiKeyHeader.header_name(), "x-api-key");
        assert_eq!(AuthScheme::ApiKeyHeader.header_value("t"), "t");
        // Bearer is the default because it is the one proven by the official client.
        assert_eq!(
            KimiTransportConfig::default().auth_scheme,
            AuthScheme::Bearer
        );
    }

    #[test]
    fn refresh_is_proactive_so_a_turn_does_not_start_on_a_dying_token() {
        let config = KimiTransportConfig::default();
        let credential = oauth_credential(1_000_000);
        assert!(!needs_refresh(&credential, 1_000_000 - 300, &config));
        assert!(needs_refresh(&credential, 1_000_000 - 60, &config));
        assert!(needs_refresh(&credential, 1_000_000, &config));
    }

    #[test]
    fn a_console_key_never_needs_refreshing() {
        let mut credential = oauth_credential(0);
        credential.kind = KimiCredentialKind::ConsoleKey;
        credential.refresh_token = String::new();
        assert!(!needs_refresh(
            &credential,
            i64::MAX,
            &KimiTransportConfig::default()
        ));
    }

    #[tokio::test]
    async fn refresh_locks_are_per_profile_and_stable() {
        let locks = RefreshLocks::default();
        let first = locks.for_profile("kimi-01").await;
        let same = locks.for_profile("kimi-01").await;
        let other = locks.for_profile("kimi-02").await;
        // The same profile must share one lock, or two refreshes would spend the same rotating
        // refresh token and kill the subscription.
        assert!(Arc::ptr_eq(&first, &same));
        // Distinct profiles must not share one, or a single refresh would stall the whole fleet.
        assert!(!Arc::ptr_eq(&first, &other));
    }

    #[tokio::test]
    async fn a_second_refresh_waits_for_the_winner_instead_of_racing_it() {
        let locks = RefreshLocks::default();
        let lock = locks.for_profile("kimi-01").await;
        let held = lock.clone().lock_owned().await;
        let contender = locks.for_profile("kimi-01").await;
        assert!(contender.try_lock().is_err(), "loser must wait");
        drop(held);
        assert!(contender.try_lock().is_ok());
    }
}
