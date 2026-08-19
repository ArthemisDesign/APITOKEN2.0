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
    /// Явный rollout-флаг advanced routing. По умолчанию выключен: `models` и
    /// `provider` отклоняются до catalog/policy/plane work.
    pub fallback_enabled: bool,
    /// Dormant large-body settings. Defaults preserve the current 32 MiB/128 MiB behavior;
    /// later stages may raise them only after bounded storage and dual admission exist.
    pub body_limits: api_limits::BodyLimits,
    pub body_idle_secs: u64,
    pub body_max_secs: u64,
}

fn env_or(key: &str, default: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| default.to_string())
}

fn parse_mib(key: &str, default: api_limits::ByteLimit) -> anyhow::Result<api_limits::ByteLimit> {
    match std::env::var(key) {
        Ok(value) => api_limits::parse_decimal_mib(&value)
            .map_err(|error| anyhow::anyhow!("{key}={value:?}: {error}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(anyhow::anyhow!("{key}: {error}")),
    }
}

fn parse_seconds(key: &str, default: u64) -> anyhow::Result<u64> {
    match std::env::var(key) {
        Ok(value) => api_limits::parse_decimal_seconds(&value)
            .map_err(|error| anyhow::anyhow!("{key}={value:?}: {error}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(anyhow::anyhow!("{key}: {error}")),
    }
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
        let body_limits = api_limits::BodyLimits {
            request: parse_mib(
                "CLAUDE_ROUTER_MAX_BODY_MIB",
                api_limits::current::ROUTER_REQUEST,
            )?,
            memory_budget: parse_mib(
                "CLAUDE_ROUTER_BODY_MEMORY_BUDGET_MIB",
                api_limits::current::ROUTER_MEMORY_BUDGET,
            )?,
            spool_budget: parse_mib(
                "CLAUDE_ROUTER_BODY_SPOOL_BUDGET_MIB",
                api_limits::current::ROUTER_SPOOL_BUDGET,
            )?,
            memory_threshold: parse_mib(
                "CLAUDE_ROUTER_BODY_MEMORY_THRESHOLD_MIB",
                api_limits::current::ROUTER_MEMORY_THRESHOLD,
            )?,
            response: api_limits::current::ROUTER_RESPONSE,
        }
        .validate(api_limits::hard::SPOOL)
        .map_err(|error| anyhow::anyhow!("invalid router body limits: {error}"))?;
        anyhow::ensure!(
            body_limits.request <= api_limits::current::ROUTER_REQUEST,
            "CLAUDE_ROUTER_MAX_BODY_MIB cannot exceed the current 32 MiB runtime ceiling before bounded storage is deployed"
        );
        anyhow::ensure!(
            body_limits.memory_budget <= api_limits::current::ROUTER_MEMORY_BUDGET,
            "CLAUDE_ROUTER_BODY_MEMORY_BUDGET_MIB cannot exceed the current 128 MiB runtime ceiling before dual admission is deployed"
        );
        anyhow::ensure!(
            body_limits.spool_budget <= api_limits::current::ROUTER_SPOOL_BUDGET,
            "CLAUDE_ROUTER_BODY_SPOOL_BUDGET_MIB cannot exceed the current 128 MiB in-memory envelope before spooling is deployed"
        );
        anyhow::ensure!(
            body_limits.memory_threshold == body_limits.request,
            "CLAUDE_ROUTER_BODY_MEMORY_THRESHOLD_MIB must equal the request limit before spooling is deployed"
        );
        let body_idle_secs = parse_seconds(
            "CLAUDE_ROUTER_BODY_IDLE_SECS",
            api_limits::current::ROUTER_BODY_IDLE_SECS,
        )?;
        let body_max_secs = parse_seconds(
            "CLAUDE_ROUTER_BODY_MAX_SECS",
            api_limits::current::ROUTER_BODY_MAX_SECS,
        )?;
        anyhow::ensure!(
            body_idle_secs <= body_max_secs,
            "router body idle timeout must not exceed the absolute timeout"
        );
        anyhow::ensure!(
            body_idle_secs <= api_limits::current::ROUTER_BODY_IDLE_SECS
                && body_max_secs <= api_limits::current::ROUTER_BODY_MAX_SECS,
            "router upload timeouts cannot exceed current runtime ceilings before bounded storage is deployed"
        );
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
            body_limits,
            body_idle_secs,
            body_max_secs,
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
