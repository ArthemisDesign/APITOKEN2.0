//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{
    CodexConfig, CodexModel, GeminiConfig, GeminiModel, PricingBridgeConfig, PricingShadowConfig,
    PricingShadowConfigValues, ProviderMode, ProxyConfig, CLAUDE_CODE_IDENTITY,
};
use std::{collections::BTreeMap, env, net::IpAddr};

pub struct Settings {
    pub provider: ProviderMode,
    pub db_path: String,
    /// Engine-owned PostgreSQL DSN. When set, SQLite is migration/rollback input only.
    pub database_url: Option<String>,
    pub instance_id: String,
    pub bind: String,
    pub fleet: Option<String>,
    pub billing: bool, // включён ли учёт баланса ключей (таблица api_keys)
    /// Reader connections this slot opens against the authority. See `CLAUDE_API_BILLING_READERS`.
    pub billing_readers: usize,
    pub mult_bp: i64,   // дефолтная наценка для `key issue` (× 10000; 900 = ×0.09)
    pub cap5h_usd: f64, // прайор ёмкости 5h окна (USD; 0 → дефолт пула под Max 20x)
    pub cap7d_usd: f64, // прайор ёмкости 7d окна
    pub reserve5h: f64, // запас 5h-окна (доля; деф 0.10 = бережём 10%)
    pub reserve7d: f64, // запас 7d-окна (доля; деф 0.03)
    pub reserve_jitter: f64, // ± разброс порога между подписками (антифингерпринт; деф 0.02)
    pub readiness_delay_secs: u64, // задержка после снятия readiness перед дренажем (деф 3с)
    pub drain_deadline_secs: u64, // предел graceful-дренажа до принудительного обрыва
    pub max_inflight: i64, // потолок параллельных запросов на подписку (деф 6; выше = больше параллели, риск бана)
    /// Optional shared L2 for ephemeral cache affinity. PostgreSQL remains authoritative.
    pub redis_url: Option<String>,
    pub affinity_secret: Option<String>,
    pub affinity_ttl_secs: u64,
    pub affinity_local_ttl_secs: u64,
    pub affinity_redis_timeout_ms: u64,
    /// Optional second provider. Disabled by default; enabling it requires the encrypted OAuth
    /// roster (sealed ChatGPT profiles) and its keyring.
    pub codex: Option<CodexConfig>,
    /// Native Gemini provider. It is instantiated only by the startup-fixed Gemini service.
    pub gemini: Option<GeminiConfig>,
    /// Compile-versioned evaluator capability evidence, assembled by trusted server composition.
    pub pricing_shadow_manifest: registry::pricing::PricingRuntimeManifestEvidence,
    pub proxy: ProxyConfig,
}

fn ev(k: &str) -> Option<String> {
    env::var(k)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}
fn ev_or(k: &str, d: &str) -> String {
    ev(k).unwrap_or_else(|| d.to_string())
}

const OPENAI_SHARED_DRAIN_DEADLINE_SECS: u64 = 620;

/// The native Codex transport owns no child processes and no home directories, so HTTP slots can
/// overlap during a cutover exactly like the Gemini fleet: the replacement is already
/// authenticated and serving through Caddy before the old slot stops, and a ten-minute turn may
/// finish gracefully.
fn provider_drain_deadline(provider: ProviderMode, configured: u64) -> u64 {
    match provider.serves_openai() {
        true => configured.max(OPENAI_SHARED_DRAIN_DEADLINE_SECS),
        false => configured,
    }
}
fn ev_bool(k: &str, d: bool) -> bool {
    match ev(k) {
        Some(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"),
        None => d,
    }
}
fn ev_opt_in(k: &str) -> bool {
    ev(k).is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn parse_provider_mode(value: Option<&str>) -> Result<ProviderMode, String> {
    match value
        .unwrap_or("combined")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "combined" => Ok(ProviderMode::Combined),
        "anthropic" => Ok(ProviderMode::Anthropic),
        "openai" => Ok(ProviderMode::OpenAi),
        "gemini" => Ok(ProviderMode::Gemini),
        other => Err(format!(
            "CLAUDE_API_PROVIDER={other:?}: expected combined, anthropic, openai, or gemini"
        )),
    }
}

fn parse_strict_bool(k: &str, value: Option<&str>, default: bool) -> Result<bool, String> {
    match value {
        None => Ok(default),
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        Some(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Some(value) => Err(format!(
            "{k}={value:?}: expected exactly 0, 1, false, or true"
        )),
    }
}

fn parse_pricing_bridge_config(
    enabled: Option<&str>,
    sample_bp: Option<&str>,
) -> Result<PricingBridgeConfig, String> {
    let enabled = parse_strict_bool("CLAUDE_API_PRICING_BRIDGE_ENABLED", enabled, false)?;
    let sample_bp = sample_bp.unwrap_or("0").parse::<i64>().map_err(|_| {
        "CLAUDE_API_PRICING_BRIDGE_SAMPLE_BP: expected an integer in 0..=10000".to_string()
    })?;
    PricingBridgeConfig::from_parts(enabled, sample_bp).map_err(|error| {
        format!(
            "invalid pricing bridge config ({}): disabled requires sample 0; enabled requires \
             sample 1..=10000",
            error.code()
        )
    })
}

const PRICING_SHADOW_ENV_KEYS: [&str; 11] = [
    "CLAUDE_API_PRICING_SHADOW_ENABLED",
    "CLAUDE_API_PRICING_SHADOW_SAMPLE_BP",
    "CLAUDE_API_PRICING_SHADOW_QUEUE_CAPACITY",
    "CLAUDE_API_PRICING_SHADOW_WORKER_CONCURRENCY",
    "CLAUDE_API_PRICING_SHADOW_TIMEOUT_MS",
    "CLAUDE_API_PRICING_SHADOW_MAX_QUEUE_AGE_SECS",
    "CLAUDE_API_PRICING_SHADOW_MAX_FIELD_BYTES",
    "CLAUDE_API_PRICING_SHADOW_MAX_ITEM_BYTES",
    "CLAUDE_API_PRICING_SHADOW_RATE_PER_SEC",
    "CLAUDE_API_PRICING_SHADOW_RATE_BURST",
    "CLAUDE_API_PRICING_SHADOW_DB_READ_CONNECTIONS",
];

fn parse_pricing_shadow_config(
    values: &BTreeMap<String, String>,
) -> Result<PricingShadowConfig, String> {
    let defaults = PricingShadowConfigValues::default();
    let parse_i64 = |name: &str, default: i64| -> Result<i64, String> {
        values.get(name).map_or(Ok(default), |value| {
            value
                .parse::<i64>()
                .map_err(|_| format!("{name}: expected a base-10 integer"))
        })
    };
    let parse_u64 = |name: &str, default: u64| -> Result<u64, String> {
        values.get(name).map_or(Ok(default), |value| {
            value
                .parse::<u64>()
                .map_err(|_| format!("{name}: expected a non-negative base-10 integer"))
        })
    };
    let parse_usize = |name: &str, default: usize| -> Result<usize, String> {
        let value = parse_u64(name, default as u64)?;
        usize::try_from(value).map_err(|_| format!("{name}: value does not fit usize"))
    };
    let configured = PricingShadowConfigValues {
        enabled: parse_strict_bool(
            "CLAUDE_API_PRICING_SHADOW_ENABLED",
            values
                .get("CLAUDE_API_PRICING_SHADOW_ENABLED")
                .map(String::as_str),
            defaults.enabled,
        )?,
        sample_bp: parse_i64("CLAUDE_API_PRICING_SHADOW_SAMPLE_BP", defaults.sample_bp)?,
        queue_capacity: parse_usize(
            "CLAUDE_API_PRICING_SHADOW_QUEUE_CAPACITY",
            defaults.queue_capacity,
        )?,
        worker_concurrency: parse_usize(
            "CLAUDE_API_PRICING_SHADOW_WORKER_CONCURRENCY",
            defaults.worker_concurrency,
        )?,
        timeout_ms: parse_u64("CLAUDE_API_PRICING_SHADOW_TIMEOUT_MS", defaults.timeout_ms)?,
        max_queue_age_secs: parse_i64(
            "CLAUDE_API_PRICING_SHADOW_MAX_QUEUE_AGE_SECS",
            defaults.max_queue_age_secs,
        )?,
        max_field_bytes: parse_usize(
            "CLAUDE_API_PRICING_SHADOW_MAX_FIELD_BYTES",
            defaults.max_field_bytes,
        )?,
        max_item_bytes: parse_usize(
            "CLAUDE_API_PRICING_SHADOW_MAX_ITEM_BYTES",
            defaults.max_item_bytes,
        )?,
        rate_per_sec: parse_u64(
            "CLAUDE_API_PRICING_SHADOW_RATE_PER_SEC",
            defaults.rate_per_sec,
        )?,
        rate_burst: parse_u64("CLAUDE_API_PRICING_SHADOW_RATE_BURST", defaults.rate_burst)?,
        db_read_connections: parse_usize(
            "CLAUDE_API_PRICING_SHADOW_DB_READ_CONNECTIONS",
            defaults.db_read_connections,
        )?,
    };
    PricingShadowConfig::from_values(configured).map_err(|error| {
        format!(
            "invalid pricing shadow config ({}); see bounded rollout limits",
            error.code()
        )
    })
}

fn pricing_shadow_runtime_manifest() -> registry::pricing::PricingRuntimeManifestEvidence {
    let capability = registry::pricing::PricingRuntimeCapabilityEvidence::new(
        registry::pricing::PRICING_SCHEMA_VERSION,
        1,
        "sha256:v1:88da6b622727dda8aac0e1cd1749524f4929f7738f097c2dd3b81ba1cc14e7fd",
    )
    .expect("built-in pricing evaluator capability is valid");
    registry::pricing::PricingRuntimeManifestEvidence::new(1, vec![capability])
        .expect("built-in pricing evaluator manifest is valid")
}

fn bounded_u64(k: &str, default: u64, min: u64, max: u64) -> u64 {
    match ev(k).and_then(|value| value.parse::<u64>().ok()) {
        Some(value) => value.clamp(min, max),
        None => default.clamp(min, max),
    }
}

fn bounded_usize(k: &str, default: usize, min: usize, max: usize) -> usize {
    bounded_u64(k, default as u64, min as u64, max as u64) as usize
}

fn bounded_i64(k: &str, default: i64, min: i64, max: i64) -> i64 {
    match ev(k).and_then(|value| value.parse::<i64>().ok()) {
        Some(value) => value.clamp(min, max),
        None => default.clamp(min, max),
    }
}

fn finite_nonnegative(k: &str, default: f64, max: f64) -> f64 {
    match ev(k).and_then(|value| value.parse::<f64>().ok()) {
        Some(value) if value.is_finite() => value.clamp(0.0, max),
        _ => default,
    }
}

fn parse_mult_bp(k: &str, v: &str, allow_zero: bool) -> Result<i64, String> {
    let parsed = v
        .parse::<i64>()
        .map_err(|_| format!("{k}={v:?}: ожидается целое число в диапазоне 1..=10000"))?;
    if (1..=10_000).contains(&parsed) || (allow_zero && parsed == 0) {
        Ok(parsed)
    } else if parsed == 0 {
        Err(format!(
            "{k}=0 запрещён без CLAUDE_API_ALLOW_ZERO_MULT_BP=1 (явный opt-in бесплатного тарифа)"
        ))
    } else {
        Err(format!(
            "{k}={parsed}: значение должно быть в диапазоне 1..=10000"
        ))
    }
}

fn ev_mult_bp(k: &str, d: i64, allow_zero: bool) -> i64 {
    match ev(k) {
        None => d,
        Some(v) => parse_mult_bp(k, &v, allow_zero).unwrap_or_else(|msg| panic!("{msg}")),
    }
}

fn validate_upstream(v: &str, allow_insecure_loopback: bool) -> Result<String, String> {
    let uri = v
        .parse::<axum::http::Uri>()
        .map_err(|_| "CLAUDE_API_UPSTREAM: ожидается абсолютный URL".to_string())?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| "CLAUDE_API_UPSTREAM: URL должен содержать схему".to_string())?;
    let authority = uri
        .authority()
        .ok_or_else(|| "CLAUDE_API_UPSTREAM: URL должен содержать host".to_string())?;

    if authority.as_str().contains('@') {
        return Err("CLAUDE_API_UPSTREAM: userinfo в URL запрещён".to_string());
    }
    if v.contains('#') || uri.query().is_some() || !matches!(uri.path(), "" | "/") {
        return Err("CLAUDE_API_UPSTREAM: path, query и fragment запрещены".to_string());
    }

    if scheme.eq_ignore_ascii_case("https")
        && authority.as_str().eq_ignore_ascii_case("api.anthropic.com")
    {
        return Ok("https://api.anthropic.com".to_string());
    }

    let host = authority.host();
    let ip_literal = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    let loopback = ip_literal
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_loopback());
    if allow_insecure_loopback && scheme.eq_ignore_ascii_case("http") && loopback {
        return Ok(v.trim_end_matches('/').to_string());
    }

    Err("CLAUDE_API_UPSTREAM: разрешён только https://api.anthropic.com; HTTP loopback требует CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1".to_string())
}

fn ev_upstream() -> String {
    let upstream = ev_or("CLAUDE_API_UPSTREAM", "https://api.anthropic.com");
    validate_upstream(
        &upstream,
        ev_opt_in("CLAUDE_API_ALLOW_INSECURE_LOOPBACK_UPSTREAM"),
    )
    .unwrap_or_else(|msg| panic!("{msg}"))
}

fn validate_gemini_upstream(v: &str, allow_insecure_loopback: bool) -> Result<String, String> {
    let uri = v
        .parse::<axum::http::Uri>()
        .map_err(|_| "CLAUDE_API_GEMINI_UPSTREAM: expected an absolute URL".to_string())?;
    let scheme = uri
        .scheme_str()
        .ok_or_else(|| "CLAUDE_API_GEMINI_UPSTREAM: URL must contain a scheme".to_string())?;
    let authority = uri
        .authority()
        .ok_or_else(|| "CLAUDE_API_GEMINI_UPSTREAM: URL must contain a host".to_string())?;
    if authority.as_str().contains('@')
        || v.contains('#')
        || uri.query().is_some()
        || !matches!(uri.path(), "" | "/")
    {
        return Err(
            "CLAUDE_API_GEMINI_UPSTREAM: userinfo, path, query and fragment are forbidden"
                .to_string(),
        );
    }
    if scheme.eq_ignore_ascii_case("https") {
        let canonical = [
            "daily-cloudcode-pa.sandbox.googleapis.com",
            "daily-cloudcode-pa.googleapis.com",
            "cloudcode-pa.googleapis.com",
        ]
        .into_iter()
        .find(|allowed| authority.as_str().eq_ignore_ascii_case(allowed));
        if let Some(host) = canonical {
            return Ok(format!("https://{host}"));
        }
    }
    let host = authority.host();
    let literal = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
        .unwrap_or(host);
    let loopback = literal
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback());
    if allow_insecure_loopback && scheme.eq_ignore_ascii_case("http") && loopback {
        return Ok(v.trim_end_matches('/').to_string());
    }
    Err("CLAUDE_API_GEMINI_UPSTREAM: only official Antigravity Cloud Code hosts are allowed; literal HTTP loopback requires CLAUDE_API_GEMINI_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1".to_string())
}

fn gemini_config() -> Option<GeminiConfig> {
    // Gemini is on by default. The deploy gates now assert the real enabled-surface envelope
    // (400 API_KEY_INVALID), which holds for an empty pre-onboarding roster, so enabling the
    // provider no longer couples a routine engine deploy to having live subscriptions.
    // CLAUDE_API_GEMINI_ENABLED stays only as an emergency kill-switch (set it to 0 to withdraw
    // the surface without a release).
    if !ev_bool("CLAUDE_API_GEMINI_ENABLED", true) {
        return None;
    }
    let requested = ev_or(
        "CLAUDE_API_GEMINI_MODELS",
        // Public Gemini 3 ids require reviewed canonical→private tier routing. A model enters this
        // production default only after generate + native stream + countTokens pass on every
        // supported thinking level of the current subscription profile. In particular, 2.5 Pro
        // advertises quota but generation returns persistent UNAVAILABLE; quota evidence alone
        // never enables a model.
        "gemini-3.1-flash-image,gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-pro-preview,gemini-3.1-flash-lite,gemini-2.5-flash,gemini-2.5-flash-lite",
    )
    .split(',')
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>();
    let catalog = metering::gemini_catalog_at(pool::now());
    let mut models = Vec::with_capacity(requested.len());
    for requested in requested {
        let Some(spec) = catalog.iter().find(|spec| spec.id == requested) else {
            panic!(
                "CLAUDE_API_GEMINI_MODELS contains unsupported model {requested:?}; use a model from the pinned Gemini price catalog"
            );
        };
        if !forward::gemini::subscription_model_supported(spec.id) {
            panic!(
                "CLAUDE_API_GEMINI_MODELS contains {requested:?}, which has no reviewed generation route on the Antigravity subscription surface"
            );
        }
        if models.iter().any(|model: &GeminiModel| model.id == spec.id) {
            continue;
        }
        models.push(GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        });
    }
    if models.is_empty() {
        panic!("CLAUDE_API_GEMINI_MODELS must contain at least one model");
    }
    let profiles_file = ev_or(
        "CLAUDE_API_GEMINI_PROFILES_FILE",
        "/srv/claude-api/data/gemini/profiles.json",
    );
    if !std::path::Path::new(&profiles_file).is_absolute() {
        panic!("CLAUDE_API_GEMINI_PROFILES_FILE must be an absolute path");
    }
    let upstream = validate_gemini_upstream(
        &ev_or(
            "CLAUDE_API_GEMINI_UPSTREAM",
            "https://daily-cloudcode-pa.sandbox.googleapis.com",
        ),
        ev_opt_in("CLAUDE_API_GEMINI_ALLOW_INSECURE_LOOPBACK_UPSTREAM"),
    )
    .unwrap_or_else(|message| panic!("{message}"));
    let credential_keys = gemini_credential::CredentialKeyring::parse(
        &ev("CLAUDE_API_GEMINI_CREDENTIAL_KEYS").unwrap_or_else(|| {
            panic!("CLAUDE_API_GEMINI_CREDENTIAL_KEYS is required for the encrypted OAuth roster")
        }),
    )
    .unwrap_or_else(|_| panic!("CLAUDE_API_GEMINI_CREDENTIAL_KEYS is invalid"));
    Some(GeminiConfig {
        enabled: true,
        upstream,
        profiles_file,
        credential_keys,
        models,
        connect_timeout_secs: bounded_u64("CLAUDE_API_GEMINI_CONNECT_TIMEOUT_SECS", 30, 1, 120),
        read_timeout_secs: bounded_u64("CLAUDE_API_GEMINI_READ_TIMEOUT_SECS", 120, 15, 600),
        max_transport_retries: bounded_usize("CLAUDE_API_GEMINI_MAX_TRANSPORT_RETRIES", 1, 0, 5),
        max_inflight_per_profile: bounded_usize(
            "CLAUDE_API_GEMINI_MAX_INFLIGHT_PER_PROFILE",
            6,
            1,
            64,
        ),
        auth_quarantine_secs: bounded_i64(
            "CLAUDE_API_GEMINI_AUTH_QUARANTINE_SECS",
            900,
            60,
            86_400,
        ),
        transport_cool_secs: bounded_i64("CLAUDE_API_GEMINI_TRANSPORT_COOL_SECS", 5, 1, 300),
        model_failure_cool_secs: bounded_i64(
            "CLAUDE_API_GEMINI_MODEL_FAILURE_COOL_SECS",
            15,
            1,
            600,
        ),
        model_failure_max_cool_secs: bounded_i64(
            "CLAUDE_API_GEMINI_MODEL_FAILURE_MAX_COOL_SECS",
            900,
            15,
            86_400,
        ),
        default_rate_limit_cool_secs: bounded_i64(
            "CLAUDE_API_GEMINI_RATE_LIMIT_COOL_SECS",
            60,
            1,
            86_400,
        ),
        quota_reserve_fraction: ev_frac("CLAUDE_API_GEMINI_QUOTA_RESERVE", 0.05),
        quota_reserve_jitter: ev_frac("CLAUDE_API_GEMINI_QUOTA_RESERVE_JITTER", 0.01),
        health_probe_interval_secs: bounded_u64(
            "CLAUDE_API_GEMINI_HEALTH_INTERVAL_SECS",
            300,
            30,
            3_600,
        ),
        reserve_overhead_tokens: bounded_u64(
            "CLAUDE_API_GEMINI_RESERVE_OVERHEAD_TOKENS",
            8_192,
            0,
            262_144,
        ),
        antigravity_version: ev_or(
            "CLAUDE_API_GEMINI_ANTIGRAVITY_VERSION",
            gemini_credential::ANTIGRAVITY_VERSION,
        ),
        node_binary: ev_or(
            "CLAUDE_API_GEMINI_NODE_BINARY",
            gemini_credential::GEMINI_NODE_BINARY,
        ),
        node_version: ev_or(
            "CLAUDE_API_GEMINI_NODE_VERSION",
            gemini_credential::GEMINI_NODE_VERSION,
        ),
        node_sha256: ev_or(
            "CLAUDE_API_GEMINI_NODE_SHA256",
            gemini_credential::GEMINI_NODE_SHA256,
        ),
    })
}

/// Advertised OpenAI-compatible models, resolved from the audited catalog in `metering`.
///
/// The composition layer only chooses which pinned ids are enabled; it never declares a rate. A
/// price change is a reviewed `metering::codex` commit with an effective date, exactly like the
/// Claude tariffs.
fn codex_model_catalog(now_unix: i64) -> Vec<CodexModel> {
    metering::codex_catalog_at(now_unix)
        .into_iter()
        .map(|spec| CodexModel {
            id: spec.id.to_string(),
            upstream: spec.upstream.to_string(),
            // Public `/v1/models` keeps the field for SDK compatibility without inventing an
            // upstream creation timestamp that the backend does not provide.
            created: 0,
            owned_by: "apitoken".to_string(),
            max_output_tokens: spec.max_output_tokens,
            reasoning_efforts: spec
                .reasoning_efforts
                .iter()
                .map(|effort| (*effort).to_string())
                .collect(),
            fast_multiplier_basis_points: spec.fast_multiplier_basis_points,
            prices: spec.prices,
        })
        .collect()
}

/// Sealed Codex profiles live in one roster JSON (`profiles: [{id, credential_file}]`) next to
/// `<roster>/credentials/<id>.json`. The authbot republishes it atomically; the gateway rescans it
/// on every health tick, so a purchased account joins without a restart.
fn codex_profiles_file() -> String {
    let profiles_file = ev_or(
        "CLAUDE_API_CODEX_PROFILES_FILE",
        "/srv/claude-api/data/codex/profiles.json",
    );
    if !std::path::Path::new(&profiles_file).is_absolute() {
        panic!("CLAUDE_API_CODEX_PROFILES_FILE must be an absolute path");
    }
    profiles_file
}

/// The native backend base URL. HTTPS is required outside an explicit loopback opt-in used only
/// by integration tests.
fn codex_base_url() -> String {
    let base_url = ev_or(
        "CLAUDE_API_CODEX_BASE_URL",
        codex_credential::CODEX_DEFAULT_BASE_URL,
    );
    let loopback = ev_opt_in("CLAUDE_API_CODEX_ALLOW_INSECURE_LOOPBACK_UPSTREAM")
        && (base_url.starts_with("http://127.0.0.1:") || base_url.starts_with("http://[::1]:"));
    if !base_url.starts_with("https://") && !loopback {
        panic!("CLAUDE_API_CODEX_BASE_URL must be https (loopback requires an explicit opt-in)");
    }
    base_url.trim_end_matches('/').to_string()
}

fn codex_config(redis_url: Option<String>, history_secret: Option<String>) -> Option<CodexConfig> {
    if !ev_bool("CLAUDE_API_CODEX_ENABLED", false) {
        return None;
    }
    let requested_models = ev_or(
        "CLAUDE_API_CODEX_MODELS",
        "gpt-5.6,gpt-5.6-sol,gpt-5.6-terra,gpt-5.6-luna,gpt-5.5,gpt-5.4",
    )
    .split(',')
    .map(str::trim)
    .filter(|value| !value.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>();
    let catalog = codex_model_catalog(pool::now());
    let mut models = Vec::with_capacity(requested_models.len());
    for requested in requested_models {
        let Some(model) = catalog.iter().find(|model| model.id == requested) else {
            panic!(
                "CLAUDE_API_CODEX_MODELS contains unsupported model {requested:?}; \
                 use a model from the pinned Codex price catalog"
            );
        };
        if models
            .iter()
            .any(|existing: &CodexModel| existing.id == model.id)
        {
            continue;
        }
        models.push(model.clone());
    }
    if models.is_empty() {
        panic!("CLAUDE_API_CODEX_MODELS must contain at least one model");
    }

    let credential_keys = codex_credential::CredentialKeyring::parse(
        &ev("CLAUDE_API_CODEX_CREDENTIAL_KEYS").unwrap_or_else(|| {
            panic!("CLAUDE_API_CODEX_CREDENTIAL_KEYS is required for the encrypted OAuth roster")
        }),
    )
    .unwrap_or_else(|_| panic!("CLAUDE_API_CODEX_CREDENTIAL_KEYS is invalid"));

    let mut default_proxy_env = BTreeMap::new();
    for name in [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "ALL_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "all_proxy",
        "no_proxy",
    ] {
        if let Some(value) = ev(name) {
            default_proxy_env.insert(name.to_string(), value);
        }
    }

    Some(CodexConfig {
        enabled: true,
        base_url: codex_base_url(),
        profiles_file: codex_profiles_file(),
        credential_keys,
        cli_version: ev_or(
            "CLAUDE_API_CODEX_CLI_VERSION",
            codex_credential::CODEX_CLI_VERSION,
        ),
        request_timeout_ms: bounded_u64(
            "CLAUDE_API_CODEX_REQUEST_TIMEOUT_MS",
            ev("CLAUDE_API_CODEX_RPC_TIMEOUT_MS")
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(15_000),
            500,
            120_000,
        ),
        turn_timeout_ms: bounded_u64("CLAUDE_API_CODEX_TURN_TIMEOUT_MS", 600_000, 5_000, 600_000),
        // Generous on purpose: this must never cut short a model that is genuinely thinking, only
        // catch one that has stopped answering. It is a liveness bound, not a latency budget.
        turn_silence_timeout_ms: bounded_u64(
            "CLAUDE_API_CODEX_TURN_SILENCE_TIMEOUT_MS",
            180_000,
            5_000,
            600_000,
        ),
        health_probe_interval_secs: bounded_u64(
            "CLAUDE_API_CODEX_HEALTH_INTERVAL_SECS",
            10,
            10,
            3_600,
        ),
        // Мягкий запас окон на профиль: не роутим выше ~90% 5h и ~97% недельного окна — подписка
        // не упирается в стену (меньше 429, нет отпечатка автомата, максящего квоту под ноль).
        // Общие fleet-ключи по умолчанию, codex-специфичные могут переопределить.
        // GPT-окна короче недели ограничиваем мягко: не выше 98% (не наследуем Claude-флотские
        // 10% — у Codex иной профиль риска и сейчас фактически только недельный лимит).
        reserve_5h: ev("CLAUDE_API_CODEX_RESERVE_5H")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| ev_frac("CLAUDE_API_CODEX_RESERVE_5H", 0.02)),
        reserve_7d: ev("CLAUDE_API_CODEX_RESERVE_7D")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| ev_frac("CLAUDE_API_RESERVE_7D", 0.03)),
        reserve_jitter: ev("CLAUDE_API_CODEX_RESERVE_JITTER")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or_else(|| ev_frac("CLAUDE_API_RESERVE_JITTER", 0.02)),
        reserve_overhead_tokens: bounded_u64(
            "CLAUDE_API_CODEX_RESERVE_OVERHEAD_TOKENS",
            16_384,
            0,
            262_144,
        ),
        history_ttl_secs: bounded_u64(
            "CLAUDE_API_CODEX_HISTORY_TTL_SECS",
            24 * 3600,
            60,
            7 * 24 * 3600,
        ),
        history_local_cap: bounded_usize("CLAUDE_API_CODEX_HISTORY_LOCAL_CAP", 10_000, 16, 100_000),
        history_redis_url: redis_url,
        history_secret,
        history_redis_timeout_ms: bounded_u64(
            "CLAUDE_API_CODEX_HISTORY_REDIS_TIMEOUT_MS",
            1_000,
            1,
            5_000,
        ),
        default_proxy_env,
        models,
    })
}

/// UA-список из env. Разделитель `|`, а НЕ `,`: реальный UA Claude Code содержит запятую
/// (`(external, sdk-cli)`), split(',') порвал бы одиночный UA на фрагменты → битый UA всему флоту.
fn split_ua_list(s: &str) -> Vec<String> {
    s.split('|')
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}
/// Доля [0,1] из env с КЛАМПОМ + предупреждением при выходе за диапазон. Частая ошибка оператора:
/// `RESERVE_7D=3` (имел в виду 3%) → 3.0 → «резерв 300%» → пул считается исчерпанным → отказы всем.
/// Кламп страхует деньги/ёмкость; не-число → дефолт (с логом, не тихо).
fn ev_frac(k: &str, d: f64) -> f64 {
    match ev(k) {
        None => d,
        Some(v) => clamp_frac(k, &v, d),
    }
}
/// Чистая логика ev_frac (тестируемая без env): парс доли + кламп/предупреждение.
fn clamp_frac(k: &str, v: &str, d: f64) -> f64 {
    match v.parse::<f64>() {
        Ok(x) if x.is_finite() && (0.0..=1.0).contains(&x) => x,
        Ok(x) if x.is_finite() => {
            eprintln!("⚠ {k}={x} вне [0,1] — кламплю (это ДОЛЯ окна, не проценты)");
            x.clamp(0.0, 1.0)
        }
        Ok(_) => {
            eprintln!("⚠ {k} не может быть NaN/inf — беру дефолт {d}");
            d
        }
        Err(_) => {
            eprintln!("⚠ {k}={v:?} не число — беру дефолт {d}");
            d
        }
    }
}

impl Settings {
    pub fn from_env() -> Self {
        let provider = parse_provider_mode(ev("CLAUDE_API_PROVIDER").as_deref())
            .unwrap_or_else(|message| panic!("{message}"));
        let cfg_dir = ev("SUB_CFG_DIR").unwrap_or_else(|| {
            let home = env::var("HOME").unwrap_or_default();
            format!("{home}/.config/claude-api")
        });
        let db_path = ev("SUBS_DB").unwrap_or_else(|| format!("{cfg_dir}/subscriptions.db"));
        let database_url = ev("CLAUDE_API_DATABASE_URL");
        let instance_id = ev("CLAUDE_API_INSTANCE_ID").unwrap_or_else(|| {
            let host = ev("HOSTNAME").unwrap_or_else(|| "engine".into());
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            format!("{host}:{}:{ts:x}", std::process::id())
        });
        let host = ev_or("CLAUDE_API_HOST", "0.0.0.0");
        let port = ev_or("CLAUDE_API_PORT", "8787");
        // Доверять loopback-админу — ТОЛЬКО при явном opt-in `CLAUDE_API_TRUST_LOOPBACK=1` И реальном
        // loopback-bind. Без opt-in — false даже на loopback: закрывает footgun «за реверс-прокси
        // (nginx→127.0.0.1) все пиры видны как 127.0.0.1 → аноним получает админ-доступ». Экспонированный
        // bind (0.0.0.0) не доверяет loopback никогда; управляющие роуты требуют CLAUDE_API_KEYS.
        let trust_loopback = ev("CLAUDE_API_TRUST_LOOPBACK")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
            && matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost");
        let parse_keys = |name: &str| {
            ev(name)
                .map(|s| {
                    s.split(',')
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let api_keys = parse_keys("CLAUDE_API_KEYS");
        let control_keys = parse_keys("CLAUDE_API_CONTROL_KEY");
        let panel_keys = parse_keys("CLAUDE_API_PANEL_KEY");
        // Слабый управляющий ключ = онлайн-брутфорс денег/форвардинга (throttle нет — см. reverse-proxy).
        // Предупреждаем громко при старте, если admin/control-ключ короче 24 символов (наши генераторы
        // дают 48-hex; короткий = операторская парольная фраза, брутфорсибельна).
        for (name, keys) in [
            ("CLAUDE_API_KEYS", &api_keys),
            ("CLAUDE_API_CONTROL_KEY", &control_keys),
        ] {
            if keys.iter().any(|k| k.len() < 24) {
                eprintln!(
                    "⚠️  {name}: есть ключ короче 24 символов — слабый для управляющего доступа. \
                           Задай длинный случайный (напр. openssl rand -hex 24)."
                );
            }
        }
        // Наценка по умолчанию (×0.20): нужна и в Settings, и в ProxyConfig (для /admin/account) →
        // считаем один раз в локальную переменную. Ноль разрешён только явным opt-in бесплатного тарифа;
        // отрицательные/слишком большие/нечисловые значения аварийно останавливают запуск.
        let mult_bp = ev_mult_bp(
            "CLAUDE_API_MULT_BP",
            2000,
            ev_opt_in("CLAUDE_API_ALLOW_ZERO_MULT_BP"),
        );
        let pricing_bridge = parse_pricing_bridge_config(
            ev("CLAUDE_API_PRICING_BRIDGE_ENABLED").as_deref(),
            ev("CLAUDE_API_PRICING_BRIDGE_SAMPLE_BP").as_deref(),
        )
        .unwrap_or_else(|message| panic!("{message}"));
        let pricing_shadow_values = PRICING_SHADOW_ENV_KEYS
            .into_iter()
            .filter_map(|name| ev(name).map(|value| (name.to_owned(), value)))
            .collect::<BTreeMap<_, _>>();
        let pricing_shadow = parse_pricing_shadow_config(&pricing_shadow_values)
            .unwrap_or_else(|message| panic!("{message}"));
        let redis_url = ev("CLAUDE_API_REDIS_URL");
        let affinity_secret = ev("CLAUDE_API_AFFINITY_SECRET");
        let codex = if provider.serves_openai() {
            codex_config(redis_url.clone(), affinity_secret.clone())
        } else {
            None
        };
        let gemini = if provider.serves_gemini() {
            gemini_config()
        } else {
            None
        };
        Settings {
            provider,
            db_path,
            database_url,
            instance_id,
            bind: format!("{host}:{port}"),
            fleet: ev("SUBS_FLEET").filter(|f| f != "all"),
            billing: ev_bool("CLAUDE_API_BILLING", true),
            // Default preserves the previous host-sized behaviour; the bound exists so a deployment
            // whose authority is shared can fit every slot — plus the extra generation a blue-green
            // cutover runs — inside the server's connection limit.
            billing_readers: bounded_usize(
                "CLAUDE_API_BILLING_READERS",
                std::thread::available_parallelism()
                    .map(|n| n.get().clamp(4, 16))
                    .unwrap_or(4),
                1,
                64,
            ),
            // Наценка по умолчанию: клиент платит 20% от реального API-эквивалента (×0.20).
            mult_bp,
            // Прайоры ёмкости окон (0 → дефолт пула под Max 20x; калибровка их уточняет).
            cap5h_usd: finite_nonnegative("CLAUDE_API_CAP5H_USD", 0.0, 1_000_000.0),
            cap7d_usd: finite_nonnegative("CLAUDE_API_CAP7D_USD", 0.0, 10_000_000.0),
            // Запас окон (headroom) с джиттером: бережём 10%/3%, порог отсечения слегка разный по
            // подпискам, чтобы флот не резался на одном проценте (антифингерпринт).
            reserve5h: ev_frac("CLAUDE_API_RESERVE_5H", 0.10),
            reserve7d: ev_frac("CLAUDE_API_RESERVE_7D", 0.03),
            reserve_jitter: ev_frac("CLAUDE_API_RESERVE_JITTER", 0.02),
            // Fail-closed clamps: readiness-delay ≤ 30с; общий drain-deadline в [5, 595]с.
            // OpenAI-слоты получают полный 620с drain: новый slot уже авторизован и
            // обслуживает трафик, поэтому старый может спокойно закончить десятиминутный turn.
            readiness_delay_secs: bounded_u64("CLAUDE_API_READINESS_DELAY_SECS", 3, 0, 30),
            drain_deadline_secs: provider_drain_deadline(
                provider,
                bounded_u64("CLAUDE_API_DRAIN_DEADLINE_SECS", 540, 5, 595),
            ),
            // Потолок параллельных запросов на подписку. Дефолт 6 (человеческий конверт/анти-бан).
            // Высокое значение снимает потолок concurrency — больше параллели ценой риска бан-сигнала.
            max_inflight: bounded_i64("CLAUDE_API_MAX_INFLIGHT", 6, 1, 1_024),
            redis_url,
            affinity_secret,
            affinity_ttl_secs: bounded_u64(
                "CLAUDE_API_AFFINITY_TTL_SECS",
                3600,
                60,
                24 * 3600,
            ),
            affinity_local_ttl_secs: bounded_u64(
                "CLAUDE_API_AFFINITY_LOCAL_TTL_SECS",
                300,
                1,
                3600,
            ),
            affinity_redis_timeout_ms: bounded_u64(
                "CLAUDE_API_AFFINITY_REDIS_TIMEOUT_MS",
                35,
                1,
                500,
            ),
            codex,
            gemini,
            pricing_shadow_manifest: pricing_shadow_runtime_manifest(),
            proxy: ProxyConfig {
                api_keys,
                control_keys,
                panel_keys,
                default_mult_bp: mult_bp,
                pricing_bridge,
                pricing_shadow,
                trust_loopback,
                // OAuth-токены можно отправлять только на канонический Anthropic origin. Локальный HTTP
                // mock разрешается исключительно явным opt-in и только на literal loopback IP.
                upstream: ev_upstream(),
                max_tries: bounded_usize("CLAUDE_API_MAX_TRIES", 3, 1, 10),
                // Fair-share: потолок одновременных запросов на клиентский ключ (кит не набивает флот).
                max_inflight_per_key: bounded_usize(
                    "CLAUDE_API_MAX_INFLIGHT_PER_KEY", 20, 1, 1_024,
                ) as u32,
                util_cap: ev_frac("CLAUDE_API_UTIL_CAP", 0.95),
                cool_secs: bounded_i64("CLAUDE_API_COOL_SECS", 300, 1, 8 * 24 * 3600),
                // Гладкий UX: тихий wait+retry ротации при транзиентной нехватке (деф 8с). 0 = выкл.
                smooth_wait_ms: bounded_u64("CLAUDE_API_SMOOTH_WAIT_MS", 8_000, 0, 60_000),
                affinity_wait_ms: bounded_u64(
                    "CLAUDE_API_AFFINITY_WAIT_MS",
                    250,
                    0,
                    2_000,
                ),
                affinity_wait_min_bytes: bounded_usize(
                    "CLAUDE_API_AFFINITY_WAIT_MIN_BYTES",
                    16 * 1024,
                    0,
                    32 * 1024 * 1024,
                ),
                poll: ev_bool("CLAUDE_API_POLL", true),
                inject_identity: ev_bool("CLAUDE_API_INJECT_IDENTITY", true),
                identity: ev_or("CLAUDE_API_IDENTITY", CLAUDE_CODE_IDENTITY),
                // billing-header (system[0]) — точный вид с живого claude 2.1.195 (mitm 2026-07-14):
                // `x-anthropic-billing-header: cc_version=2.1.195.d49; cc_entrypoint=sdk-cli; cch=<hex>;`
                // cc_version флот-константна (коген с UA), cch — per-персона (см. inject).
                inject_billing: ev_bool("CLAUDE_API_INJECT_BILLING", true),
                // БАЗА cc_version (без .dNN-суффикса); суффикс `.dNN` добавляем per-подписка в proxy
                // (persona_ccbuild) — живые захваты показали, что .dNN варьируется, фиксировать на флот
                // = кластер. refresh-fingerprint.sh кладёт сюда базу (срезает .dNN из живого захвата).
                cc_version: ev_or("CLAUDE_API_CC_VERSION", "2.1.195"),
                cc_entrypoint: ev_or("CLAUDE_API_CC_ENTRYPOINT", "sdk-cli"),
                // Полный CC-набор beta (не только oauth): без `claude-code-20250219` мы «OAuth-клиент, но
                // НЕ Claude Code». ТОЧНЫЙ актуальный набор снимает refresh-fingerprint.sh с живого claude
                // в config.env; это fallback. (Проверено живым /v1 — набор совместим с OAuth-подпиской.)
                // ТОЧНЫЙ набор снят с живого claude 2.1.195 (mitm-захват /v1/messages, 2026-07-14):
                // 10 бет, разделитель "," без пробела. Порядок ЗНАЧЕНИЙ внутри — Set-итерация (не
                // фингерпринт), важен НАБОР. extended-cache-ttl → ttl:"1h", prompt-caching-scope →
                // scope:"global" на cache_control (см. inject_identity). Проверено живым /v1 (200).
                default_beta: ev_or("CLAUDE_API_BETA",
                    "oauth-2025-04-20,interleaved-thinking-2025-05-14,thinking-token-count-2026-05-13,context-management-2025-06-27,prompt-caching-scope-2026-01-05,claude-code-20250219,advisor-tool-2026-03-01,advanced-tool-use-2025-11-20,extended-cache-ttl-2025-04-11,cache-diagnosis-2026-04-07"),
                // Дефолт-fallback; актуальное значение — env CLAUDE_API_UA (авто-рефреш скриптом).
                // CLAUDE_API_UA можно задать СПИСКОМ через `|` (пул реальных UA) — тогда каждая персона
                // пинит один. Иначе один UA + разброс patch-версии между персонами (ниже).
                // ВАЖНО: разделитель `|`, а НЕ `,` — реальный UA Claude Code содержит запятую
                // (`(external, sdk-cli)`), и split(',') порвал бы дефолт на фрагменты → битый UA флоту.
                user_agent: ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)"),
                user_agents: split_ua_list(&ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)")),
                // Разброс patch-версии UA между персонами (антифингерпринт флота). 0/1 → выключено.
                ua_spread: bounded_usize("CLAUDE_API_UA_SPREAD", 8, 1, 100) as u32,
                anthropic_version: ev_or("CLAUDE_API_ANTHROPIC_VERSION", "2023-06-01"),
                connect_timeout: bounded_u64("CLAUDE_API_CONNECT_TIMEOUT", 30, 1, 120),
                // Отпечаток Stainless-SDK клиента Claude Code. Дефолты — правдоподобные; ТОЧНЫЕ значения
                // снимаются с живого claude (refresh-fingerprint.sh) и кладутся в config.env. Флот-константны.
                x_app: ev_or("CLAUDE_API_X_APP", "cli"),
                stainless_lang: ev_or("CLAUDE_API_SL_LANG", "js"),
                stainless_runtime: ev_or("CLAUDE_API_SL_RUNTIME", "node"),
                // Сняты с живого claude 2.1.195 (mitm-захват 2026-07-14): runtime-version =
                // process.version бандл-Bun (v26.3.0), package-version = @anthropic-ai/sdk (0.94.0).
                // Коген с UA 2.1.195. (0.208.0 в бандле — версия ДРУГОГО пакета, не stainless.)
                stainless_runtime_version: ev_or("CLAUDE_API_SL_RT_VER", "v26.3.0"),
                stainless_package_version: ev_or("CLAUDE_API_SL_PKG_VER", "0.94.0"),
                stainless_os: ev_or("CLAUDE_API_SL_OS", "Linux"),
                stainless_arch: ev_or("CLAUDE_API_SL_ARCH", "x64"),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ua_list_default_stays_single_not_fragments() {
        // Регресс CRIT-бага: дефолтный UA содержит запятую — split должен дать ОДИН UA, не фрагменты.
        let v = split_ua_list("claude-cli/2.1.195 (external, sdk-cli)");
        assert_eq!(v.len(), 1, "запятая в UA не должна рвать его на куски");
        assert_eq!(v[0], "claude-cli/2.1.195 (external, sdk-cli)");
        // Пул задаётся через '|'
        assert_eq!(split_ua_list("a/1.0|b/2.0|c/3.0").len(), 3);
        assert!(split_ua_list("").is_empty());
        assert!(split_ua_list("  |  ").is_empty()); // пустые куски отфильтрованы
    }

    #[test]
    fn provider_mode_is_explicit_and_bounded() {
        assert_eq!(parse_provider_mode(None), Ok(ProviderMode::Combined));
        assert_eq!(
            parse_provider_mode(Some("anthropic")),
            Ok(ProviderMode::Anthropic)
        );
        assert_eq!(
            parse_provider_mode(Some(" OPENAI ")),
            Ok(ProviderMode::OpenAi)
        );
        assert_eq!(
            parse_provider_mode(Some(" GEMINI ")),
            Ok(ProviderMode::Gemini)
        );
        assert!(parse_provider_mode(Some("both")).is_err());
        assert!(parse_provider_mode(Some("codex")).is_err());
    }

    #[test]
    fn pricing_bridge_config_is_strict_default_off_and_accepts_only_bounded_rollout_samples() {
        let disabled = parse_pricing_bridge_config(None, None).unwrap();
        assert!(!disabled.enabled());
        assert_eq!(disabled.sample_bp(), 0);
        assert!(!parse_pricing_bridge_config(Some("false"), Some("0"))
            .unwrap()
            .enabled());
        assert!(!parse_pricing_bridge_config(Some("0"), Some("0"))
            .unwrap()
            .enabled());

        for invalid in ["yes", "off", "garbage", "2", "", " true ", "0 "] {
            assert!(parse_pricing_bridge_config(Some(invalid), Some("0")).is_err());
        }
        for invalid in ["-1", "10001", "garbage", "1.5"] {
            assert!(parse_pricing_bridge_config(Some("false"), Some(invalid)).is_err());
        }
        assert!(parse_pricing_bridge_config(Some("false"), Some("1")).is_err());
        assert!(parse_pricing_bridge_config(Some("true"), Some("0")).is_err());
        let sampled = parse_pricing_bridge_config(Some("true"), Some("1")).unwrap();
        assert!(sampled.enabled());
        assert_eq!(sampled.sample_bp(), 1);
        let full = parse_pricing_bridge_config(Some("1"), Some("10000")).unwrap();
        assert!(full.enabled());
        assert_eq!(full.sample_bp(), 10_000);
    }

    #[test]
    fn pricing_shadow_config_is_strict_default_off_and_all_limits_are_validated() {
        let empty = BTreeMap::new();
        let disabled = parse_pricing_shadow_config(&empty).unwrap();
        assert!(!disabled.enabled());
        assert_eq!(disabled.sample_bp(), 0);
        assert_eq!(disabled.queue_capacity(), 256);
        assert_eq!(disabled.worker_concurrency(), 2);
        assert_eq!(disabled.timeout_ms(), 750);
        assert_eq!(disabled.max_queue_age_secs(), 300);
        assert_eq!(disabled.max_field_bytes(), 512);
        assert_eq!(disabled.max_item_bytes(), 16 * 1024);
        assert_eq!(disabled.rate_per_sec(), 20);
        assert_eq!(disabled.rate_burst(), 40);
        assert_eq!(disabled.db_read_connections(), 2);

        let mut enabled = BTreeMap::from([
            (
                "CLAUDE_API_PRICING_SHADOW_ENABLED".to_owned(),
                "true".to_owned(),
            ),
            (
                "CLAUDE_API_PRICING_SHADOW_SAMPLE_BP".to_owned(),
                "1".to_owned(),
            ),
        ]);
        assert!(parse_pricing_shadow_config(&enabled).unwrap().enabled());
        enabled.insert(
            "CLAUDE_API_PRICING_SHADOW_MAX_QUEUE_AGE_SECS".to_owned(),
            (24 * 60 * 60).to_string(),
        );
        assert!(parse_pricing_shadow_config(&enabled).is_err());
        enabled.insert(
            "CLAUDE_API_PRICING_SHADOW_MAX_QUEUE_AGE_SECS".to_owned(),
            "300".to_owned(),
        );
        for (name, value) in [
            ("CLAUDE_API_PRICING_SHADOW_QUEUE_CAPACITY", "0"),
            ("CLAUDE_API_PRICING_SHADOW_WORKER_CONCURRENCY", "33"),
            ("CLAUDE_API_PRICING_SHADOW_TIMEOUT_MS", "0"),
            ("CLAUDE_API_PRICING_SHADOW_MAX_FIELD_BYTES", "1"),
            ("CLAUDE_API_PRICING_SHADOW_MAX_ITEM_BYTES", "999"),
            ("CLAUDE_API_PRICING_SHADOW_RATE_PER_SEC", "0"),
            ("CLAUDE_API_PRICING_SHADOW_RATE_BURST", "0"),
            ("CLAUDE_API_PRICING_SHADOW_DB_READ_CONNECTIONS", "0"),
        ] {
            let mut invalid = enabled.clone();
            invalid.insert(name.to_owned(), value.to_owned());
            assert!(parse_pricing_shadow_config(&invalid).is_err(), "{name}");
        }
        enabled.insert(
            "CLAUDE_API_PRICING_SHADOW_ENABLED".to_owned(),
            "false".to_owned(),
        );
        assert!(parse_pricing_shadow_config(&enabled).is_err());
    }

    #[test]
    fn pricing_shadow_manifest_is_fixed_registry_canonical_evidence() {
        let manifest = pricing_shadow_runtime_manifest();
        assert_eq!(manifest.manifest_generation(), 1);
        assert_eq!(manifest.capabilities().len(), 1);
        assert_eq!(
            manifest.capabilities()[0].pricing_schema_version(),
            registry::pricing::PRICING_SCHEMA_VERSION
        );
        assert_eq!(manifest.capabilities()[0].capability_generation(), 1);
        assert_eq!(
            manifest.capabilities()[0].capability_digest(),
            "sha256:v1:88da6b622727dda8aac0e1cd1749524f4929f7738f097c2dd3b81ba1cc14e7fd"
        );
        assert!(manifest.manifest_digest().starts_with("sha256:v1:"));
    }

    #[test]
    fn openai_slots_get_the_long_drain_and_other_providers_do_not() {
        assert_eq!(provider_drain_deadline(ProviderMode::OpenAi, 540), 620);
        assert_eq!(provider_drain_deadline(ProviderMode::Combined, 540), 620);
        assert_eq!(provider_drain_deadline(ProviderMode::OpenAi, 12), 620);
        assert_eq!(provider_drain_deadline(ProviderMode::Anthropic, 540), 540);
        assert_eq!(provider_drain_deadline(ProviderMode::Gemini, 540), 540);
    }

    #[test]
    fn frac_clamps_percent_typo_and_rejects_garbage() {
        assert_eq!(clamp_frac("k", "0.03", 0.5), 0.03); // норм доля
        assert_eq!(clamp_frac("k", "3", 0.5), 1.0); // «3» (проценты вместо доли) → кламп в 1.0
        assert_eq!(clamp_frac("k", "-1", 0.5), 0.0); // отрицательное → 0
        assert_eq!(clamp_frac("k", "NaN", 0.5), 0.5);
        assert_eq!(clamp_frac("k", "inf", 0.5), 0.5);
        assert_eq!(clamp_frac("k", "хлам", 0.5), 0.5); // не число → дефолт
        assert_eq!(clamp_frac("k", "0,1", 0.5), 0.5); // запятая-десятичная (опечатка) → дефолт
    }

    #[test]
    fn multiplier_rejects_free_or_out_of_range_defaults() {
        assert_eq!(parse_mult_bp("MULT", "2000", false), Ok(2000));
        assert_eq!(parse_mult_bp("MULT", "0", true), Ok(0));
        assert!(parse_mult_bp("MULT", "0", false).is_err());
        assert!(parse_mult_bp("MULT", "-1", true).is_err());
        assert!(parse_mult_bp("MULT", "10001", false).is_err());
        assert!(parse_mult_bp("MULT", "garbage", false).is_err());
    }

    #[test]
    fn upstream_is_pinned_or_explicit_literal_loopback() {
        assert_eq!(
            validate_upstream("https://api.anthropic.com/", false),
            Ok("https://api.anthropic.com".to_string()),
        );
        assert_eq!(
            validate_upstream("http://127.0.0.1:18080", true),
            Ok("http://127.0.0.1:18080".to_string()),
        );
        assert_eq!(
            validate_upstream("http://[::1]:18080/", true),
            Ok("http://[::1]:18080".to_string()),
        );
        assert!(validate_upstream("http://127.0.0.1:18080", false).is_err());
        assert!(validate_upstream("http://198.51.100.7", true).is_err());
        assert!(validate_upstream("https://api.anthropic.com:443", false).is_err());
        assert!(validate_upstream("https://api.anthropic.com/v1", false).is_err());
        assert!(validate_upstream("https://user@api.anthropic.com", false).is_err());
        assert!(validate_upstream("http://localhost:18080", true).is_err());

        assert_eq!(
            validate_gemini_upstream("https://daily-cloudcode-pa.sandbox.googleapis.com/", false),
            Ok("https://daily-cloudcode-pa.sandbox.googleapis.com".to_string()),
        );
        assert_eq!(
            validate_gemini_upstream("https://daily-cloudcode-pa.googleapis.com", false),
            Ok("https://daily-cloudcode-pa.googleapis.com".to_string()),
        );
        assert_eq!(
            validate_gemini_upstream("https://cloudcode-pa.googleapis.com/", false),
            Ok("https://cloudcode-pa.googleapis.com".to_string()),
        );
        assert_eq!(
            validate_gemini_upstream("http://127.0.0.1:18081", true),
            Ok("http://127.0.0.1:18081".to_string()),
        );
        assert!(validate_gemini_upstream("http://127.0.0.1:18081", false).is_err());
        assert!(validate_gemini_upstream("https://example.com", true).is_err());
    }
}
