//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{
    CodexConfig, CodexModel, GeminiConfig, GeminiModel, ProviderMode, ProxyConfig,
    CLAUDE_CODE_IDENTITY,
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
    pub billing: bool,       // включён ли учёт баланса ключей (таблица api_keys)
    pub mult_bp: i64,        // дефолтная наценка для `key issue` (× 10000; 900 = ×0.09)
    pub cap5h_usd: f64,      // прайор ёмкости 5h окна (USD; 0 → дефолт пула под Max 20x)
    pub cap7d_usd: f64,      // прайор ёмкости 7d окна
    pub reserve5h: f64,      // запас 5h-окна (доля; деф 0.10 = бережём 10%)
    pub reserve7d: f64,      // запас 7d-окна (доля; деф 0.03)
    pub reserve_jitter: f64, // ± разброс порога между подписками (антифингерпринт; деф 0.02)
    pub readiness_delay_secs: u64, // задержка после снятия readiness перед дренажем (деф 3с)
    pub drain_deadline_secs: u64, // предел graceful-дренажа до принудительного обрыва (деф 540с)
    pub max_inflight: i64, // потолок параллельных запросов на подписку (деф 6; выше = больше параллели, риск бана)
    /// Optional shared L2 for ephemeral cache affinity. PostgreSQL remains authoritative.
    pub redis_url: Option<String>,
    pub affinity_secret: Option<String>,
    pub affinity_ttl_secs: u64,
    pub affinity_local_ttl_secs: u64,
    pub affinity_redis_timeout_ms: u64,
    /// Optional second provider. Disabled by default; enabling it requires a pinned binary and a
    /// dedicated ChatGPT-authenticated CODEX_HOME.
    pub codex: Option<CodexConfig>,
    /// Native Gemini provider. It is instantiated only by the startup-fixed Gemini service.
    pub gemini: Option<GeminiConfig>,
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

fn bounded_f64(k: &str, default: f64, min: f64, max: f64) -> f64 {
    match ev(k).and_then(|value| value.parse::<f64>().ok()) {
        Some(value) if value.is_finite() => value.clamp(min, max),
        _ => default.clamp(min, max),
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
    if scheme.eq_ignore_ascii_case("https")
        && authority
            .as_str()
            .eq_ignore_ascii_case("cloudcode-pa.googleapis.com")
    {
        return Ok("https://cloudcode-pa.googleapis.com".to_string());
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
    Err("CLAUDE_API_GEMINI_UPSTREAM: only https://cloudcode-pa.googleapis.com is allowed; literal HTTP loopback requires CLAUDE_API_GEMINI_ALLOW_INSECURE_LOOPBACK_UPSTREAM=1".to_string())
}

fn gemini_config() -> Option<GeminiConfig> {
    let requested = ev_or(
        "CLAUDE_API_GEMINI_MODELS",
        "gemini-3.6-flash,gemini-3.5-flash,gemini-3.1-flash-lite,gemini-2.5-pro,gemini-2.5-flash,gemini-2.5-flash-lite",
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
            "https://cloudcode-pa.googleapis.com",
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
        auth_quarantine_secs: bounded_i64(
            "CLAUDE_API_GEMINI_AUTH_QUARANTINE_SECS",
            900,
            60,
            86_400,
        ),
        transport_cool_secs: bounded_i64("CLAUDE_API_GEMINI_TRANSPORT_COOL_SECS", 5, 1, 300),
        default_rate_limit_cool_secs: bounded_i64(
            "CLAUDE_API_GEMINI_RATE_LIMIT_COOL_SECS",
            60,
            1,
            86_400,
        ),
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
            // upstream creation timestamp that app-server does not provide.
            created: 0,
            owned_by: "apitoken".to_string(),
            max_output_tokens: spec.max_output_tokens,
            reasoning_efforts: spec
                .reasoning_efforts
                .iter()
                .map(|effort| (*effort).to_string())
                .collect(),
            prices: spec.prices,
        })
        .collect()
}

/// Authenticated Codex profiles, in pool order.
///
/// `CLAUDE_API_CODEX_HOMES` is the pool form; `CLAUDE_API_CODEX_HOME` remains the single-home
/// spelling so an existing production environment keeps working unchanged. Duplicates are rejected:
/// two children sharing one auth store would corrupt its token refresh and would also double-count
/// one subscription's capacity in the pool.
fn codex_homes() -> Vec<String> {
    let configured = ev("CLAUDE_API_CODEX_HOMES")
        .or_else(|| ev("CLAUDE_API_CODEX_HOME"))
        .unwrap_or_else(|| "/srv/claude-api/data/codex/home".to_string());
    let mut homes: Vec<String> = Vec::new();
    for home in configured
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        if homes.iter().any(|existing| existing == home) {
            panic!("CLAUDE_API_CODEX_HOMES lists {home:?} more than once; every home needs its own authentication store");
        }
        homes.push(home.to_string());
    }
    if homes.is_empty() {
        panic!("CLAUDE_API_CODEX_HOMES must contain at least one absolute CODEX_HOME path");
    }
    homes
}

/// Directory scanned for profiles the authbot has finished buying.
///
/// Defaulted rather than required: the pool directory simply may not exist yet, which reads as an
/// empty pool. Set it to an empty value to disable discovery and pin the pool to the explicit list.
fn codex_homes_dir() -> Option<String> {
    let configured = ev_or(
        "CLAUDE_API_CODEX_HOMES_DIR",
        "/srv/claude-api/data/codex-homes",
    );
    let configured = configured.trim().to_string();
    if configured.is_empty() {
        return None;
    }
    if !std::path::Path::new(&configured).is_absolute() {
        panic!("CLAUDE_API_CODEX_HOMES_DIR must be an absolute path");
    }
    Some(configured)
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

    let mut child_proxy_env = BTreeMap::new();
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
            child_proxy_env.insert(name.to_string(), value);
        }
    }

    Some(CodexConfig {
        enabled: true,
        ownership_lock_file: "/run/apitoken/codex-home.lock".to_string(),
        binary: ev_or(
            "CLAUDE_API_CODEX_BIN",
            "/srv/claude-api/data/codex/bin/codex",
        ),
        binary_sha256: ev("CLAUDE_API_CODEX_BIN_SHA256").unwrap_or_else(|| {
            panic!("CLAUDE_API_CODEX_BIN_SHA256 is required when CLAUDE_API_CODEX_ENABLED=true")
        }),
        expected_version: ev_or("CLAUDE_API_CODEX_VERSION", "codex-cli 0.145.0"),
        homes: codex_homes(),
        homes_dir: codex_homes_dir(),
        work_dir: ev_or(
            "CLAUDE_API_CODEX_WORK_DIR",
            "/srv/claude-api/data/codex/work",
        ),
        startup_timeout_ms: bounded_u64(
            "CLAUDE_API_CODEX_STARTUP_TIMEOUT_MS",
            20_000,
            1_000,
            120_000,
        ),
        request_timeout_ms: bounded_u64("CLAUDE_API_CODEX_RPC_TIMEOUT_MS", 15_000, 500, 120_000),
        turn_timeout_ms: bounded_u64("CLAUDE_API_CODEX_TURN_TIMEOUT_MS", 600_000, 5_000, 600_000),
        max_concurrent_turns: bounded_usize("CLAUDE_API_CODEX_MAX_CONCURRENT", 4, 1, 64),
        admit_below_used_percent: bounded_i64(
            "CLAUDE_API_CODEX_ADMIT_BELOW_USED_PERCENT",
            95,
            1,
            100,
        ),
        window_cap_usd_prior: bounded_f64(
            "CLAUDE_API_CODEX_WINDOW_CAP_USD",
            1_500.0,
            10.0,
            1_000_000.0,
        ),
        health_probe_interval_secs: bounded_u64(
            "CLAUDE_API_CODEX_HEALTH_INTERVAL_SECS",
            300,
            30,
            3_600,
        ),
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
        child_proxy_env,
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
            // Fail-closed clamps: readiness-delay ≤ 30с; drain-deadline в [5, 595]с. На deadline
            // Codex отменяет остаток turns; запас до TimeoutStopSec=660 остаётся на reap и billing flush.
            readiness_delay_secs: bounded_u64("CLAUDE_API_READINESS_DELAY_SECS", 3, 0, 30),
            drain_deadline_secs: bounded_u64("CLAUDE_API_DRAIN_DEADLINE_SECS", 540, 5, 595),
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
            proxy: ProxyConfig {
                api_keys,
                control_keys,
                panel_keys,
                default_mult_bp: mult_bp,
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
