//! Конфиг форвардинга — то, что нужно прокси/поллеру. Наполняется крейтом `server`
//! из окружения (композиция). Сам forward env не читает — только принимает готовое.

/// Системный блок-идентичность Claude Code (значение по УМОЛЧАНИЮ / fallback). Anthropic
/// пускает OAuth-токены подписок на /v1/messages при валидном Claude-Code-подобном инжекте.
/// АКТУАЛЬНОЕ значение подтягивается из env `CLAUDE_API_IDENTITY`, которое авто-обновляет
/// `tools/refresh-fingerprint.sh` (снимает с живого claude CLI) — чтобы не протухало.
/// Снято с Claude Code 2.1.195 (2026-07-11).
pub const CLAUDE_CODE_IDENTITY: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

pub const CLAUDESTORE_CLAUDE_FALLBACK_BASE_URL: &str = "https://api.llmsrelay.com";
pub const CLAUDESTORE_CODEX_FALLBACK_BASE_URL: &str = "https://api3.claudestore.store";

/// Secret-bearing configuration for the last-resort ClaudeStore transport. The base URL is fixed
/// in production so a compromised environment cannot turn the fallback into an arbitrary request
/// target. `Debug` deliberately redacts the API key because `ProxyConfig` is otherwise printable.
#[derive(Clone)]
pub struct ClaudeStoreFallbackConfig {
    base_url: String,
    api_key: String,
}

impl std::fmt::Debug for ClaudeStoreFallbackConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClaudeStoreFallbackConfig")
            .field("base_url", &self.base_url)
            .field("api_key", &"[REDACTED]")
            .finish()
    }
}

impl ClaudeStoreFallbackConfig {
    pub fn production_claude(api_key: String) -> Result<Self, &'static str> {
        Self::production_for(CLAUDESTORE_CLAUDE_FALLBACK_BASE_URL, api_key)
    }

    pub fn production_codex(api_key: String) -> Result<Self, &'static str> {
        Self::production_for(CLAUDESTORE_CODEX_FALLBACK_BASE_URL, api_key)
    }

    fn production_for(base_url: &str, api_key: String) -> Result<Self, &'static str> {
        if !(32..=256).contains(&api_key.len())
            || !api_key.starts_with("sk-cs4-")
            || !api_key.bytes().all(|byte| byte.is_ascii_graphic())
        {
            return Err("expected a 32..=256 byte printable sk-cs4-* API key");
        }
        Ok(Self {
            base_url: base_url.to_owned(),
            api_key,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(base_url: String) -> Self {
        Self {
            base_url,
            api_key: "sk-cs4-test-only-placeholder-000000000000".to_owned(),
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn api_key(&self) -> &str {
        &self.api_key
    }

    /// Compare two secret-bearing configs without exposing either credential to the composition
    /// layer. ClaudeStore keys are tier-switched, so one value cannot safely back both the Claude
    /// and Codex emergency transports at the same time.
    pub fn shares_api_key_with(&self, other: &Self) -> bool {
        self.api_key == other.api_key
    }
}

#[derive(Clone, Debug)]
pub struct ProxyConfig {
    pub api_keys: Vec<String>, // ключи НАШЕГО API (пусто → см. trust_loopback)
    /// Control-плоскость: ключи для `/admin/*` (создание аккаунтов/ключей/кредит). Ими пользуется
    /// коммерция (control-plane) — ОТДЕЛЬНО от `api_keys` (раздача /v1). Пусто → /admin/* доступен
    /// только admin-ключам (`api_keys`) или loopback-админу. Разделение секретов: компрометация
    /// коммерц-ключа не даёт бесплатный форвардинг, компрометация forwarding-ключа не даёт эмиссию денег.
    pub control_keys: Vec<String>,
    /// Read-only ключи панели/дашбордов (`/capacity`, `/metrics`). Ими смотрят ёмкость/метрики БЕЗ
    /// прав на /admin/* и БЕЗ неметеренного /v1. Пусто → дашборды доступны admin/control-ключам.
    pub panel_keys: Vec<String>,
    /// Наценка по умолчанию (basis points) для аккаунтов, созданных через `/admin/account` без явного
    /// mult_bp. Коммерция обычно задаёт свою; это fallback (= CLAUDE_API_MULT_BP).
    pub default_mult_bp: i64,
    /// Доверять ли loopback-пиру как админу при ПУСТЫХ `api_keys`. true ТОЛЬКО когда сервер
    /// слушает loopback-интерфейс. При bind на 0.0.0.0/публичный IP — false: за реверс-прокси
    /// (nginx) peer виден как 127.0.0.1, и доверие loopback открыло бы аноним-админ + бесплатный
    /// форвардинг. На экспонированном bind без `api_keys` сервер отклоняет всё (безопасный дефолт).
    pub trust_loopback: bool,
    pub upstream: String, // база апстрима (https://api.anthropic.com)
    /// Disabled-by-default external transport. It is attempted only by `forward` after local
    /// pre-byte rotation is terminal and an existing metered reservation owns the request.
    pub claudestore_fallback: Option<ClaudeStoreFallbackConfig>,
    pub max_tries: usize, // попыток ротации при 429/5xx
    pub util_cap: f64,    // клиентский потолок утилизации окна (для /pool)
    pub cool_secs: i64,   // cooling при 429 без известного reset
    /// Гладкий UX: сколько миллисекунд движок может ТИХО ждать+ретраить ротацию при транзиентной
    /// нехватке ёмкости (все подписки cooling/за util_cap, breaker, upstream-429), прежде чем отдать
    /// клиенту ошибку. Решение pre-stream → для клиента невидимо (стрим ещё не начат). 0 = выключено
    /// (мгновенный отбой, старое поведение). Env `CLAUDE_API_SMOOTH_WAIT_MS`.
    pub smooth_wait_ms: u64,
    pub poll: bool,            // включён ли фоновый поллер (для /pool)
    pub inject_identity: bool, // инжектить Claude Code identity в system
    pub identity: String,      // сама строка идентичности
    /// Инжектить system[0] = `x-anthropic-billing-header: cc_version=…; cc_entrypoint=…; cch=…;`
    /// (реальный CC шлёт его ПЕРВЫМ system-блоком; отсутствие = структурный отпечаток). cc_version
    /// флот-константна (все на 2.1.195 шлют одно), cch варьируется per-персона (per-install реализм).
    pub inject_billing: bool,
    pub cc_version: String, // БАЗА "2.1.195" (коген с UA); суффикс .dNN добавляем per-подписка
    pub cc_entrypoint: String, // "sdk-cli"
    pub default_beta: String, // anthropic-beta, добавляемый к клиентским
    pub user_agent: String, // UA-fallback (client-level default; поллер/детект)
    /// Пул реальных UA. len>1 → каждая персона пинит один (hash(email)%len). len≤1 → берём его
    /// как базу и варьируем patch-версию на `ua_spread` (см. `persona_ua`). Флот из байт-в-байт
    /// одинаковых UA — сам по себе отпечаток; персональный стабильный UA убирает эту аномалию.
    pub user_agents: Vec<String>,
    /// Разброс patch-версии claude-cli в UA между персонами (0/1 = выключено). patch-релизы почти
    /// наверняка существовали → правдоподобно. Для гарантии реальности — задай `user_agents` списком.
    pub ua_spread: u32,
    pub anthropic_version: String, // деф. anthropic-version, если клиент не прислал
    pub connect_timeout: u64,      // сек на установку соединения с апстримом/прокси
    /// Пауза между чтениями, после которой соединение считается мёртвым. Применима к стриму:
    /// Anthropic шлёт SSE-ping, поэтому живой поток её не встречает, а зависший ловится быстро.
    pub read_timeout: u64,
    /// То же для НЕ-стриминговых запросов, где тишина до конца генерации — штатное состояние и
    /// отличить её от смерти по времени невозможно. Живость там держит TCP keep-alive, а эта
    /// величина остаётся страховкой от утёкшего слота, а не потолком длительности ответа.
    pub nonstream_read_timeout: u64,
    /// Отпечаток Claude Code клиента (Stainless SDK) — синтезируем эти заголовки в КАЖДЫЙ апстрим-
    /// запрос, чтобы выглядеть как настоящий CC, и НЕ пробрасываем клиентские (иначе Python-SDK
    /// клиент дал бы `x-stainless-lang: python` при нашем claude-cli UA = противоречие). Флот-константны
    /// и ВАЛИДНЫ (реальные юзеры на одной версии CC их делят); различие аккаунтов — через IP+user_id.
    pub x_app: String, // "cli"
    pub stainless_lang: String,            // "js"
    pub stainless_runtime: String,         // "node"
    pub stainless_runtime_version: String, // "v22.x.x"
    pub stainless_package_version: String, // версия @anthropic-ai/sdk (снимается refresh-скриптом)
    pub stainless_os: String,              // "MacOS"
    pub stainless_arch: String,            // "arm64"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claudestore_configs_pin_plane_origins_validate_shape_and_redact_secret() {
        let secret = "sk-cs4-test-secret-12345678901234567890".to_owned();
        let claude = ClaudeStoreFallbackConfig::production_claude(secret.clone()).unwrap();
        let codex = ClaudeStoreFallbackConfig::production_codex(secret.clone()).unwrap();
        assert_eq!(claude.base_url(), CLAUDESTORE_CLAUDE_FALLBACK_BASE_URL);
        assert_eq!(codex.base_url(), CLAUDESTORE_CODEX_FALLBACK_BASE_URL);
        let debug = format!("{claude:?}");
        assert!(debug.contains("[REDACTED]"));
        assert!(!debug.contains(&secret));

        assert!(ClaudeStoreFallbackConfig::production_claude("sk-cs4-short".to_owned()).is_err());
        assert!(ClaudeStoreFallbackConfig::production_claude(
            "sk-other-test-secret-12345678901234567890".to_owned()
        )
        .is_err());
        assert!(ClaudeStoreFallbackConfig::production_codex(
            "sk-cs4-test-secret-1234567890123456\n7890".to_owned()
        )
        .is_err());

        let same = ClaudeStoreFallbackConfig::production_codex(secret).unwrap();
        let other = ClaudeStoreFallbackConfig::production_codex(
            "sk-cs4-other-secret-12345678901234567890".to_owned(),
        )
        .unwrap();
        assert!(claude.shares_api_key_with(&same));
        assert!(!claude.shares_api_key_with(&other));
    }
}
