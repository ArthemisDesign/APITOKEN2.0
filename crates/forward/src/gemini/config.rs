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
/// Current first-party Antigravity image-tool traffic uses the daily production origin. Keep image
/// generation separate from the configured sandbox text origin and from legacy Gemini CLI OAuth:
/// those are different authenticated transports even though their Code Assist path is the same.
pub const ANTIGRAVITY_MEDIA_UPSTREAM: &str = "https://daily-cloudcode-pa.googleapis.com";

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

    /// Public effort values accepted by every universal Gemini adapter for this exact model.
    /// The list is provider-owned because private bucket routing is model-specific; clients must
    /// not infer it from a model-name prefix.
    pub fn reasoning_efforts(&self) -> &'static [&'static str] {
        if self.is_image_generation() {
            &[]
        } else if self.id == "gemini-3.7-flash" {
            // 2026-08-15 exact-SHA (916dee0d…) live matrix admitted every explicit level on
            // the Ultra plan: low/medium/high each returned the byte-exact visible output
            // across incremental SSE frames with terminal STOP, authoritative usage and a
            // positive thinking token class on the confirmed tiered wire. `minimal` is
            // rejected by the model itself, so it is not an advertised effort.
            &["low", "medium", "high"]
        } else if self.id == "gemini-3.1-pro-preview" {
            &["low", "medium", "high"]
        } else {
            &["minimal", "low", "medium", "high"]
        }
    }

    /// Capabilities of the public universal Chat/Responses adapters for this exact model. The
    /// subscription image route is deliberately narrower: it accepts reference images and emits
    /// text+image, but rejects tools and structured-output controls before admission. Only the
    /// reviewed Flash Preview route accepts audio, and only in the exact PCM WAV form enforced by
    /// native admission.
    pub fn input_modalities(&self) -> &'static [&'static str] {
        if self.id == "gemini-3-flash-preview" {
            // 2026-08-16 fleet media matrix: video/mp4 and application/pdf inline inputs
            // returned the content-perception marker with terminal usage; audio was admitted
            // earlier with the exact PCM WAV contract.
            &["text", "image", "audio", "video", "pdf"]
        } else if self.id == "gemini-3.7-flash" {
            // 2026-08-16 exact-SHA (fc556402…) media matrix: audio/wav, video/mp4 and
            // application/pdf inline inputs each returned the content-perception marker with
            // terminal usage on the confirmed tiered wire.
            &["text", "image", "audio", "video", "pdf"]
        } else if self.id == "gemini-2.5-flash" {
            // 2026-08-16 fleet media matrix: audio (marker `TONE`) and video (marker `red`)
            // admitted with terminal usage/parity. PDF has no official claim for this model.
            &["text", "image", "audio", "video"]
        } else if self.is_image_generation() {
            // Official input surface is Text/Image/PDF; the 2026-08-16 fleet matrix admitted
            // inline application/pdf (beacon extraction, terminal usage/parity).
            &["text", "image", "pdf"]
        } else {
            // 2026-08-16 fleet media matrix: audio/video/pdf each admitted with the
            // content-perception marker and terminal usage/parity on this model.
            &["text", "image", "audio", "video", "pdf"]
        }
    }

    pub fn output_modalities(&self) -> &'static [&'static str] {
        if self.is_image_generation() {
            &["text", "image"]
        } else {
            &["text"]
        }
    }

    pub fn tool_calling(&self) -> bool {
        !self.is_image_generation()
    }

    pub fn structured_outputs(&self) -> bool {
        !self.is_image_generation()
    }

    /// Resolve the public Developer API model to the private Antigravity quota/generation bucket.
    /// Reviewed model families use owned generation evidence. Gemini 3.7 keeps its confirmed
    /// private alias strictly behind the public product identity.
    ///
    /// Keep this mapping deliberately closed. A quota row is only availability evidence, not proof
    /// that an arbitrary private id has the public model's semantics or price.
    pub fn wire_model_id(&self, thinking_level: Option<&str>) -> Result<&str, &'static str> {
        let level = thinking_level
            .map(str::trim)
            .filter(|level| !level.is_empty())
            .unwrap_or("thinking_level_unspecified");
        match self.id.as_str() {
            "gemini-3.7-flash" => {
                if level.eq_ignore_ascii_case("low")
                    || level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("high")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    // The 2026-08-15 exact-SHA live admission proved this private alias with real
                    // output, terminal usage and incremental SSE. It is never exposed to clients.
                    Ok("gemini-3.7-flash-tiered")
                } else {
                    Err("Gemini 3.7 Flash supports low, medium, or high thinking levels.")
                }
            }
            "gemini-3-flash-preview" => {
                if level.eq_ignore_ascii_case("minimal")
                    || level.eq_ignore_ascii_case("low")
                    || level.eq_ignore_ascii_case("medium")
                    || level.eq_ignore_ascii_case("high")
                    || level.eq_ignore_ascii_case("thinking_level_unspecified")
                {
                    // The public Developer API id 404s on Antigravity generation. Owned live
                    // evidence proves that the private catalogue id serves the same family and
                    // echoes its canonical modelVersion; the public id remains the billing and
                    // client-facing identity.
                    Ok("gemini-3-flash")
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
            .expect("every configured Gemini model must have a closed default wire candidate")
    }

    /// Closed quota identities used for this public model. Billing and the public API deliberately
    /// retain one canonical model id even when the reviewed Antigravity route uses private buckets.
    pub fn quota_model_ids(&self) -> Vec<&str> {
        match self.id.as_str() {
            "gemini-3.7-flash" => vec!["gemini-3.7-flash-tiered"],
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
/// are the only models whose Antigravity wire candidate and modality contract have a closed
/// mapping. Live generation remains an additional deployment gate before any model enters
/// systemd's public allowlist.
const SUBSCRIPTION_MODELS: [&str; 9] = [
    "gemini-3.1-flash-image",
    "gemini-3.7-flash",
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
    /// Ordinary runtimes use the sibling `credentials/<profile>.json` tree. The one-shot
    /// admission runtime instead receives systemd's read-only flattened credential namespace.
    pub credential_layout: GeminiCredentialLayout,
    pub credential_keys: gemini_credential::CredentialKeyring,
    pub models: Vec<GeminiModel>,
    pub connect_timeout_secs: u64,
    /// Silence allowance for auxiliary calls (token refresh, quota and catalogue snapshots). These
    /// must fail fast so a wedged profile rotates out quickly.
    pub read_timeout_secs: u64,
    /// Silence allowance for customer generation, or `0` for no deadline at all — the production
    /// default. Time cannot distinguish a dead peer from a thinking model, so it is not used to
    /// try: liveness comes from TCP keepalive probes, a departed client from cancel-on-disconnect,
    /// and the blast radius of a genuinely stuck upstream from the inflight caps, which bound
    /// concurrency rather than duration. A non-zero value here is an operator escape hatch.
    pub generation_idle_timeout_secs: u64,
    pub max_transport_retries: usize,
    /// Quarantine after a terminal OAuth/auth rejection before the profile can re-enter rotation.
    pub auth_quarantine_secs: i64,
    /// First backoff step after an environment-derived auth rejection (upstream 401/403 that a
    /// fresh bearer did not resolve). Doubles per consecutive rejection up to
    /// `auth_quarantine_secs`, and never removes the profile from the authenticated set.
    pub auth_blocked_cool_secs: i64,
    /// Floor between health sweeps requested by the data path, so an exhausted pool re-probes
    /// promptly without letting every customer request trigger another full roster probe.
    pub min_probe_interval_secs: i64,
    pub transport_cool_secs: i64,
    pub model_failure_cool_secs: i64,
    pub model_failure_max_cool_secs: i64,
    pub default_rate_limit_cool_secs: i64,
    /// Short cool applied when a generation 429 carries no authoritative retry hint AND the
    /// profile's own fresh quota catalogue still reports a positive remainder for the model. Google
    /// is then reporting an RPM/concurrency stall, not exhaustion, so a long cooling only converts
    /// a momentary throttle into a minute-long model outage across the fleet. Kept short on
    /// purpose: it just spaces concurrent retries without parking the model.
    pub rate_limit_rpm_cool_secs: i64,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GeminiCredentialLayout {
    #[default]
    SealedRoster,
    SystemdFlat,
}

impl GeminiConfig {
    pub fn model(&self, id: &str) -> Option<&GeminiModel> {
        self.models.iter().find(|model| model.id == id)
    }

    /// Match a generation wire id to the private quota rows that can account for it. The owned
    /// catalogue exposes both Gemini 3 Flash rows and their exact debit relationship is not yet
    /// attributable, so either row participates in admission and an explicit zero on either one
    /// remains a conservative block. Other routes retain their exact one-row identity.
    pub fn quota_model_id_matches_wire(
        &self,
        oauth_kind: gemini_credential::OAuthKind,
        wire_model_id: &str,
        quota_model_id: &str,
    ) -> bool {
        if oauth_kind == gemini_credential::OAuthKind::Antigravity
            && wire_model_id == "gemini-3-flash"
        {
            matches!(quota_model_id, "gemini-3-flash" | "gemini-3-flash-agent")
        } else {
            quota_model_id == wire_model_id
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
        _model: &str,
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
        self.upstream_for(oauth_kind)
    }

    pub fn user_agent(&self, oauth_kind: gemini_credential::OAuthKind, model: &str) -> String {
        match oauth_kind {
            gemini_credential::OAuthKind::Antigravity => {
                let version = &self.antigravity_version;
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
    use super::{subscription_model_supported, GeminiModel, SUBSCRIPTION_MODELS};

    #[test]
    fn public_reasoning_efforts_are_model_specific() {
        let model = |id: &str| GeminiModel {
            id: id.to_string(),
            display_name: id.to_string(),
            input_token_limit: 1,
            output_token_limit: 1,
            prices: metering::GeminiPrices {
                input: 0,
                audio_input: 0,
                cached_input: 0,
                cached_audio_input: 0,
                output: 0,
                image_output: 0,
                long_context_threshold: u64::MAX,
                long_input: 0,
                long_audio_input: 0,
                long_cached_input: 0,
                long_cached_audio_input: 0,
                long_output: 0,
                search: metering::GeminiSearchBilling::PerGroundedPrompt { nano: 0 },
            },
        };
        assert_eq!(
            model("gemini-3.6-flash").reasoning_efforts(),
            ["minimal", "low", "medium", "high"]
        );
        assert_eq!(
            model("gemini-3.7-flash").reasoning_efforts(),
            ["low", "medium", "high"]
        );
        assert_eq!(
            model("gemini-3.1-pro-preview").reasoning_efforts(),
            ["low", "medium", "high"]
        );
        assert!(model("gemini-3.1-flash-image")
            .reasoning_efforts()
            .is_empty());
        assert_eq!(
            model("gemini-3.1-flash-image").output_modalities(),
            ["text", "image"]
        );
        assert!(!model("gemini-3.1-flash-image").tool_calling());
        assert!(!model("gemini-3.1-flash-image").structured_outputs());
        assert_eq!(model("gemini-3.6-flash").output_modalities(), ["text"]);
        assert!(model("gemini-3.6-flash").tool_calling());
        assert!(model("gemini-3.6-flash").structured_outputs());
        let flash_37 = model("gemini-3.7-flash");
        assert_eq!(
            flash_37.input_modalities(),
            ["text", "image", "audio", "video", "pdf"]
        );
        assert_eq!(flash_37.output_modalities(), ["text"]);
        assert_eq!(flash_37.reasoning_efforts(), ["low", "medium", "high"]);
        assert!(flash_37.tool_calling());
        assert!(flash_37.structured_outputs());
        assert_eq!(
            model("gemini-3-flash-preview").input_modalities(),
            ["text", "image", "audio", "video", "pdf"]
        );
        assert_eq!(
            model("gemini-2.5-flash").input_modalities(),
            ["text", "image", "audio", "video"]
        );
        assert_eq!(
            model("gemini-3.1-flash-image").input_modalities(),
            ["text", "image", "pdf"]
        );
        assert_eq!(
            model("gemini-3.6-flash").input_modalities(),
            ["text", "image", "audio", "video", "pdf"]
        );
    }

    #[test]
    fn subscription_catalog_is_exact_and_fail_closed() {
        assert_eq!(
            SUBSCRIPTION_MODELS,
            [
                "gemini-3.1-flash-image",
                "gemini-3.7-flash",
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
