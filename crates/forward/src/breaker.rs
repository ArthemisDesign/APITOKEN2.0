//! Глобальный circuit breaker апстрима.
//!
//! 5xx/сетевые ошибки — вина `api.anthropic.com`, а не подписки. Без брейкера при брауноуте
//! апстрима КАЖДЫЙ запрос веерил бы до `max_tries` подписок, студя их по очереди и долбя
//! шатающийся апстрим со всех IP разом (амплификация + ложное выведение пула из строя).
//!
//! Копим свежие backend-фейлы в окне; порог превышен → размыкаем на `cooldown`. Пока разомкнут —
//! новые запросы отбиваем быстрым `503 + Retry-After` (не веером). Здоровый ответ сбрасывает окно.

use std::collections::HashSet;
use std::sync::Mutex;

/// Столько РАЗНЫХ подписок должны зафейлить в окне → размыкание. Считаем distinct-подписки, а не
/// сырые фейлы: аутейдж api.anthropic.com бьёт МНОГО разных аккаунтов, тогда как одна флаки-прокси
/// (или poison-запрос на одну подписку) даёт фейлы одного email → breaker НЕ размыкается зря.
const DISTINCT_THRESHOLD: usize = 6;
/// Окно счёта фейлов (сек).
const WINDOW: i64 = 10;
/// На сколько размыкаемся (сек) — короткий отбой, чтобы быстро проверить восстановление.
const COOLDOWN: i64 = 15;

struct Inner {
    fails: Vec<(i64, String)>, // (таймстемп, email) свежих backend-фейлов
    open_until: i64,           // до какого времени брейкер разомкнут (0 = замкнут)
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

    /// Зафиксировать backend-фейл подписки `email` (5xx/timeout/сетевой). Размыкаем, только если в
    /// окне зафейлило ≥ `DISTINCT_THRESHOLD` РАЗНЫХ подписок (признак аутейджа апстрима, а не одной прокси).
    pub fn record_fail(&self, now: i64, email: &str) {
        let mut g = self.inner.lock().unwrap();
        g.fails.retain(|(t, _)| now - t < WINDOW);
        g.fails.push((now, email.to_string()));
        let distinct: HashSet<&str> = g.fails.iter().map(|(_, e)| e.as_str()).collect();
        if distinct.len() >= DISTINCT_THRESHOLD {
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
    fn trips_only_on_distinct_subs_and_recovers() {
        let b = Breaker::new();
        let t = 1_000_000i64;
        // много фейлов ОДНОЙ подписки (флаки-прокси/poison) — breaker НЕ размыкается
        for _ in 0..20 { b.record_fail(t, "a@x.io"); }
        assert!(b.open_for(t).is_none(), "одна подписка не должна размыкать");
        // DISTINCT_THRESHOLD РАЗНЫХ подписок → аутейдж апстрима → размыкание
        for i in 0..DISTINCT_THRESHOLD { b.record_fail(t, &format!("s{i}@x.io")); }
        assert!(b.open_for(t).is_some(), "много разных подписок → разомкнут");
        assert!(b.open_for(t + COOLDOWN + 1).is_none(), "после cooldown — замкнут");
    }

    #[test]
    fn stale_fails_expire_out_of_window() {
        let b = Breaker::new();
        for i in 0..DISTINCT_THRESHOLD - 1 { b.record_fail(1_000_000, &format!("s{i}@x.io")); }
        // спустя окно — старые протухли, один новый (даже новой подписки) не размыкает
        b.record_fail(1_000_000 + WINDOW + 1, "late@x.io");
        assert!(b.open_for(1_000_000 + WINDOW + 1).is_none());
    }
}
