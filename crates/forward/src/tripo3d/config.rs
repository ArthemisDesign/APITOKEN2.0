//! Tripo3D (VAST / Holymolly) prepaid API plane configuration.
//!
//! `forward` never reads the environment; `server` fills this and hands it down. Contract:
//! `docs/engine/TRIPO3D_PROVIDER.md` §0 and §4.
//!
//! The plane is **off by default**. Tripo3D is deliberately backend-only: no public catalogue,
//! no router namespace, no pricing surface. Enabling it is an explicit operator action, not a
//! consequence of the binary containing the code.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use tripo3d_credential::CredentialKeyring;

use super::transport::{AuthScheme, ProbeRoute, Tripo3dTransportConfig};

/// Everything the plane needs to run. Absent means the plane is disabled.
#[derive(Clone)]
pub struct Tripo3dPlaneConfig {
    /// Roster root: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub roster_dir: PathBuf,
    pub keyring: CredentialKeyring,
    pub transport: Tripo3dTransportConfig,
    /// Which non-billable endpoint readiness probes. Always the balance route: it is free and
    /// auth-validating, and it is the only machine-readable quota surface this provider has
    /// (manifest §5.2). It is NOT proof of generation capability (open `unknown`, manifest §6).
    pub readiness_probe: ProbeRoute,
    /// How often the free balance poll runs when a profile is idle.
    pub balance_poll_interval: Duration,
    /// Root the detached drain downloads task artifacts into: `<dir>/<request_id>/<name>`.
    /// Upstream result URLs expire in ≤60 s (manifest §5.4), so artifacts live in OUR storage
    /// and customer-facing delivery never exposes the upstream signed URL.
    pub artifact_dir: PathBuf,
}

impl std::fmt::Debug for Tripo3dPlaneConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Tripo3dPlaneConfig")
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
/// There is deliberately **no `base_url` field**: the platform origin is per-profile (global
/// and CN keys are not interchangeable, `docs/engine/TRIPO3D_PROVIDER.md` §2) and lives inside
/// the sealed credential, so the fleet has nothing to override.
pub struct Tripo3dPlaneInput {
    pub enabled: bool,
    pub roster_dir: String,
    pub credential_keys: Option<String>,
    /// The kid the Auth Bot seals new envelopes under. The runtime only ever opens envelopes
    /// (the envelope itself names its key id), so this is a consistency guard: when set, it
    /// must exist in the keyring, catching a mismatched bot/engine keyring pair at boot.
    pub credential_active_kid: Option<String>,
    pub balance_poll_secs: u64,
    pub artifact_dir: String,
}

impl Default for Tripo3dPlaneInput {
    fn default() -> Self {
        Self {
            enabled: false,
            roster_dir: "/srv/claude-api/data/tripo3d".into(),
            credential_keys: None,
            credential_active_kid: None,
            balance_poll_secs: 300,
            artifact_dir: "/srv/claude-api/data/tripo3d/artifacts".into(),
        }
    }
}

/// Build the plane configuration, or `None` when it is disabled.
///
/// Every rejection is a refusal to start the plane rather than a warning, because a
/// half-configured provider that accepts traffic is worse than one that is simply off.
pub fn build(input: &Tripo3dPlaneInput) -> Result<Option<Tripo3dPlaneConfig>> {
    if !input.enabled {
        return Ok(None);
    }
    let roster_dir = PathBuf::from(&input.roster_dir);
    if !roster_dir.is_absolute() {
        bail!("CLAUDE_API_TRIPO3D_ROSTER_DIR must be an absolute path");
    }
    let artifact_dir = PathBuf::from(&input.artifact_dir);
    if !artifact_dir.is_absolute() {
        bail!("CLAUDE_API_TRIPO3D_ARTIFACT_DIR must be an absolute path");
    }
    let Some(keys) = input.credential_keys.as_ref().filter(|k| !k.is_empty()) else {
        // Without the keyring no envelope can be opened, so every profile would fail on first use.
        bail!("CLAUDE_API_TRIPO3D_CREDENTIAL_KEYS is required for the encrypted Tripo3D roster");
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
            bail!("CLAUDE_API_TRIPO3D_CREDENTIAL_ACTIVE_KID is absent from the keyring");
        }
    }
    if input.balance_poll_secs == 0 {
        bail!("CLAUDE_API_TRIPO3D_BALANCE_POLL_SECS must be positive");
    }

    Ok(Some(Tripo3dPlaneConfig {
        roster_dir,
        keyring,
        transport: Tripo3dTransportConfig {
            auth_scheme: AuthScheme::Bearer,
            ..Tripo3dTransportConfig::default()
        },
        // The balance route is the only correct probe: it is free, it authenticates (an invalid
        // key is a documented 401), and unlike task creation it never spends prepaid credits.
        readiness_probe: ProbeRoute::Balance,
        balance_poll_interval: Duration::from_secs(input.balance_poll_secs),
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
/// profile opens against the keyring (roster load), each profile passes a free balance probe on
/// its own platform origin (auth + capacity evidence), and the turn-evidence writer is
/// draining (persistence). Balance exhaustion is deliberately NOT a readiness failure: it is a
/// capacity state a top-up clears, and refusing to start on it would take a healthy fleet out
/// of service. One usable subscription is real capacity — there is no arbitrary minimum fleet.
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

    fn input() -> Tripo3dPlaneInput {
        Tripo3dPlaneInput {
            enabled: true,
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            ..Tripo3dPlaneInput::default()
        }
    }

    #[test]
    fn the_plane_is_off_unless_explicitly_enabled() {
        // Backend-only means shipping the code must not turn the provider on.
        let disabled = Tripo3dPlaneInput::default();
        assert!(!disabled.enabled);
        assert!(build(&disabled).unwrap().is_none());
    }

    #[test]
    fn a_disabled_plane_ignores_an_otherwise_invalid_configuration() {
        // An operator must not be blocked from booting by settings for a provider they are not
        // running.
        let broken = Tripo3dPlaneInput {
            enabled: false,
            roster_dir: "relative/path".into(),
            artifact_dir: "relative/artifacts".into(),
            balance_poll_secs: 0,
            ..Tripo3dPlaneInput::default()
        };
        assert!(build(&broken).unwrap().is_none());
    }

    #[test]
    fn readiness_probes_balance_and_can_never_probe_task_creation() {
        // Creating a task spends prepaid credits; the balance route is free and still
        // authenticates the key (a documented 401 for an invalid key).
        let config = build(&input()).unwrap().unwrap();
        assert_eq!(config.readiness_probe, ProbeRoute::Balance);
        assert!(!config.readiness_probe.path().contains("task"));
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
        relative.roster_dir = "data/tripo3d".into();
        assert!(build(&relative).is_err());
        let mut relative = input();
        relative.artifact_dir = "data/artifacts".into();
        assert!(build(&relative).is_err());
    }

    #[test]
    fn a_zero_poll_interval_is_refused_rather_than_becoming_a_busy_loop() {
        let mut config = input();
        config.balance_poll_secs = 0;
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
