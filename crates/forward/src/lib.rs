//! # forward — прозрачный форвардинг на api.anthropic.com (Шаг B)
//!
//! Для клиента сервер неотличим от настоящего Anthropic API. Крейт содержит: конфиг прокси,
//! кэш http-клиентов, поллер лимитов, axum-хендлер форвардинга с инжектом Claude Code identity,
//! ротацией по лимитам и стримингом ответа байт-в-байт.
//!
//! **Границы крейта:** сеть + HTTP-транспорт форвардинга. Зависит от `pool` (выбор подписки)
//! и `registry` (тип Sub). НЕ читает окружение и НЕ содержит CLI/роутинг управляющих
//! эндпоинтов — это делает крейт `server` (композиция).

mod affinity;
mod billing;
mod breaker;
pub mod codex;
mod config;
mod keylimiter;
mod meter;
mod metrics;
pub mod nodetls;
mod proxy;
mod state;
mod upstream;

pub use affinity::{
    AffinityInput, AffinityResolution, AffinitySource, AffinityStats, AffinityStore,
};
pub use billing::AsyncBilling;
pub use breaker::Breaker;
pub use codex::{
    openai_chat_completions, openai_model, openai_models, openai_responses, CodexConfig,
    CodexGateway, CodexModel, CodexOperationalStatus, CodexPrices, CodexRateLimitWindow,
    CodexRateLimits,
};
pub use config::{ProxyConfig, CLAUDE_CODE_IDENTITY};
pub use keylimiter::KeyLimiter;
pub use metrics::Metrics;
pub use proxy::{authed, client_key, control_authed, forward, readonly_authed};
pub use state::AppState;
pub use upstream::{
    detect_plan, fresh_request_id, limits_from_headers, persona_ccbuild, persona_cch,
    persona_session_id, persona_ua, persona_user_id, poll_sub, Clients, Limits, PlanDetect,
    PollResult,
};
