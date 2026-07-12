//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::breaker::Breaker;
use crate::config::ProxyConfig;
use crate::keylimiter::KeyLimiter;
use crate::metrics::Metrics;
use crate::upstream::Clients;
use pool::Pool;
use registry::Billing;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<ProxyConfig>,
    pub pool: Arc<Pool>,
    pub clients: Arc<Clients>,
    /// Биллинг ключей клиентов. `None` → биллинг выключен (только env-ключи/localhost).
    pub billing: Option<Arc<Billing>>,
    /// Глобальный circuit breaker апстрима (анти-амплификация при брауноуте api.anthropic.com).
    pub breaker: Arc<Breaker>,
    /// Счётчики форвардинга для `/metrics`.
    pub metrics: Arc<Metrics>,
    /// Fair-share: счётчик одновременных запросов на клиентский ключ (кит не набивает флот).
    pub key_limiter: Arc<KeyLimiter>,
}
