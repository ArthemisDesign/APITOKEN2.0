//! Конфиг из окружения. Всё с дефолтами — сервер стартует без единой переменной
//! (тогда слушает localhost без ключей). Реальные значения — через env/systemd EnvironmentFile.

use std::env;

/// Системный блок-идентичность Claude Code. Anthropic пускает OAuth-токены подписок
/// (Max/Pro) на /v1/messages, только если ПЕРВЫЙ system-блок запроса ровно такой.
/// Поэтому под капотом мы его инжектим (см. proxy.rs) — для КЛИЕНТА протокол не меняется.
pub const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

#[derive(Clone, Debug)]
pub struct Config {
    pub db_path: String,
    pub bind: String,               // host:port
    pub api_keys: Vec<String>,      // ключи НАШЕГО API (пусто → только localhost)
    pub upstream: String,           // база апстрима (деф. https://api.anthropic.com)
    pub fleet: Option<String>,      // брать подписки только этого флота (None = все)
    pub max_tries: usize,           // попыток ротации при 429/5xx
    pub util_cap: f64,              // клиентский потолок утилизации окна
    pub cool_secs: i64,             // cooling при 429 без известного reset
    pub poll: bool,                 // фоновый поллер лимитов
    pub inject_identity: bool,      // инжектить Claude Code identity в system (деф. true)
    pub identity: String,           // сама строка идентичности
    pub default_beta: String,       // anthropic-beta, добавляемый к клиентским (oauth-...)
    pub user_agent: String,         // UA, которым представляемся апстриму (как Claude Code)
    pub anthropic_version: String,  // деф. anthropic-version, если клиент не прислал
    pub connect_timeout: u64,       // сек на установку соединения с апстримом/прокси
}

fn ev(k: &str) -> Option<String> {
    env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}
fn ev_or(k: &str, d: &str) -> String { ev(k).unwrap_or_else(|| d.to_string()) }
fn ev_bool(k: &str, d: bool) -> bool {
    match ev(k) { Some(v) => !matches!(v.to_lowercase().as_str(), "0" | "false" | "no" | "off"), None => d }
}

impl Config {
    pub fn from_env() -> Self {
        let cfg_dir = ev("SUB_CFG_DIR").unwrap_or_else(|| {
            let home = env::var("HOME").unwrap_or_default();
            format!("{home}/.config/claude-api")
        });
        let db_path = ev("SUBS_DB").unwrap_or_else(|| format!("{cfg_dir}/subscriptions.db"));
        let host = ev_or("CLAUDE_API_HOST", "0.0.0.0");
        let port = ev_or("CLAUDE_API_PORT", "8787");
        let api_keys = ev("CLAUDE_API_KEYS").map(|s| {
            s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
        }).unwrap_or_default();
        Config {
            db_path,
            bind: format!("{host}:{port}"),
            api_keys,
            upstream: ev_or("CLAUDE_API_UPSTREAM", "https://api.anthropic.com"),
            fleet: ev("SUBS_FLEET").filter(|f| f != "all"),
            max_tries: ev("CLAUDE_API_MAX_TRIES").and_then(|s| s.parse().ok()).unwrap_or(3),
            util_cap: ev("CLAUDE_API_UTIL_CAP").and_then(|s| s.parse().ok()).unwrap_or(0.95),
            cool_secs: ev("CLAUDE_API_COOL_SECS").and_then(|s| s.parse().ok()).unwrap_or(300),
            poll: ev_bool("CLAUDE_API_POLL", true),
            inject_identity: ev_bool("CLAUDE_API_INJECT_IDENTITY", true),
            identity: ev_or("CLAUDE_API_IDENTITY", CLAUDE_CODE_IDENTITY),
            default_beta: ev_or("CLAUDE_API_BETA", "oauth-2025-04-20"),
            user_agent: ev_or("CLAUDE_API_UA", "claude-cli/2.1.0 (external, cli)"),
            anthropic_version: ev_or("CLAUDE_API_ANTHROPIC_VERSION", "2023-06-01"),
            connect_timeout: ev("CLAUDE_API_CONNECT_TIMEOUT").and_then(|s| s.parse().ok()).unwrap_or(30),
        }
    }
}
