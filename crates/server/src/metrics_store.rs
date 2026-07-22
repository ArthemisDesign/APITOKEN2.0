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
}
