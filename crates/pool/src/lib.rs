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
/// Настраивается через env `CLAUDE_API_MAX_INFLIGHT` (`server::config` → `set_max_inflight` при
/// старте, ДО создания пула). Дефолт 6. Высокое значение снимает потолок concurrency на подписку
/// (больше параллели, но выше риск бан-сигнала — держать осознанно).
static MAX_INFLIGHT_CELL: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(6);
#[inline]
pub fn max_inflight() -> i64 { MAX_INFLIGHT_CELL.load(std::sync::atomic::Ordering::Relaxed) }
pub fn set_max_inflight(v: i64) { MAX_INFLIGHT_CELL.store(v.max(1), std::sync::atomic::Ordering::Relaxed); }
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
        // Неизвестный тариф → КОНСЕРВАТИВНО (как Max5), НЕ как Max20. Прайор — лишь оценка до первой
        // калибровки; ошибка в БОЛЬШУЮ сторону опаснее: place_best переоценит свободную ёмкость и
        // нальёт туда сессии → 429-шторм на реально-меньшей подписке (ban-signal, кластер — ровно то,
        // чего избегаем). Недооценка же лишь слегка недогружает Max20 на пару часов (калибровка
        // выправит). Авторитетно — явный `sub set-plan` (detect-plan даёт noscope на sub-токенах).
        _ => 0.25,
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
    pub auth_dead: bool,    // токен отвергнут Anthropic (корроборированно) — «мёртвая» подписка (state==Dead)
    pub auth_state: String, // durable: "healthy" | "suspect" | "dead"
    pub dead_reason: String,// "authentication_error" (re-auth) | "permission_error" (banned) | ""
    pub dead_since_ts: i64, // когда стала dead (0 = не dead)
}

/// Durable-вердикт живости токена подписки. Один 401/403 — НЕ приговор (транзиент/битый запрос);
/// `Dead` ставится только по КОРРЕЛИРОВАННЫМ чистым probe (см. `record_probe`). Персистится в
/// engine PostgreSQL (`subs.auth_state`), переживает рестарт/blue-green — в отличие от старого
/// эфемерного `auth_dead`-бита, который поллер терял на каждом деплое.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AuthState {
    #[default]
    Healthy,
    /// Auth падает, но ещё не доказано терминально — под наблюдением, форсированно re-probe-ится.
    Suspect,
    /// Токен корроборированно мёртв/забанен. Исключён из ротации; авто-ревайв при смене токена или
    /// успешном медленном resurrection-probe.
    Dead,
}

impl AuthState {
    pub fn as_str(self) -> &'static str {
        match self { AuthState::Healthy => "healthy", AuthState::Suspect => "suspect", AuthState::Dead => "dead" }
    }
    pub fn from_str(s: &str) -> Self {
        match s { "dead" => AuthState::Dead, "suspect" => AuthState::Suspect, _ => AuthState::Healthy }
    }
}

/// Корроборация: `Dead` только после стольких чистых 401/403-probe подряд, растянутых на ≥ окно.
/// Чистый probe поллера НЕ несёт клиентского ввода → 401/403 на нём однозначен (не «битый запрос»),
/// поэтому порога 2 достаточно; окно ниже лишь отсекает мгновенный транзиентный блип Anthropic.
const DEAD_STREAK: i64 = 2;
/// …и только если первый отказ серии был не позже, чем `DEAD_MIN_SECS` назад (≥5 мин) — так
/// одиночный блип не доводит до терминала, а настоящий бан/revoke — за ~5 мин (2 probe в 5 минут).
const DEAD_MIN_SECS: i64 = 300;

/// Короткий отпечаток токена подписки — стабильная метка «к какому токену относится auth-вердикт».
/// Не секрет (одностороннний хэш), но сравнение fp детектит замену токена (авто-ревайв). Общий для
/// поллера (`record_probe`) и `replace_subs`, чтобы метки совпадали.
pub fn token_fp(token: &str) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(token, &mut h);
    format!("{:016x}", std::hash::Hasher::finish(&h))
}

/// Чистая машина auth-health одного probe (детерминирована по `now` → тестируема без системных часов).
/// Один 401/403 НЕ убивает: нужна серия `DEAD_STREAK` за ≥`DEAD_MIN_SECS`. 2xx (чистый probe) снимает
/// подозрение/смерть. Смена токена (fp) обнуляет вердикт о старом токене (авто-ревайв).
fn apply_probe(l: &mut Live, http: u16, token_fp: &str, now: i64) {
    l.polled_ts = now; // probe заякорил интервал liveness/suspect/dead
    if !token_fp.is_empty() && l.auth_token_fp != token_fp {
        l.auth_token_fp = token_fp.to_string();
        reset_auth_healthy(l);
    }
    match http {
        401 | 403 => {
            l.auth_fail_streak += 1;
            if l.first_auth_fail_ts == 0 { l.first_auth_fail_ts = now; }
            l.last_auth_fail_ts = now;
            l.last_auth_http = http as i64;
            let reason = if http == 403 { "permission_error" } else { "authentication_error" };
            if l.auth_state == AuthState::Healthy { l.auth_state = AuthState::Suspect; }
            if l.auth_fail_streak >= DEAD_STREAK && now - l.first_auth_fail_ts >= DEAD_MIN_SECS {
                l.auth_state = AuthState::Dead;
                if l.dead_since_ts == 0 { l.dead_since_ts = now; }
                l.dead_reason = reason.to_string();
            }
        }
        c if (200..300).contains(&c) => reset_auth_healthy(l), // чистый probe → токен жив
        _ => {} // 429/5xx/прочее — не auth-сигнал, состояние не трогаем
    }
}

/// Сбросить auth-health `Live` в healthy (чистый probe/2xx/смена токена/ручной revive).
fn reset_auth_healthy(l: &mut Live) {    l.auth_state = AuthState::Healthy;
    l.auth_fail_streak = 0;
    l.first_auth_fail_ts = 0;
    l.last_auth_fail_ts = 0;
    l.last_auth_http = 0;
    l.dead_since_ts = 0;
    l.dead_reason.clear();
}

/// Собрать durable-строку auth-health из `Live` (для персиста/сравнения «изменилось ли»).
fn health_row(email: &str, l: &Live) -> registry::SubHealth {
    registry::SubHealth {
        email: email.to_string(),
        auth_state: l.auth_state.as_str().to_string(),
        auth_fail_streak: l.auth_fail_streak,
        first_auth_fail_ts: l.first_auth_fail_ts,
        last_auth_fail_ts: l.last_auth_fail_ts,
        last_auth_http: l.last_auth_http,
        dead_since_ts: l.dead_since_ts,
        dead_reason: l.dead_reason.clone(),
        auth_token_fp: l.auth_token_fp.clone(),
    }
}



#[derive(Clone, Default, Debug)]
pub struct Live {
    pub util5h: f64,
    pub util7d: f64,
    pub status: String,
    pub cooling_until: i64,
    pub last_used: i64,
    pub polled_ts: i64,
    /// Момент последнего ФОРСИРОВАННОГО probe-запроса (`request_probe`) — для дебаунса. Отдельно от
    /// `polled_ts` (тот пассивный сбор держит свежим на каждом ответе), иначе флуд 401/403 гнал бы
    /// непрерывный probe-трафик. Эфемерный, не персистится.
    pub last_probe_req: i64,
    /// Durable auth-health (переживает рестарт через `subs.auth_state`). Заменяет старый эфемерный
    /// `auth_dead`-бит: теперь смерть коррелирована (N probe за ≥T) и авторитетна. Поллер ведёт эти
    /// поля через `record_probe`, server персистит вердикт в PostgreSQL.
    pub auth_state: AuthState,
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64,
    pub last_auth_fail_ts: i64,
    pub last_auth_http: i64,
    pub dead_since_ts: i64,
    pub dead_reason: String,
    /// Отпечаток токена, к которому относится вердикт. Смена (authbot заменил токен) → авто-ревайв.
    pub auth_token_fp: String,
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
    /// CAS version of the durable pool-state row. Not involved in routing decisions.
    pub persist_version: i64,
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
/// Подписка корроборированно мертва (токен забанен/отвергнут) → НЕ шлём на неё живой трафик.
/// Исключается из ротации/placement/пина полностью (гарантированный 401 клиенту хуже, чем
/// прозрачный Overloaded). Ревайв — авто при смене токена / успешном resurrection-probe, либо вручную.
fn is_dead(g: &Inner, e: &str) -> bool {
    g.live.get(e).map(|l| l.auth_state == AuthState::Dead).unwrap_or(false)
}
/// Единая «живая» утилизация окна для capacity И маршрутизации: последний заголовок + наш расход
/// после него / измеренная (либо тарифная прайорная) ёмкость. Прошедший reset обнуляет окно.
fn live_util(g: &Inner, e: &str, scale: f64, w: u8, now: i64, p5: f64, p7: f64) -> f64 {
    let Some(l) = g.live.get(e) else { return 0.0; };
    let (util, reset, measured_cap, prior_cap) = if w == 7 {
        (l.util7d, l.reset7d, l.cap7d_usd, p7)
    } else {
        (l.util5h, l.reset5h, l.cap5h_usd, p5)
    };
    if reset != 0 && now >= reset { return 0.0; }

    let cap = if measured_cap > 0.0 { measured_cap } else { prior_cap * scale };
    let since_header = (l.spent_total_usd - l.spent_at_header_usd).max(0.0);
    (util + since_header / cap).clamp(0.0, 1.0)
}
fn inflight_of(g: &Inner, e: &str) -> i64 {
    g.live.get(e).map(|l| l.inflight).unwrap_or(0)
}

/// Общий ротационный выбор. `allow_cooling_fallback=false` нужен для spill: внутреннее давление
/// конкуррентности не должно превращаться в длинный Retry-After от чужой cooling-персоны.
fn select_best_with_policy(g: &Inner, exclude: &HashSet<String>, now: i64, rsv: &Reserve,
                           p5: f64, p7: f64, allow_cooling_fallback: bool) -> Option<Sub> {
    let warn = |e: &str| g.live.get(e).map(|l| l.status.contains("warning")).unwrap_or(false);
    let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);
    let over_cap = |s: &Sub| -> bool {
        let (c5, c7) = rsv.caps(&s.email);
        !(live_util(g, &s.email, plan_scale(&s.plan), 7, now, p5, p7) < c7
            && live_util(g, &s.email, plan_scale(&s.plan), 5, now, p5, p7) < c5)
    };
    let over_envelope = |s: &Sub| inflight_of(g, &s.email) >= max_inflight();

    g.subs.iter()
        .filter(|s| !exclude.contains(&s.email))
        .filter(|s| !is_dead(g, &s.email)) // мёртвые (забанены) — вне ротации
        .filter(|s| allow_cooling_fallback || !is_cooling(g, &s.email, now))
        .min_by(|a, b| {
            let cooling_order = is_cooling(g, &a.email, now).cmp(&is_cooling(g, &b.email, now));
            if cooling_order != Equal { return cooling_order; }

            // Конверт — полноценный tier: пока есть кандидат ниже MAX_INFLIGHT, перегруженный не
            // выбирается. Если весь доступный tier уже переполнен, сначала берём минимальный inflight,
            // а лишь затем сравниваем небольшие различия util.
            let a_over = over_envelope(a);
            let b_over = over_envelope(b);
            let envelope_order = a_over.cmp(&b_over);
            if envelope_order != Equal { return envelope_order; }
            if a_over && b_over {
                return inflight_of(g, &a.email).cmp(&inflight_of(g, &b.email))
                    .then_with(|| over_cap(a).cmp(&over_cap(b)))
                    .then_with(|| live_util(g, &a.email, plan_scale(&a.plan), 7, now, p5, p7)
                        .partial_cmp(&live_util(g, &b.email, plan_scale(&b.plan), 7, now, p5, p7)).unwrap_or(Equal))
                    .then_with(|| live_util(g, &a.email, plan_scale(&a.plan), 5, now, p5, p7)
                        .partial_cmp(&live_util(g, &b.email, plan_scale(&b.plan), 5, now, p5, p7)).unwrap_or(Equal))
                    .then_with(|| warn(&a.email).cmp(&warn(&b.email)))
                    .then_with(|| lru(&a.email).cmp(&lru(&b.email)));
            }

            over_cap(a).cmp(&over_cap(b))
                .then_with(|| live_util(g, &a.email, plan_scale(&a.plan), 7, now, p5, p7)
                    .partial_cmp(&live_util(g, &b.email, plan_scale(&b.plan), 7, now, p5, p7)).unwrap_or(Equal))
                .then_with(|| live_util(g, &a.email, plan_scale(&a.plan), 5, now, p5, p7)
                    .partial_cmp(&live_util(g, &b.email, plan_scale(&b.plan), 5, now, p5, p7)).unwrap_or(Equal))
                .then_with(|| warn(&a.email).cmp(&warn(&b.email)))
                .then_with(|| inflight_of(g, &a.email).cmp(&inflight_of(g, &b.email)))
                .then_with(|| lru(&a.email).cmp(&lru(&b.email)))
        })
        .cloned()
}

/// Ротационный выбор (ретраи/обычный трафик): не зависает, cooling ослабляется только если иных нет.
fn select_best(g: &Inner, exclude: &HashSet<String>, now: i64, rsv: &Reserve,
               p5: f64, p7: f64) -> Option<Sub> {
    select_best_with_policy(g, exclude, now, rsv, p5, p7, true)
}

/// Строгий spill: cooling-кандидаты не возвращаем даже как fallback.
fn select_best_non_cooling(g: &Inner, exclude: &HashSet<String>, now: i64, rsv: &Reserve,
                           p5: f64, p7: f64) -> Option<Sub> {
    select_best_with_policy(g, exclude, now, rsv, p5, p7, false)
}

/// Capacity-weighted placement НОВОЙ сессии: среди здоровых персон под потолком util И под
/// конвертом конкуррентности (`inflight < max_inflight()`) — та, где больше свободной ёмкости (USD).
/// Так новые сессии наливаются в самые пустые аккаунты, но не сверх человеческого конверта; когда
/// эмптейший упёрся в конверт — перелив на следующий по ёмкости. Никого под конвертом → не зависаем
/// (обычный `select_best`, эффект — краткая деградация естественности под пиком).
fn place_best(g: &Inner, now: i64, rsv: &Reserve, p5: f64, p7: f64) -> Option<Sub> {
    let free = |s: &Sub| -> f64 {
        let l = g.live.get(&s.email);
        let sc = plan_scale(&s.plan); // прайор по тарифу (Pro/Max5 меньше Max20) до калибровки
        let cap5 = l.map(|l| l.cap5h_usd).filter(|c| *c > 0.0).unwrap_or(p5 * sc);
        let cap7 = l.map(|l| l.cap7d_usd).filter(|c| *c > 0.0).unwrap_or(p7 * sc);
        (cap5 * (1.0 - live_util(g, &s.email, plan_scale(&s.plan), 5, now, p5, p7)))
            .min(cap7 * (1.0 - live_util(g, &s.email, plan_scale(&s.plan), 7, now, p5, p7)))
    };
    let lru = |e: &str| g.live.get(e).map(|l| l.last_used).unwrap_or(0);
    let eligible = g.subs.iter().filter(|s| {
        let (c5, c7) = rsv.caps(&s.email);
        !is_cooling(g, &s.email, now)
            && !is_dead(g, &s.email) // мёртвые (забанены) не принимают новые сессии
            && live_util(g, &s.email, plan_scale(&s.plan), 7, now, p5, p7) < c7
            && live_util(g, &s.email, plan_scale(&s.plan), 5, now, p5, p7) < c5
            && inflight_of(g, &s.email) < max_inflight()
    });
    let best = eligible.max_by(|a, b|
        free(a).partial_cmp(&free(b)).unwrap_or(Equal)                    // больше свободной ёмкости
            .then(inflight_of(g, &b.email).cmp(&inflight_of(g, &a.email)))// при равенстве — меньше in-flight (веер)
            .then(lru(&b.email).cmp(&lru(&a.email))));                    // затем давнее использование
    match best {
        Some(s) => Some(s.clone()),
        None => select_best(g, &HashSet::new(), now, &Reserve::FULL, p5, p7), // все за конвертом → не зависаем
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
    /// ВАЖНО: подписку, которая временно выпала из ростера (смена статуса/флота, удаление токена)
    /// и может вернуться, НЕ теряем в cooling — иначе бан (на минуты/дни) молча сбрасывался бы, и
    /// следующий `route`/`pick` слал бы живой трафик обратно на отлимиченный аккаунт (429-шторм).
    /// `import_state` работает только на старте, reload `pool_state` не перечитывает — поэтому
    /// удерживаем cooling здесь, в памяти. Протухший cooling отсеется на следующем replace_subs.
    pub fn replace_subs(&self, subs: Vec<Sub>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let keep: HashSet<String> = subs.iter().map(|s| s.email.clone()).collect();
        let now = now();
        g.live.retain(|k, v| keep.contains(k) || v.cooling_until > now);
        // Смена токена существующей подписки (authbot заменил) → вердикт о СТАРОМ токене недействителен:
        // авто-ревайв в ПАМЯТИ сразу (не ждём probe), чтобы забаненная-и-перевыпущенная вернулась в строй.
        // Durable-строку в БД сбрасывает add/add_file (ON CONFLICT), здесь чиним in-memory зеркало.
        for s in &subs {
            let fp = token_fp(&s.token);
            if let Some(l) = g.live.get_mut(&s.email) {
                if !l.auth_token_fp.is_empty() && l.auth_token_fp != fp {
                    reset_auth_healthy(l);
                    l.auth_token_fp = fp;
                }
            }
        }
        g.subs = subs;
    }

    pub fn len(&self) -> usize { self.inner.lock().unwrap_or_else(|e| e.into_inner()).subs.len() }
    pub fn is_empty(&self) -> bool { self.inner.lock().unwrap_or_else(|e| e.into_inner()).subs.is_empty() }

    /// Подписка сейчас в cooling? Быстрый отбой в forward: если `route`/`pick` вернули cooling-персону
    /// (значит НЕТ ни одной не-cooling), нельзя слать на неё живой трафик — будет свежий 429 (ban-signal).
    pub fn is_cooling(&self, email: &str) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        is_cooling(&g, email, now())
    }

    /// Наименее загруженная живая подписка не из `exclude` (для ретраев ротации/спилла).
    /// allow_full=true → до 100% (приоритетные ходы), иначе per-подписочный запас (`self.reserve`).
    pub fn pick(&self, exclude: &HashSet<String>, allow_full: bool) -> Option<Sub> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let rsv = if allow_full { Reserve::FULL } else { self.reserve };
        select_best(&g, exclude, now(), &rsv, self.prior5h_usd, self.prior7d_usd)
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
                let home_scale = g.subs.iter().find(|s| s.email == home)
                    .map(|s| plan_scale(&s.plan)).unwrap_or_else(|| plan_scale(""));
                let (e5, e7) = (
                    live_util(&g, &home, home_scale, 5, now, p5, p7),
                    live_util(&g, &home, home_scale, 7, now, p5, p7),
                );
                let deep_cooling = cooling > now && cooling - now >= REBIND_AFTER;
                let (c5, c7) = rsv.caps(&home);
                let over_cap = e5 >= c5 || e7 >= c7;
                let home_dead = is_dead(&g, &home); // забаненный дом → пере-привязка (не пиним 401)
                if !deep_cooling && !over_cap && !home_dead {
                    // дом всё ещё «наш» — кэш тёплый; либо пин, либо кратковременный спилл
                    if let Some(b) = g.bindings.get_mut(&session) { b.last_seen = now; }
                    let busy = cooling > now || inflight_of(&g, &home) >= max_inflight();
                    if !busy {
                        g.route_pin += 1;
                        return g.subs.iter().find(|s| s.email == home).cloned(); // ПИН (cache-hit)
                    }
                    // временно занят → спилл ЭТОГО запроса, дом за сессией сохраняем. Cooling-
                    // альтернативы здесь запрещены: если здоровой замены нет, ослабляем конверт на
                    // собственном доме, а не превращаем busy в ложный fleet-exhaustion Retry-After.
                    g.route_spill += 1;
                    let ex: HashSet<String> = std::iter::once(home.clone()).collect();
                    return select_best_non_cooling(&g, &ex, now, &rsv, p5, p7)
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
        if l.inflight >= max_inflight() {
            // Не скрываем реальную нагрузку клампом: попытка уже выбрана и forward сейчас её пошлёт.
            // Лог без email/токена делает окно гонки наблюдаемым до атомарной lease-миграции.
            eprintln!("pool: in-flight envelope oversubscribed (current={})", l.inflight);
        }
        l.inflight = l.inflight.saturating_add(1);
        // AUDIT-TODO(C43): route/pick должны атомарно резервировать слот и возвращать RAII lease.
    }

    /// Подписка здорова: снять истёкший cooling, обновить last_used. In-flight НЕ трогаем — слотом
    /// владеет вызывающий (в forward — `InflightGuard`/`end_stream`), чтобы не было двойного учёта.
    /// Годится и для успеха (стрим ещё течёт), и для клиентской 4xx (подписка ни при чём).
    pub fn mark_healthy(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        if l.cooling_until <= now() { l.cooling_until = 0; }
        l.last_used = now();
        // Живой 2xx/4xx-трафик доказал, что токен рабочий → снять «мёртвость»/подозрение в ПАМЯТИ.
        // Durable-вердикт в БД скорректирует ближайший чистый probe (record_probe → persist healthy).
        reset_auth_healthy(l);
    }

    /// Скормить результат ФОНОВОГО probe в durable-машину auth-health. Возвращает `Some(SubHealth)`,
    /// когда durable-поле изменилось (вызывающий персистит вердикт в PostgreSQL), иначе `None`.
    /// Один 401/403 НЕ убивает — нужна серия `DEAD_STREAK` за ≥`DEAD_MIN_SECS`. Только чистые probe
    /// поллера зовут это (не живой трафик — там вина могла быть в запросе клиента).
    pub fn record_probe(&self, email: &str, http: u16, token_fp: &str) -> Option<registry::SubHealth> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let l = g.live.entry(email.to_string()).or_default();
        let before = health_row(email, l);
        apply_probe(l, http, token_fp, now());
        let after = health_row(email, l);
        if before != after { Some(after) } else { None }
    }

    /// Оператор/старт: применить durable auth-health из БД к in-memory состоянию. Мёртвые сразу
    /// исключаются из ротации; suspect — под наблюдением поллера.
    pub fn import_health(&self, rows: Vec<registry::SubHealth>) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let known: HashSet<String> = g.subs.iter().map(|s| s.email.clone()).collect();
        for r in rows {
            if !known.contains(&r.email) { continue; }
            let l = g.live.entry(r.email.clone()).or_default();
            l.auth_state = AuthState::from_str(&r.auth_state);
            l.auth_fail_streak = r.auth_fail_streak.max(0);
            l.first_auth_fail_ts = r.first_auth_fail_ts;
            l.last_auth_fail_ts = r.last_auth_fail_ts;
            l.last_auth_http = r.last_auth_http;
            l.dead_since_ts = r.dead_since_ts;
            l.dead_reason = r.dead_reason;
            l.auth_token_fp = r.auth_token_fp;
        }
    }

    /// Оператор: вручную вернуть подписку в строй (после ручной проверки/замены). Возвращает вердикт
    /// для персиста. In-memory сразу healthy → снова в ротации.
    pub fn revive(&self, email: &str) -> Option<registry::SubHealth> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !g.subs.iter().any(|s| s.email == email) { return None; } // ревайвим только известную подписку
        let l = g.live.entry(email.to_string()).or_default();
        reset_auth_healthy(l);
        Some(health_row(email, l))
    }

    /// Оператор: вручную пометить подписку мёртвой (например, знаем о бане вне 401-детекта).
    pub fn kill(&self, email: &str, reason: &str) -> Option<registry::SubHealth> {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !g.subs.iter().any(|s| s.email == email) { return None; } // килим только известную подписку
        let l = g.live.entry(email.to_string()).or_default();
        let now = now();
        l.auth_state = AuthState::Dead;
        if l.dead_since_ts == 0 { l.dead_since_ts = now; }
        l.dead_reason = if reason.is_empty() { "operator".to_string() } else { reason.to_string() };
        Some(health_row(email, l))
    }

    /// Подписка мёртва (токен отвергнут корроборированно)? Быстрая проверка для forward.
    pub fn is_auth_dead(&self, email: &str) -> bool {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.live.get(email).map(|l| l.auth_state == AuthState::Dead).unwrap_or(false)
    }

    /// Пометить подписку для НЕМЕДЛЕННОГО liveness-probe поллером (сброс `polled_ts` в 0 → `next_probe_at`
    /// сразу due). Зовётся из forward, когда подписка отдала 401/403, а запрос успешно ушёл на ДРУГУЮ
    /// (значит вина не запроса, а токена этой). Чистый probe поллера авторитетно рассудит: мёртвый токен
    /// → карантин, живой (был crafted-запрос) → без вреда. Так revoked-токен перестаёт быть placement-
    /// магнитом за ~1 цикл поллера, а НЕ за LIVENESS_INTERVAL — и БЕЗ cooling здесь (иначе crafted-запрос
    /// студил бы флот = DoS). In-flight/cooling не трогаем.
    pub fn request_probe(&self, email: &str) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(l) = g.live.get_mut(email) {
            // Дебаунс по ОТДЕЛЬНОМУ last_probe_req (не polled_ts — его пассивный сбор держит свежим на
            // каждом ответе, включая тот 401/403, что нас позвал → дебаунс по нему заблокировал бы и
            // легитимный probe). Форсим не чаще раза в ~15с: флуд crafted-401/403 не гонит probe-трафик.
            let now = now();
            if l.last_probe_req >= now - 15 { return; }
            l.last_probe_req = now;
            l.polled_ts = 0; // → next_probe_at сразу due, поллер проверит чистым probe
        }
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
            let deadline = now().saturating_add(secs.max(1));
            let l = g.live.entry(email.to_string()).or_default();
            l.cooling_until = l.cooling_until.max(deadline);
        }
        self.signal_change();
    }

    /// Студить БЕЗ трогания in-flight — для фонового поллера (он не делал `mark_used`).
    pub fn cool(&self, email: &str, secs: i64) {
        {
            let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            let deadline = now().saturating_add(secs.max(1));
            let l = g.live.entry(email.to_string()).or_default();
            l.cooling_until = l.cooling_until.max(deadline);
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

            // Та же «живая» утилизация, что используется route/pick/place: один источник истины.
            let u5 = live_util(&g, &s.email, sc, 5, now, p5, p7);
            let u7 = live_util(&g, &s.email, sc, 7, now, p5, p7);
            let rem5 = cap5 * (1.0 - u5);
            let rem7 = cap7 * (1.0 - u7);

            // сколько сбросов окна попадает в (now, now+H] — окно наливается заново (квота, не rate).
            // reset мог УЖЕ пройти (idle-подписка) — нормализуем к следующему будущему сбросу,
            // иначе считали бы прошлые сбросы как будущие и завышали доступность.
            let resets = |reset: i64, win: i64, h: i64| -> i64 {
                // Строгое `>`: при reset==now окно уже считается сброшенным в `live` (now>=reset → rem
                // полное), поэтому ЭТУ точку нельзя ещё раз считать будущим сбросом — иначе одно окно
                // учтено дважды. Нормализуем к следующему сбросу (now+win), согласованно с `live`.
                let next = if reset > now { reset } else { reset + ((now - reset) / win + 1) * win };
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
                auth_dead: l.auth_state == AuthState::Dead,
                auth_state: l.auth_state.as_str().to_string(),
                dead_reason: l.dead_reason.clone(),
                dead_since_ts: l.dead_since_ts,
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

    /// Экспорт durable-состояния для персиста, включая временно неактивные подписки с удержанным
    /// cooling. Иначе следующий safety-flush после рестарта мог бы стереть dormant-бан до реактивации.
    pub fn export_state(&self) -> Vec<registry::PoolStateRow> {
        let g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        g.live.iter().map(|(email, l)| registry::PoolStateRow {
            email: email.clone(),
            cooling_until: l.cooling_until,
            cap5h_usd: l.cap5h_usd,
            cap7d_usd: l.cap7d_usd,
            spent_total_usd: l.spent_total_usd,
            util5h: l.util5h,
            util7d: l.util7d,
            reset5h: l.reset5h,
            reset7d: l.reset7d,
            calib_n: l.calib_n as i64,
            version: l.persist_version,
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
            // Активным восстанавливаем всё. Неактивным удерживаем строку только пока важен будущий
            // cooling: replace_subs потом сольёт её в live при реактивации и не пустит трафик в бан.
            if !known.contains(&r.email) && r.cooling_until <= now { continue; }
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
            l.persist_version = r.version.max(0);
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

    /// Accept versions returned by a successful fenced PostgreSQL CAS write without replacing live
    /// utilization/cooling values that may have advanced while persistence was in flight.
    pub fn accept_persist_versions(&self, versions: &[(String, i64)]) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for (email, version) in versions {
            if let Some(live) = g.live.get_mut(email) {
                live.persist_version = live.persist_version.max(*version);
            }
        }
    }

    /// Rebase local observations after a PostgreSQL CAS conflict. Cooling and utilization merge
    /// conservatively, newer reset windows replace older ones, and calibrated capacity never grows
    /// merely because two writers raced. The caller retries the fenced CAS with the returned version.
    pub fn merge_persisted_state(&self, rows: Vec<registry::PoolStateRow>) {
        fn conservative_cap(local: f64, remote: f64) -> f64 {
            match (local > 0.0, remote > 0.0) {
                (true, true) => local.min(remote),
                (false, true) => remote,
                _ => local,
            }
        }
        fn merge_window(local_util: &mut f64, local_reset: &mut i64, remote_util: f64, remote_reset: i64) {
            if remote_reset > *local_reset {
                *local_reset = remote_reset;
                *local_util = remote_util;
            } else if remote_reset == *local_reset {
                *local_util = local_util.max(remote_util);
            }
        }

        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        for row in rows {
            let live = g.live.entry(row.email).or_default();
            live.cooling_until = live.cooling_until.max(row.cooling_until);
            live.spent_total_usd = live.spent_total_usd.max(row.spent_total_usd);
            live.cap5h_usd = conservative_cap(live.cap5h_usd, row.cap5h_usd);
            live.cap7d_usd = conservative_cap(live.cap7d_usd, row.cap7d_usd);
            live.calib_n = live.calib_n.max(row.calib_n.max(0) as u32);
            merge_window(&mut live.util5h, &mut live.reset5h, row.util5h, row.reset5h);
            merge_window(&mut live.util7d, &mut live.reset7d, row.util7d, row.reset7d);
            live.persist_version = live.persist_version.max(row.version.max(0));
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

    /// Неизвестный тариф → консервативный прайор (НЕ Max20), чтобы не переоценить ёмкость → 429-шторм.
    #[test]
    fn plan_scale_unknown_is_conservative() {
        assert_eq!(plan_scale("max20"), 1.0);
        assert!(plan_scale("") < 1.0, "unknown не должен быть Max20");
        assert!(plan_scale("нечто") < 1.0);
        assert_eq!(plan_scale(""), plan_scale("max5")); // консервативно = как Max5
    }

    /// Весь флот в cooling: pick НЕ зависает (вернёт подписку), но `is_cooling` это распознаёт —
    /// forward по нему делает быстрый 429+soonest_ready, а не шлёт живой трафик на лимитированный аккаунт.
    #[test]
    fn all_cooling_detected_for_fast_reject() {
        let p = pool(&["a", "b"]);
        p.mark_cooling("a", 300);
        p.mark_cooling("b", 300);
        let picked = p.pick(&none(), false).expect("pick не зависает даже когда все cooling");
        assert!(p.is_cooling(&picked.email), "is_cooling обязан видеть, что pick вернул cooling-подписку");
        assert!(p.soonest_ready().is_some(), "есть время до готовности для Retry-After");
    }

    /// Бан (cooling) переживает выпадение подписки из ростера и его обновление (флап) — иначе
    /// вернувшийся трафик полетел бы на забаненный аккаунт (429-шторм).
    #[test]
    fn cooling_survives_roster_flap() {
        let p = pool(&["a", "b"]);
        p.mark_cooling("a", 3600);
        p.replace_subs(vec![sub("b")]);          // "a" выпала из активного набора
        assert!(p.is_cooling("a"), "cooling пережил replace_subs (бан не забыт)");
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
        for _ in 0..max_inflight() { p.mark_used("a"); }             // …но забит до конверта
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
        for _ in 0..max_inflight() { p.mark_used(&home); } // забили дом до конверта
        let spilled = p.route(s).unwrap().email;
        assert_ne!(spilled, home, "занятый дом → спилл на другой");
        for _ in 0..max_inflight() { p.end_stream(&home); } // слоты освободились
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

    #[test]
    fn cas_rebase_is_conservative_and_accepts_newer_window() {
        let p = pool(&["a"]);
        p.import_state(vec![registry::PoolStateRow {
            email: "a".into(), cooling_until: now() + 60,
            cap5h_usd: 50.0, cap7d_usd: 500.0, spent_total_usd: 10.0,
            util5h: 0.4, util7d: 0.3, reset5h: 100, reset7d: 100,
            calib_n: 4, version: 2,
        }]);
        p.merge_persisted_state(vec![registry::PoolStateRow {
            email: "a".into(), cooling_until: now() + 120,
            cap5h_usd: 80.0, cap7d_usd: 400.0, spent_total_usd: 12.0,
            util5h: 0.6, util7d: 0.1, reset5h: 100, reset7d: 200,
            calib_n: 5, version: 3,
        }]);
        let row = p.export_state().into_iter().find(|r| r.email == "a").unwrap();
        assert_eq!(row.version, 3);
        assert_eq!(row.cap5h_usd, 50.0, "a race must not inflate calibrated capacity");
        assert_eq!(row.cap7d_usd, 400.0);
        assert_eq!(row.util5h, 0.6, "same window keeps the conservative utilization");
        assert_eq!(row.reset7d, 200);
        assert_eq!(row.util7d, 0.1, "a newer provider window replaces the expired one");
        assert!(row.cooling_until >= now() + 119);
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

    /// Поздний короткий failure не имеет права укоротить уже установленный длинный quota-cooling.
    #[test]
    fn cooling_deadline_only_extends() {
        let p = pool(&["a"]);
        p.mark_cooling("a", 3600);
        let long = p.snapshot()[0].1.cooling_until;
        p.mark_cooling("a", 10);
        assert!(p.snapshot()[0].1.cooling_until >= long);
        p.cool("a", 5);
        assert!(p.snapshot()[0].1.cooling_until >= long);
    }

    /// Retry/pick не льёт в аккаунт за MAX_INFLIGHT, пока есть кандидат под конвертом.
    #[test]
    fn retry_selection_respects_concurrency_envelope() {
        let p = pool(&["a", "b"]);
        p.set_util("a", Some(0.10), Some(0.10), None, None, None);
        p.set_util("b", Some(0.00), Some(0.11), None, None, None);
        for _ in 0..max_inflight() { p.mark_used("a"); }
        assert_eq!(picked(&p), "b", "слегка более горячий, но свободный b должен разгрузить a");
    }

    /// Локально известный расход сразу участвует в reserve-гейте, не ждёт следующего util-заголовка.
    #[test]
    fn routing_uses_live_spend_between_headers() {
        let p = pool(&["a", "b"]);
        let f5 = now() + WIN5_SECS;
        let f7 = now() + WIN7_SECS;
        p.set_util("a", Some(0.85), Some(0.10), None, Some(f5), Some(f7));
        p.set_util("b", Some(0.50), Some(0.11), None, Some(f5), Some(f7));
        {
            let mut g = p.inner.lock().unwrap_or_else(|e| e.into_inner());
            let a = g.live.get_mut("a").unwrap();
            a.cap5h_usd = 50.0;
            a.cap7d_usd = 1500.0;
        }
        assert_eq!(picked(&p), "a", "до расхода меньший 7d util выигрывает");
        p.record_spend("a", 5 * 1_000_000_000); // live 5h = cap 0.95, а eligibility требует strict <
        assert_eq!(picked(&p), "b", "после расхода a должна выйти за reserve-cap немедленно");
    }

    /// Busy-дом не спиллирует на cooling-альтернативу и не создаёт ложный долгий fleet exhaustion.
    #[test]
    fn spill_does_not_choose_cooling_alternative() {
        let p = pool(&["a", "b"]);
        for e in ["a", "b"] { p.set_util(e, Some(0.1), Some(0.1), None, None, None); }
        let session = 4242;
        let home = p.route(session).unwrap().email;
        let other = if home == "a" { "b" } else { "a" };
        p.mark_cooling(other, 3600);
        for _ in 0..max_inflight() { p.mark_used(&home); }
        let chosen = p.route(session).unwrap();
        assert_eq!(chosen.email, home, "без здоровой альтернативы ослабляем конверт на доме");
        assert!(!p.is_cooling(&chosen.email));
    }

    /// Future cooling импортируется даже для неактивной на старте подписки и переживает flush/reactivate.
    #[test]
    fn import_retains_inactive_future_cooling() {
        let source = pool(&["a"]);
        source.mark_cooling("a", 3600);
        let rows = source.export_state();

        let restarted = pool(&["b"]); // a неактивна в момент старта
        restarted.import_state(rows);
        assert!(restarted.is_cooling("a"), "dormant future cooling нельзя отбрасывать");
        assert!(restarted.export_state().iter().any(|r| r.email == "a"),
                "safety-flush обязан сохранить dormant cooling");
        restarted.replace_subs(vec![sub("a"), sub("b")]);
        assert_eq!(picked(&restarted), "b", "реактивированная a остаётся в cooling");
    }

    /// request_probe сбрасывает polled_ts в 0 → подписка «созревает» для немедленного clean-probe
    /// (revoked-токен перестаёт быть placement-магнитом за ~1 цикл поллера, а не за LIVENESS_INTERVAL).
    #[test]
    fn request_probe_marks_due() {
        let p = pool(&["a"]);
        p.set_util("a", Some(0.2), Some(0.1), None, None, None); // двигает polled_ts в now()
        assert!(p.snapshot()[0].1.polled_ts > 0, "после set_util polled_ts свеж");
        p.request_probe("a");
        assert_eq!(p.snapshot()[0].1.polled_ts, 0, "request_probe → polled_ts=0 (due сейчас)");
        // дебаунс: повторный форс в окне 15с — no-op, даже если пассивный сбор снова обновил polled_ts
        p.set_util("a", Some(0.3), None, None, None, None); // polled_ts снова > 0
        p.request_probe("a");
        assert!(p.snapshot()[0].1.polled_ts > 0, "второй request_probe в окне дебаунса — no-op");
    }

    // ── Durable auth-health: корроборированный dead-детект ──────────────────────────────────

    /// Один 401/403 — НЕ приговор: только `Suspect`. Терминал `Dead` требует серии за ≥ окно.
    #[test]
    fn single_auth_fail_is_only_suspect() {
        let mut l = Live::default();
        apply_probe(&mut l, 401, "fp1", 1_000_000);
        assert_eq!(l.auth_state, AuthState::Suspect);
        assert_eq!(l.auth_fail_streak, 1);
        assert_eq!(l.last_auth_http, 401);
    }

    /// Несколько 401 подряд, но в пределах `DEAD_MIN_SECS` (транзиентный блип) → всё ещё `Suspect`,
    /// НЕ `Dead`. Защита от ложного приговора: серия должна быть РАСТЯНУТА во времени (≥5 мин).
    #[test]
    fn rapid_fails_within_window_do_not_kill() {
        let mut l = Live::default();
        let t = 1_000_000;
        apply_probe(&mut l, 401, "fp1", t);
        apply_probe(&mut l, 401, "fp1", t + 10);
        apply_probe(&mut l, 401, "fp1", t + 20);
        assert_eq!(l.auth_fail_streak, 3);
        assert_eq!(l.auth_state, AuthState::Suspect, "streak набран, но <5мин → ещё не dead");
    }

    /// Два 401-probe, растянутых на ≥`DEAD_MIN_SECS` (2 вызова в 5 минут) → корроборировано `Dead`
    /// с причиной re-auth. Это ровно политика «2 звонка за 5 минут».
    #[test]
    fn corroborated_fails_over_window_go_dead() {
        let mut l = Live::default();
        let t = 1_000_000;
        apply_probe(&mut l, 401, "fp1", t);
        assert_eq!(l.auth_state, AuthState::Suspect, "первый 401 → только suspect");
        apply_probe(&mut l, 401, "fp1", t + DEAD_MIN_SECS);
        assert_eq!(l.auth_state, AuthState::Dead, "второй 401 через 5 мин → dead");
        assert_eq!(l.dead_reason, "authentication_error");
        assert_eq!(l.dead_since_ts, t + DEAD_MIN_SECS);
    }

    /// 403 → причина permission_error (аккаунт заблокирован/забанен, а не просто re-auth).
    #[test]
    fn permission_error_reason_for_403() {
        let mut l = Live::default();
        let t = 2_000_000;
        apply_probe(&mut l, 403, "fp", t);
        apply_probe(&mut l, 403, "fp", t + DEAD_MIN_SECS + 5);
        assert_eq!(l.auth_state, AuthState::Dead);
        assert_eq!(l.dead_reason, "permission_error");
    }

    /// Чистый 2xx-probe снимает подозрение/смерть (токен доказал живость).
    #[test]
    fn clean_probe_clears_suspect_and_dead() {
        let mut l = Live::default();
        let t = 3_000_000;
        for i in 0..3 { apply_probe(&mut l, 401, "fp", t + i * (DEAD_MIN_SECS / 2 + 1)); }
        assert_eq!(l.auth_state, AuthState::Dead);
        apply_probe(&mut l, 200, "fp", t + 10 * DEAD_MIN_SECS);
        assert_eq!(l.auth_state, AuthState::Healthy);
        assert_eq!(l.auth_fail_streak, 0);
        assert_eq!(l.dead_since_ts, 0);
    }

    /// Смена токена (authbot заменил) обнуляет вердикт о старом токене → авто-ревайв.
    #[test]
    fn token_change_resets_verdict() {
        let mut l = Live::default();
        let t = 4_000_000;
        for i in 0..3 { apply_probe(&mut l, 401, "old", t + i * (DEAD_MIN_SECS / 2 + 1)); }
        assert_eq!(l.auth_state, AuthState::Dead);
        // новый отпечаток токена → сброс к healthy ДО оценки текущего http (тут даже 401 не важен)
        apply_probe(&mut l, 200, "new", t + 10 * DEAD_MIN_SECS);
        assert_eq!(l.auth_state, AuthState::Healthy);
        assert_eq!(l.auth_token_fp, "new");
    }

    /// Мёртвая подписка ИСКЛЮЧЕНА из ротации (не шлём гарантированный 401 клиенту).
    #[test]
    fn dead_subscription_excluded_from_selection() {
        let p = pool(&["a", "b"]);
        p.kill("a", "permission_error");
        // pick никогда не вернёт мёртвую a
        for _ in 0..10 { assert_eq!(picked(&p), "b"); }
        // route (новая сессия) тоже минует мёртвую
        assert_eq!(p.route(12345).unwrap().email, "b");
        // если ОБЕ мертвы — пул честно пуст (forward отдаст Overloaded, а не 401)
        p.kill("b", "authentication_error");
        assert!(p.pick(&none(), false).is_none(), "все мёртвы → нет кандидата (не зависаем в 401)");
    }

    /// Ручной revive возвращает мёртвую подписку в строй; is_auth_dead это отражает.
    #[test]
    fn revive_returns_dead_sub_to_rotation() {
        let p = pool(&["a", "b"]);
        p.kill("a", "operator");
        assert!(p.is_auth_dead("a"));
        p.revive("a");
        assert!(!p.is_auth_dead("a"));
        // после revive снова участвует в выборе (least-loaded может выбрать a)
        p.set_util("b", Some(0.9), Some(0.9), None, None, None); // нагрузим b → выберут a
        assert_eq!(picked(&p), "a");
    }

    /// import_health (старт из БД) применяет durable-вердикт: мёртвая сразу вне ротации, переживает
    /// рестарт (в отличие от старого эфемерного auth_dead, который сбрасывался healthy на деплой).
    #[test]
    fn import_health_restores_dead_across_restart() {
        let p = pool(&["a", "b"]);
        p.import_health(vec![registry::SubHealth {
            email: "a".into(), auth_state: "dead".into(), auth_fail_streak: 3,
            dead_since_ts: 999, dead_reason: "permission_error".into(), auth_token_fp: "fp".into(),
            ..Default::default()
        }]);
        assert!(p.is_auth_dead("a"));
        assert_eq!(picked(&p), "b", "восстановленная из БД мёртвая — вне ротации");
    }

    /// record_probe возвращает строку для персиста ТОЛЬКО при изменении durable-полей (иначе None →
    /// поллер не пишет лишнего в БД на каждый здоровый probe). Первый probe фиксирует token-fp (1 запись).
    #[test]
    fn record_probe_persists_only_on_change() {
        let p = pool(&["a"]);
        // первый probe фиксирует отпечаток токена (полезно) → изменение → Some
        assert!(p.record_probe("a", 200, "fp").is_some());
        // второй ИДЕНТИЧНЫЙ здоровый probe: durable-поля не изменились → None (нет лишней записи)
        assert!(p.record_probe("a", 200, "fp").is_none());
        // 401 → Suspect: изменение → Some
        let changed = p.record_probe("a", 401, "fp");
        assert!(changed.is_some());
        assert_eq!(changed.unwrap().auth_state, "suspect");
    }
}
