//! Конфиг форвардинга — то, что нужно прокси/поллеру. Наполняется крейтом `server`
//! из окружения (композиция). Сам forward env не читает — только принимает готовое.

/// Системный блок-идентичность Claude Code. Anthropic пускает OAuth-токены подписок
/// (Max/Pro) на /v1/messages, только если ПЕРВЫЙ system-блок запроса ровно такой.
/// Поэтому под капотом мы его инжектим (см. proxy.rs) — для КЛИЕНТА протокол не меняется.
pub const CLAUDE_CODE_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub api_keys: Vec<String>,     // ключи НАШЕГО API (пусто → только localhost)
    pub upstream: String,          // база апстрима (https://api.anthropic.com)
    pub max_tries: usize,          // попыток ротации при 429/5xx
    pub util_cap: f64,             // клиентский потолок утилизации окна (для /pool)
    pub cool_secs: i64,            // cooling при 429 без известного reset
    pub poll: bool,                // включён ли фоновый поллер (для /pool)
    pub inject_identity: bool,     // инжектить Claude Code identity в system
    pub identity: String,          // сама строка идентичности
    pub default_beta: String,      // anthropic-beta, добавляемый к клиентским
    pub user_agent: String,        // UA, которым представляемся апстриму
    pub anthropic_version: String, // деф. anthropic-version, если клиент не прислал
    pub connect_timeout: u64,      // сек на установку соединения с апстримом/прокси
}
