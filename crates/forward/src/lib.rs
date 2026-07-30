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
pub mod gemini;
mod keylimiter;
mod meter;
mod metrics;
pub mod nodetls;
mod pricing;
mod proxy;
mod state;
mod upstream;

pub use affinity::{
    AffinityInput, AffinityResolution, AffinitySource, AffinityStats, AffinityStore,
};
pub use billing::AsyncBilling;
pub use breaker::Breaker;
pub use codex::{
    openai_chat_completions, openai_delete_response, openai_get_response, openai_input_tokens,
    openai_model, openai_models, openai_response_input_items, openai_responses, CodexConfig,
    CodexGateway, CodexModel, CodexOperationalStatus, CodexPrices, CodexRateLimitWindow,
    CodexRateLimits, CodexTransport,
};
pub use config::{ProxyConfig, CLAUDE_CODE_IDENTITY};
pub use gemini::{
    gemini_api, GeminiConfig, GeminiGateway, GeminiModel, GeminiModelStatus,
    GeminiOperationalStatus, GeminiPrices, GeminiProfileSpec, GeminiProfileStatus,
    GeminiProfilesFile, GEMINI_NODE_EXPECTED_JA3, GEMINI_NODE_EXPECTED_JA4,
    GEMINI_NODE_FETCH_EXPECTED_JA3, GEMINI_NODE_FETCH_EXPECTED_JA4,
    GEMINI_NODE_FETCH_TRANSPORT_PROFILE, GEMINI_NODE_TRANSPORT_PROFILE,
};
pub use keylimiter::KeyLimiter;
pub use metrics::Metrics;
pub use pricing::{
    resolve_pricing, PricingDependencyKind, PricingResolution, PricingResolutionLineage,
    PricingResolutionRejection, PricingResolutionRequest, ResolvedPricingDependency,
    ResolvedPricingLineage, ResolvedPricingRule, RuntimePricingCapability, RuntimePricingManifest,
};
pub use proxy::{
    authed, client_keys, control_authed, forward, readonly_authed, resolve_client_key,
    resolve_client_keys, TerminalErrorReason,
};
pub use state::{AppState, ProviderMode};
pub use upstream::{
    detect_plan, fresh_request_id, limits_from_headers, persona_ccbuild, persona_cch,
    persona_session_id, persona_ua, persona_user_id, poll_sub, Clients, Limits, PlanDetect,
    PollResult,
};
