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

// ── Калибровка ёмкости окон (USD real-API за полное окно) ────────────────────
// Anthropic НЕ сообщает абсолютный размер окна — только долю (util) и reset. Абсолют
// вычисляем из собственного расхода: ΔUSD / Δutil = ёмкость окна. Копим EMA по подписке.
const WIN5_SECS: i64 = 5 * 3600;
const WIN7_SECS: i64 = 7 * 86400;
/// Прайор ёмкости до первой калибровки — оценка под **Claude Max 20x** (мы используем только их).
/// Это лишь СТАРТОВАЯ оценка: реальная ёмкость каждой подписки измеряется калибровкой из трафика
/// (ΔUSD/Δutil) и заменяет прайор за первые часы. Тюнится env CLAUDE_API_CAP5H_USD/CAP7D_USD.
/// 7d≈$1500, 5h≈$50: за неделю ~33 пятичасовых окна × $50 = $1650 ёмкости 5h против $1500
/// недельного потолка → недельное окно связывает (реалистично для 20x).
const PRIOR_CAP5H_USD: f64 = 50.0;
const PRIOR_CAP7D_USD: f64 = 1500.0;
const CALIB_ALPHA: f64 = 0.3;        // вес нового наблюдения в EMA
// Калибруем только на Δutil ≥ 3%: util в заголовках Anthropic ЛАГАЕТ (сразу после расхода
// занижен) и квантован ~1% — на мелком Δutil cap раздувается. Больший порог = чище от лага.
const CALIB_MIN_DELTA: f64 = 0.03;

/// Один шаг калибровки окна. Копим интервал (util_anchor→new_util) против расхода
/// (spent_anchor→spent_total); когда Δutil перерос порог — ёмкость = ΔUSD/Δutil (EMA),
/// сдвигаем якорь. Сброс окна (util упал ниже якоря) → пере-заякориваемся без калибровки.
const CALIB_MAX_JUMP: f64 = 4.0;     // одно наблюдение не двигает cap больше чем в 4× (стабильность)

fn calib_window(anchor: &mut f64, spent_anchor: &mut f64, cap: &mut f64,
                new_util: f64, spent_total: f64, calib_n: &mut u32) {
    if new_util + 1e-9 < *anchor {
        // сброс окна: util упал ниже якоря → пере-заякориваемся без калибровки
        *anchor = new_util;
        *spent_anchor = spent_total;
        return;
    }
    let d = new_util - *anchor;
    if d < CALIB_MIN_DELTA {
        return; // Δutil ещё мал — копим (якорь НЕ двигаем, ΔUSD не теряется)
    }
    let du = spent_total - *spent_anchor;
    if du > 0.0 {
        // ёмкость = ΔUSD/Δutil; клампим, чтобы шумный квантованный шаг не швырнул cap
        let mut obs = du / d;
        if *cap > 0.0 { obs = obs.clamp(*cap / CALIB_MAX_JUMP, *cap * CALIB_MAX_JUMP); }
        *cap = if *cap > 0.0 { *cap * (1.0 - CALIB_ALPHA) + obs * CALIB_ALPHA } else { obs };
        *calib_n += 1;
    }
    // util перерос порог — пере-заякориваемся В ЛЮБОМ случае (даже если ΔUSD=0: это скачок
    // базовой линии — чужой трафик/рестарт, а не наш расход; иначе якорь застревал бы на нуле).
    *anchor = new_util;
    *spent_anchor = spent_total;
}

/// Снимок доступности по подписке (USD real-API-эквивалента) на разные горизонты.
#[derive(Clone, Debug)]
pub struct Cap {
    pub email: String,
    pub calibrated: bool,   // была ли хоть одна реальная калибровка (иначе цифры — прайор)
    pub util5h: f64,        // «живая» утилизация (заголовок + наш расход − rollover)
    pub util7d: f64,
    pub reset5h_in: i64,    // секунд до сброса 5h окна (0 если неизвестно/прошло)
    pub reset7d_in: i64,
    pub cap5h_usd: f64,     // калиброванная (или прайорная) ёмкость окна
    pub cap7d_usd: f64,
    pub rem5h_usd: f64,     // остаток в текущем окне
    pub rem7d_usd: f64,
    pub avail_1h_usd: f64,  // доступно на горизонте (учёт сбросов внутри горизонта)
    pub avail_5h_usd: f64,
    pub avail_1d_usd: f64,
    pub avail_7d_usd: f64,
    pub status: String,
    pub cooling: bool,
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
    /// Калиброванная ёмкость окна (USD real-API за полное окно). 0 = ещё не калибровано.
    pub cap5h_usd: f64,
    pub cap7d_usd: f64,
    /// Монотонный суммарный расход подписки (USD real-API) — не сбрасывается.
    pub spent_total_usd: f64,
    /// Значение `spent_total` на момент последнего заголовка — для «живой» утилизации
    /// (util_header + расход_с_тех_пор / cap).
    pub spent_at_header_usd: f64,
    /// Якоря калибровки на окно (двигаются ТОЛЬКО при калибровке/rollover; между ними копим,
    /// пока Δutil не перерастёт порог — иначе мелкие шаги util теряли бы накопленный ΔUSD).
    pub util5_anchor: f64,
    pub spent5_anchor_usd: f64,
    pub util7_anchor: f64,
    pub spent7_anchor_usd: f64,
    /// Сколько раз реально калибровали (>0 → cap измерен, а не прайор).
    pub calib_n: u32,
}

struct Inner {
    subs: Vec<Sub>,
    live: HashMap<String, Live>,
}

pub struct Pool {
    inner: Mutex<Inner>,
    util_cap: f64,
    prior5h_usd: f64, // прайор ёмкости 5h окна до калибровки (env, деф под Max 20x)
    prior7d_usd: f64, // прайор ёмкости 7d окна
}

impl Pool {
    /// `prior5h/prior7d` — стартовые оценки ёмкости окон (USD real-API). До первой калибровки
    /// цифры считаются по ним; дальше — по измеренной ёмкости. 0 → дефолтные Max-20x-прайоры.
    pub fn new(subs: Vec<Sub>, util_cap: f64, prior5h: f64, prior7d: f64) -> Self {
        Pool {
            inner: Mutex::new(Inner { subs, live: HashMap::new() }),
            util_cap,
            prior5h_usd: if prior5h > 0.0 { prior5h } else { PRIOR_CAP5H_USD },
            prior7d_usd: if prior7d > 0.0 { prior7d } else { PRIOR_CAP7D_USD },
        }
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

    /// Учесть реальный расход запроса (USD real-API, ×1.0 до наценки) — монотонно.
    /// Питает калибровку ёмкости окон и «живую» утилизацию. Вызывается из tee-метеринга forward.
    pub fn record_spend(&self, email: &str, real_nano: i128) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();
        l.spent_total_usd += (real_nano as f64) / 1e9;
    }

    pub fn set_util(&self, email: &str, u5: Option<f64>, u7: Option<f64>,
                    status: Option<String>, r5: Option<i64>, r7: Option<i64>) {
        let mut g = self.inner.lock().unwrap();
        let l = g.live.entry(email.to_string()).or_default();

        // КАЛИБРОВКА ёмкости окон: cap = ΔUSD/Δutil. Якоря на окно двигаем только когда Δutil
        // перерос порог (иначе мелкие шаги util теряли бы накопленный ΔUSD). При сбросе окна —
        // пере-заякориваемся без калибровки.
        let spent_total = l.spent_total_usd;
        if let Some(n5) = u5 {
            calib_window(&mut l.util5_anchor, &mut l.spent5_anchor_usd, &mut l.cap5h_usd,
                         n5, spent_total, &mut l.calib_n);
        }
        if let Some(n7) = u7 {
            let mut ignore = 0; // 7d не увеличивает calib_n (флаг «калибровано» ведём по 5h)
            calib_window(&mut l.util7_anchor, &mut l.spent7_anchor_usd, &mut l.cap7d_usd,
                         n7, spent_total, &mut ignore);
        }
        // якорь «живой» утилизации = состояние на момент этого заголовка
        l.spent_at_header_usd = spent_total;

        if let Some(v) = u5 { l.util5h = v; }
        if let Some(v) = u7 { l.util7d = v; }
        if let Some(v) = status { l.status = v; }
        if let Some(v) = r5 { l.reset5h = v; }
        if let Some(v) = r7 { l.reset7d = v; }
        l.polled_ts = now();
    }

    /// Доступность по каждой подписке (USD real-API-эквивалента) на горизонты 1ч/5ч/1д/7д.
    /// Чистая математика над состоянием — пересчитывается на каждый вызов, без сети.
    pub fn capacity(&self) -> Vec<Cap> {
        let g = self.inner.lock().unwrap();
        let now = now();
        let (p5, p7) = (self.prior5h_usd, self.prior7d_usd);
        g.subs.iter().map(|s| {
            let l = g.live.get(&s.email).cloned().unwrap_or_default();
            let cap5 = if l.cap5h_usd > 0.0 { l.cap5h_usd } else { p5 };
            let cap7 = if l.cap7d_usd > 0.0 { l.cap7d_usd } else { p7 };

            // эффективный reset: если ещё не наблюдали заголовок — считаем, что окно сбросится
            // через полный период от now (нейтральная оценка, чтобы горизонты не схлопывались).
            let er5 = if l.reset5h > 0 { l.reset5h } else { now + WIN5_SECS };
            let er7 = if l.reset7d > 0 { l.reset7d } else { now + WIN7_SECS };

            // «живая» утилизация: заголовок + наш расход с момента заголовка / ёмкость; rollover.
            let since_header = l.spent_total_usd - l.spent_at_header_usd;
            let live = |util: f64, reset: i64, cap: f64| -> f64 {
                if now >= reset { 0.0 }
                else { (util + since_header / cap).clamp(0.0, 1.0) }
            };
            let u5 = live(l.util5h, er5, cap5);
            let u7 = live(l.util7d, er7, cap7);
            let rem5 = cap5 * (1.0 - u5);
            let rem7 = cap7 * (1.0 - u7);

            // сколько сбросов окна попадает в (now, now+H] — окно наливается заново (квота, не rate).
            // reset мог УЖЕ пройти (idle-подписка) — нормализуем к следующему будущему сбросу,
            // иначе считали бы прошлые сбросы как будущие и завышали доступность.
            let resets = |reset: i64, win: i64, h: i64| -> i64 {
                let next = if reset >= now { reset } else { reset + ((now - reset) / win + 1) * win };
                if next > now + h { 0 } else { 1 + (now + h - next) / win }
            };
            let avail = |h: i64| -> f64 {
                let a5 = rem5 + resets(er5, WIN5_SECS, h) as f64 * cap5;
                let a7 = rem7 + resets(er7, WIN7_SECS, h) as f64 * cap7;
                a5.min(a7)
            };

            Cap {
                email: s.email.clone(),
                calibrated: l.calib_n > 0,
                util5h: u5, util7d: u7,
                reset5h_in: (er5 - now).max(0),
                reset7d_in: (er7 - now).max(0),
                cap5h_usd: cap5, cap7d_usd: cap7,
                rem5h_usd: rem5, rem7d_usd: rem7,
                avail_1h_usd: avail(3600),
                avail_5h_usd: avail(18000),
                avail_1d_usd: avail(86400),
                avail_7d_usd: avail(604800),
                status: l.status.clone(),
                cooling: l.cooling_until > now,
            }
        }).collect()
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
        Pool::new(emails.iter().map(|e| sub(e)).collect(), 0.95, 50.0, 1500.0)
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

    /// Калибровка: из ΔUSD/Δutil вычисляем ёмкость окна.
    #[test]
    fn calibrates_window_capacity_from_spend() {
        let p = pool(&["a"]);
        let future5 = now() + WIN5_SECS;
        let future7 = now() + WIN7_SECS;
        // якорь: util 0
        p.set_util("a", Some(0.0), Some(0.0), None, Some(future5), Some(future7));
        // потратили $5 real-API
        p.record_spend("a", 5 * 1_000_000_000);
        // новый заголовок: 5h вырос на 0.10 (→ cap5 = 5/0.10 = $50), 7d на 0.004 (< min_delta → 7d не калибруем)
        p.set_util("a", Some(0.10), Some(0.004), None, Some(future5), Some(future7));
        let c = &p.capacity()[0];
        assert!(c.calibrated);
        assert!((c.cap5h_usd - 50.0).abs() < 1e-6, "cap5={}", c.cap5h_usd);
        // остаток 5h = 50 * (1 - 0.10) = $45
        assert!((c.rem5h_usd - 45.0).abs() < 1e-6, "rem5={}", c.rem5h_usd);
    }

    /// Живая утилизация: расход между заголовками сразу уменьшает остаток.
    #[test]
    fn live_spend_reduces_headroom_between_headers() {
        let p = pool(&["a"]);
        let f5 = now() + WIN5_SECS;
        let f7 = now() + WIN7_SECS;
        p.set_util("a", Some(0.0), Some(0.0), None, Some(f5), Some(f7));
        // калибруем cap5=$50
        p.record_spend("a", 5 * 1_000_000_000);
        p.set_util("a", Some(0.10), Some(0.10), None, Some(f5), Some(f7));
        let before = p.capacity()[0].rem5h_usd;
        // тратим ещё $10 БЕЗ нового заголовка → остаток должен упасть на ~$10
        p.record_spend("a", 10 * 1_000_000_000);
        let after = p.capacity()[0].rem5h_usd;
        assert!((before - after - 10.0).abs() < 0.5, "before={before} after={after}");
    }

    /// До калибровки — прайор (Max 20x), помечено calibrated=false.
    #[test]
    fn uses_prior_before_calibration() {
        let p = pool(&["a"]);
        let c = &p.capacity()[0];
        assert!(!c.calibrated);
        assert!((c.cap7d_usd - 1500.0).abs() < 1e-6);
        assert!((c.rem7d_usd - 1500.0).abs() < 1e-6); // util 0 → весь бюджет доступен
    }
}
