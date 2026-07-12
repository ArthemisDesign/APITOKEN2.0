//! Глобальный circuit breaker апстрима.
//!
//! 5xx/сетевые ошибки — вина `api.anthropic.com`, а не подписки. Без брейкера при брауноуте
//! апстрима КАЖДЫЙ запрос веерил бы до `max_tries` подписок, студя их по очереди и долбя
//! шатающийся апстрим со всех IP разом (амплификация + ложное выведение пула из строя).
//!
//! Копим свежие backend-фейлы в окне; порог превышен → размыкаем на `cooldown`. Пока разомкнут —
//! новые запросы отбиваем быстрым `503 + Retry-After` (не веером). Здоровый ответ сбрасывает окно.

use std::sync::Mutex;

/// Столько backend-фейлов в окне → размыкание.
const THRESHOLD: u32 = 12;
/// Окно счёта фейлов (сек).
const WINDOW: i64 = 10;
/// На сколько размыкаемся (сек) — короткий отбой, чтобы быстро проверить восстановление.
const COOLDOWN: i64 = 15;

struct Inner {
    fails: Vec<i64>,   // таймстемпы свежих backend-фейлов
    open_until: i64,   // до какого времени брейкер разомкнут (0 = замкнут)
}

pub struct Breaker {
    inner: Mutex<Inner>,
}

impl Default for Breaker {
    fn default() -> Self { Self::new() }
}

impl Breaker {
    pub fn new() -> Self {
        Breaker { inner: Mutex::new(Inner { fails: Vec::new(), open_until: 0 }) }
    }

    /// Зафиксировать backend-фейл (5xx/timeout/сетевой). Порог в окне → разомкнуть.
    pub fn record_fail(&self, now: i64) {
        let mut g = self.inner.lock().unwrap();
        g.fails.retain(|t| now - t < WINDOW);
        g.fails.push(now);
        if g.fails.len() as u32 >= THRESHOLD {
            g.open_until = now + COOLDOWN;
            g.fails.clear();
        }
    }

    /// Здоровый ответ апстрима → окно фейлов сбрасываем (half-open восстановление).
    pub fn record_ok(&self, now: i64) {
        let mut g = self.inner.lock().unwrap();
        if g.open_until <= now && !g.fails.is_empty() {
            g.fails.clear();
        }
    }

    /// Разомкнут? → `Some(секунд до замыкания)` для `Retry-After`; иначе `None`.
    pub fn open_for(&self, now: i64) -> Option<i64> {
        let g = self.inner.lock().unwrap();
        if g.open_until > now { Some((g.open_until - now).max(1)) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_after_threshold_and_recovers() {
        let b = Breaker::new();
        let t = 1_000_000i64;
        for _ in 0..THRESHOLD - 1 { b.record_fail(t); }
        assert!(b.open_for(t).is_none(), "ещё под порогом — замкнут");
        b.record_fail(t);
        assert!(b.open_for(t).is_some(), "порог достигнут — разомкнут");
        assert!(b.open_for(t + COOLDOWN + 1).is_none(), "после cooldown — замкнут");
    }

    #[test]
    fn stale_fails_expire_out_of_window() {
        let b = Breaker::new();
        for _ in 0..THRESHOLD - 1 { b.record_fail(1_000_000); }
        // спустя окно — старые фейлы протухли, один новый не размыкает
        b.record_fail(1_000_000 + WINDOW + 1);
        assert!(b.open_for(1_000_000 + WINDOW + 1).is_none());
    }
}
