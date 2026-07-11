//! Композиционный конфиг: читает ВСЁ окружение и собирает настройки сервера +
//! [`forward::ProxyConfig`]. Единственное место в проекте, где читается env.

use forward::{ProxyConfig, CLAUDE_CODE_IDENTITY};
use std::env;

pub struct Settings {
    pub db_path: String,
    pub bind: String,
    pub fleet: Option<String>,
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
        let api_keys = ev("CLAUDE_API_KEYS").map(|s| {
            s.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect()
        }).unwrap_or_default();
        Settings {
            db_path,
            bind: format!("{host}:{port}"),
            fleet: ev("SUBS_FLEET").filter(|f| f != "all"),
            proxy: ProxyConfig {
                api_keys,
                upstream: ev_or("CLAUDE_API_UPSTREAM", "https://api.anthropic.com"),
                max_tries: ev("CLAUDE_API_MAX_TRIES").and_then(|s| s.parse().ok()).unwrap_or(3),
                util_cap: ev("CLAUDE_API_UTIL_CAP").and_then(|s| s.parse().ok()).unwrap_or(0.95),
                cool_secs: ev("CLAUDE_API_COOL_SECS").and_then(|s| s.parse().ok()).unwrap_or(300),
                poll: ev_bool("CLAUDE_API_POLL", true),
                inject_identity: ev_bool("CLAUDE_API_INJECT_IDENTITY", true),
                identity: ev_or("CLAUDE_API_IDENTITY", CLAUDE_CODE_IDENTITY),
                default_beta: ev_or("CLAUDE_API_BETA", "oauth-2025-04-20"),
                // Дефолт-fallback; актуальное значение — env CLAUDE_API_UA (авто-рефреш скриптом).
                user_agent: ev_or("CLAUDE_API_UA", "claude-cli/2.1.195 (external, sdk-cli)"),
                anthropic_version: ev_or("CLAUDE_API_ANTHROPIC_VERSION", "2023-06-01"),
                connect_timeout: ev("CLAUDE_API_CONNECT_TIMEOUT").and_then(|s| s.parse().ok()).unwrap_or(30),
            },
        }
    }
}
