//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{ProxyConfig, CLAUDE_CODE_IDENTITY};
use std::env;

pub struct Settings {
    pub db_path: String,
    pub bind: String,
    pub fleet: Option<String>,
    pub billing: bool,       // включён ли учёт баланса ключей (таблица api_keys)
    pub mult_bp: i64,        // дефолтная наценка для `key issue` (× 10000; 900 = ×0.09)
    pub cap5h_usd: f64,      // прайор ёмкости 5h окна (USD; 0 → дефолт пула под Max 20x)
    pub cap7d_usd: f64,      // прайор ёмкости 7d окна
    pub reserve5h: f64,      // запас 5h-окна (доля; деф 0.10 = бережём 10%)
    pub reserve7d: f64,      // запас 7d-окна (доля; деф 0.03)
    pub reserve_jitter: f64, // ± разброс порога между подписками (антифингерпринт; деф 0.02)
    pub proxy: ProxyConfig,
}

fn ev(k: &str) -> Option<String> {
    env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn ev_or(k: &str, d: &str) -> String { ev(k).unwrap_or_else(|| d.to_string()) }
fn ev_bool(k: &str, d: bool) -> bool {
    match ev(k) { Some(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"), None => d }
}

impl Settings {
    pub fn from_env() -> Self {
        let cfg_dir = ev("SUB_CFG_DIR").unwrap_or_else(|| {
            let home = env::var("HOME").unwrap_or_default();
            format!("{home}/.config/claude-api")
        });
        let db_path = ev("SUBS_DB").unwrap_or_else(|| format!("{cfg_dir}/subscriptions.db"));
        let host = ev_or("CLAUDE_API_HOST", "0.0.0.0");
        let port = ev_or("CLAUDE_API_PORT", "8787");
        // Доверять loopback-админу только когда РЕАЛЬНО слушаем loopback (иначе за реверс-прокси
        // все пиры видны как 127.0.0.1 → аноним-админ). Экспонированный bind требует CLAUDE_API_KEYS.
        let trust_loopback = matches!(host.as_str(), "127.0.0.1" | "::1" | "localhost");
        let api_keys = ev("CLAUDE_API_KEYS").map(|s| {
            s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
        }).unwrap_or_default();
        Settings {
            db_path,
            bind: format!("{host}:{port}"),
            fleet: ev("SUBS_FLEET").filter(|f| f != "all"),
            billing: ev_bool("CLAUDE_API_BILLING", true),
            // Наценка по умолчанию: клиент платит 20% от реального API-эквивалента (×0.20).
            mult_bp: ev("CLAUDE_API_MULT_BP").and_then(|s| s.parse().ok()).unwrap_or(2000),
            // Прайоры ёмкости окон (0 → дефолт пула под Max 20x; калибровка их уточняет).
            cap5h_usd: ev("CLAUDE_API_CAP5H_USD").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            cap7d_usd: ev("CLAUDE_API_CAP7D_USD").and_then(|s| s.parse().ok()).unwrap_or(0.0),
            // Запас окон (headroom) с джиттером: бережём 10%/3%, порог отсечения слегка разный по
            // подпискам, чтобы флот не резался на одном проценте (антифингерпринт).
            reserve5h: ev("CLAUDE_API_RESERVE_5H").and_then(|s| s.parse().ok()).unwrap_or(0.10),
            reserve7d: ev("CLAUDE_API_RESERVE_7D").and_then(|s| s.parse().ok()).unwrap_or(0.03),
            reserve_jitter: ev("CLAUDE_API_RESERVE_JITTER").and_then(|s| s.parse().ok()).unwrap_or(0.02),
            proxy: ProxyConfig {
                api_keys,
                trust_loopback,
                upstream: ev_or("CLAUDE_API_UPSTREAM", "https://api.anthropic.com"),
                max_tries: ev("CLAUDE_API_MAX_TRIES").and_then(|s| s.parse().ok()).unwrap_or(3),
                util_cap: ev("CLAUDE_API_UTIL_CAP").and_then(|s| s.parse().ok()).unwrap_or(0.95),
                cool_secs: ev("CLAUDE_API_COOL_SECS").and_then(|s| s.parse().ok()).unwrap_or(300),
                poll: ev_bool("CLAUDE_API_POLL", true),
                inject_identity: ev_bool("CLAUDE_API_INJECT_IDENTITY", true),
                identity: ev_or("CLAUDE_API_IDENTITY", CLAUDE_CODE_IDENTITY),
                default_beta: ev_or("CLAUDE_API_BETA", "oauth-2025-04-20"),
                // Дефолт-fallback; актуальное значение — env CLAUDE_API_UA (авто-рефреш скриптом).
                // CLAUDE_API_UA можно задать СПИСКОМ через запятую (пул реальных UA) — тогда каждая
                // персона пинит один. Иначе один UA + разброс patch-версии между персонами (ниже).
                user_agent: ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)"),
                user_agents: ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)")
                    .split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect(),
                // Разброс patch-версии UA между персонами (антифингерпринт флота). 0/1 → выключено.
                ua_spread: ev("CLAUDE_API_UA_SPREAD").and_then(|s| s.parse().ok()).unwrap_or(8),
                anthropic_version: ev_or("CLAUDE_API_ANTHROPIC_VERSION", "2023-06-01"),
                connect_timeout: ev("CLAUDE_API_CONNECT_TIMEOUT").and_then(|s| s.parse().ok()).unwrap_or(30),
            },
        }
    }
}
