//! Счётчики форвардинга для наблюдаемости (`/metrics`). Дёшевые атомики, монотонные с запуска.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct Metrics {
    pub requests: AtomicU64,     // всего обслуженных запросов (после авторизации)
    pub upstream_429: AtomicU64, // ответов апстрима 429 (квота подписки)
    pub upstream_auth: AtomicU64, // 401/403 (мёртвый токен → карантин)
    pub upstream_5xx: AtomicU64, // backend-fault (5xx/408/409/425)
    pub breaker_rejects: AtomicU64, // отбито разомкнутым circuit breaker
    pub exhausted: AtomicU64,    // исчерпание пула (все за лимитом) → 429+Retry-After
    pub key_throttled: AtomicU64, // отбито fair-share (кит превысил потолок одновременных)
    pub auth_failures: AtomicU64, // неудачных авторизаций (спайк = брутфорс/скан управляющих ключей)
    /// Successful Gemini generations that ended without authoritative usage. Metered non-stream
    /// delivery is withheld; a stream already delivered settles its conservative hold.
    pub gemini_usage_missing: AtomicU64,
}

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }
    #[inline]
    pub fn inc(c: &AtomicU64) {
        c.fetch_add(1, Ordering::Relaxed);
    }
    #[inline]
    pub fn get(c: &AtomicU64) -> u64 {
        c.load(Ordering::Relaxed)
    }
}
