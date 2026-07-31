//! История метрик пула (time-series) — фундамент под capacity-planning и предсказательную модель.
//! ОТДЕЛЬНАЯ `metrics.db` (НЕ money-БД): своя ретенция/downsampling, запись не мешает биллингу.
//! Пишется фоновым `poller::metrics_loop` из ТОГО ЖЕ агрегата, что и `/overview` (`http::overview_value`).
//! Схема плоская (по колонке на метрику) — удобно для будущих SQL-агрегатов/обучения модели.

use rusqlite::Connection;
use serde_json::Value;

pub fn open(path: &str) -> rusqlite::Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = std::fs::create_dir_all(dir);
        }
    }
    let c = Connection::open(path)?;
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS snapshots(
            ts INTEGER PRIMARY KEY,
            subs INTEGER, calibrated INTEGER,
            avail_1h REAL, avail_5h REAL, avail_1d REAL, avail_7d REAL,
            cap5h REAL, cap7d REAL, cons5h REAL, cons7d REAL,
            util5h REAL, util7d REAL, healthy INTEGER, cooling INTEGER,
            balance_usd REAL, reserved_usd REAL, spent_usd REAL, active_keys INTEGER,
            potential_realapi REAL, coverage7d REAL,
            headroom5h REAL, headroom7d REAL, subs_needed INTEGER, gap INTEGER);
         CREATE TABLE IF NOT EXISTS sub_snapshots(
            email TEXT NOT NULL, ts INTEGER NOT NULL,
            cap5h REAL, cap7d REAL, util5h REAL, util7d REAL,
            PRIMARY KEY(email, ts));
         CREATE TABLE IF NOT EXISTS sub_peaks(
            email TEXT PRIMARY KEY,
            max_cap5h REAL DEFAULT 0, max_cap7d REAL DEFAULT 0,
            samples INTEGER DEFAULT 0, updated_ts INTEGER DEFAULT 0);",
    )?;
    Ok(c)
}

/// Снапшот ёмкости ПО КАЖДОЙ подписке (email полный — metrics.db не публичен). На дистанции даёт
/// реальный потолок: `MAX(cap5h/cap7d)` по подписке (не только текущая EMA-калибровка).
pub fn insert_sub_snapshots(
    c: &Connection,
    ts: i64,
    subs: &[(String, f64, f64, f64, f64)],
) -> rusqlite::Result<()> {
    for (email, cap5h, cap7d, util5h, util7d) in subs {
        // raw-история (подрезается) — для тренда/анализа
        c.execute(
            "INSERT OR REPLACE INTO sub_snapshots(email,ts,cap5h,cap7d,util5h,util7d) \
             VALUES(?1,?2,?3,?4,?5,?6)",
            rusqlite::params![email, ts, cap5h, cap7d, util5h, util7d],
        )?;
        // durable-пик (НЕ подрезается): истинный max за всю дистанцию, переживает prune и рестарт
        c.execute(
            "INSERT INTO sub_peaks(email,max_cap5h,max_cap7d,samples,updated_ts) VALUES(?1,?2,?3,1,?4) \
             ON CONFLICT(email) DO UPDATE SET \
               max_cap5h=MAX(max_cap5h,excluded.max_cap5h), \
               max_cap7d=MAX(max_cap7d,excluded.max_cap7d), \
               samples=samples+1, updated_ts=excluded.updated_ts",
            rusqlite::params![email, cap5h, cap7d, ts])?;
    }
    Ok(())
}

/// Истинный пик ёмкости по подписке за всю дистанцию (из durable sub_peaks, prune не стирает):
/// (email, max_cap5h, max_cap7d, samples).
pub fn sub_maxes(c: &Connection) -> rusqlite::Result<Vec<(String, f64, f64, i64)>> {
    let mut stmt = c.prepare("SELECT email, max_cap5h, max_cap7d, samples FROM sub_peaks")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, f64>(1)?,
            r.get::<_, f64>(2)?,
            r.get::<_, i64>(3)?,
        ))
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Записать один снапшот из агрегата `/overview` (JSON). Поля, которых нет → NULL (напр. headroom=∞).
pub fn insert_snapshot(c: &Connection, o: &Value) -> rusqlite::Result<()> {
    let f = |p: &str| o.pointer(p).and_then(|v| v.as_f64());
    let i = |p: &str| o.pointer(p).and_then(|v| v.as_i64());
    let b = |p: &str| o.pointer(p).and_then(|v| v.as_bool()).map(|x| x as i64);
    c.execute(
        "INSERT OR REPLACE INTO snapshots(ts,subs,calibrated,avail_1h,avail_5h,avail_1d,avail_7d,\
            cap5h,cap7d,cons5h,cons7d,util5h,util7d,healthy,cooling,\
            balance_usd,reserved_usd,spent_usd,active_keys,potential_realapi,coverage7d,\
            headroom5h,headroom7d,subs_needed,gap) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25)",
        rusqlite::params![
            i("/now"), i("/subs"), b("/calibrated"),
            f("/supply/avail_usd/1h"), f("/supply/avail_usd/5h"), f("/supply/avail_usd/1d"), f("/supply/avail_usd/7d"),
            f("/supply/cap_usd/5h"), f("/supply/cap_usd/7d"),
            f("/supply/consumed_usd/5h"), f("/supply/consumed_usd/7d"),
            f("/supply/util/5h"), f("/supply/util/7d"),
            i("/supply/health/healthy"), i("/supply/health/cooling"),
            f("/demand/balance_usd"), f("/demand/reserved_usd"), f("/demand/spent_usd"),
            i("/demand/active_accounts"), f("/demand/potential_realapi_usd"), f("/coverage/7d"),
            f("/headroom/5h"), f("/headroom/7d"), i("/recommend/subs_needed"), i("/recommend/gap"),
        ],
    )?;
    Ok(())
}

/// Обрезать снапшоты старше retention (metrics.db не растёт вечно). Снапшоты редки → без батчинга.
/// Пиковые max(cap) при этом НЕ теряем осознанно: обрезаем raw-историю, но пик уже «зафиксирован»
/// в старых данных; для долгой памяти пика retention ставь щедро (деф 90д) или считай max в приложении.
pub fn prune(c: &Connection, older_than_ts: i64) -> rusqlite::Result<usize> {
    let n = c.execute(
        "DELETE FROM snapshots WHERE ts < ?1",
        rusqlite::params![older_than_ts],
    )?;
    let _ = c.execute(
        "DELETE FROM sub_snapshots WHERE ts < ?1",
        rusqlite::params![older_than_ts],
    );
    Ok(n)
}

// ── Чтение истории (read-path для /fleet-history) ────────────────────────────

/// Окно истории → (длина окна, сек; размер бакета, сек). Бакеты выбраны так, чтобы ответ
/// укладывался в ≤ ~500 точек при минутной записи снапшотов: 24h→5м (288), 7d→30м (336),
/// 30d→2ч (360), 90d→6ч (360).
pub fn window_bucket(window: &str) -> Option<(i64, i64)> {
    Some(match window {
        "24h" => (86_400, 300),
        "7d" => (7 * 86_400, 1_800),
        "30d" => (30 * 86_400, 7_200),
        "90d" => (90 * 86_400, 21_600),
        _ => return None,
    })
}

/// Точка флот-истории: сырая строка `snapshots` (NULL → None) или сбакетированный агрегат той же
/// формы. `calibrated` и `active_keys` панели трендов не нужны — read-path их не поднимает.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FleetPoint {
    pub ts: i64,
    pub avail_1h: Option<f64>,
    pub avail_5h: Option<f64>,
    pub avail_1d: Option<f64>,
    pub avail_7d: Option<f64>,
    pub util5h: Option<f64>,
    pub util7d: Option<f64>,
    pub cap5h: Option<f64>,
    pub cap7d: Option<f64>,
    pub cons5h: Option<f64>,
    pub cons7d: Option<f64>,
    pub healthy: Option<i64>,
    pub cooling: Option<i64>,
    pub subs: Option<i64>,
    pub balance_usd: Option<f64>,
    pub reserved_usd: Option<f64>,
    pub spent_usd: Option<f64>,
    pub potential_realapi: Option<f64>,
    pub coverage7d: Option<f64>,
    pub headroom5h: Option<f64>,
    pub headroom7d: Option<f64>,
    pub subs_needed: Option<i64>,
    pub gap: Option<i64>,
}

/// Точка per-sub истории из `sub_snapshots` (сырая или сбакетированная).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SubPoint {
    pub ts: i64,
    pub cap5h: Option<f64>,
    pub cap7d: Option<f64>,
    pub util5h: Option<f64>,
    pub util7d: Option<f64>,
}

/// Флот-история за окно: один indexed range-scan по ts (PRIMARY KEY) + бакетирование в памяти.
/// Читает через ОТДЕЛЬНОЕ подключение к той же metrics.db (WAL) — параллельному писателю
/// (`poller::metrics_loop`) читатель не мешает, запись не ломается.
pub fn fleet_history(
    c: &Connection,
    since_ts: i64,
    bucket_secs: i64,
) -> rusqlite::Result<Vec<FleetPoint>> {
    let mut stmt = c.prepare(
        "SELECT ts,avail_1h,avail_5h,avail_1d,avail_7d,util5h,util7d,cap5h,cap7d,cons5h,cons7d,\
                healthy,cooling,subs,balance_usd,reserved_usd,spent_usd,potential_realapi,coverage7d,\
                headroom5h,headroom7d,subs_needed,gap \
         FROM snapshots WHERE ts >= ?1 ORDER BY ts",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts], |r| {
        Ok(FleetPoint {
            ts: r.get(0)?,
            avail_1h: r.get(1)?,
            avail_5h: r.get(2)?,
            avail_1d: r.get(3)?,
            avail_7d: r.get(4)?,
            util5h: r.get(5)?,
            util7d: r.get(6)?,
            cap5h: r.get(7)?,
            cap7d: r.get(8)?,
            cons5h: r.get(9)?,
            cons7d: r.get(10)?,
            healthy: r.get(11)?,
            cooling: r.get(12)?,
            subs: r.get(13)?,
            balance_usd: r.get(14)?,
            reserved_usd: r.get(15)?,
            spent_usd: r.get(16)?,
            potential_realapi: r.get(17)?,
            coverage7d: r.get(18)?,
            headroom5h: r.get(19)?,
            headroom7d: r.get(20)?,
            subs_needed: r.get(21)?,
            gap: r.get(22)?,
        })
    })?;
    let rows: Vec<FleetPoint> = rows.filter_map(|x| x.ok()).collect();
    Ok(bucket_fleet(&rows, bucket_secs))
}

/// Per-sub история за окно по ПРЕФИКСУ email: панель знает только маску «abcd…» (первые 4 символа,
/// как у /subs и /capacity), полные email в HTTP не отдаём. Префиксный GLOB использует индекс
/// PRIMARY KEY(email, ts); метасимволы GLOB в префиксе экранируются, чтобы не подцепить чужие
/// адреса. Известное ограничение: две подписки с общими первыми 4 символами email склеиваются в
/// один ряд — при текущем размере флота приемлемо (маска /subs имеет ту же коллизию).
pub fn sub_history(
    c: &Connection,
    email_prefix: &str,
    since_ts: i64,
    bucket_secs: i64,
) -> rusqlite::Result<Vec<SubPoint>> {
    let mut pattern = String::with_capacity(email_prefix.len() + 4);
    for ch in email_prefix.chars() {
        if matches!(ch, '*' | '?' | '[' | ']') {
            pattern.push('[');
            pattern.push(ch);
            pattern.push(']');
        } else {
            pattern.push(ch);
        }
    }
    pattern.push('*');
    let mut stmt = c.prepare(
        "SELECT ts,cap5h,cap7d,util5h,util7d FROM sub_snapshots \
         WHERE email GLOB ?1 AND ts >= ?2 ORDER BY ts",
    )?;
    let rows = stmt.query_map(rusqlite::params![pattern, since_ts], |r| {
        Ok(SubPoint {
            ts: r.get(0)?,
            cap5h: r.get(1)?,
            cap7d: r.get(2)?,
            util5h: r.get(3)?,
            util7d: r.get(4)?,
        })
    })?;
    let rows: Vec<SubPoint> = rows.filter_map(|x| x.ok()).collect();
    Ok(bucket_subs(&rows, bucket_secs))
}

/// Бакетирование флот-ряда (строки уже отсортированы по ts).
/// Семантика агрегации:
/// - AVG — уровни (avail/cap/cons/util), деньги (balance/reserved/spent/potential), coverage и
///   headroom: минутный шум сглаживаем, для тренда важнее типичное значение бакета;
/// - AVG с округлением до целого — счётчики флота (subs/healthy/cooling);
/// - MAX — дефицитные метрики (gap, subs_needed): закупку подписок планируем по худшей точке
///   бакета, среднее занизило бы дефицит.
/// NULL в AVG не участвует (напр. headroom=∞ хранится как NULL); бакет из одних NULL → None.
/// ts точки — начало бакета.
pub fn bucket_fleet(rows: &[FleetPoint], bucket_secs: i64) -> Vec<FleetPoint> {
    fn avg_f64(rows: &[FleetPoint], f: impl Fn(&FleetPoint) -> Option<f64>) -> Option<f64> {
        let (mut sum, mut n) = (0.0, 0u64);
        for r in rows {
            if let Some(v) = f(r) {
                sum += v;
                n += 1;
            }
        }
        (n > 0).then_some(sum / n as f64)
    }
    fn avg_i64(rows: &[FleetPoint], f: impl Fn(&FleetPoint) -> Option<i64>) -> Option<i64> {
        let (mut sum, mut n) = (0i64, 0u64);
        for r in rows {
            if let Some(v) = f(r) {
                sum += v;
                n += 1;
            }
        }
        (n > 0).then_some((sum as f64 / n as f64).round() as i64)
    }
    fn max_i64(rows: &[FleetPoint], f: impl Fn(&FleetPoint) -> Option<i64>) -> Option<i64> {
        rows.iter().filter_map(|r| f(r)).max()
    }
    let bucket_secs = bucket_secs.max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let bucket = rows[i].ts.div_euclid(bucket_secs);
        let mut j = i + 1;
        while j < rows.len() && rows[j].ts.div_euclid(bucket_secs) == bucket {
            j += 1;
        }
        let chunk = &rows[i..j];
        out.push(FleetPoint {
            ts: bucket * bucket_secs,
            avail_1h: avg_f64(chunk, |r| r.avail_1h),
            avail_5h: avg_f64(chunk, |r| r.avail_5h),
            avail_1d: avg_f64(chunk, |r| r.avail_1d),
            avail_7d: avg_f64(chunk, |r| r.avail_7d),
            util5h: avg_f64(chunk, |r| r.util5h),
            util7d: avg_f64(chunk, |r| r.util7d),
            cap5h: avg_f64(chunk, |r| r.cap5h),
            cap7d: avg_f64(chunk, |r| r.cap7d),
            cons5h: avg_f64(chunk, |r| r.cons5h),
            cons7d: avg_f64(chunk, |r| r.cons7d),
            healthy: avg_i64(chunk, |r| r.healthy),
            cooling: avg_i64(chunk, |r| r.cooling),
            subs: avg_i64(chunk, |r| r.subs),
            balance_usd: avg_f64(chunk, |r| r.balance_usd),
            reserved_usd: avg_f64(chunk, |r| r.reserved_usd),
            spent_usd: avg_f64(chunk, |r| r.spent_usd),
            potential_realapi: avg_f64(chunk, |r| r.potential_realapi),
            coverage7d: avg_f64(chunk, |r| r.coverage7d),
            headroom5h: avg_f64(chunk, |r| r.headroom5h),
            headroom7d: avg_f64(chunk, |r| r.headroom7d),
            subs_needed: max_i64(chunk, |r| r.subs_needed),
            gap: max_i64(chunk, |r| r.gap),
        });
        i = j;
    }
    out
}

/// Бакетирование per-sub ряда: AVG по cap/util (уровни; для деградации ёмкости важен тренд,
/// а не минутный пик). NULL в AVG не участвует; бакет из одних NULL → None. ts — начало бакета.
pub fn bucket_subs(rows: &[SubPoint], bucket_secs: i64) -> Vec<SubPoint> {
    fn avg(rows: &[SubPoint], f: impl Fn(&SubPoint) -> Option<f64>) -> Option<f64> {
        let (mut sum, mut n) = (0.0, 0u64);
        for r in rows {
            if let Some(v) = f(r) {
                sum += v;
                n += 1;
            }
        }
        (n > 0).then_some(sum / n as f64)
    }
    let bucket_secs = bucket_secs.max(1);
    let mut out = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let bucket = rows[i].ts.div_euclid(bucket_secs);
        let mut j = i + 1;
        while j < rows.len() && rows[j].ts.div_euclid(bucket_secs) == bucket {
            j += 1;
        }
        let chunk = &rows[i..j];
        out.push(SubPoint {
            ts: bucket * bucket_secs,
            cap5h: avg(chunk, |r| r.cap5h),
            cap7d: avg(chunk, |r| r.cap7d),
            util5h: avg(chunk, |r| r.util5h),
            util7d: avg(chunk, |r| r.util7d),
        });
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_and_read_back() {
        let p = format!(
            "{}/metrics_test_{}.db",
            std::env::temp_dir().display(),
            std::process::id()
        );
        let _ = std::fs::remove_file(&p);
        let c = open(&p).unwrap();
        let o = serde_json::json!({
            "now": 1000, "subs": 3, "calibrated": true,
            "supply": {"avail_usd": {"1h": 10.0, "5h": 20.0, "1d": 30.0, "7d": 40.0},
                       "cap_usd": {"5h": 20.0, "7d": 100.0}, "consumed_usd": {"5h": 1.0, "7d": 5.0},
                       "util": {"5h": 0.05, "7d": 0.05}, "health": {"healthy": 2, "cooling": 1}},
            "demand": {"balance_usd": 500.0, "reserved_usd": 1.0, "spent_usd": 9.0,
                       "active_accounts": 4, "potential_realapi_usd": 2500.0},
            "headroom": {"5h": null, "7d": 8.0}, "coverage": {"7d": 62.5},
            "recommend": {"subs_needed": 1, "gap": -2}
        });
        insert_snapshot(&c, &o).unwrap();
        let (ts, subs, bal, h7, gap): (i64, i64, f64, Option<f64>, i64) = c
            .query_row(
                "SELECT ts,subs,balance_usd,headroom7d,gap FROM snapshots",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!((ts, subs, gap), (1000, 3, -2));
        assert_eq!(bal, 500.0);
        assert_eq!(h7, Some(8.0)); // headroom5h был null → NULL, 7d=8.0
        let _ = std::fs::remove_file(&p);
    }

    fn temp_db(tag: &str) -> String {
        let p = format!(
            "{}/metrics_test_{}_{}.db",
            std::env::temp_dir().display(),
            tag,
            std::process::id()
        );
        let _ = std::fs::remove_file(&p);
        p
    }

    fn snapshot_json(ts: i64, avail_5h: f64, gap: i64) -> serde_json::Value {
        serde_json::json!({
            "now": ts, "subs": 3, "calibrated": true,
            "supply": {"avail_usd": {"1h": 10.0, "5h": avail_5h, "1d": 30.0, "7d": 40.0},
                       "cap_usd": {"5h": 20.0, "7d": 100.0}, "consumed_usd": {"5h": 1.0, "7d": 5.0},
                       "util": {"5h": 0.05, "7d": 0.5}, "health": {"healthy": 2, "cooling": 1}},
            "demand": {"balance_usd": 500.0, "reserved_usd": 1.0, "spent_usd": 9.0,
                       "active_accounts": 4, "potential_realapi_usd": 2500.0},
            "headroom": {"5h": null, "7d": 8.0}, "coverage": {"7d": 62.5},
            "recommend": {"subs_needed": 1, "gap": gap}
        })
    }

    #[test]
    fn window_bucket_keeps_responses_under_500_points() {
        for (window, secs, bucket) in [
            ("24h", 86_400, 300),
            ("7d", 604_800, 1_800),
            ("30d", 2_592_000, 7_200),
            ("90d", 7_776_000, 21_600),
        ] {
            assert_eq!(window_bucket(window), Some((secs, bucket)), "{window}");
            assert!(secs / bucket <= 500, "{window} даёт слишком много точек");
        }
        assert_eq!(window_bucket(""), None);
        assert_eq!(window_bucket("3d"), None);
        assert_eq!(window_bucket("24H"), None);
    }

    #[test]
    fn bucket_fleet_averages_levels_and_keeps_worst_deficit() {
        let row = |ts, avail_5h, headroom5h, gap| FleetPoint {
            ts,
            avail_5h: Some(avail_5h),
            headroom5h,
            subs: Some(3),
            gap: Some(gap),
            subs_needed: Some(2),
            ..Default::default()
        };
        // Два снапшота в бакете 0 (bucket=300), один в бакете 2; бакет 1 пуст → точки для него нет.
        let out = bucket_fleet(
            &[
                row(0, 10.0, None, -1),
                row(120, 20.0, Some(4.0), 2),
                row(600, 30.0, None, 0),
            ],
            300,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].ts, 0);
        assert_eq!(out[0].avail_5h, Some(15.0)); // avg(10,20)
        assert_eq!(out[0].subs, Some(3));
        assert_eq!(out[0].gap, Some(2)); // max(-1,2): дефицит по худшей точке, не среднее
        assert_eq!(out[0].subs_needed, Some(2));
        assert_eq!(out[0].headroom5h, Some(4.0)); // NULL не участвует в avg
        assert_eq!(out[1].ts, 600);
        assert_eq!(out[1].headroom5h, None); // бакет из одних NULL → None («∞»)
                                             // Пустой вход → пустой выход.
        assert!(bucket_fleet(&[], 300).is_empty());
    }

    #[test]
    fn bucket_subs_averages_cap_and_util() {
        let row = |ts, cap7d, util7d| SubPoint {
            ts,
            cap5h: Some(10.0),
            cap7d: Some(cap7d),
            util5h: Some(0.2),
            util7d: Some(util7d),
        };
        let out = bucket_subs(&[row(0, 100.0, 0.4), row(1800, 80.0, 0.6)], 1800);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].cap7d, Some(100.0));
        let out = bucket_subs(&[row(0, 100.0, 0.4), row(60, 80.0, 0.6)], 1800);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].ts, 0);
        assert_eq!(out[0].cap7d, Some(90.0));
        assert_eq!(out[0].util7d, Some(0.5));
    }

    #[test]
    fn fleet_history_reads_window_and_buckets() {
        let p = temp_db("fleet_history");
        let c = open(&p).unwrap();
        // Три минутных снапшота подряд + один далеко за окном.
        for ts in [10_000, 10_060, 10_120] {
            insert_snapshot(&c, &snapshot_json(ts, 20.0, -2)).unwrap();
        }
        insert_snapshot(&c, &snapshot_json(1_000, 99.0, 5)).unwrap();
        let points = fleet_history(&c, 9_000, 300).unwrap();
        assert_eq!(
            points.len(),
            1,
            "три минутных снапшота склеиваются в один 5-мин бакет"
        );
        assert_eq!(points[0].ts, 9_900);
        assert_eq!(points[0].avail_5h, Some(20.0));
        assert_eq!(points[0].balance_usd, Some(500.0));
        assert_eq!(points[0].gap, Some(-2));
        assert_eq!(points[0].healthy, Some(2));
        // Снапшот вне окна не попадает.
        assert!(points.iter().all(|pt| pt.avail_5h == Some(20.0)));
        // Пустая история → пустой ряд, а не ошибка.
        let c2 = open(&temp_db("fleet_history_empty")).unwrap();
        assert!(fleet_history(&c2, 0, 300).unwrap().is_empty());
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn sub_history_matches_email_prefix_only() {
        let p = temp_db("sub_history");
        let c = open(&p).unwrap();
        insert_sub_snapshots(
            &c,
            10_000,
            &[("alpha@example.com".into(), 10.0, 100.0, 0.2, 0.4)],
        )
        .unwrap();
        insert_sub_snapshots(
            &c,
            10_060,
            &[("alpha@example.com".into(), 12.0, 80.0, 0.3, 0.6)],
        )
        .unwrap();
        insert_sub_snapshots(
            &c,
            10_000,
            &[("beta@example.com".into(), 99.0, 999.0, 0.9, 0.9)],
        )
        .unwrap();
        // Префикс маски «alph…» выбирает только свою подписку и бакетует две минуты в одну точку.
        let points = sub_history(&c, "alph", 0, 300).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].cap7d, Some(90.0)); // avg(100,80)
        assert_eq!(points[0].util7d, Some(0.5));
        // Чужой префикс → пустой ряд; метасимволы GLOB в префиксе — литералы, не wildcards.
        assert!(sub_history(&c, "zzzz", 0, 300).unwrap().is_empty());
        assert!(sub_history(&c, "*", 0, 300).unwrap().is_empty());
        let _ = std::fs::remove_file(&p);
    }
}
