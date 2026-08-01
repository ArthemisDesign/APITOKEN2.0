//! Конфиг форвардинга — то, что нужно прокси/поллеру. Наполняется крейтом `server`
//! из окружения (композиция). Сам forward env не читает — только принимает готовое.

/// Системный блок-идентичность Claude Code (значение по УМОЛЧАНИЮ / fallback). Anthropic
/// пускает OAuth-токены подписок на /v1/messages при валидном Claude-Code-подобном инжекте.
/// АКТУАЛЬНОЕ значение подтягивается из env `CLAUDE_API_IDENTITY`, которое авто-обновляет
/// `tools/refresh-fingerprint.sh` (снимает с живого claude CLI) — чтобы не протухало.
/// Снято с Claude Code 2.1.195 (2026-07-11).
pub const CLAUDE_CODE_IDENTITY: &str =
    "You are a Claude agent, built on Anthropic's Claude Agent SDK.";

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
    /// Default-off atomic legacy-snapshot bridge rollout config for metered Anthropic/OpenAI
    /// reserve. It never enables policy shadow/resolver behavior.
    pub pricing_bridge: crate::pricing::PricingBridgeConfig,
    /// Default-off bounded evaluation-time shadow configuration. Runtime composition supplies the
    /// trusted manifest and PostgreSQL actors separately; this value contains rollout limits only.
    pub pricing_shadow: crate::pricing::PricingShadowConfig,
    /// Доверять ли loopback-пиру как админу при ПУСТЫХ `api_keys`. true ТОЛЬКО когда сервер
    /// слушает loopback-интерфейс. При bind на 0.0.0.0/публичный IP — false: за реверс-прокси
    /// (nginx) peer виден как 127.0.0.1, и доверие loopback открыло бы аноним-админ + бесплатный
    /// форвардинг. На экспонированном bind без `api_keys` сервер отклоняет всё (безопасный дефолт).
    pub trust_loopback: bool,
    pub upstream: String, // база апстрима (https://api.anthropic.com)
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
    /// Отпечаток Claude Code клиента (Stainless SDK) — синтезируем эти заголовки в КАЖДЫЙ апстрим-
    /// запрос, чтобы выглядеть как настоящий CC, и НЕ пробрасываем клиентские (иначе Python-SDK
    /// клиент дал бы `x-stainless-lang: python` при нашем claude-cli UA = противоречие). Флот-константны
    /// и ВАЛИДНЫ (реальные юзеры на одной версии CC их делят); различие аккаунтов — через IP+user_id.
    pub x_app: String, // "cli"
    pub stainless_lang: String,    // "js"
    pub stainless_runtime: String, // "node"
    pub stainless_runtime_version: String, // "v22.x.x"
    pub stainless_package_version: String, // версия @anthropic-ai/sdk (снимается refresh-скриптом)
    pub stainless_os: String,      // "MacOS"
    pub stainless_arch: String,    // "arm64"
}
