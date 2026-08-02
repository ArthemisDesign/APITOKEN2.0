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
/// Current Antigravity text-generation endpoint. The signed 2.4.3 language server and two
/// independent maintained implementations use the non-sandbox daily host. New preview models may
/// be absent from the older sandbox deployment even when its generic countTokens path accepts the
/// public model id.
pub const ANTIGRAVITY_DAILY_UPSTREAM: &str = "https://daily-cloudcode-pa.googleapis.com";
/// Signed Antigravity release that first advertises Gemini 3 Flash Preview. Keep this identity
/// scoped to the preview experiment: the existing text fleet retains its live-proven configured
/// release until the new tuple has independent production evidence.
pub const ANTIGRAVITY_FLASH_PREVIEW_VERSION: &str = "2.4.3";
/// The Antigravity language server sends production generation traffic here. The sandbox host is
/// useful for the reviewed text surface, but advertises the image quota bucket without serving the
/// image backend and returns a generic 503 for otherwise valid Nano Banana requests.
pub const ANTIGRAVITY_MEDIA_UPSTREAM: &str = "https://cloudcode-pa.googleapis.com";

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

impl GeminiModel {
    pub fn is_image_generation(&self) -> bool {
        self.id == "gemini-3.1-flash-image"
    }

    /// Resolve the public Developer API model to the exact private Antigravity quota/generation
    /// bucket. The authenticated Code Assist catalogue encodes Gemini 3 reasoning effort in the
    /// model id; sending the public family id itself returns `INVALID_ARGUMENT`/`UNAVAILABLE`.
    ///
    /// Keep this mapping deliberately closed. A quota row is only availability evidence, not proof
    /// that an arbitrary private id has the public model's semantics or price.
    pub fn wire_model_id(&self, thinking_level: Option<&str>) -> Result<&str, &'static str> {
        let level = thinking_level
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .unwrap_or("thinking_level_unspecified");
        match self.id.as_str() {
            "gemini-3-flash-preview" => {
                if level.eq_ignore_ascii_case("minimal")
                    || level.eq_ignore_ascii_case("low")
                    || level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("high")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    // Both the official Gemini CLI and Antigravity 2.4.3 send the public preview
                    // id unchanged. Antigravity accounts it against a separate agent quota row;
                    // that accounting identity is resolved below and never leaks into the wire.
                    Ok("gemini-3-flash-preview")
                } else {
                    Err("Gemini 3 Flash Preview supports minimal, low, medium, or high thinking levels.")
                }
            }
            "gemini-3.6-flash" => {
                if level.eq_ignore_ascii_case("minimal") || level.eq_ignore_ascii_case("low") {
                    Ok("gemini-3.6-flash-low")
                } else if level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    Ok("gemini-3.6-flash-medium")
                } else if level.eq_ignore_ascii_case("high") {
                    Ok("gemini-3.6-flash-high")
                } else {
                    Err("Gemini 3.6 Flash supports minimal, low, medium, or high thinking levels.")
                }
            }
            "gemini-3.5-flash" => {
                // The current Antigravity catalogue exposes two admission buckets for 3.5 Flash:
                // `extra-low` for minimal and `low` for every other public level. The original
                // thinkingLevel stays in generationConfig, so low/medium/high semantics remain
                // distinct even though they share one private quota/generation id.
                if level.eq_ignore_ascii_case("minimal") {
                    Ok("gemini-3.5-flash-extra-low")
                } else if level.eq_ignore_ascii_case("low")
                    || level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("high")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    Ok("gemini-3.5-flash-low")
                } else {
                    Err("Gemini 3.5 Flash supports minimal, low, medium, or high thinking levels.")
                }
            }
            "gemini-3.1-pro-preview" => {
                if level.eq_ignore_ascii_case("low") {
                    Ok("gemini-3.1-pro-low")
                } else if level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("high")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    // The current Antigravity picker calls its high/default Pro bucket
                    // `gemini-pro-agent`; `gemini-3.1-pro-high` is only a compatibility alias.
                    Ok("gemini-pro-agent")
                } else {
                    Err("Gemini 3.1 Pro Preview supports low, medium, or high thinking levels.")
                }
            }
            _ => Ok(&self.id),
        }
    }

    pub fn default_wire_model_id(&self) -> &str {
        self.wire_model_id(None)
            .expect("every configured Gemini model must have a reviewed default wire route")
    }

    /// Exact private quota buckets that can serve this public model. Antigravity encodes
    /// reasoning effort in the bucket id for a few model families, while billing and the public
    /// API deliberately retain one canonical model id. The control plane publishes this bounded
    /// mapping so the admin UI can join official quota rows without guessing private aliases.
    pub fn quota_model_ids(&self) -> Vec<&str> {
        match self.id.as_str() {
            "gemini-3-flash-preview" => {
                vec!["gemini-3-flash", "gemini-3-flash-agent"]
            }
            "gemini-3.6-flash" => vec![
                "gemini-3.6-flash-low",
                "gemini-3.6-flash-medium",
                "gemini-3.6-flash-high",
            ],
            "gemini-3.5-flash" => vec!["gemini-3.5-flash-extra-low", "gemini-3.5-flash-low"],
            "gemini-3.1-pro-preview" => {
                vec!["gemini-3.1-pro-low", "gemini-pro-agent"]
            }
            _ => vec![self.id.as_str()],
        }
    }
}

/// Product access is intentionally narrower than the global Developer API price catalogue. These
/// are the only models whose Antigravity wire identity and modality contract have a reviewed
/// mapping; live generation remains an additional deployment gate before a model enters systemd's
/// public allowlist.
const SUBSCRIPTION_MODELS: [&str; 8] = [
    "gemini-3.1-flash-image",
    "gemini-3.6-flash",
    "gemini-3.5-flash",
    "gemini-3-flash-preview",
    "gemini-3.1-pro-preview",
    "gemini-3.1-flash-lite",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
];

pub fn subscription_model_supported(id: &str) -> bool {
    SUBSCRIPTION_MODELS.contains(&id)
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
    /// Quarantine after a terminal OAuth/auth rejection before the profile can re-enter rotation.
    pub auth_quarantine_secs: i64,
    pub transport_cool_secs: i64,
    pub model_failure_cool_secs: i64,
    pub model_failure_max_cool_secs: i64,
    pub default_rate_limit_cool_secs: i64,
    /// Soft reserve used only while another profile has healthier quota headroom. The service floor
    /// is preserved: if every eligible profile is below its reserve, routing fails open to them.
    pub quota_reserve_fraction: f64,
    pub quota_reserve_jitter: f64,
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

    /// Resolve the provider quota row used for admission separately from the generation wire id.
    /// Antigravity's official client sends the public Gemini 3 Flash Preview id with
    /// `requestType=agent`, while `fetchAvailableModels` reports that capacity under the private
    /// `gemini-3-flash-agent` row. Legacy Gemini CLI quota uses the public id directly.
    pub fn quota_model_id_for_wire<'a>(
        &self,
        oauth_kind: gemini_credential::OAuthKind,
        wire_model_id: &'a str,
    ) -> &'a str {
        if oauth_kind == gemini_credential::OAuthKind::Antigravity
            && wire_model_id == "gemini-3-flash-preview"
        {
            "gemini-3-flash-agent"
        } else {
            wire_model_id
        }
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

    pub fn generation_upstream_for(
        &self,
        oauth_kind: gemini_credential::OAuthKind,
        image_generation: bool,
        model: &str,
    ) -> &str {
        // Keep loopback integration tests on their explicit mock. Paid image generation follows
        // the production Antigravity LS route observed in working implementations; text retains
        // the configured, already live-verified endpoint.
        if self.upstream.starts_with("http://") {
            return &self.upstream;
        }
        if oauth_kind == gemini_credential::OAuthKind::Antigravity && image_generation {
            return ANTIGRAVITY_MEDIA_UPSTREAM;
        }
        if oauth_kind == gemini_credential::OAuthKind::Antigravity
            && model == "gemini-3-flash-preview"
        {
            return ANTIGRAVITY_DAILY_UPSTREAM;
        }
        self.upstream_for(oauth_kind)
    }

    pub fn user_agent(&self, oauth_kind: gemini_credential::OAuthKind, model: &str) -> String {
        match oauth_kind {
            gemini_credential::OAuthKind::Antigravity => {
                let version = if model == "gemini-3-flash-preview" {
                    ANTIGRAVITY_FLASH_PREVIEW_VERSION
                } else {
                    &self.antigravity_version
                };
                format!(
                    "antigravity/hub/{version} {}",
                    gemini_credential::ANTIGRAVITY_PLATFORM
                )
            }
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

#[cfg(test)]
mod tests {
    use super::{subscription_model_supported, SUBSCRIPTION_MODELS};

    #[test]
    fn subscription_catalog_is_exact_and_fail_closed() {
        assert_eq!(
            SUBSCRIPTION_MODELS,
            [
                "gemini-3.1-flash-image",
                "gemini-3.6-flash",
                "gemini-3.5-flash",
                "gemini-3-flash-preview",
                "gemini-3.1-pro-preview",
                "gemini-3.1-flash-lite",
                "gemini-2.5-flash",
                "gemini-2.5-flash-lite",
            ]
        );
        assert!(SUBSCRIPTION_MODELS
            .iter()
            .all(|id| subscription_model_supported(id)));

        for rejected in [
            // Priced Developer API models that the live subscription cannot serve.
            "gemini-3.5-flash-lite",
            "gemini-2.5-pro",
            // Private quota/tier identities are never public product ids.
            "gemini-3.6-flash-tiered",
            "gemini-3.6-flash-low",
            "gemini-3.6-flash-medium",
            "gemini-3.6-flash-high",
            "gemini-3.5-flash-extra-low",
            "gemini-3.5-flash-low",
            "gemini-3-flash",
            "gemini-3-flash-agent",
            "gemini-3.1-pro-low",
            "gemini-pro-agent",
            // Non-text and foreign-provider rows may appear in upstream catalogues.
            "gemini-2.5-flash-image",
            "gemini-2.5-flash-native-audio-preview-12-2025",
            "gemini-2.5-flash-preview-tts",
            "claude-sonnet-4-5",
            "gpt-5.6",
            "chat_20706",
            "tab_flash_lite_preview",
            // Matching is deliberately exact: aliases and prefixed route names stay closed.
            "models/gemini-3.6-flash",
            "Gemini-3.6-Flash",
            " gemini-3.6-flash",
            "",
        ] {
            assert!(
                !subscription_model_supported(rejected),
                "unreviewed subscription model unexpectedly admitted: {rejected}"
            );
        }
    }
}
