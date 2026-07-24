//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{CodexConfig, CodexModel, CodexPrices, ProxyConfig, CLAUDE_CODE_IDENTITY};
use std::{collections::BTreeMap, env, net::IpAddr};

pub struct Settings {
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

fn codex_model_catalog() -> Vec<CodexModel> {
    let prices = |input, cached_input, cache_write_input, output| CodexPrices {
        input,
        cached_input,
        cache_write_input,
        output,
        long_context_threshold: 272_000,
        long_input_basis_points: 20_000,
        long_output_basis_points: 15_000,
    };
    let efforts = |with_max: bool| {
        let mut values = vec!["none", "low", "medium", "high", "xhigh"]
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if with_max {
            values.push("max".to_string());
        }
        values
    };
    let model =
        |id: &str, upstream: &str, model_prices: CodexPrices, with_max: bool| -> CodexModel {
            CodexModel {
                id: id.to_string(),
                upstream: upstream.to_string(),
                // Public `/v1/models` keeps the field for SDK compatibility without inventing an
                // upstream creation timestamp that app-server does not provide.
                created: 0,
                owned_by: "apitoken".to_string(),
                max_output_tokens: 128_000,
                reasoning_efforts: efforts(with_max),
                prices: model_prices,
            }
        };
    vec![
        model(
            "gpt-5.6",
            "gpt-5.6-sol",
            prices(5_000, 500, 6_250, 30_000),
            true,
        ),
        model(
            "gpt-5.6-sol",
            "gpt-5.6-sol",
            prices(5_000, 500, 6_250, 30_000),
            true,
        ),
        model(
            "gpt-5.6-terra",
            "gpt-5.6-terra",
            prices(2_500, 250, 3_125, 15_000),
            true,
        ),
        model(
            "gpt-5.6-luna",
            "gpt-5.6-luna",
            prices(1_000, 100, 1_250, 6_000),
            true,
        ),
        model(
            "gpt-5.5",
            "gpt-5.5",
            prices(5_000, 500, 5_000, 30_000),
            false,
        ),
        model(
            "gpt-5.4",
            "gpt-5.4",
            prices(2_500, 250, 2_500, 15_000),
            false,
        ),
    ]
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
    let catalog = codex_model_catalog();
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
        binary: ev_or(
            "CLAUDE_API_CODEX_BIN",
            "/srv/claude-api/data/codex/bin/codex",
        ),
        binary_sha256: ev("CLAUDE_API_CODEX_BIN_SHA256").unwrap_or_else(|| {
            panic!("CLAUDE_API_CODEX_BIN_SHA256 is required when CLAUDE_API_CODEX_ENABLED=true")
        }),
        expected_version: ev_or("CLAUDE_API_CODEX_VERSION", "codex-cli 0.145.0"),
        codex_home: ev_or("CLAUDE_API_CODEX_HOME", "/srv/claude-api/data/codex/home"),
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
        turn_timeout_ms: bounded_u64(
            "CLAUDE_API_CODEX_TURN_TIMEOUT_MS",
            600_000,
            5_000,
            3_600_000,
        ),
        max_concurrent_turns: bounded_usize("CLAUDE_API_CODEX_MAX_CONCURRENT", 4, 1, 64),
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
            100,
            1,
            2_000,
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
        let codex = codex_config(redis_url.clone(), affinity_secret.clone());
        Settings {
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
            // Fail-closed clamps: readiness-delay ≤ 30с; drain-deadline в [5, 595]с — оставляем
            // headroom под systemd TimeoutStopSec=600 и исключаем переполнение Instant + deadline.
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
    }
}
