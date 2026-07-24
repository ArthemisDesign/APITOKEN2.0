//! Общее состояние форвардинга, клонируется в каждый axum-хендлер.

use crate::affinity::AffinityStore;
use crate::billing::AsyncBilling;
use crate::breaker::Breaker;
use crate::codex::CodexGateway;
use crate::config::ProxyConfig;
use crate::keylimiter::KeyLimiter;
use crate::metrics::Metrics;
use crate::upstream::Clients;
use pool::Pool;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pub cfg: Arc<ProxyConfig>,
    /// Authority selector for read-only admin overviews. Secrets are never rendered in logs/responses.
    pub authority: Arc<registry::authority::AuthorityConfig>,
    /// Local data directory path remains the home of non-authoritative metrics.db.
    pub data_db_path: Arc<String>,
    pub pool: Arc<Pool>,
    /// Ephemeral cache-lineage affinity. Local L1 is always available; Redis, when configured,
    /// shares bindings across engine slots. It never authorizes money or capacity.
    pub affinity: Arc<AffinityStore>,
    pub clients: Arc<Clients>,
    /// Optional OpenAI-compatible text provider backed by an official Codex app-server child.
    /// It owns no OAuth material; the child reads the dedicated authenticated `CODEX_HOME`.
    pub codex: Option<Arc<CodexGateway>>,
    /// Биллинг клиентов (async DB-актор — синхронный SQLite не блокирует воркеры).
    /// `None` → биллинг выключен (только env-ключи/localhost).
    pub billing: Option<Arc<AsyncBilling>>,
    /// Live dependency health. PostgreSQL heartbeat toggles this and request admission fails closed.
    pub authority_ready: Arc<AtomicBool>,
    /// Глобальный circuit breaker апстрима (анти-амплификация при брауноуте api.anthropic.com).
    pub breaker: Arc<Breaker>,
    /// Счётчики форвардинга для `/metrics`.
    pub metrics: Arc<Metrics>,
    /// Fair-share: счётчик одновременных запросов на клиентский ключ (кит не набивает флот).
    pub key_limiter: Arc<KeyLimiter>,
    /// ГЛОБАЛЬНЫЙ потолок одновременной обработки запросов (анти-DoS): флуд неверными ключами или
    /// тяжёлыми телами не должен насытить пул DB-читателей/память сверх лимита. Разрешение держится
    /// до EOF/ошибки/Drop response body, включая длинный SSE-стрим. Переполнение → 503.
    /// Connection-level slowloris/idle-таймауты —
    /// это reverse-proxy (Caddy/nginx/CF, Фаза 3 вместе с TLS), здесь бьём по стоимости обработки.
    pub concurrency: Arc<tokio::sync::Semaphore>,
    /// Разбудить liveness-поллер вне расписания (forward зовёт после `pool.request_probe`, когда
    /// подписка отдала 401/403 → надо СРАЗУ рассудить чистым probe, мёртв ли токен). `None` → поллер
    /// выключен (`CLAUDE_API_POLL=0`), тогда probe-по-требованию просто не нужен.
    pub probe_poke: Option<Arc<tokio::sync::Notify>>,
}
