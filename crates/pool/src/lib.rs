//! # pool — пул подписок + ротация (пункт 2)
//!
//! Держит текущий список подписок (обновляется из БД фоном сервером) и волатильное
//! состояние по каждой (утилизация окон 5h/7d, cooling, last_used). На каждый ход отдаёт
//! наименее загруженную живую подписку; при 429 — cooling и следующая.
//!
//! **Границы крейта:** чистая in-memory логика выбора и состояния. НИКАКОЙ сети/HTTP/БД.
//! Зависит только от `registry` (тип [`Sub`]). Опрос лимитов и форвардинг — крейтом выше.

use registry::Sub;
use std::cmp::Ordering::Equal;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// **Запас окна на подписку** (headroom): не роутим сверх `1 − base`, оставляя буфер, чтобы (а) не
/// доводить окно до 100% (меньше 429), (б) не выглядеть как автомат, максящий квоту под ноль. Порог
/// джиттерится детерминированно по email — весь флот НЕ режется на одном и том же проценте (это само
/// по себе было бы отличительным знаком). Стабилен во времени для подписки (как и per-persona UA).
#[derive(Clone, Copy, Debug)]
pub struct Reserve {
    pub base5h: f64, // доля 5h-окна в запасе (деф 0.10)
    pub base7d: f64, // доля 7d-окна в запасе (деф 0.03)
    pub jitter: f64, // ± разброс порога между подписками (деф 0.02)
}

impl Default for Reserve {
    fn default() -> Self { Reserve { base5h: 0.10, base7d: 0.03, jitter: 0.02 } }
}

impl Reserve {
    /// Без запаса (для ослабления фильтра «не зависать»: под пиком дотягиваем до 100%, чем 429 клиенту).
    pub const FULL: Reserve = Reserve { base5h: 0.0, base7d: 0.0, jitter: 0.0 };
    pub fn new(base5h: f64, base7d: f64, jitter: f64) -> Self { Reserve { base5h, base7d, jitter } }

    /// Эффективные потолки (cap5h, cap7d) для подписки: `1 − (base ± jitter)`. Джиттер детерминирован
    /// по (email, окно) → стабилен во времени, но у каждой подписки свой порог и по 5h, и по 7d.
    fn caps(&self, email: &str) -> (f64, f64) {
        let jit = |salt: u8| -> f64 {
            if self.jitter <= 0.0 { return 0.0; }
            let mut h = std::collections::hash_map::DefaultHasher::new();
            email.hash(&mut h);
            salt.hash(&mut h);
            let r = (h.finish() % 2001) as f64 / 1000.0 - 1.0; // равномерно в [-1, 1]
            r * self.jitter
        };
        let c5 = (1.0 - (self.base5h + jit(5))).clamp(0.5, 1.0);
        let c7 = (1.0 - (self.base7d + jit(7))).clamp(0.5, 1.0);
        (c5, c7)
    }
}

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

// ── Cache-first планировщик: сессии → персоны ────────────────────────────────
// Единица планирования — СЕССИЯ (диалог), а не запрос. Сессия привязывается к «домашней»
// подписке-персоне на всё время жизни prompt-кэша: пока запросы идут — сидит на одном аккаунте
// (кэш префикса тёплый → повторный ввод ~10% цены; трафик выглядит как один связный юзер).
/// Сессия липнет к дому, пока запросы приходят не реже этого интервала (покрывает 1h prompt-cache).
/// Простой дольше → кэш всё равно остыл, привязку можно свободно пересобрать (rebind бесплатен).
const AFFINITY_TTL: i64 = 3600;
/// Дом остывает дольше этого → сессию ПЕРЕ-привязываем (глубокий бан/лимит: держаться нет смысла).
/// Короче (бёрст-cooling) или просто занят → спилл ОДНОГО запроса, дом за сессией СОХРАНЯЕМ.
const REBIND_AFTER: i64 = 60;
/// Потолок параллельных запросов на персону — «человеческий конверт» (живой юзер не крутит 20
/// стримов разом) + защита аккаунта от загона в лимит. Гейтит placement и переводит пин в спилл.
const MAX_INFLIGHT: i64 = 6;
/// Потолок таблицы привязок (память). Переполнение → вытесняем самые старые по last_seen.
const BINDINGS_CAP: usize = 100_000;

/// Привязка сессии к дому-персоне: где живёт и когда последний раз обслуживалась.
struct Binding {
    email: String,
    last_seen: i64,
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

/// Множитель прайора ёмкости по тарифу (относительно Max 20x = 1.0). Прайоры заданы под Max 20x;
/// Pro/Max5 имеют кратно меньшие окна (5x/20x — кратность над Pro → Pro≈1, Max5≈5, Max20≈20). До
/// первой калибровки это убирает ПЕРЕоценку Pro/Max5-подписок (иначе их переливают → 429-штормы).
fn plan_scale(plan: &str) -> f64 {
    match plan {
        "max20" => 1.0,
        "max5" => 0.25, // 5x/20x
        "pro" => 0.05,  // 1x/20x
        _ => 1.0,       // неизвестно → консервативно как Max20 (калибровка уточнит)
    }
}
const CALIB_ALPHA: f64 = 0.3;        // вес нового наблюдения в EMA
// Калибруем только на Δutil ≥ 3%: util в заголовках Anthropic ЛАГАЕТ (сразу после расхода
// занижен) и квантован ~1% — на мелком Δutil cap раздувается. Больший порог = чище от лага.
const CALIB_MIN_DELTA: f64 = 0.03;

/// Один шаг калибровки окна. Копим интервал (util_anchor→new_util) против расхода
/// (spent_anchor→spent_total); когда Δutil перерос порог — ёмкость = ΔUSD/Δutil (EMA),
/// сдвигаем якорь. Сброс окна (util упал ниже якоря) → пере-заякориваемся без калибровки.
const CALIB_MAX_JUMP: f64 = 4.0;     // одно наблюдение не двигает cap больше чем в 4× (стабильность)
/// Минимальная доля Δutil, которую должен объяснять НАШ расход, чтобы калибровать (иначе util
/// раздут чужим использованием аккаунта → не портим cap, только ре-заякориваемся).
const CALIB_MIN_SHARE: f64 = 0.5;

fn calib_window(seeded: &mut bool, anchor: &mut f64, spent_anchor: &mut f64, cap: &mut f64,
                new_util: f64, spent_total: f64, calib_n: &mut u32) {
    if !*seeded {
        // первое наблюдение: засеваем якорь реальным util (а не 0), чтобы первый интервал
        // калибровки мерил от фактического старта окна, а не от нуля.
        *seeded = true;
        *anchor = new_util;
        *spent_anchor = spent_total;
        return;
    }
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
        // Защита от ЧУЖОГО использования аккаунта (подписка — живой Max владельца): util мог вырасти
        // из-за его расхода, а не нашего. Калибруем ТОЛЬКО если наш ΔUSD объясняет заметную долю
        // Δutil при текущем cap (иначе obs=du/d занижает cap, аккаунт недоиспользуется). До первой
        // калибровки (cap≤0, бутстрап) — пускаем всегда.
        let ours_explains = *cap <= 0.0 || du >= d * *cap * CALIB_MIN_SHARE;
        if ours_explains {
            // ёмкость = ΔUSD/Δutil; клампим, чтобы шумный квантованный шаг не швырнул cap
            let mut obs = du / d;
            if *cap > 0.0 { obs = obs.clamp(*cap / CALIB_MAX_JUMP, *cap * CALIB_MAX_JUMP); }
            *cap = if *cap > 0.0 { *cap * (1.0 - CALIB_ALPHA) + obs * CALIB_ALPHA } else { obs };
            *calib_n += 1;
        }
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
    /// Засеян ли якорь окна первым наблюдением. Без этого при СТАРТЕ посреди окна (util≠0, а
    /// расход с 0) первый интервал калибровки грязный (мерил бы от 0 вместо реального util).
    pub seed5: bool,
    pub seed7: bool,
    /// Сколько раз реально калибровали (>0 → cap измерен, а не прайор).
    pub calib_n: u32,
}

struct Inner {
    subs: Vec<Sub>,
    live: HashMap<String, Live>,
    /// Таблица привязок сессия→персона (cache-first). Растёт по активным сессиям, ограничена
    /// `BINDINGS_CAP`; протухшие (idle > AFFINITY_TTL) вытесняются лениво при вставке.
    bindings: HashMap<u64, Binding>,
    // Счётчики исходов route (наблюдаемость): pin = cache-hit (сессия на тёплом доме),
    // spill = дом занят, place = новая/пере-привязка. pin/(pin+place) ≈ доля cache-hit.
    route_pin: u64,
    route_spill: u64,
    route_place: u64,
}

/// Счётчики исходов роутинга сессий (для `/metrics`).
#[derive(Clone, Default, Debug)]
pub struct RouteStats {
    pub pin: u64,
    pub spill: u64,
    pub place: u64,
}

fn is_cooling(g: &Inner, e: &str, now: i64) -> bool {
    g.live.get(e).map(|l| l.cooling_until > now).unwrap_or(false)
}
/// Эффективная утилизация окна с учётом сброса: reset уже прошёл → окно обнулилось (util ~0),
/// даже если поллер/трафик ещё не обновили число.
fn eff_util(g: &Inner, e: &str, w: u8, now: i64) -> f64 {
    g.live.get(e).map(|l| {
        let (util, reset) = if w == 7 { (l.util7d, l.reset7d) } else { (l.util5h, l.reset5h) };
        if reset != 0 && now >= reset { 0.0 } else { util }
    }).unwrap_or(0.0)
}
fn inflight_of(g: &Inner, e: &str) -> i64 {
    g.live.get(e).map(|l| l.inflight).unwrap_or(0)
}

/// Ротационный выбор (для ретраев/спилла): наименее загруженная живая подписка не из `exclude`.
/// Стратегия: беречь недельный бюджет (7d) → размазывать 5h → «предупреждённые» ниже → меньше
/// in-flight → LRU. Фильтры ослабляются постепенно (пул НИКОГДА не зависает).
fn select_best(g: &Inner, exclude: &HashSet<String>, now: i64, rsv: &Reserve) -> Option<Sub> {
    let cands: Vec<&Sub> = g.subs.iter().filter(|s| !exclude.contains(&s.email)).collect();
    if cands.is_empty() { return None; }
    let not_cool: Vec<&Sub> = cands.iter().copied().filter(|s| !is_cooling(g, &s.email, now)).collect();
    let stage1: Vec<&Sub> = if not_cool.is_empty() { cands } else { not_cool };
    let ready: Vec<&Sub> = stage1.iter().copied()
        .filter(|s| { let (c5, c7) = rsv.caps(&s.email);
            eff_util(g, &s.email, 7, now) < c7 && eff_util(g, &s.email, 5, now) < c5 })
        .collect();
    let mut poolv: Vec<&Sub> = if ready.is_empty() { stage1 } else { ready };
    let warn = |e: &str| g.live.get(e).map(|l| l.status.contains("warning")).unwrap_or(false);
    let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);
    poolv.sort_by(|a, b| {
        eff_util(g, &a.email, 7, now).partial_cmp(&eff_util(g, &b.email, 7, now)).unwrap_or(Equal)
            .then(eff_util(g, &a.email, 5, now).partial_cmp(&eff_util(g, &b.email, 5, now)).unwrap_or(Equal))
            .then(warn(&a.email).cmp(&warn(&b.email)))
            .then(inflight_of(g, &a.email).cmp(&inflight_of(g, &b.email)))
            .then(lru(&a.email).cmp(&lru(&b.email)))
    });
    poolv.first().map(|s| (*s).clone())
}

/// Capacity-weighted placement НОВОЙ сессии: среди здоровых персон под потолком util И под
/// конвертом конкуррентности (`inflight < MAX_INFLIGHT`) — та, где больше свободной ёмкости (USD).
/// Так новые сессии наливаются в самые пустые аккаунты, но не сверх человеческого конверта; когда
/// эмптейший упёрся в конверт — перелив на следующий по ёмкости. Никого под конвертом → не зависаем
/// (обычный `select_best`, эффект — краткая деградация естественности под пиком).
fn place_best(g: &Inner, now: i64, rsv: &Reserve, p5: f64, p7: f64) -> Option<Sub> {
    let free = |s: &Sub| -> f64 {
        let l = g.live.get(&s.email);
        let sc = plan_scale(&s.plan); // прайор по тарифу (Pro/Max5 меньше Max20) до калибровки
        let cap5 = l.map(|l| l.cap5h_usd).filter(|c| *c > 0.0).unwrap_or(p5 * sc);
        let cap7 = l.map(|l| l.cap7d_usd).filter(|c| *c > 0.0).unwrap_or(p7 * sc);
        (cap5 * (1.0 - eff_util(g, &s.email, 5, now))).min(cap7 * (1.0 - eff_util(g, &s.email, 7, now)))
    };
    let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);
    let eligible = g.subs.iter().filter(|s| {
        let (c5, c7) = rsv.caps(&s.email);
        !is_cooling(g, &s.email, now)
            && eff_util(g, &s.email, 7, now) < c7 && eff_util(g, &s.email, 5, now) < c5
            && inflight_of(g, &s.email) < MAX_INFLIGHT
    });
    let best = eligible.max_by(|a, b|
        free(a).partial_cmp(&free(b)).unwrap_or(Equal)                    // больше свободной ёмкости
            .then(inflight_of(g, &b.email).cmp(&inflight_of(g, &a.email)))// при равенстве — меньше in-flight (веер)
            .then(lru(&b.email).cmp(&lru(&a.email))));                    // затем давнее использование
    match best {
        Some(s) => Some(s.clone()),
        None => select_best(g, &HashSet::new(), now, &Reserve::FULL), // все за конвертом → не зависаем
    }
}

pub struct Pool {
    inner: Mutex<Inner>,
    reserve: Reserve, // per-подписочный запас окон (headroom + антифингерпринт-джиттер)
    prior5h_usd: f64, // прайор ёмкости 5h окна до калибровки (env, деф под Max 20x)
    prior7d_usd: f64, // прайор ёмкости 7d окна
    /// Хук «durable-состояние изменилось» (cooling). Server ставит его, чтобы тут же персистить
    /// (write-through: бан переживает рестарт немедленно). Pool остаётся чистым — зовёт opaque Fn,
    /// без tokio/БД. Не зовём на калибровке (слишком часто) — она едет на safety-flush + cooling-флашах.
    on_change: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl Pool {
    /// `prior5h/prior7d` — стартовые оценки ёмкости окон (USD real-API). `reserve` — per-подписочный
    /// запас окон с джиттером (см. [`Reserve`]). До первой калибровки цифры по прайорам, дальше — по
    /// измеренной ёмкости.
    pub fn new(subs: Vec<Sub>, reserve: Reserve, prior5h: f64, prior7d: f64) -> Self {
        Pool {
            inner: Mutex::new(Inner {
                subs, live: HashMap::new(), bindings: HashMap::new(),
                route_pin: 0, route_spill: 0, route_place: 0,
            }),
            reserve,
            prior5h_usd: if prior5h > 0.0 { prior5h } else { PRIOR_CAP5H_USD },
            prior7d_usd: if prior7d > 0.0 { prior7d } else { PRIOR_CAP7D_USD },
            on_change: Mutex::new(None),
        }
    }

    /// Поставить хук durable-изменений (вызывается на cooling-переходах). Server → poke персиста.
    pub fn set_on_change(&self, f: Arc<dyn Fn() + Send + Sync>) {
        *self.on_change.lock().unwrap_or_else(|e| e.into_inner()) = Some(f);
    }
    fn signal_change(&self) {
        if let Some(f) = self.on_change.lock().unwrap_or_else(|e| e.into_inner()).as_ref() { f(); }
    }

    /// Заменить список подписок (из БД), сохранив волатильное состояние существующих.
    pub fn replace_subs(&self, subs: Vec<Sub>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let keep: HashSet<String> = subs.iter().map(|s| s.email.clone()).collect();
        g.live.retain(|k, _| keep.contains(k));
        g.subs = subs;
    }

    pub fn len(&self) -> usize { self.inner.lock().unwrap_or_else(|e| e.into_inner()).subs.len() }
    pub fn is_empty(&self) -> bool { self.inner.lock().unwrap_or_else(|e| e.into_inner()).subs.is_empty() }

    /// Наименее загруженная живая подписка не из `exclude` (для ретраев ротации/спилла).
    /// allow_full=true → до 100% (приоритетные ходы), иначе per-подписочный запас (`self.reserve`).
    pub fn pick(&self, exclude: &HashSet<String>, allow_full: bool) -> Option<Sub> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let rsv = if allow_full { Reserve::FULL } else { self.reserve };
        select_best(&g, exclude, now(), &rsv)
    }

    /// **Cache-first роутинг сессии** (первая попытка запроса). Держит диалог на «домашней» персоне,
    /// пока жив prompt-кэш, и раскладывает новые сессии по флоту с учётом ёмкости и конверта:
    ///
    /// 1. Есть свежая привязка к здоровому дому → **пин** (кэш тёплый, паттерн одного юзера).
    /// 2. Дом «ещё наш» (кэш не остыл, не в глубоком бане, под потолком), но временно занят
    ///    (бёрст-cooling < REBIND_AFTER или in-flight ≥ конверта) → **спилл одного запроса** на пул,
    ///    привязку СОХРАНЯЕМ — следующий запрос вернётся на тёплый дом.
    /// 3. Дом глубоко недоступен (cooling ≥ REBIND_AFTER / за потолком / выбыл) или привязки нет /
    ///    протухла → **(пере)привязка**: capacity-weighted placement новой персоны + запись.
    ///
    /// Ретраи ПОСЛЕ 429/5xx идут не сюда, а в [`pick`] (дом уже исключён через `tried`).
    pub fn route(&self, session: u64) -> Option<Sub> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now();
        let rsv = self.reserve;
        let (p5, p7) = (self.prior5h_usd, self.prior7d_usd);

        // берём привязку как owned-значения → отпускаем borrow таблицы, дальше свободно мутируем g
        if let Some((home, last_seen)) = g.bindings.get(&session).map(|b| (b.email.clone(), b.last_seen)) {
            let fresh = now - last_seen < AFFINITY_TTL;
            let exists = g.subs.iter().any(|s| s.email == home);
            if fresh && exists {
                let cooling = g.live.get(&home).map(|l| l.cooling_until).unwrap_or(0);
                let (e5, e7) = (eff_util(&g, &home, 5, now), eff_util(&g, &home, 7, now));
                let deep_cooling = cooling > now && cooling - now >= REBIND_AFTER;
                let (c5, c7) = rsv.caps(&home);
                let over_cap = e5 >= c5 || e7 >= c7;
                if !deep_cooling && !over_cap {
                    // дом всё ещё «наш» — кэш тёплый; либо пин, либо кратковременный спилл
                    if let Some(b) = g.bindings.get_mut(&session) { b.last_seen = now; }
                    let busy = cooling > now || inflight_of(&g, &home) >= MAX_INFLIGHT;
                    if !busy {
                        g.route_pin += 1;
                        return g.subs.iter().find(|s| s.email == home).cloned(); // ПИН (cache-hit)
                    }
                    // временно занят → спилл ЭТОГО запроса, дом за сессией сохраняем
                    g.route_spill += 1;
                    let ex: HashSet<String> = std::iter::once(home.clone()).collect();
                    return select_best(&g, &ex, now, &rsv)
                        .or_else(|| g.subs.iter().find(|s| s.email == home).cloned());
                }
                // deep_cooling || over_cap → пере-привязка ниже
            }
        }

        // новая сессия или пере-привязка → capacity-weighted placement, записать дом
        let chosen = place_best(&g, now, &rsv, p5, p7)?;
        g.route_place += 1;
        if g.bindings.len() >= BINDINGS_CAP {
            let cutoff = now - AFFINITY_TTL;
            g.bindings.retain(|_, b| b.last_seen >= cutoff); // сперва протухшие
            if g.bindings.len() >= BINDINGS_CAP {
                // всё ещё полно (все свежие) — грубо срезаем половину самых старых
                let mut v: Vec<(u64, i64)> = g.bindings.iter().map(|(k, b)| (*k, b.last_seen)).collect();
                v.sort_by_key(|(_, t)| *t);
                for (k, _) in v.into_iter().take(BINDINGS_CAP / 2) { g.bindings.remove(&k); }
            }
        }
        g.bindings.insert(session, Binding { email: chosen.email.clone(), last_seen: now });
        Some(chosen)
    }

    /// Взяли подписку в работу (pick): отметить время и +1 в in-flight.
    pub fn mark_used(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        l.last_used = now();
        l.inflight += 1;
    }

    /// Подписка здорова: снять истёкший cooling, обновить last_used. In-flight НЕ трогаем — слотом
    /// владеет вызывающий (в forward — `InflightGuard`/`end_stream`), чтобы не было двойного учёта.
    /// Годится и для успеха (стрим ещё течёт), и для клиентской 4xx (подписка ни при чём).
    pub fn mark_healthy(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        if l.cooling_until <= now() { l.cooling_until = 0; }
        l.last_used = now();
    }

    /// Стрим ответа завершён или оборван клиентом → освобождаем слот конкуррентности персоны.
    /// Парен с `mark_used`+`mark_healthy` (успех); вызывается из tee-метеринга forward. Клампится в 0.
    pub fn end_stream(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        l.inflight = (l.inflight - 1).max(0);
        l.last_used = now();
    }

    /// Circuit-breaker из ФОРВАРДА: студим подписку на `secs`. In-flight НЕ трогаем — слот закрывает
    /// `InflightGuard` (или `end_stream`) в forward, чтобы декремент был ровно один и переживал даже
    /// отмену запроса. Сигналит durable-изменение (персист бана).
    pub fn mark_cooling(&self, email: &str, secs: i64) {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.live.entry(email.to_string()).or_default().cooling_until = now() + secs.max(1);
        }
        self.signal_change();
    }

    /// Студить БЕЗ трогания in-flight — для фонового поллера (он не делал `mark_used`).
    pub fn cool(&self, email: &str, secs: i64) {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            g.live.entry(email.to_string()).or_default().cooling_until = now() + secs.max(1);
        }
        self.signal_change();
    }

    /// Канонический релиз слота конкуррентности (−1, клампится в 0), БЕЗ cooling/last_used. Парен с
    /// `mark_used`. Зовётся из `InflightGuard::drop` в forward на ЛЮБОМ не-стриминговом исходе попытки
    /// (ошибка/ротация/4xx/отмена запроса) — так декремент ровно один и не теряется при cancel future.
    pub fn mark_done(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        l.inflight = (l.inflight - 1).max(0);
    }

    /// Через сколько секунд освободится ближайшая остывающая подписка (для `Retry-After` при
    /// исчерпании пула). None → остывающих нет.
    pub fn soonest_ready(&self) -> Option<i64> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now();
        g.live.values()
            .map(|l| l.cooling_until - now)
            .filter(|&d| d > 0)
            .min()
    }

    /// Учесть реальный расход запроса (USD real-API, ×1.0 до наценки) — монотонно.
    /// Питает калибровку ёмкости окон и «живую» утилизацию. Вызывается из tee-метеринга forward.
    pub fn record_spend(&self, email: &str, real_nano: i128) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        l.spent_total_usd += (real_nano as f64) / 1e9;
    }

    pub fn set_util(&self, email: &str, u5: Option<f64>, u7: Option<f64>,
                    status: Option<String>, r5: Option<i64>, r7: Option<i64>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();

        // КАЛИБРОВКА ёмкости окон: cap = ΔUSD/Δutil. Якоря на окно двигаем только когда Δutil
        // перерос порог (иначе мелкие шаги util теряли бы накопленный ΔUSD). При сбросе окна —
        // пере-заякориваемся без калибровки.
        let spent_total = l.spent_total_usd;
        if let Some(n5) = u5 {
            calib_window(&mut l.seed5, &mut l.util5_anchor, &mut l.spent5_anchor_usd,
                         &mut l.cap5h_usd, n5, spent_total, &mut l.calib_n);
        }
        if let Some(n7) = u7 {
            let mut ignore = 0; // 7d не увеличивает calib_n (флаг «калибровано» ведём по 5h)
            calib_window(&mut l.seed7, &mut l.util7_anchor, &mut l.spent7_anchor_usd,
                         &mut l.cap7d_usd, n7, spent_total, &mut ignore);
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
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now();
        let (p5, p7) = (self.prior5h_usd, self.prior7d_usd);
        g.subs.iter().map(|s| {
            let l = g.live.get(&s.email).cloned().unwrap_or_default();
            let sc = plan_scale(&s.plan); // прайор масштабируем по тарифу (Pro/Max5 меньше Max20)
            let cap5 = if l.cap5h_usd > 0.0 { l.cap5h_usd } else { p5 * sc };
            let cap7 = if l.cap7d_usd > 0.0 { l.cap7d_usd } else { p7 * sc };

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
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.subs.iter().map(|s| (s.clone(), g.live.get(&s.email).cloned().unwrap_or_default())).collect()
    }

    /// Счётчики исходов роутинга (наблюдаемость `/metrics`).
    pub fn route_stats(&self) -> RouteStats {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        RouteStats { pin: g.route_pin, spill: g.route_spill, place: g.route_place }
    }

    /// Суммарный in-flight по флоту (gauge) + число остывающих (для `/metrics`; утечку слота
    /// сразу видно по монотонному росту in-flight при простое).
    pub fn gauges(&self) -> (i64, usize) {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now();
        let inflight: i64 = g.live.values().map(|l| l.inflight).sum();
        let cooling = g.live.values().filter(|l| l.cooling_until > now).count();
        (inflight, cooling)
    }

    /// Экспорт durable-состояния для персиста (по текущим подпискам).
    pub fn export_state(&self) -> Vec<registry::PoolStateRow> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.subs.iter().filter_map(|s| {
            let l = g.live.get(&s.email)?;
            Some(registry::PoolStateRow {
                email: s.email.clone(),
                cooling_until: l.cooling_until,
                cap5h_usd: l.cap5h_usd,
                cap7d_usd: l.cap7d_usd,
                spent_total_usd: l.spent_total_usd,
                util5h: l.util5h,
                util7d: l.util7d,
                reset5h: l.reset5h,
                reset7d: l.reset7d,
                calib_n: l.calib_n as i64,
            })
        }).collect()
    }

    /// Восстановить состояние на старте (после build пула из реестра). Возвращаем только осмысленное:
    /// cooling (если ещё в будущем — бан на дни переживает рестарт), калибровку ёмкости, spent, util/
    /// reset; засеваем якоря калибровки восстановленной точкой, чтобы продолжить, а не мерить с нуля.
    pub fn import_state(&self, rows: Vec<registry::PoolStateRow>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let now = now();
        let known: HashSet<String> = g.subs.iter().map(|s| s.email.clone()).collect();
        for r in rows {
            if !known.contains(&r.email) { continue; } // подписки уже нет — не воскрешаем
            let l = g.live.entry(r.email.clone()).or_default();
            if r.cooling_until > now { l.cooling_until = r.cooling_until; }
            if r.cap5h_usd > 0.0 { l.cap5h_usd = r.cap5h_usd; }
            if r.cap7d_usd > 0.0 { l.cap7d_usd = r.cap7d_usd; }
            l.spent_total_usd = r.spent_total_usd;
            l.spent_at_header_usd = r.spent_total_usd;
            l.util5h = r.util5h;
            l.util7d = r.util7d;
            l.reset5h = r.reset5h;
            l.reset7d = r.reset7d;
            l.calib_n = r.calib_n.max(0) as u32;
            // засеять якоря калибровки восстановленной точкой (продолжаем EMA, не мерим с нуля)
            l.seed5 = true; l.util5_anchor = r.util5h; l.spent5_anchor_usd = r.spent_total_usd;
            l.seed7 = true; l.util7_anchor = r.util7d; l.spent7_anchor_usd = r.spent_total_usd;
            // разбросать «свежесть» по подпискам, чтобы поллер не пробил весь восстановленный флот
            // разом на старте (иначе все polled_ts==0 → залповый probe со всех IP).
            let mut h = std::collections::hash_map::DefaultHasher::new();
            r.email.hash(&mut h);
            l.polled_ts = now - (h.finish() % 240) as i64;
            l.last_used = now;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sub(email: &str) -> Sub {
        Sub { email: email.into(), token: "t".into(), proxy: String::new(), fleet: "prod".into(), plan: "max20".into() }
    }
    fn pool(emails: &[&str]) -> Pool {
        // запас 0.05/0.05 без джиттера → потолки окон 0.95 (детерминизм, как со старым util_cap).
        Pool::new(emails.iter().map(|e| sub(e)).collect(), Reserve::new(0.05, 0.05, 0.0), 50.0, 1500.0)
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

    /// Cache-first: сессия липнет к своему дому, пока тот здоров (кэш тёплый).
    #[test]
    fn route_pins_session_to_home() {
        let p = pool(&["a", "b", "c", "d"]);
        for e in ["a", "b", "c", "d"] { p.set_util(e, Some(0.1), Some(0.1), None, None, None); }
        let s = 123456789u64;
        let home = p.route(s).unwrap().email;
        for _ in 0..10 { assert_eq!(p.route(s).unwrap().email, home); }
    }

    /// Placement новой сессии — capacity-weighted: садим на персону с бóльшим свободным окном.
    #[test]
    fn route_places_new_session_by_capacity() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.80), Some(0.80), None, None, None); // мало свободного
        p.set_util("b", Some(0.10), Some(0.10), None, None, None); // много свободного
        assert_eq!(p.route(555).unwrap().email, "b");
    }

    /// Конверт конкуррентности: эмптейший дом упёрся в MAX_INFLIGHT → новая сессия уходит на другой.
    #[test]
    fn route_placement_respects_concurrency_envelope() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.05), Some(0.05), None, None, None); // эмптейший…
        p.set_util("b", Some(0.20), Some(0.20), None, None, None);
        for _ in 0..MAX_INFLIGHT { p.mark_used("a"); }             // …но забит до конверта
        assert_eq!(p.route(777).unwrap().email, "b");
    }

    /// Временно занятый дом (in-flight ≥ конверта) → спилл ОДНОГО запроса, привязка сохраняется;
    /// как только слоты освободились — сессия возвращается на тёплый дом.
    #[test]
    fn route_spills_but_keeps_home_when_busy() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.05), Some(0.05), None, None, None);
        p.set_util("b", Some(0.20), Some(0.20), None, None, None);
        let s = 999u64;
        let home = p.route(s).unwrap().email;         // сел на эмптейший (a)
        for _ in 0..MAX_INFLIGHT { p.mark_used(&home); } // забили дом до конверта
        let spilled = p.route(s).unwrap().email;
        assert_ne!(spilled, home, "занятый дом → спилл на другой");
        for _ in 0..MAX_INFLIGHT { p.end_stream(&home); } // слоты освободились
        assert_eq!(p.route(s).unwrap().email, home, "вернулись на тёплый дом");
    }

    /// Глубокий cooling дома (≥ REBIND_AFTER) → сессия ПЕРЕ-привязывается на здоровую персону.
    #[test]
    fn route_rebinds_on_deep_cooling() {
        let p = pool(&["a", "b"]);
        for e in ["a", "b"] { p.set_util(e, Some(0.1), Some(0.1), None, None, None); }
        let s = 314u64;
        let home = p.route(s).unwrap().email;
        p.mark_cooling(&home, 300);                    // глубокий бан
        let rebound = p.route(s).unwrap().email;
        assert_ne!(rebound, home, "глубокий cooling → пере-привязка");
        assert_eq!(p.route(s).unwrap().email, rebound, "новая привязка стабильна");
    }

    /// Разные сессии раскладываются по флоту (нагрузка распределена, а не в одну персону).
    #[test]
    fn route_spreads_sessions_across_fleet() {
        let p = pool(&["a", "b", "c", "d"]);
        for e in ["a", "b", "c", "d"] { p.set_util(e, Some(0.1), Some(0.1), None, None, None); }
        let mut seen = HashSet::new();
        for s in 0..200u64 {
            let e = p.route(s).unwrap().email;
            // симулируем короткую параллельную нагрузку, чтобы конверт раскидывал placement
            p.mark_used(&e);
            seen.insert(e);
        }
        assert!(seen.len() >= 3, "сессии должны разложиться по флоту, got {}", seen.len());
    }

    /// Персист: export→import переносит cooling и калибровку через «рестарт».
    #[test]
    fn export_import_preserves_cooling_and_calibration() {
        let p = pool(&["a"]);
        let f5 = now() + WIN5_SECS;
        let f7 = now() + WIN7_SECS;
        p.set_util("a", Some(0.0), Some(0.0), None, Some(f5), Some(f7)); // якорь
        p.record_spend("a", 5 * 1_000_000_000);
        p.set_util("a", Some(0.10), Some(0.004), None, Some(f5), Some(f7)); // cap5 → $50
        p.mark_cooling("a", 3600);
        let rows = p.export_state();
        // «рестарт» — новый пул восстанавливает состояние
        let p2 = pool(&["a"]);
        p2.import_state(rows);
        let c = &p2.capacity()[0];
        assert!(c.calibrated, "калибровка восстановлена");
        assert!((c.cap5h_usd - 50.0).abs() < 1e-6, "cap5={}", c.cap5h_usd);
        assert!(p2.soonest_ready().map(|s| s > 0).unwrap_or(false), "cooling пережил рестарт");
    }

    /// Тариф масштабирует прайор ёмкости: Pro-подписка до калибровки НЕ считается как Max20.
    #[test]
    fn plan_scales_prior_capacity() {
        let mk = |e: &str, plan: &str| Sub {
            email: e.into(), token: "t".into(), proxy: String::new(), fleet: "prod".into(), plan: plan.into(),
        };
        let p = Pool::new(vec![mk("pro@x", "pro"), mk("m20@x", "max20")],
                          Reserve::new(0.05, 0.05, 0.0), 50.0, 1500.0);
        let caps = p.capacity();
        let pro = caps.iter().find(|c| c.email == "pro@x").unwrap();
        let m20 = caps.iter().find(|c| c.email == "m20@x").unwrap();
        assert!((pro.cap7d_usd - 75.0).abs() < 1e-6, "pro cap7={} (=1500×0.05)", pro.cap7d_usd);
        assert!((m20.cap7d_usd - 1500.0).abs() < 1e-6, "m20 cap7={}", m20.cap7d_usd);
        // новая сессия садится на Max20 (там больше реальной свободной ёмкости)
        assert_eq!(p.route(1).unwrap().email, "m20@x");
    }

    /// on_change хук зовётся на cooling (write-through триггер).
    #[test]
    fn cooling_fires_on_change_hook() {
        use std::sync::atomic::{AtomicU32, Ordering};
        let p = pool(&["a"]);
        let hits = Arc::new(AtomicU32::new(0));
        let h = hits.clone();
        p.set_on_change(Arc::new(move || { h.fetch_add(1, Ordering::SeqCst); }));
        p.mark_cooling("a", 60);
        p.cool("a", 60);
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }

    /// Запас окна: подписку выше порога (1−base) не выбираем, пока есть под порогом.
    #[test]
    fn reserve_keeps_headroom() {
        // запас 5h=10% (порог 0.90), без джиттера
        let p = Pool::new(vec![sub("a"), sub("b")], Reserve::new(0.10, 0.03, 0.0), 50.0, 1500.0);
        p.set_util("a", Some(0.93), Some(0.10), None, None, None); // 5h за запасом (>0.90)
        p.set_util("b", Some(0.50), Some(0.10), None, None, None); // под запасом
        assert_eq!(picked(&p), "b"); // a бережём (headroom), берём b
    }

    /// Джиттер: порог отсечения различается между подписками (флот не режется на одном проценте).
    #[test]
    fn reserve_jitter_varies_thresholds() {
        let r = Reserve::new(0.10, 0.03, 0.02);
        let mut caps5 = std::collections::HashSet::new();
        for i in 0..50 { caps5.insert(format!("{:.4}", r.caps(&format!("u{i}@x.io")).0)); }
        assert!(caps5.len() >= 5, "пороги 5h должны различаться по флоту, got {}", caps5.len());
        // все в разумном коридоре 1−(0.10±0.02) = [0.88, 0.92]
        for i in 0..50 {
            let c5 = r.caps(&format!("u{i}@x.io")).0;
            assert!((0.87..=0.93).contains(&c5), "cap5={c5}");
        }
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
