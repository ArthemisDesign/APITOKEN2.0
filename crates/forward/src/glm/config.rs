//! GLM (Zhipu AI / Z.ai) Coding Plan plane configuration.
//!
//! `forward` never reads the environment; `server` fills this and hands it down. Contract:
//! `docs/engine/GLM_PROVIDER.md` §0 and §4.
//!
//! The plane is **off by default**. GLM is deliberately backend-only: no public catalogue, no
//! router namespace, no pricing surface. Enabling it is an explicit operator action, not a
//! consequence of the binary containing the code.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use glm_credential::CredentialKeyring;

use super::transport::{AuthScheme, GlmTransportConfig, ProbeRoute};

/// Everything the plane needs to run. Absent means the plane is disabled.
#[derive(Clone)]
pub struct GlmPlaneConfig {
    /// Roster root: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub roster_dir: PathBuf,
    pub keyring: CredentialKeyring,
    pub transport: GlmTransportConfig,
    /// Which non-billable endpoint readiness probes. Always the quota route: it is free and
    /// auth-validating, and it is the only machine-readable surface this provider has. It is
    /// NOT proof of generation capability (open `unknown`, manifest §6).
    pub readiness_probe: ProbeRoute,
    /// How often the free quota poll runs when a profile is idle.
    pub quota_poll_interval: Duration,
}

impl std::fmt::Debug for GlmPlaneConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GlmPlaneConfig")
            .field("roster_dir", &self.roster_dir)
            .field("transport", &self.transport)
            .field("readiness_probe", &self.readiness_probe)
            .field("keyring", &"REDACTED")
            .finish()
    }
}

/// Raw operator input, already read from the environment by `server`.
///
/// There is deliberately **no `base_url` field**: the console origin is per-profile (int and
/// CN keys are not interchangeable, `docs/engine/GLM_PROVIDER.md` §2) and lives inside the
/// sealed credential, so the fleet has nothing to override.
pub struct GlmPlaneInput {
    pub enabled: bool,
    pub roster_dir: String,
    pub credential_keys: Option<String>,
    pub auth_scheme: String,
    pub quota_poll_secs: u64,
}

impl Default for GlmPlaneInput {
    fn default() -> Self {
        Self {
            enabled: false,
            roster_dir: "/srv/claude-api/data/glm".into(),
            credential_keys: None,
            auth_scheme: "bearer".into(),
            quota_poll_secs: 300,
        }
    }
}

/// Build the plane configuration, or `None` when it is disabled.
///
/// Every rejection is a refusal to start the plane rather than a warning, because a
/// half-configured provider that accepts traffic is worse than one that is simply off.
pub fn build(input: &GlmPlaneInput) -> Result<Option<GlmPlaneConfig>> {
    if !input.enabled {
        return Ok(None);
    }
    let roster_dir = PathBuf::from(&input.roster_dir);
    if !roster_dir.is_absolute() {
        bail!("CLAUDE_API_GLM_ROSTER_DIR must be an absolute path");
    }
    let Some(keys) = input.credential_keys.as_ref().filter(|k| !k.is_empty()) else {
        // Without the keyring no envelope can be opened, so every profile would fail on first use.
        bail!("CLAUDE_API_GLM_CREDENTIAL_KEYS is required for the encrypted GLM roster");
    };
    let keyring = CredentialKeyring::parse(keys)?;

    let auth_scheme = match input.auth_scheme.to_ascii_lowercase().as_str() {
        "bearer" => AuthScheme::Bearer,
        // `x-api-key` acceptance on the Anthropic route is an open unknown (manifest §4), so
        // nothing else is selectable until live evidence proves it.
        other => bail!("unsupported CLAUDE_API_GLM_AUTH_SCHEME: {other}"),
    };
    if input.quota_poll_secs == 0 {
        bail!("CLAUDE_API_GLM_QUOTA_POLL_SECS must be positive");
    }

    Ok(Some(GlmPlaneConfig {
        roster_dir,
        keyring,
        transport: GlmTransportConfig {
            auth_scheme,
            ..GlmTransportConfig::default()
        },
        // The quota route is the only correct probe: it is free, it authenticates (an invalid
        // key is rejected via the business code even under HTTP 200), and unlike generation it
        // never spends plan quota.
        readiness_probe: ProbeRoute::Quota,
        quota_poll_interval: Duration::from_secs(input.quota_poll_secs),
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
/// profile opens against the keyring (roster load), each profile passes a free quota probe on
/// its own console origin (auth + capacity evidence), and the turn-evidence writer is
/// draining (persistence). Quota exhaustion is deliberately NOT a readiness failure: it is a
/// capacity state that resets, and refusing to start on it would take a healthy fleet out of
/// service for a wall that clears on its own. One usable subscription is real capacity —
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

    fn input() -> GlmPlaneInput {
        GlmPlaneInput {
            enabled: true,
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            ..GlmPlaneInput::default()
        }
    }

    #[test]
    fn the_plane_is_off_unless_explicitly_enabled() {
        // Backend-only means shipping the code must not turn the provider on.
        let disabled = GlmPlaneInput::default();
        assert!(!disabled.enabled);
        assert!(build(&disabled).unwrap().is_none());
    }

    #[test]
    fn a_disabled_plane_ignores_an_otherwise_invalid_configuration() {
        // An operator must not be blocked from booting by settings for a provider they are not
        // running.
        let mut broken = GlmPlaneInput {
            roster_dir: "relative/path".into(),
            auth_scheme: "bogus".into(),
            ..GlmPlaneInput::default()
        };
        broken.enabled = false;
        assert!(build(&broken).unwrap().is_none());
    }

    #[test]
    fn readiness_probes_quota_and_can_never_probe_generation() {
        // Generation costs plan quota; the quota route is free and still authenticates the key
        // (HTTP 200 + code:401 trap handled by the parser, not by the status).
        let config = build(&input()).unwrap().unwrap();
        assert_eq!(config.readiness_probe, ProbeRoute::Quota);
        assert!(!config.readiness_probe.path().contains("messages"));
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
    fn a_relative_roster_directory_is_refused() {
        let mut relative = input();
        relative.roster_dir = "data/glm".into();
        assert!(build(&relative).is_err());
    }

    #[test]
    fn only_bearer_is_selectable_until_x_api_key_is_proven_live() {
        let mut config = input();
        assert_eq!(
            build(&config).unwrap().unwrap().transport.auth_scheme,
            AuthScheme::Bearer
        );
        // The manifest records x-api-key acceptance as unknown; it stays unselectable rather
        // than guessed into the wire.
        config.auth_scheme = "x-api-key".into();
        assert!(build(&config).is_err());
        config.auth_scheme = "basic".into();
        assert!(build(&config).is_err());
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
