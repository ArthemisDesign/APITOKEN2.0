//! Explicit Gemini provider configuration. Environment access remains in `server::config`.

pub use metering::GeminiPrices;

/// Attested production transport observed from the exact Linux Node runtime used by the official
/// Gemini CLI stack. JA3 changes across Node/OpenSSL platform builds, so all four values are kept
/// together and exposed read-only in the admin surface.
pub const GEMINI_NODE_TRANSPORT_PROFILE: &str = "node-v24.18.0-linux-x64";
pub const GEMINI_NODE_EXPECTED_JA3: &str = "944d1e1858cd278718f8a46b65d3212f";
pub const GEMINI_NODE_EXPECTED_JA4: &str = "t13d5211_b262b3658495_8e6e362c5eac";
/// Gemini CLI's global-fetch userinfo request has an independently attested Undici ClientHello.
pub const GEMINI_NODE_FETCH_TRANSPORT_PROFILE: &str = "node-v24.18.0-linux-x64-undici-fetch";
pub const GEMINI_NODE_FETCH_EXPECTED_JA3: &str = "d67b094811e5145139d7cea5f014309f";
pub const GEMINI_NODE_FETCH_EXPECTED_JA4: &str = "t13d5212h1_b262b3658495_8e6e362c5eac";
pub const GEMINI_GOOGLE_AUTH_LIBRARY_VERSION: &str =
    gemini_credential::GEMINI_GOOGLE_AUTH_LIBRARY_VERSION;
pub const LEGACY_GEMINI_UPSTREAM: &str = "https://cloudcode-pa.googleapis.com";

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GeminiProfileSpec {
    /// Stable non-identifying label used in metrics. It must not contain an email or project id.
    pub id: String,
    /// Absolute path to a versioned AEAD envelope. Google identity, OAuth tokens, client secret,
    /// plan/project and authenticated proxy are all encrypted inside it.
    pub credential_file: String,
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
    pub credential_keys: gemini_credential::CredentialKeyring,
    pub models: Vec<GeminiModel>,
    pub connect_timeout_secs: u64,
    pub read_timeout_secs: u64,
    pub max_transport_retries: usize,
    pub auth_quarantine_secs: i64,
    pub transport_cool_secs: i64,
    pub default_rate_limit_cool_secs: i64,
    pub health_probe_interval_secs: u64,
    pub reserve_overhead_tokens: u64,
    /// Public Antigravity release used to shape the Cloud Code User-Agent. Legacy credentials keep
    /// their reviewed Gemini CLI identity until the sealed roster naturally migrates.
    pub antigravity_version: String,
    /// Exact official Node/OpenSSL runtime used by Gemini CLI's gaxios/node-fetch path. Production
    /// startup verifies the binary SHA and every helper handshake verifies version/platform/arch.
    pub node_binary: String,
    pub node_version: String,
    pub node_sha256: String,
}

impl GeminiConfig {
    pub fn model(&self, id: &str) -> Option<&GeminiModel> {
        self.models.iter().find(|model| model.id == id)
    }

    pub fn upstream_for(&self, oauth_kind: gemini_credential::OAuthKind) -> &str {
        // A literal loopback is an explicitly opted-in integration-test endpoint and must observe
        // both wire variants. Production legacy credentials remain pinned to their original host.
        if self.upstream.starts_with("http://") {
            return &self.upstream;
        }
        match oauth_kind {
            gemini_credential::OAuthKind::Antigravity => &self.upstream,
            gemini_credential::OAuthKind::LegacyGeminiCli => LEGACY_GEMINI_UPSTREAM,
        }
    }

    pub fn user_agent(&self, oauth_kind: gemini_credential::OAuthKind, model: &str) -> String {
        match oauth_kind {
            gemini_credential::OAuthKind::Antigravity => format!(
                "antigravity/hub/{} {}",
                self.antigravity_version,
                gemini_credential::ANTIGRAVITY_PLATFORM
            ),
            gemini_credential::OAuthKind::LegacyGeminiCli => {
                // OAuth2Client's request interceptor appends this library token on the actual wire.
                format!(
                    "GeminiCLI/{}/{model} (linux; x64; cli) google-api-nodejs-client/{}",
                    gemini_credential::GEMINI_CLI_VERSION,
                    GEMINI_GOOGLE_AUTH_LIBRARY_VERSION
                )
            }
        }
    }

    pub fn google_api_client(&self) -> String {
        format!("gl-node/{}", self.node_version.trim_start_matches('v'))
    }

    pub fn refresh_user_agent(&self, oauth_kind: gemini_credential::OAuthKind) -> String {
        match oauth_kind {
            gemini_credential::OAuthKind::Antigravity => "Go-http-client/2.0".to_string(),
            gemini_credential::OAuthKind::LegacyGeminiCli => format!(
                "google-api-nodejs-client/{}",
                GEMINI_GOOGLE_AUTH_LIBRARY_VERSION
            ),
        }
    }

    pub fn background_user_agent(&self, oauth_kind: gemini_credential::OAuthKind) -> String {
        // Legacy model-free calls inherit the CLI's resolved current model. Antigravity ignores the
        // model argument, but sharing the selector keeps the migration branch explicit.
        self.user_agent(oauth_kind, gemini_credential::GEMINI_CLI_DEFAULT_MODEL)
    }
}
