//! Глобальный circuit breaker апстрима.
//!
//! Correlated provider 5xx responses feed the global breaker. Transport/proxy failures remain
//! per-subscription signals: conflating them would let one flaky proxy declare a fleet outage.
//!
//! Копим свежие backend-фейлы в окне; порог превышен → размыкаем на `cooldown`. Пока разомкнут —
//! новые запросы отбиваем быстрым `503 + Retry-After` (не веером). Здоровый ответ сбрасывает окно.

use std::collections::HashSet;
use std::sync::Mutex;

/// Окно счёта фейлов (сек).
const WINDOW: i64 = 10;
/// На сколько размыкаемся (сек) — короткий отбой, чтобы быстро проверить восстановление.
const COOLDOWN: i64 = 15;

struct Inner {
    fails: Vec<(i64, String)>, // (таймстемп, email) свежих backend-фейлов
    open_until: i64,           // до какого времени брейкер разомкнут (0 = замкнут)
    fleet_size: usize,
}

pub struct Breaker {
    inner: Mutex<Inner>,
}

fn distinct_threshold(fleet_size: usize) -> usize {
    fleet_size.max(1) / 2 + 1
}

impl Default for Breaker {
    fn default() -> Self {
        Self::new(1)
    }
}

impl Breaker {
    pub fn new(fleet_size: usize) -> Self {
        Breaker {
            inner: Mutex::new(Inner {
                fails: Vec::new(),
                open_until: 0,
                fleet_size,
            }),
        }
    }

    /// Keep the strict-majority denominator aligned with hot roster reloads. Samples collected
    /// under another roster are not comparable, so discard them when the size changes.
    pub fn set_fleet_size(&self, fleet_size: usize) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.fleet_size != fleet_size {
            g.fleet_size = fleet_size;
            g.fails.clear();
        }
    }

    /// Зафиксировать provider backend-фейл подписки `email` (5xx/408/409/425). Размыкаем, только если в
    /// окне зафейлило ≥ `DISTINCT_THRESHOLD` РАЗНЫХ подписок (признак аутейджа апстрима, а не одной прокси).
    pub fn record_fail(&self, now: i64, email: &str) {
        // poison-tolerant, как pool/keylimiter/billing: паника под держанием лока НЕ должна навсегда
        // ронять горячий путь (open_for зовётся на КАЖДЫЙ запрос → иначе один poison = вечный отказ).
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.fails.retain(|(t, _)| now - t < WINDOW);
        g.fails.push((now, email.to_string()));
        let distinct: HashSet<&str> = g.fails.iter().map(|(_, e)| e.as_str()).collect();
        if distinct.len() >= distinct_threshold(g.fleet_size) {
            g.open_until = now + COOLDOWN;
            g.fails.clear();
        }
    }

    /// Здоровый ответ апстрима → окно фейлов сбрасываем (half-open восстановление).
    pub fn record_ok(&self, now: i64) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.open_until <= now && !g.fails.is_empty() {
            g.fails.clear();
        }
    }

    /// Разомкнут? → `Some(секунд до замыкания)` для `Retry-After`; иначе `None`.
    pub fn open_for(&self, now: i64) -> Option<i64> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if g.open_until > now {
            Some((g.open_until - now).max(1))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_only_on_distinct_subs_and_recovers() {
        let b = Breaker::new(10);
        let t = 1_000_000i64;
        // много фейлов ОДНОЙ подписки (флаки-прокси/poison) — breaker НЕ размыкается
        for _ in 0..20 {
            b.record_fail(t, "a@x.io");
        }
        assert!(b.open_for(t).is_none(), "одна подписка не должна размыкать");
        // DISTINCT_THRESHOLD РАЗНЫХ подписок → аутейдж апстрима → размыкание
        for i in 0..distinct_threshold(10) {
            b.record_fail(t, &format!("s{i}@x.io"));
        }
        assert!(b.open_for(t).is_some(), "много разных подписок → разомкнут");
        assert!(
            b.open_for(t + COOLDOWN + 1).is_none(),
            "после cooldown — замкнут"
        );
    }

    #[test]
    fn stale_fails_expire_out_of_window() {
        let b = Breaker::new(10);
        for i in 0..distinct_threshold(10) - 1 {
            b.record_fail(1_000_000, &format!("s{i}@x.io"));
        }
        // спустя окно — старые протухли, один новый (даже новой подписки) не размыкает
        b.record_fail(1_000_000 + WINDOW + 1, "late@x.io");
        assert!(b.open_for(1_000_000 + WINDOW + 1).is_none());
    }

    #[test]
    fn threshold_tracks_roster_reload() {
        let b = Breaker::new(1);
        b.set_fleet_size(10);
        for i in 0..distinct_threshold(10) - 1 {
            b.record_fail(1_000_000, &format!("s{i}@x.io"));
        }
        assert!(b.open_for(1_000_000).is_none());
        b.record_fail(1_000_000, "majority@x.io");
        assert!(b.open_for(1_000_000).is_some());
    }
}
