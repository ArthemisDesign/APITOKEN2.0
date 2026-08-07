//! KIMI plane configuration.
//!
//! `forward` never reads the environment; `server` fills this and hands it down. Contract:
//! `docs/engine/KIMI_PROVIDER.md` §0 and §4.
//!
//! The plane is **off by default**. KIMI is deliberately backend-only: no public catalogue, no
//! router namespace, no pricing surface. Enabling it is an explicit operator action, not a
//! consequence of the binary containing the code.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use kimi_credential::{CredentialKeyring, KIMI_CODE_BASE_URL};

use super::transport::{AuthScheme, KimiTransportConfig, ProbeRoute};

/// Everything the plane needs to run. Absent means the plane is disabled.
#[derive(Clone)]
pub struct KimiPlaneConfig {
    /// Roster root: `<dir>/profiles.json` + `<dir>/credentials/<id>.json`.
    pub roster_dir: PathBuf,
    pub keyring: CredentialKeyring,
    pub transport: KimiTransportConfig,
    /// Which non-billable endpoint readiness probes.
    pub readiness_probe: ProbeRoute,
    /// How often the free quota poll runs when a profile is idle.
    pub quota_poll_interval: Duration,
    /// How long a request may wait for capacity before failing, from `CLAUDE_API_SMOOTH_WAIT_MS`.
    ///
    /// Spent only while the round produced no provider verdict at all — every profile is cooling
    /// and we never reached the upstream. A real provider wall is answered honestly and
    /// immediately; waiting on it would only delay a `429` the caller must see.
    pub smooth_wait: Duration,
}

impl std::fmt::Debug for KimiPlaneConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KimiPlaneConfig")
            .field("roster_dir", &self.roster_dir)
            .field("transport", &self.transport)
            .field("readiness_probe", &self.readiness_probe)
            .field("keyring", &"REDACTED")
            .finish()
    }
}

/// Raw operator input, already read from the environment by `server`.
pub struct KimiPlaneInput {
    pub enabled: bool,
    pub roster_dir: String,
    pub credential_keys: Option<String>,
    pub base_url: String,
    pub auth_scheme: String,
    pub quota_poll_secs: u64,
    /// `CLAUDE_API_SMOOTH_WAIT_MS`, shared with the other planes. Zero disables waiting.
    pub smooth_wait_ms: u64,
}

impl Default for KimiPlaneInput {
    fn default() -> Self {
        Self {
            enabled: false,
            roster_dir: "/srv/claude-api/data/kimi".into(),
            credential_keys: None,
            base_url: KIMI_CODE_BASE_URL.into(),
            auth_scheme: "bearer".into(),
            quota_poll_secs: 300,
            smooth_wait_ms: 8_000,
        }
    }
}

/// Build the plane configuration, or `None` when it is disabled.
///
/// Every rejection is a refusal to start the plane rather than a warning, because a
/// half-configured provider that accepts traffic is worse than one that is simply off.
pub fn build(input: &KimiPlaneInput) -> Result<Option<KimiPlaneConfig>> {
    if !input.enabled {
        return Ok(None);
    }
    let roster_dir = PathBuf::from(&input.roster_dir);
    if !roster_dir.is_absolute() {
        bail!("CLAUDE_API_KIMI_ROSTER_DIR must be an absolute path");
    }
    let Some(keys) = input.credential_keys.as_ref().filter(|k| !k.is_empty()) else {
        // Without the keyring no envelope can be opened, so every profile would fail on first use.
        bail!("CLAUDE_API_KIMI_CREDENTIAL_KEYS is required for the encrypted KIMI roster");
    };
    let keyring = CredentialKeyring::parse(keys)?;

    let base_url = input.base_url.trim_end_matches('/').to_string();
    if !base_url.starts_with("https://") {
        // The credential is a bearer token; a plaintext base URL would put it on the wire.
        bail!("CLAUDE_API_KIMI_BASE_URL must be https");
    }
    let auth_scheme = match input.auth_scheme.to_ascii_lowercase().as_str() {
        "bearer" => AuthScheme::Bearer,
        "x-api-key" | "api-key" => AuthScheme::ApiKeyHeader,
        other => bail!("unsupported CLAUDE_API_KIMI_AUTH_SCHEME: {other}"),
    };
    if input.quota_poll_secs == 0 {
        bail!("CLAUDE_API_KIMI_QUOTA_POLL_SECS must be positive");
    }

    Ok(Some(KimiPlaneConfig {
        roster_dir,
        keyring,
        transport: KimiTransportConfig {
            base_url,
            auth_scheme,
            ..KimiTransportConfig::default()
        },
        // Identity is the correct probe: it is free, it authenticates, and unlike the catalogue
        // route it cannot answer 200 for a dead credential.
        readiness_probe: ProbeRoute::Identity,
        quota_poll_interval: Duration::from_secs(input.quota_poll_secs),
        smooth_wait: Duration::from_millis(input.smooth_wait_ms),
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
/// Quota exhaustion is deliberately NOT a readiness failure: it is a capacity state that resets,
/// and refusing to start on it would take a healthy fleet out of service for a wall that clears
/// on its own. One usable subscription is real capacity — there is no arbitrary minimum fleet.
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

    fn input() -> KimiPlaneInput {
        KimiPlaneInput {
            enabled: true,
            credential_keys: Some(format!("a1:{}", "11".repeat(32))),
            ..KimiPlaneInput::default()
        }
    }

    #[test]
    fn the_plane_is_off_unless_explicitly_enabled() {
        // Backend-only means shipping the code must not turn the provider on.
        let disabled = KimiPlaneInput::default();
        assert!(!disabled.enabled);
        assert!(build(&disabled).unwrap().is_none());
    }

    #[test]
    fn a_disabled_plane_ignores_an_otherwise_invalid_configuration() {
        // An operator must not be blocked from booting by settings for a provider they are not
        // running.
        let mut broken = KimiPlaneInput {
            roster_dir: "relative/path".into(),
            base_url: "http://insecure".into(),
            ..KimiPlaneInput::default()
        };
        broken.enabled = false;
        assert!(build(&broken).unwrap().is_none());
    }

    #[test]
    fn readiness_probes_identity_and_can_never_probe_the_catalogue() {
        // /v1/models is ungated: it answers 200 for an invalid key while generation then returns
        // 403, so a probe on it reports a dead subscription as healthy.
        let config = build(&input()).unwrap().unwrap();
        assert_eq!(config.readiness_probe, ProbeRoute::Identity);
        assert!(!config.readiness_probe.path().contains("models"));
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
    fn a_plaintext_base_url_is_refused() {
        // The credential is a bearer token; http would put it on the wire.
        let mut insecure = input();
        insecure.base_url = "http://api.kimi.com/coding/v1".into();
        assert!(build(&insecure).is_err());
    }

    #[test]
    fn a_relative_roster_directory_is_refused() {
        let mut relative = input();
        relative.roster_dir = "data/kimi".into();
        assert!(build(&relative).is_err());
    }

    #[test]
    fn both_documented_auth_schemes_are_selectable_and_others_are_not() {
        let mut config = input();
        assert_eq!(
            build(&config).unwrap().unwrap().transport.auth_scheme,
            AuthScheme::Bearer
        );
        config.auth_scheme = "x-api-key".into();
        assert_eq!(
            build(&config).unwrap().unwrap().transport.auth_scheme,
            AuthScheme::ApiKeyHeader
        );
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
    fn a_trailing_slash_in_the_base_url_is_normalised_away() {
        let mut config = input();
        config.base_url = "https://api.kimi.com/coding/v1/".into();
        let built = build(&config).unwrap().unwrap();
        assert_eq!(built.transport.base_url, "https://api.kimi.com/coding/v1");
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
