//! Suno (suno.com) subscription session-pool plane configuration.
//!
//! `forward` never reads the environment; `server` fills this and hands it down. Contract:
//! `docs/engine/SUNO_PROVIDER.md` §0 and §4.
//!
//! The plane is **off by default**. Suno is deliberately backend-only: no public catalogue,
//! no router namespace, no pricing surface (the ToS prohibits resale and no partner agreement
//! exists). Enabling it is an explicit operator action, not a consequence of the binary
//! containing the code.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use suno_credential::CredentialKeyring;

use super::transport::{ProbeRoute, SunoTransportConfig};

/// Everything the plane needs to run. Absent means the plane is disabled.
#[derive(Clone)]
pub struct SunoPlaneConfig {
    /// Roster root: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub roster_dir: PathBuf,
    pub keyring: CredentialKeyring,
    pub transport: SunoTransportConfig,
    /// Which non-billable endpoint readiness probes. Always the billing-info route: it is free
    /// and auth-validating, and it is the only machine-readable quota surface this provider has
    /// (manifest §5.2). It is NOT proof of generation capability (open `unknown`, manifest §6).
    pub readiness_probe: ProbeRoute,
    /// How often the free quota poll runs when a profile is idle.
    pub quota_poll_interval: Duration,
    /// Root the detached drain downloads generated audio into: `<dir>/<request_id>/<name>`.
    /// Upstream media URLs are never exposed to the customer (manifest §4), so artifacts live
    /// in OUR storage and customer-facing delivery serves only from this root.
    pub artifact_dir: PathBuf,
}

impl std::fmt::Debug for SunoPlaneConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SunoPlaneConfig")
            .field("roster_dir", &self.roster_dir)
            .field("transport", &self.transport)
            .field("readiness_probe", &self.readiness_probe)
            .field("artifact_dir", &self.artifact_dir)
            .field("keyring", &"REDACTED")
            .finish()
    }
}

/// Raw operator input, already read from the environment by `server`.
///
/// There is deliberately **no `base_url` field**: the provider has one platform with fixed
/// official hosts (`suno_credential::SUNO_AUTH_BASE_URL` / `SUNO_API_BASE_URL`), so a host
/// override knob could only smuggle a session to a foreign origin. `server` refuses any
/// `CLAUDE_API_SUNO_BASE_URL`-style key rather than letting it look half-supported.
pub struct SunoPlaneInput {
    pub enabled: bool,
    pub roster_dir: String,
    pub credential_keys: Option<String>,
    /// The kid the Auth Bot seals new envelopes under. The runtime only ever opens envelopes
    /// (the envelope itself names its key id), so this is a consistency guard: when set, it
    /// must exist in the keyring, catching a mismatched bot/engine keyring pair at boot.
    pub credential_active_kid: Option<String>,
    pub quota_poll_secs: u64,
    pub artifact_dir: String,
}

impl Default for SunoPlaneInput {
    fn default() -> Self {
        Self {
            enabled: false,
            roster_dir: "/srv/claude-api/data/suno".into(),
            credential_keys: None,
            credential_active_kid: None,
            quota_poll_secs: 300,
            artifact_dir: "/srv/claude-api/data/suno/artifacts".into(),
        }
    }
}

/// Build the plane configuration, or `None` when it is disabled.
///
/// Every rejection is a refusal to start the plane rather than a warning, because a
/// half-configured provider that accepts traffic is worse than one that is simply off.
pub fn build(input: &SunoPlaneInput) -> Result<Option<SunoPlaneConfig>> {
    if !input.enabled {
        return Ok(None);
    }
    let roster_dir = PathBuf::from(&input.roster_dir);
    if !roster_dir.is_absolute() {
        bail!("CLAUDE_API_SUNO_ROSTER_DIR must be an absolute path");
    }
    let artifact_dir = PathBuf::from(&input.artifact_dir);
    if !artifact_dir.is_absolute() {
        bail!("CLAUDE_API_SUNO_ARTIFACT_DIR must be an absolute path");
    }
    let Some(keys) = input.credential_keys.as_ref().filter(|k| !k.is_empty()) else {
        // Without the keyring no envelope can be opened, so every profile would fail on first use.
        bail!("CLAUDE_API_SUNO_CREDENTIAL_KEYS is required for the encrypted Suno roster");
    };
    let keyring = CredentialKeyring::parse(keys)?;
    if let Some(kid) = input
        .credential_active_kid
        .as_ref()
        .filter(|kid| !kid.is_empty())
    {
        if !keyring.contains(kid) {
            // The kid the Auth Bot seals under is missing from this keyring: freshly published
            // profiles would be undecryptable here. Fail at boot, not at the first acquisition.
            bail!("CLAUDE_API_SUNO_CREDENTIAL_ACTIVE_KID is absent from the keyring");
        }
    }
    if input.quota_poll_secs == 0 {
        bail!("CLAUDE_API_SUNO_QUOTA_POLL_SECS must be positive");
    }

    Ok(Some(SunoPlaneConfig {
        roster_dir,
        keyring,
        transport: SunoTransportConfig::default(),
        // The billing-info route is the only correct probe: it is free, it authenticates the
        // session (a 401/403 means the session is dead), and unlike generation it never spends
        // subscription credits.
        readiness_probe: ProbeRoute::BillingInfo,
        quota_poll_interval: Duration::from_secs(input.quota_poll_secs),
        artifact_dir,
    }))
}

/// Why the plane is not ready to serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotReady {
    /// No profile in the roster authenticated.
    NoLiveProfile,
    /// Calibration evidence is undelivered, so measured capacity is not current.
    DeliveryDegraded,
}

/// Readiness for one candidate process.
///
/// What readiness checks, end to end: the roster directory is readable and every sealed
/// profile opens against the keyring (roster load), each profile passes a free billing probe on
/// the fixed business host through its pinned egress (auth + quota evidence), and the
/// turn-evidence writer is draining (persistence). Quota exhaustion is deliberately NOT a
/// readiness failure: it is a capacity state the monthly refill clears, and refusing to start
/// on it would take a healthy fleet out of service. One usable subscription is real capacity —
/// there is no arbitrary minimum fleet.
pub fn readiness(live_profiles: usize, persistence_ok: bool) -> Result<(), NotReady> {
    if live_profiles == 0 {
        return Err(NotReady::NoLiveProfile);
    }
    if !persistence_ok {
        return Err(NotReady::DeliveryDegraded);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input() -> SunoPlaneInput {
        SunoPlaneInput {
            enabled: true,
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            ..SunoPlaneInput::default()
        }
    }

    #[test]
    fn the_plane_is_off_unless_explicitly_enabled() {
        // Backend-only means shipping the code must not turn the provider on.
        let disabled = SunoPlaneInput::default();
        assert!(!disabled.enabled);
        assert!(build(&disabled).unwrap().is_none());
    }

    #[test]
    fn a_disabled_plane_ignores_an_otherwise_invalid_configuration() {
        // An operator must not be blocked from booting by settings for a provider they are not
        // running.
        let broken = SunoPlaneInput {
            enabled: false,
            roster_dir: "relative/path".into(),
            artifact_dir: "relative/artifacts".into(),
            quota_poll_secs: 0,
            ..SunoPlaneInput::default()
        };
        assert!(build(&broken).unwrap().is_none());
    }

    #[test]
    fn readiness_probes_billing_info_and_can_never_probe_generation() {
        // Creating a generation spends subscription credits; the billing route is free and
        // still authenticates the session (a 401/403 for a dead session).
        let config = build(&input()).unwrap().unwrap();
        assert_eq!(config.readiness_probe, ProbeRoute::BillingInfo);
        assert!(!config.readiness_probe.path().contains("generate"));
        assert_eq!(config.readiness_probe.path(), "/api/billing/info/");
    }

    #[test]
    fn the_defaults_match_the_documented_env_contract() {
        let defaults = SunoPlaneInput::default();
        assert_eq!(defaults.roster_dir, "/srv/claude-api/data/suno");
        assert_eq!(defaults.artifact_dir, "/srv/claude-api/data/suno/artifacts");
        assert!(defaults.quota_poll_secs > 0);
    }

    #[test]
    fn a_missing_keyring_refuses_to_start_the_plane() {
        // Without it every envelope fails to open, so every profile would die on first use.
        let mut without = input();
        without.credential_keys = None;
        assert!(build(&without).is_err());
        without.credential_keys = Some(String::new());
        assert!(build(&without).is_err());
    }

    #[test]
    fn the_active_kid_must_exist_in_the_keyring_when_set() {
        let mut config = input();
        config.credential_active_kid = Some("a1".into());
        assert!(build(&config).is_ok());
        config.credential_active_kid = Some("zz".into());
        assert!(build(&config).is_err());
    }

    #[test]
    fn relative_roster_and_artifact_directories_are_refused() {
        let mut relative = input();
        relative.roster_dir = "data/suno".into();
        assert!(build(&relative).is_err());
        let mut relative = input();
        relative.artifact_dir = "data/artifacts".into();
        assert!(build(&relative).is_err());
    }

    #[test]
    fn a_zero_poll_interval_is_refused_rather_than_becoming_a_busy_loop() {
        let mut config = input();
        config.quota_poll_secs = 0;
        assert!(build(&config).is_err());
    }

    #[test]
    fn config_debug_never_prints_the_keyring() {
        let config = build(&input()).unwrap().unwrap();
        let rendered = format!("{config:?}");
        assert!(!rendered.contains("1111"));
        assert!(rendered.contains("REDACTED"));
    }

    #[test]
    fn one_usable_subscription_is_enough_to_be_ready() {
        // No arbitrary minimum fleet: a single working profile is real capacity.
        assert_eq!(readiness(1, true), Ok(()));
    }

    #[test]
    fn zero_live_profiles_and_degraded_delivery_both_block_readiness() {
        assert_eq!(readiness(0, true), Err(NotReady::NoLiveProfile));
        // Undelivered evidence means measured capacity is not current, so it must not be sold.
        assert_eq!(readiness(3, false), Err(NotReady::DeliveryDegraded));
    }
}
