//! Env-конфиг router'а. Единственное место в крейте, где читается окружение
//! (аналог `crates/server/src/config.rs` для engine-бинаря). Router не хранит
//! состояния и не знает секретов: клиентский ключ проходит сквозь него verbatim.

/// Конфигурация одного процесса router'а.
#[derive(Clone, Debug)]
pub struct Config {
    /// Адрес прослушивания. Только loopback: публичная граница — Caddy.
    pub host: String,
    pub port: u16,
    /// Stable origin Anthropic-плоскости (Caddy blue-green balancer).
    pub anthropic_origin: String,
    /// Stable origin OpenAI-плоскости.
    pub openai_origin: String,
    /// Stable origin Gemini-плоскости.
    pub gemini_origin: String,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let cfg = Config {
            host: env_or("CLAUDE_ROUTER_HOST", "127.0.0.1"),
            port: env_or("CLAUDE_ROUTER_PORT", "8798")
                .parse()
                .map_err(|e| anyhow::anyhow!("CLAUDE_ROUTER_PORT: {e}"))?,
            anthropic_origin: env_or("CLAUDE_ROUTER_ANTHROPIC_ORIGIN", "http://127.0.0.1:8790"),
            openai_origin: env_or("CLAUDE_ROUTER_OPENAI_ORIGIN", "http://127.0.0.1:8792"),
            gemini_origin: env_or("CLAUDE_ROUTER_GEMINI_ORIGIN", "http://127.0.0.1:8794"),
        };
        for (name, origin) in [
            ("CLAUDE_ROUTER_ANTHROPIC_ORIGIN", &cfg.anthropic_origin),
            ("CLAUDE_ROUTER_OPENAI_ORIGIN", &cfg.openai_origin),
            ("CLAUDE_ROUTER_GEMINI_ORIGIN", &cfg.gemini_origin),
        ] {
            anyhow::ensure!(
                origin.starts_with("http://") || origin.starts_with("https://"),
                "{name} must be an http(s) origin, got {origin:?}"
            );
            anyhow::ensure!(
                !origin.ends_with('/'),
                "{name} must not end with '/', got {origin:?}"
            );
        }
        Ok(cfg)
    }
}
