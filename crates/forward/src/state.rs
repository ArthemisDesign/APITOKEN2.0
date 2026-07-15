//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::config::ProxyConfig;
use crate::keylimiter::KeyLimiter;
use crate::metrics::Metrics;
use crate::upstream::Clients;
use pool::Pool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<ProxyConfig>,
    /// Путь к реестру (subscriptions.db) — для read-only админ-обзоров (`/subs`: срок/прокси
    /// подписок). Money-путь БД к нему не обращается (у него свой AsyncBilling), это только чтение.
    pub db_path: Arc<String>,
    pub pool: Arc<Pool>,
    pub clients: Arc<Clients>,
    /// Биллинг клиентов (async DB-актор — синхронный SQLite не блокирует воркеры).
    /// `None` → биллинг выключен (только env-ключи/localhost).
    pub billing: Option<Arc<AsyncBilling>>,
    /// Глобальный circuit breaker апстрима (анти-амплификация при брауноуте api.anthropic.com).
    pub breaker: Arc<Breaker>,
    /// Счётчики форвардинга для `/metrics`.
    pub metrics: Arc<Metrics>,
    /// Fair-share: счётчик одновременных запросов на клиентский ключ (кит не набивает флот).
    pub key_limiter: Arc<KeyLimiter>,
    /// ГЛОБАЛЬНЫЙ потолок одновременной обработки запросов (анти-DoS): флуд неверными ключами или
    /// тяжёлыми телами не должен насытить пул DB-читателей/память сверх лимита. Разрешение держится
    /// на время обработки (парсинг+авторизация+резерв+запрос апстрима), отпускается перед стримом →
    /// длинные стримы потолок НЕ занимают. Переполнение → 503. Connection-level slowloris/idle-таймауты —
    /// это reverse-proxy (Caddy/nginx/CF, Фаза 3 вместе с TLS), здесь бьём по стоимости обработки.
    pub concurrency: Arc<tokio::sync::Semaphore>,
    /// Разбудить liveness-поллер вне расписания (forward зовёт после `pool.request_probe`, когда
    /// подписка отдала 401/403 → надо СРАЗУ рассудить чистым probe, мёртв ли токен). `None` → поллер
    /// выключен (`CLAUDE_API_POLL=0`), тогда probe-по-требованию просто не нужен.
    pub probe_poke: Option<Arc<tokio::sync::Notify>>,
}
