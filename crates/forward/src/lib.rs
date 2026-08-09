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
mod anthropic;
mod anthropic_calibration;
pub mod anthropic_responses;
mod anthropic_stream;
mod billing;
mod breaker;
pub mod codex;
mod config;
mod execution;
pub mod gemini;
mod gemini_schema;
mod gemini_stream;
pub mod glm;
mod glm_calibration;
pub mod kimi;
mod kimi_calibration;
mod lifecycle;
mod meter;
mod metrics;
pub mod settlement_policy;
pub mod nodetls;
mod openai_responses_stream;
mod pricing;
mod proxy;
mod state;
mod upstream;
mod validation;

pub use affinity::{
    AffinityInput, AffinityResolution, AffinitySource, AffinityStats, AffinityStore,
};
pub use anthropic::anthropic_chat_completions;
pub use anthropic_responses::anthropic_responses;
pub use billing::{
    AnthropicCalibrationDeliveryStatus, AsyncBilling, GeminiCalibrationDeliveryStatus,
    PgCommandLatencyStats, PgCommandOp, PG_COMMAND_LATENCY_BUCKETS_MS,
};
pub use breaker::Breaker;
pub use codex::{
    codex_messages_count_tokens, codex_messages_skin, openai_chat_completions,
    openai_delete_response, openai_get_response, openai_image_edits, openai_image_generations,
    openai_input_tokens, openai_model, openai_models, openai_response_input_items,
    openai_responses, CodexConfig, CodexGateway, CodexImageError, CodexImageResult, CodexModel,
    CodexOperationalStatus, CodexPrices, CodexProfileSpec, CodexProfilesFile, CodexRateLimitWindow,
    CodexRateLimits, GptImage2, ImageBackground, ImageEditRequest, ImageErrorContext,
    ImageGenerationRequest, ImageQuality, ImageReference, ImageSize, ImageTurnId, GPT_IMAGE_2,
};
pub use config::{
    ClaudeStoreFallbackConfig, ProxyConfig, CLAUDESTORE_FALLBACK_BASE_URL, CLAUDE_CODE_IDENTITY,
};
pub use gemini::{
    gemini_api, gemini_chat_completions, gemini_messages_count_tokens, gemini_messages_skin,
    gemini_responses, GeminiConfig, GeminiGateway, GeminiModel, GeminiModelStatus,
    GeminiOperationalStatus, GeminiPrices, GeminiProfileSpec, GeminiProfileStatus,
    GeminiProfilesFile, GeminiWindowCapacityReport, GEMINI_NODE_EXPECTED_JA3,
    GEMINI_NODE_EXPECTED_JA4, GEMINI_NODE_FETCH_EXPECTED_JA3, GEMINI_NODE_FETCH_EXPECTED_JA4,
    GEMINI_NODE_FETCH_TRANSPORT_PROFILE, GEMINI_NODE_TRANSPORT_PROFILE,
};
pub use kimi::{KimiGateway, KimiOperationalStatus, KimiProfileStatus, KimiQuotaWindowStatus};
pub use metrics::Metrics;
pub use pricing::tariff_book;
pub use pricing::PricingBridgeFallbackReason;
pub use proxy::{
    authed, client_keys, control_authed, forward, is_exact_not_started_response, readonly_authed,
    resolve_client_key, resolve_client_keys, TerminalErrorReason,
};
pub use state::{AppState, ProviderMode};
pub use upstream::{
    detect_plan, fresh_request_id, limits_from_headers, persona_ccbuild, persona_cch,
    persona_session_id, persona_ua, persona_user_id, poll_sub, Clients, Limits, PlanDetect,
    PollResult, QuotaFraction,
};
