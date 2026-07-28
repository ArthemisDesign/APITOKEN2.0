//! Typed configuration for the Codex app-server transport.
//!
//! `forward` deliberately does not read the environment. The composition layer builds this value
//! in `server::config`, then the transport receives only the explicit child-process environment it
//! is allowed to inherit.

use std::collections::BTreeMap;

/// Token rates are owned by the audited, effective-dated catalog in `metering`. `forward` never
/// declares a price of its own; it receives the resolved rates in this config.
pub use metering::CodexPrices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexModel {
    /// Public OpenAI-compatible model id.
    pub id: String,
    /// Model id sent to Codex app-server.
    pub upstream: String,
    pub created: i64,
    pub owned_by: String,
    pub max_output_tokens: u64,
    pub reasoning_efforts: Vec<String>,
    pub prices: CodexPrices,
}

impl CodexModel {
    pub fn supports_effort(&self, effort: &str) -> bool {
        self.reasoning_efforts
            .iter()
            .any(|candidate| candidate == effort)
    }
}

/// One authenticated profile in the pool, as discovered on disk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexHomeSpec {
    /// Absolute path used as the child's `CODEX_HOME`.
    pub path: String,
    /// Stable, non-identifying id used in logs and metric labels. Derived from the directory name,
    /// so it survives restarts and does not shift when another home is added or removed — and it
    /// never carries the account's identity.
    pub id: String,
    /// Optional dedicated egress for this account's child, read from the home. `None` falls back to
    /// the gateway-wide proxy environment.
    pub proxy: Option<String>,
}

#[derive(Clone, Debug)]
pub struct CodexConfig {
    pub enabled: bool,
    /// Absolute path to the pinned Codex binary.
    pub binary: String,
    /// Lowercase SHA-256 of the exact platform build. Version text alone is not an attestation.
    pub binary_sha256: String,
    /// Exact `codex --version` output expected from the pinned build.
    pub expected_version: String,
    /// Explicitly configured profiles. Each becomes one supervised child with its own `CODEX_HOME`,
    /// its own health state and its own subscription window.
    pub homes: Vec<String>,
    /// Directory scanned for additional profiles, so an account purchased by the authbot joins the
    /// pool without a restart, a config edit or root. A subdirectory counts only once it holds a
    /// completed auth store; a half-finished purchase is invisible to the pool.
    pub homes_dir: Option<String>,
    /// Empty, non-sensitive working directory used for ephemeral API threads.
    pub work_dir: String,
    pub startup_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub turn_timeout_ms: u64,
    /// Retained for configuration compatibility only. Per-home turn concurrency is intentionally
    /// unbounded — like the Claude fleet, a home accepts as many simultaneous turns as arrive — so
    /// this value is no longer enforced as a cap. The provider-wide `AppState::concurrency` guard
    /// (shared with the Claude path) remains the only global admission ceiling.
    pub max_concurrent_turns: usize,
    /// Stop admitting a home once a reported window reaches this utilisation, keeping headroom so
    /// the wall is met by a clean 429 with a real reset rather than mid-turn.
    pub admit_below_used_percent: i64,
    /// Prior for one subscription's WEEKLY window capacity in official-price USD, used until
    /// calibration measures the real figure (and as its plausibility anchor). Set it to the plan's
    /// expectation — a ChatGPT Pro account measured ≈$1.7–1.8k/week; samples outside [0.25x, 4x]
    /// of this value are rejected, so a Plus pool should configure a correspondingly lower prior.
    pub window_cap_usd_prior: f64,
    /// How often the background loop re-checks each home's authentication and window snapshot.
    pub health_probe_interval_secs: u64,
    /// Conservative preflight allowance for provider-hidden/runtime tokens that are not present in
    /// the public JSON body.
    pub reserve_overhead_tokens: u64,
    pub history_ttl_secs: u64,
    pub history_local_cap: usize,
    pub history_redis_url: Option<String>,
    pub history_secret: Option<String>,
    pub history_redis_timeout_ms: u64,
    /// Explicit allowlist of proxy-related variables for the Codex child. No other parent
    /// environment is inherited.
    pub child_proxy_env: BTreeMap<String, String>,
    pub models: Vec<CodexModel>,
}

impl CodexConfig {
    pub fn model(&self, public_id: &str) -> Option<&CodexModel> {
        self.models.iter().find(|model| model.id == public_id)
    }
}
