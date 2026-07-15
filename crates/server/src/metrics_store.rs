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
            headroom5h REAL, headroom7d REAL, subs_needed INTEGER, gap INTEGER);",
    )?;
    Ok(c)
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
            i("/demand/active_keys"), f("/demand/potential_realapi_usd"), f("/coverage/7d"),
            f("/headroom/5h"), f("/headroom/7d"), i("/recommend/subs_needed"), i("/recommend/gap"),
        ],
    )?;
    Ok(())
}

/// Обрезать снапшоты старше retention (metrics.db не растёт вечно). Снапшоты редки → без батчинга.
pub fn prune(c: &Connection, older_than_ts: i64) -> rusqlite::Result<usize> {
    c.execute("DELETE FROM snapshots WHERE ts < ?1", rusqlite::params![older_than_ts])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn insert_and_read_back() {
        let p = format!("{}/metrics_test_{}.db", std::env::temp_dir().display(), std::process::id());
        let _ = std::fs::remove_file(&p);
        let c = open(&p).unwrap();
        let o = serde_json::json!({
            "now": 1000, "subs": 3, "calibrated": true,
            "supply": {"avail_usd": {"1h": 10.0, "5h": 20.0, "1d": 30.0, "7d": 40.0},
                       "cap_usd": {"5h": 20.0, "7d": 100.0}, "consumed_usd": {"5h": 1.0, "7d": 5.0},
                       "util": {"5h": 0.05, "7d": 0.05}, "health": {"healthy": 2, "cooling": 1}},
            "demand": {"balance_usd": 500.0, "reserved_usd": 1.0, "spent_usd": 9.0,
                       "active_keys": 4, "potential_realapi_usd": 2500.0},
            "headroom": {"5h": null, "7d": 8.0}, "coverage": {"7d": 62.5},
            "recommend": {"subs_needed": 1, "gap": -2}
        });
        insert_snapshot(&c, &o).unwrap();
        let (ts, subs, bal, h7, gap): (i64, i64, f64, Option<f64>, i64) = c.query_row(
            "SELECT ts,subs,balance_usd,headroom7d,gap FROM snapshots",
            [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).unwrap();
        assert_eq!((ts, subs, gap), (1000, 3, -2));
        assert_eq!(bal, 500.0);
        assert_eq!(h7, Some(8.0));   // headroom5h был null → NULL, 7d=8.0
        let _ = std::fs::remove_file(&p);
    }
}
