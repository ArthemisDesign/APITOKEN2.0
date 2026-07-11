//! # pool — пул подписок + ротация (пункт 2)
//!
//! Держит текущий список подписок (обновляется из БД фоном сервером) и волатильное
//! состояние по каждой (утилизация окон 5h/7d, cooling, last_used). На каждый ход отдаёт
//! наименее загруженную живую подписку; при 429 — cooling и следующая.
//!
//! **Границы крейта:** чистая in-memory логика выбора и состояния. НИКАКОЙ сети/HTTP/БД.
//! Зависит только от `registry` (тип [`Sub`]). Опрос лимитов и форвардинг — крейтом выше.

use registry::Sub;
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

#[derive(Clone, Default, Debug)]
pub struct Live {
    pub util5h: f64,
    pub util7d: f64,
    pub status: String,
    pub cooling_until: i64,
    pub last_used: i64,
    pub polled_ts: i64,
    pub reset5h: i64,
    pub reset7d: i64,
}

struct Inner {
    subs: Vec<Sub>,
    live: HashMap<String, Live>,
}

pub struct Pool {
    inner: Mutex<Inner>,
    util_cap: f64,
}

impl Pool {
    pub fn new(subs: Vec<Sub>, util_cap: f64) -> Self {
        Pool { inner: Mutex::new(Inner { subs, live: HashMap::new() }), util_cap }
    }

    /// Заменить список подписок (из БД), сохранив волатильное состояние существующих.
    pub fn replace_subs(&self, subs: Vec<Sub>) {
        let mut g = self.inner.lock().unwrap();
        let keep: HashSet<String> = subs.iter().map(|s| s.email.clone()).collect();
        g.live.retain(|k, _| keep.contains(k));
        g.subs = subs;
    }

    pub fn len(&self) -> usize { self.inner.lock().unwrap().subs.len() }

    /// Наименее загруженная живая подписка не из `exclude`.
    /// allow_full=true → пускаем до 100% (приоритетные ходы), иначе потолок util_cap.
    pub fn pick(&self, exclude: &HashSet<String>, allow_full: bool) -> Option<Sub> {
        let g = self.inner.lock().unwrap();
        let now = now();
        let cap = if allow_full { 1.0 } else { self.util_cap };
        let candidates: Vec<&Sub> = g.subs.iter().filter(|s| !exclude.contains(&s.email)).collect();
        if candidates.is_empty() { return None; }

        let cool = |e: &str| g.live.get(e).map(|l| l.cooling_until > now).unwrap_or(false);
        let u = |e: &str, w: u8| g.live.get(e)
            .map(|l| if w == 7 { l.util7d } else { l.util5h }).unwrap_or(0.0);
        let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);

        // 1) не остывающие; 2) под потолком — с постепенным ослаблением фильтра
        let not_cool: Vec<&&Sub> = candidates.iter().filter(|s| !cool(&s.email)).collect();
        let stage1: Vec<&&Sub> = if not_cool.is_empty() { candidates.iter().collect() } else { not_cool };
        let ready: Vec<&&Sub> = stage1.iter()
            .filter(|s| u(&s.email, 7) < cap && u(&s.email, 5) < cap).cloned().collect();
        let mut pool: Vec<&&Sub> = if ready.is_empty() { stage1 } else { ready };

        pool.sort_by(|a, b| {
            let (ea, eb) = (&a.email, &b.email);
            u(ea, 7).partial_cmp(&u(eb, 7)).unwrap_or(std::cmp::Ordering::Equal)
                .then(u(ea, 5).partial_cmp(&u(eb, 5)).unwrap_or(std::cmp::Ordering::Equal))
                .then(lru(ea).cmp(&lru(eb)))
        });
        pool.first().map(|s| (***s).clone())
    }

    pub fn mark_used(&self, email: &str) {
        let mut g = self.inner.lock().unwrap();
        g.live.entry(email.to_string()).or_default().last_used = now();
    }

    pub fn mark_ok(&self, email: &str) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        if l.cooling_until <= now() { l.cooling_until = 0; }
        l.last_used = now();
    }

    /// Circuit-breaker при 429: студим подписку на `secs` секунд.
    pub fn mark_cooling(&self, email: &str, secs: i64) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        l.cooling_until = now() + secs.max(1);
    }

    pub fn set_util(&self, email: &str, u5: Option<f64>, u7: Option<f64>,
                    status: Option<String>, r5: Option<i64>, r7: Option<i64>) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        if let Some(v) = u5 { l.util5h = v; }
        if let Some(v) = u7 { l.util7d = v; }
        if let Some(v) = status { l.status = v; }
        if let Some(v) = r5 { l.reset5h = v; }
        if let Some(v) = r7 { l.reset7d = v; }
        l.polled_ts = now();
    }

    /// Снапшот для /pool (без секретов) + список подписок для поллера.
    pub fn snapshot(&self) -> Vec<(Sub, Live)> {
        let g = self.inner.lock().unwrap();
        g.subs.iter().map(|s| (s.clone(), g.live.get(&s.email).cloned().unwrap_or_default())).collect()
    }
}
