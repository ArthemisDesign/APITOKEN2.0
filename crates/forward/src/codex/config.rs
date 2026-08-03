//! Typed configuration for the native Codex (ChatGPT) transport.
//!
//! `forward` deliberately does not read the environment. The composition layer builds this value
//! in `server::config`, then the transport receives only explicit values.

use std::collections::BTreeMap;

/// Token rates are owned by the audited, effective-dated catalog in `metering`. `forward` never
/// declares a price of its own; it receives the resolved rates in this config.
pub use metering::CodexPrices;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexModel {
    /// Public OpenAI-compatible model id.
    pub id: String,
    /// Model id sent to the Codex backend.
    pub upstream: String,
    pub created: i64,
    pub owned_by: String,
    pub max_output_tokens: u64,
    pub reasoning_efforts: Vec<String>,
    /// Input/output modalities and controls that the public OpenAI-compatible adapters can
    /// actually execute for this model. These are part of the reviewed serving contract rather
    /// than guesses made by discovery clients from the model id.
    pub input_modalities: Vec<String>,
    pub output_modalities: Vec<String>,
    pub tool_calling: bool,
    pub structured_outputs: bool,
    /// ChatGPT-subscription credit multiplier for Fast mode. `None` disables the tier.
    pub fast_multiplier_basis_points: Option<i64>,
    pub prices: CodexPrices,
}

impl CodexModel {
    pub fn supports_effort(&self, effort: &str) -> bool {
        self.reasoning_efforts
            .iter()
            .any(|candidate| candidate == effort)
    }

    pub fn supports_fast(&self) -> bool {
        self.fast_multiplier_basis_points.is_some()
    }
}

/// One authenticated profile in the pool, as listed in the roster file.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexProfileSpec {
    /// Stable non-identifying label used in logs and metric labels. It must not contain an email
    /// or account id.
    pub id: String,
    /// Absolute path to a versioned AEAD envelope. OpenAI identity, OAuth tokens, plan and the
    /// authenticated proxy are all encrypted inside it.
    pub credential_file: String,
}

/// Roster of sealed Codex profiles. The authbot republishes this file atomically when an account
/// joins or leaves; the runtime rescans it on every health tick, so a purchased account joins the
/// pool without an engine restart.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CodexProfilesFile {
    pub profiles: Vec<CodexProfileSpec>,
}

#[derive(Clone, Debug)]
pub struct CodexConfig {
    pub enabled: bool,
    /// Native ChatGPT-backed Codex backend, without a trailing slash.
    pub base_url: String,
    /// Roster JSON listing the sealed profiles. Sits next to `<roster>/credentials/<id>.json`.
    pub profiles_file: String,
    /// Keyring that opens the sealed credential envelopes.
    pub credential_keys: codex_credential::CredentialKeyring,
    /// Pinned official-client version presented on the wire (`codex_cli_rs/<version>`).
    pub cli_version: String,
    pub request_timeout_ms: u64,
    pub turn_timeout_ms: u64,
    /// How long the gateway waits for *any* SSE event inside a running turn. Separate from
    /// `turn_timeout_ms`: the total deadline must stay generous (a reasoning model thinks for
    /// minutes), while silence answers "is this profile still there at all".
    pub turn_silence_timeout_ms: u64,
    /// How often the background loop re-checks each profile's usage snapshot.
    pub health_probe_interval_secs: u64,
    /// Soft reserve kept free on the 5h window, as a fraction (0.10 = never route above ~90%).
    pub reserve_5h: f64,
    /// Soft reserve kept free on the weekly window (0.03 = never route above ~97%).
    pub reserve_7d: f64,
    /// Deterministic per-profile spread of both thresholds (anti-fingerprint).
    pub reserve_jitter: f64,
    /// Conservative preflight allowance for provider-hidden/runtime tokens that are not present in
    /// the public JSON body.
    pub reserve_overhead_tokens: u64,
    pub history_ttl_secs: u64,
    pub history_local_cap: usize,
    pub history_redis_url: Option<String>,
    pub history_secret: Option<String>,
    pub history_redis_timeout_ms: u64,
    /// Optional fallback proxy variables applied when a credential carries no per-profile proxy.
    pub default_proxy_env: BTreeMap<String, String>,
    pub models: Vec<CodexModel>,
}

impl CodexConfig {
    pub fn model(&self, public_id: &str) -> Option<&CodexModel> {
        self.models.iter().find(|model| model.id == public_id)
    }

    /// `POST {base}/responses` — the only generation endpoint of the native backend.
    pub fn responses_url(&self) -> String {
        format!("{}/responses", self.base_url)
    }

    /// `GET {origin}/backend-api/wham/usage` — plan and window utilisation for one account.
    pub fn usage_url(&self) -> String {
        format!(
            "{}/wham/usage",
            self.base_url.trim_end_matches("/codex").trim_end_matches('/')
        )
    }

    /// `GET {base}/models` — live availability catalogue (best effort; last-good is retained).
    pub fn models_url(&self) -> String {
        format!("{}/models", self.base_url)
    }

    /// Exact User-Agent shape of the official client on Linux servers.
    pub fn user_agent(&self) -> String {
        codex_credential::codex_user_agent(&self.cli_version, "Linux", "x86_64", "codex_cli_rs")
    }

    /// Soft window-reserve policy for selection (5h/weekly caps plus per-profile jitter).
    pub(crate) fn window_reserve(&self) -> super::WindowReserve {
        super::WindowReserve {
            base5h: self.reserve_5h,
            base7d: self.reserve_7d,
            jitter: self.reserve_jitter,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_urls_stay_on_the_chatgpt_backend() {
        let cfg = CodexConfig {
            enabled: true,
            base_url: codex_credential::CODEX_DEFAULT_BASE_URL.to_string(),
            profiles_file: "/tmp/roster.json".to_string(),
            credential_keys: codex_credential::CredentialKeyring::parse(&format!(
                "current:{}",
                "11".repeat(32)
            ))
            .unwrap(),
            cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
            request_timeout_ms: 1_000,
            turn_timeout_ms: 1_000,
            turn_silence_timeout_ms: 1_000,
            health_probe_interval_secs: 300,
            reserve_5h: 0.10,
            reserve_7d: 0.03,
            reserve_jitter: 0.0,
            reserve_overhead_tokens: 0,
            history_ttl_secs: 600,
            history_local_cap: 32,
            history_redis_url: None,
            history_secret: None,
            history_redis_timeout_ms: 10,
            default_proxy_env: BTreeMap::new(),
            models: Vec::new(),
        };
        assert_eq!(
            cfg.responses_url(),
            "https://chatgpt.com/backend-api/codex/responses"
        );
        assert_eq!(
            cfg.usage_url(),
            "https://chatgpt.com/backend-api/wham/usage"
        );
        assert_eq!(
            cfg.models_url(),
            "https://chatgpt.com/backend-api/codex/models"
        );
        assert!(cfg.user_agent().starts_with("codex_cli_rs/"));
    }
}
