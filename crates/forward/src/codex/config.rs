//! Typed configuration for the Codex app-server transport.
//!
//! `forward` deliberately does not read the environment. The composition layer builds this value
//! in `server::config`, then the transport receives only the explicit child-process environment it
//! is allowed to inherit.

use std::collections::BTreeMap;

/// Official API-equivalent token rates in nanodollars per token.
///
/// The values are intentionally attached to each advertised model rather than inferred from a
/// substring at settlement time. Updating a public alias therefore cannot silently change money.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexPrices {
    pub input: i128,
    pub cached_input: i128,
    pub cache_write_input: i128,
    pub output: i128,
    /// OpenAI applies long-context pricing to the whole request above this input-token boundary.
    pub long_context_threshold: u64,
    pub long_input_basis_points: i64,
    pub long_output_basis_points: i64,
}

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

#[derive(Clone, Debug)]
pub struct CodexConfig {
    pub enabled: bool,
    /// Absolute path to the pinned Codex binary.
    pub binary: String,
    /// Lowercase SHA-256 of the exact platform build. Version text alone is not an attestation.
    pub binary_sha256: String,
    /// Exact `codex --version` output expected from the pinned build.
    pub expected_version: String,
    /// Dedicated authenticated profile. The child receives this as `CODEX_HOME`.
    pub codex_home: String,
    /// Empty, non-sensitive working directory used for ephemeral API threads.
    pub work_dir: String,
    pub startup_timeout_ms: u64,
    pub request_timeout_ms: u64,
    pub turn_timeout_ms: u64,
    pub max_concurrent_turns: usize,
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
