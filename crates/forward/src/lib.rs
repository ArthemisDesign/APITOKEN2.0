//! # forward — прозрачный форвардинг на api.anthropic.com (Шаг B)
//!
//! Для клиента сервер неотличим от настоящего Anthropic API. Крейт содержит: конфиг прокси,
//! кэш http-клиентов, поллер лимитов, axum-хендлер форвардинга с инжектом Claude Code identity,
//! ротацией по лимитам и стримингом ответа байт-в-байт.
//!
//! **Границы крейта:** сеть + HTTP-транспорт форвардинга. Зависит от `pool` (выбор подписки)
//! и `registry` (тип Sub). НЕ читает окружение и НЕ содержит CLI/роутинг управляющих
//! эндпоинтов — это делает крейт `server` (композиция).

mod breaker;
mod config;
mod meter;
mod proxy;
mod state;
mod upstream;

pub use breaker::Breaker;
pub use config::{ProxyConfig, CLAUDE_CODE_IDENTITY};
pub use proxy::{authed, client_key, forward};
pub use state::AppState;
pub use upstream::{detect_plan, limits_from_headers, persona_ua, poll_sub, Clients, Limits, PlanDetect, PollResult};
