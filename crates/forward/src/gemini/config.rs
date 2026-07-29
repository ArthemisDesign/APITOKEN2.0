//! Explicit Gemini provider configuration. Environment access remains in `server::config`.

pub use metering::GeminiPrices;

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeminiProfileSpec {
    /// Stable non-identifying label used in metrics. It must not contain an email or project id.
    pub id: String,
    /// Google applies Developer API quotas per project, not per API key. The gateway uses this only
    /// to reject duplicate quota domains and never exports it to logs or metrics.
    pub project_id: String,
    /// Absolute path to a root/operator-provisioned file containing exactly one paid API key.
    pub api_key_file: String,
    #[serde(default)]
    pub proxy: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeminiProfilesFile {
    pub profiles: Vec<GeminiProfileSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiModel {
    pub id: String,
    pub display_name: String,
    pub input_token_limit: u64,
    pub output_token_limit: u64,
    pub prices: GeminiPrices,
}

#[derive(Clone, Debug)]
pub struct GeminiConfig {
    pub enabled: bool,
    pub upstream: String,
    pub profiles_file: String,
    pub models: Vec<GeminiModel>,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub max_transport_retries: usize,
    pub auth_quarantine_secs: i64,
    pub transport_cool_secs: i64,
    pub default_rate_limit_cool_secs: i64,
    pub health_probe_interval_secs: u64,
    pub reserve_overhead_tokens: u64,
}

impl GeminiConfig {
    pub fn model(&self, id: &str) -> Option<&GeminiModel> {
        self.models.iter().find(|model| model.id == id)
    }
}
