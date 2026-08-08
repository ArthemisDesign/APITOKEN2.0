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
    /// Origin, публикующий каталог KIMI. По умолчанию совпадает с Anthropic-плоскостью:
    /// KIMI-шлюз скомпонован в тех же слотах и говорит на том же протоколе.
    pub kimi_origin: String,
    /// Явный rollout-флаг advanced routing. По умолчанию выключен: `models`,
    /// `provider` и `preset/*` отклоняются до catalog/policy/plane work.
    pub fallback_enabled: bool,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_strict_bool(key: &str, value: Option<&str>, default: bool) -> anyhow::Result<bool> {
    match value {
        None => Ok(default),
        Some("0") => Ok(false),
        Some("1") => Ok(true),
        Some(value) if value.eq_ignore_ascii_case("false") => Ok(false),
        Some(value) if value.eq_ignore_ascii_case("true") => Ok(true),
        Some(value) => anyhow::bail!("{key}={value:?}: expected exactly 0, 1, false, or true"),
    }
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let anthropic_origin = env_or("CLAUDE_ROUTER_ANTHROPIC_ORIGIN", "http://127.0.0.1:8790");
        let cfg = Config {
            host: env_or("CLAUDE_ROUTER_HOST", "127.0.0.1"),
            port: env_or("CLAUDE_ROUTER_PORT", "8798")
                .parse()
                .map_err(|e| anyhow::anyhow!("CLAUDE_ROUTER_PORT: {e}"))?,
            kimi_origin: env_or("CLAUDE_ROUTER_KIMI_ORIGIN", &anthropic_origin),
            anthropic_origin,
            openai_origin: env_or("CLAUDE_ROUTER_OPENAI_ORIGIN", "http://127.0.0.1:8792"),
            gemini_origin: env_or("CLAUDE_ROUTER_GEMINI_ORIGIN", "http://127.0.0.1:8794"),
            fallback_enabled: parse_strict_bool(
                "CLAUDE_ROUTER_FALLBACK_ENABLED",
                std::env::var("CLAUDE_ROUTER_FALLBACK_ENABLED")
                    .ok()
                    .as_deref(),
                false,
            )?,
        };
        for (name, origin) in [
            ("CLAUDE_ROUTER_ANTHROPIC_ORIGIN", &cfg.anthropic_origin),
            ("CLAUDE_ROUTER_OPENAI_ORIGIN", &cfg.openai_origin),
            ("CLAUDE_ROUTER_GEMINI_ORIGIN", &cfg.gemini_origin),
            ("CLAUDE_ROUTER_KIMI_ORIGIN", &cfg.kimi_origin),
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

#[cfg(test)]
mod tests {
    use super::parse_strict_bool;

    #[test]
    fn fallback_flag_is_strict_and_defaults_off() {
        assert!(!parse_strict_bool("FLAG", None, false).unwrap());
        assert!(parse_strict_bool("FLAG", Some("1"), false).unwrap());
        assert!(parse_strict_bool("FLAG", Some("TRUE"), false).unwrap());
        assert!(!parse_strict_bool("FLAG", Some("0"), true).unwrap());
        assert!(!parse_strict_bool("FLAG", Some("false"), true).unwrap());
        assert!(parse_strict_bool("FLAG", Some("yes"), false).is_err());
        assert!(parse_strict_bool("FLAG", Some(" true "), false).is_err());
    }
}
