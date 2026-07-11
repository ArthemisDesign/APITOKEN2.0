//! Реестр подписок (пункт 1) — SQLite. Источник истины: таблица `subs`.
//! Совместим с исторической subscriptions.db (мягкая миграция недостающих колонок).
//! Для форвардинг-прокси подписке нужны только: OAuth-токен + прокси (+ флот/статус).
//! Токен берём из колонки `token` (inline) либо из файла `token_file`.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;

/// Рантайм-запись подписки с УЖЕ разрешённым токеном (inline или из файла).
#[derive(Clone, Debug)]
pub struct Sub {
    pub email: String,
    pub token: String,   // OAuth Bearer подписки (секрет)
    pub proxy: String,   // http://user:pass@ip:port ("" = без прокси)
    pub fleet: String,
}

const COLS: &[(&str, &str)] = &[
    ("email", "TEXT PRIMARY KEY"),
    ("token", "TEXT"),
    ("token_file", "TEXT"),
    ("proxy", "TEXT"),
    ("status", "TEXT"),
    ("fleet", "TEXT"),
    ("added_ts", "INTEGER"),
    ("added", "TEXT"),
];

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() { let _ = fs::create_dir_all(dir); }
    }
    let c = Connection::open(path).with_context(|| format!("открыть БД {path}"))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS subs(email TEXT PRIMARY KEY, token TEXT, token_file TEXT, \
         proxy TEXT, status TEXT DEFAULT 'active', fleet TEXT DEFAULT 'prod', \
         added_ts INTEGER, added TEXT)",
        [],
    )?;
    // мягкая миграция: доливаем недостающие колонки в существующую (историческую) таблицу
    for (name, ty) in COLS {
        let _ = c.execute(&format!("ALTER TABLE subs ADD COLUMN {name} {ty}"), []);
    }
    Ok(c)
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

fn resolve_token(inline: Option<String>, token_file: Option<String>) -> String {
    if let Some(t) = inline { let t = t.trim().to_string(); if !t.is_empty() { return t; } }
    if let Some(f) = token_file {
        if !f.trim().is_empty() {
            if let Ok(s) = fs::read_to_string(f.trim()) { return s.trim().to_string(); }
        }
    }
    String::new()
}

/// Активные подписки нужного флота, у которых есть непустой токен.
pub fn load_active(conn: &Connection, fleet: Option<&str>) -> Result<Vec<Sub>> {
    let mut stmt = conn.prepare(
        "SELECT email, token, token_file, proxy, COALESCE(status,'active'), COALESCE(fleet,'prod') \
         FROM subs",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (email, token, token_file, proxy, status, sfleet) = row?;
        if status != "active" { continue; }
        if let Some(f) = fleet { if f != sfleet { continue; } }
        let tok = resolve_token(token, token_file);
        if tok.is_empty() { continue; }
        out.push(Sub { email, token: tok, proxy: proxy.unwrap_or_default(), fleet: sfleet });
    }
    Ok(out)
}

// ── CLI-операции реестра ────────────────────────────────────────────────────
pub fn add(conn: &Connection, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, proxy, status, fleet, added_ts, added) \
         VALUES(?1, ?2, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=excluded.token, proxy=excluded.proxy, \
         status='active', fleet=excluded.fleet",
        rusqlite::params![
            email, token, proxy, fleet, now(),
            chrono_like(now())
        ],
    )?;
    Ok(())
}

pub fn add_file(conn: &Connection, email: &str, token_file: &str, proxy: &str, fleet: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, ?2, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token_file=excluded.token_file, proxy=excluded.proxy, \
         status='active', fleet=excluded.fleet",
        rusqlite::params![email, token_file, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, email: &str, status: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE subs SET status=?1 WHERE email=?2", rusqlite::params![status, email])?)
}
pub fn set_proxy(conn: &Connection, email: &str, proxy: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE subs SET proxy=?1 WHERE email=?2", rusqlite::params![proxy, email])?)
}
pub fn set_fleet(conn: &Connection, email: &str, fleet: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE subs SET fleet=?1 WHERE email=?2", rusqlite::params![fleet, email])?)
}
pub fn remove(conn: &Connection, email: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM subs WHERE email=?1", rusqlite::params![email])?)
}

/// Список для CLI (без утечки токена — только флаг наличия).
pub fn list(conn: &Connection) -> Result<Vec<(String, String, String, bool, String)>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,'') \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?.map(|s| !s.is_empty()).unwrap_or(false),
            r.get::<_, String>(4)?,
        ))
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Простая UTC-строка YYYY-MM-DD HH:MM без внешних крейтов (для колонки `added`).
fn chrono_like(ts: i64) -> String {
    // дней от эпохи → григорианская дата (алгоритм Howard Hinnant)
    let days = ts.div_euclid(86400);
    let secs = ts.rem_euclid(86400);
    let (h, mi) = (secs / 3600, (secs % 3600) / 60);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
}
