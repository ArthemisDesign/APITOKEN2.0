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
    /// Сколько запросов сейчас «в полёте» на этой подписке (мягкий счётчик для веерного
    /// разброса параллельных запросов между опросами util). Инкремент на pick, декремент
    /// на завершении попытки. Клампится в 0 — это подсказка, а не точный учёт.
    pub inflight: i64,
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
        // Эффективная утилизация с УЧЁТОМ обнуления окна: если reset уже прошёл, окно
        // обнулилось (util снова ~0), даже если поллер ещё не обновил число.
        let eff = |e: &str, w: u8| g.live.get(e).map(|l| {
            let (util, reset) = if w == 7 { (l.util7d, l.reset7d) } else { (l.util5h, l.reset5h) };
            if reset != 0 && now >= reset { 0.0 } else { util }
        }).unwrap_or(0.0);
        let warn = |e: &str| g.live.get(e).map(|l| l.status.contains("warning")).unwrap_or(false);
        let inflight = |e: &str| g.live.get(e).map(|l| l.inflight).unwrap_or(0);
        let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);

        // 1) не остывающие; 2) под потолком — с постепенным ослаблением фильтра
        let not_cool: Vec<&&Sub> = candidates.iter().filter(|s| !cool(&s.email)).collect();
        let stage1: Vec<&&Sub> = if not_cool.is_empty() { candidates.iter().collect() } else { not_cool };
        let ready: Vec<&&Sub> = stage1.iter()
            .filter(|s| eff(&s.email, 7) < cap && eff(&s.email, 5) < cap).cloned().collect();
        let mut pool: Vec<&&Sub> = if ready.is_empty() { stage1 } else { ready };

        // Стратегия: беречь недельный бюджет (7d) → размазывать 5h → «предупреждённые» ниже →
        // меньше in-flight (веер параллельных) → давнее использование (LRU).
        pool.sort_by(|a, b| {
            let (ea, eb) = (&a.email, &b.email);
            eff(ea, 7).partial_cmp(&eff(eb, 7)).unwrap_or(std::cmp::Ordering::Equal)
                .then(eff(ea, 5).partial_cmp(&eff(eb, 5)).unwrap_or(std::cmp::Ordering::Equal))
                .then(warn(ea).cmp(&warn(eb)))          // false (allowed) < true (allowed_warning)
                .then(inflight(ea).cmp(&inflight(eb)))
                .then(lru(ea).cmp(&lru(eb)))
        });
        pool.first().map(|s| (***s).clone())
    }

    /// Взяли подписку в работу (pick): отметить время и +1 в in-flight.
    pub fn mark_used(&self, email: &str) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        l.last_used = now();
        l.inflight += 1;
    }

    pub fn mark_ok(&self, email: &str) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        if l.cooling_until <= now() { l.cooling_until = 0; }
        l.last_used = now();
        l.inflight = (l.inflight - 1).max(0);
    }

    /// Circuit-breaker из ФОРВАРДА: студим на `secs` и завершаем попытку → −1 in-flight.
    /// (Парен с `mark_used`; клампится в 0.)
    pub fn mark_cooling(&self, email: &str, secs: i64) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        l.cooling_until = now() + secs.max(1);
        l.inflight = (l.inflight - 1).max(0);
    }

    /// Студить БЕЗ трогания in-flight — для фонового поллера (он не делал `mark_used`).
    pub fn cool(&self, email: &str, secs: i64) {
        let mut g = self.inner.lock().unwrap();
        g.live.entry(email.to_string()).or_default().cooling_until = now() + secs.max(1);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(email: &str) -> Sub {
        Sub { email: email.into(), token: "t".into(), proxy: String::new(), fleet: "prod".into() }
    }
    fn pool(emails: &[&str]) -> Pool {
        Pool::new(emails.iter().map(|e| sub(e)).collect(), 0.95)
    }
    fn none() -> HashSet<String> { HashSet::new() }
    fn picked(p: &Pool) -> String { p.pick(&none(), false).unwrap().email }

    /// Стратегия «беречь 7d»: выбираем подписку с наименьшим недельным util, даже если её 5h выше.
    #[test]
    fn conserves_weekly_budget() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.10), Some(0.50), None, None, None); // низкий 5h, высокий 7d
        p.set_util("b", Some(0.90), Some(0.10), None, None, None); // высокий 5h, низкий 7d
        assert_eq!(picked(&p), "b"); // 7d решает: b бережём неделю лучше
    }

    /// Rollover: если reset уже прошёл — окно обнулилось, util трактуем как 0.
    #[test]
    fn window_rollover_resets_util() {
        let p = pool(&["a", "b"]);
        let past = now() - 10;
        let future = now() + 3600;
        // a: 5h почти выбран, НО окно уже сброшено (reset в прошлом) → eff5 = 0
        p.set_util("a", Some(0.99), Some(0.0), None, Some(past), None);
        // b: 5h чуть занят, окно ещё живо
        p.set_util("b", Some(0.20), Some(0.0), None, Some(future), None);
        assert_eq!(picked(&p), "a"); // a свежая после сброса, несмотря на util5h=0.99
    }

    /// Cooling исключает подписку, пока есть живая альтернатива.
    #[test]
    fn cooling_excludes_while_alternative_exists() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.0), Some(0.0), None, None, None);
        p.set_util("b", Some(0.0), Some(0.0), None, None, None);
        p.mark_cooling("a", 300);
        assert_eq!(picked(&p), "b");
    }

    /// in-flight веер: при равных util уходим на менее загруженную прямо сейчас.
    #[test]
    fn inflight_fans_out() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.10), Some(0.10), None, None, None);
        p.set_util("b", Some(0.10), Some(0.10), None, None, None);
        p.mark_used("a"); // a теперь inflight=1
        assert_eq!(picked(&p), "b");
    }

    /// Пул не зависает: даже если все за потолком — отдаём наименее горячую.
    #[test]
    fn never_stalls_when_all_hot() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.99), Some(0.99), None, None, None);
        p.set_util("b", Some(0.97), Some(0.96), None, None, None);
        assert_eq!(picked(&p), "b"); // обе > cap, но b менее горячая по 7d
    }
}
