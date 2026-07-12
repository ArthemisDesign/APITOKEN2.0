//! Конфиг форвардинга — то, что нужно прокси/поллеру. Наполняется крейтом `server`
//! из окружения (композиция). Сам forward env не читает — только принимает готовое.

/// Системный блок-идентичность Claude Code (значение по УМОЛЧАНИЮ / fallback). Anthropic
/// пускает OAuth-токены подписок на /v1/messages при валидном Claude-Code-подобном инжекте.
/// АКТУАЛЬНОЕ значение подтягивается из env `CLAUDE_API_IDENTITY`, которое авто-обновляет
/// `tools/refresh-fingerprint.sh` (снимает с живого claude CLI) — чтобы не протухало.
/// Снято с Claude Code 2.1.195 (2026-07-11).
pub const CLAUDE_CODE_IDENTITY: &str = "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub api_keys: Vec<String>,     // ключи НАШЕГО API (пусто → см. trust_loopback)
    /// Доверять ли loopback-пиру как админу при ПУСТЫХ `api_keys`. true ТОЛЬКО когда сервер
    /// слушает loopback-интерфейс. При bind на 0.0.0.0/публичный IP — false: за реверс-прокси
    /// (nginx) peer виден как 127.0.0.1, и доверие loopback открыло бы аноним-админ + бесплатный
    /// форвардинг. На экспонированном bind без `api_keys` сервер отклоняет всё (безопасный дефолт).
    pub trust_loopback: bool,
    pub upstream: String,          // база апстрима (https://api.anthropic.com)
    pub max_tries: usize,          // попыток ротации при 429/5xx
    pub util_cap: f64,             // клиентский потолок утилизации окна (для /pool)
    pub cool_secs: i64,            // cooling при 429 без известного reset
    pub poll: bool,                // включён ли фоновый поллер (для /pool)
    pub inject_identity: bool,     // инжектить Claude Code identity в system
    pub identity: String,          // сама строка идентичности
    pub default_beta: String,      // anthropic-beta, добавляемый к клиентским
    pub user_agent: String,        // UA-fallback (client-level default; поллер/детект)
    /// Пул реальных UA. len>1 → каждая персона пинит один (hash(email)%len). len≤1 → берём его
    /// как базу и варьируем patch-версию на `ua_spread` (см. `persona_ua`). Флот из байт-в-байт
    /// одинаковых UA — сам по себе отпечаток; персональный стабильный UA убирает эту аномалию.
    pub user_agents: Vec<String>,
    /// Разброс patch-версии claude-cli в UA между персонами (0/1 = выключено). patch-релизы почти
    /// наверняка существовали → правдоподобно. Для гарантии реальности — задай `user_agents` списком.
    pub ua_spread: u32,
    pub anthropic_version: String, // деф. anthropic-version, если клиент не прислал
    pub connect_timeout: u64,      // сек на установку соединения с апстримом/прокси
}
