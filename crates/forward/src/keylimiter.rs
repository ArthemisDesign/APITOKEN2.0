//! Fair-share: потолок ОДНОВРЕМЕННЫХ запросов на клиентский ключ.
//!
//! Баланс ограничивает СУММАРНЫЙ расход клиента, но не одновременность — один «кит» мог бы в рамках
//! своего баланса залить флот бёрстом параллельных запросов и оставить остальных на 429. Этот счётчик
//! кап-ирует, сколько запросов ключа одновременно «в полёте», чтобы нагрузка делилась между клиентами.

use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Default)]
pub struct KeyLimiter {
    inner: Mutex<HashMap<String, u32>>,
}

impl KeyLimiter {
    pub fn new() -> Self { Self::default() }

    /// Занять слот для `key`. `cap == 0` → без лимита (всегда true). Возвращает false, если потолок
    /// уже достигнут (при этом счётчик НЕ увеличивается). Карта растёт только по активным ключам,
    /// пустые записи вычищаются в [`release`] → память ограничена числом одновременных клиентов.
    pub fn try_acquire(&self, key: &str, cap: u32) -> bool {
        if cap == 0 { return true; }
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let n = g.entry(key.to_string()).or_insert(0);
        if *n >= cap { return false; }
        *n += 1;
        true
    }

    /// Освободить слот `key` (парен с успешным `try_acquire`). Клампится в 0, пустую запись удаляет.
    pub fn release(&self, key: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(n) = g.get_mut(key) {
            *n = n.saturating_sub(1);
            if *n == 0 { g.remove(key); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_concurrency_and_releases() {
        let l = KeyLimiter::new();
        assert!(l.try_acquire("k", 2));
        assert!(l.try_acquire("k", 2));
        assert!(!l.try_acquire("k", 2), "3-й одновременный сверх потолка → отказ");
        l.release("k");
        assert!(l.try_acquire("k", 2), "после release слот снова свободен");
        // другой ключ не затронут
        assert!(l.try_acquire("other", 2));
    }

    #[test]
    fn zero_cap_is_unlimited() {
        let l = KeyLimiter::new();
        for _ in 0..1000 { assert!(l.try_acquire("k", 0)); }
    }
}
