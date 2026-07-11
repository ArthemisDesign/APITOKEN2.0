//! # registry — реестр подписок (пункт 1)
//!
//! Источник истины пула: таблица `subs` в SQLite. Для форвардинг-прокси подписке нужны
//! только OAuth-токен + прокси (+ статус/флот). Токен берётся из колонки `token` (inline)
//! либо из файла `token_file`. Совместим с исторической subscriptions.db (мягкая миграция).
//!
//! **Границы крейта:** только хранение/чтение подписок. НИКАКОЙ сети, HTTP, логики пула.
//! Зависит лишь от rusqlite/anyhow. Ниже по стеку зависеть не от кого.

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::fs;
use std::sync::Mutex;

/// Рантайм-запись подписки с УЖЕ разрешённым токеном (inline или из файла).
#[derive(Clone, Debug)]
pub struct Sub {
    pub email: String,
    pub token: String, // OAuth Bearer подписки (секрет)
    pub proxy: String, // http://user:pass@ip:port ("" = без прокси)
    pub fleet: String,
}

const COLS: &[(&str, &str)] = &[
    ("email", "TEXT PRIMARY KEY"),
    ("token", "TEXT"),
    ("token_file", "TEXT"),
    ("proxy", "TEXT"),
    ("plan", "TEXT"),
    ("status", "TEXT"),
    ("fleet", "TEXT"),
    ("added_ts", "INTEGER"),
    ("added", "TEXT"),
];

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = fs::create_dir_all(dir);
        }
    }
    let c = Connection::open(path).with_context(|| format!("открыть БД {path}"))?;
    c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS subs(email TEXT PRIMARY KEY, token TEXT, token_file TEXT, \
         proxy TEXT, plan TEXT DEFAULT '', status TEXT DEFAULT 'active', fleet TEXT DEFAULT 'prod', \
         added_ts INTEGER, added TEXT)",
        [],
    )?;
    // мягкая миграция: доливаем недостающие колонки в существующую (историческую) таблицу
    for (name, ty) in COLS {
        let _ = c.execute(&format!("ALTER TABLE subs ADD COLUMN {name} {ty}"), []);
    }
    // Биллинг: ключи клиентов с балансом в нанодолларах (1 USD = 1e9 нано). i64 INTEGER
    // вмещает до ~$9.2 млрд — с запасом. balance может уйти в минус (тогда ключ блокируется).
    c.execute(
        "CREATE TABLE IF NOT EXISTS api_keys(key TEXT PRIMARY KEY, balance_nano INTEGER NOT NULL DEFAULT 0, \
         spent_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 900, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT)",
        [],
    )?;
    Ok(c)
}

fn now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn resolve_token(inline: Option<String>, token_file: Option<String>) -> String {
    if let Some(t) = inline {
        let t = t.trim().to_string();
        if !t.is_empty() {
            return t;
        }
    }
    if let Some(f) = token_file {
        if !f.trim().is_empty() {
            if let Ok(s) = fs::read_to_string(f.trim()) {
                return s.trim().to_string();
            }
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
        if status != "active" {
            continue;
        }
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        let tok = resolve_token(token, token_file);
        if tok.is_empty() {
            continue;
        }
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
        rusqlite::params![email, token, proxy, fleet, now(), chrono_like(now())],
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
pub fn set_plan(conn: &Connection, email: &str, plan: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE subs SET plan=?1 WHERE email=?2", rusqlite::params![plan, email])?)
}

/// (разрешённый токен, proxy) для одной подписки (любого статуса) — для детекта тарифа.
pub fn get_creds(conn: &Connection, email: &str) -> Result<Option<(String, String)>> {
    let row = conn.query_row(
        "SELECT token, token_file, proxy FROM subs WHERE email=?1",
        rusqlite::params![email],
        |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, Option<String>>(2)?)),
    );
    match row {
        Ok((token, token_file, proxy)) => {
            let tok = resolve_token(token, token_file);
            if tok.is_empty() { Ok(None) } else { Ok(Some((tok, proxy.unwrap_or_default()))) }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
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

/// Удалить ВСЕ подписки (опционально только одного флота). Возвращает число удалённых.
pub fn clear(conn: &Connection, fleet: Option<&str>) -> Result<usize> {
    Ok(match fleet {
        Some(f) => conn.execute("DELETE FROM subs WHERE COALESCE(fleet,'prod')=?1", rusqlite::params![f])?,
        None => conn.execute("DELETE FROM subs", [])?,
    })
}

/// Строка списка для CLI (без утечки токена — только флаг наличия).
pub struct SubRow {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub plan: String,
    pub has_token: bool,
    pub proxy: String,
}

pub fn list(conn: &Connection) -> Result<Vec<SubRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), COALESCE(plan,''), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,'') \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(SubRow {
            email: r.get::<_, String>(0)?,
            status: r.get::<_, String>(1)?,
            fleet: r.get::<_, String>(2)?,
            plan: r.get::<_, String>(3)?,
            has_token: r.get::<_, Option<String>>(4)?.map(|s| !s.is_empty()).unwrap_or(false),
            proxy: r.get::<_, String>(5)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

// ── Биллинг: ключи клиентов с USD-балансом (нанодоллары) ─────────────────────
//
// Слой хранения: только персист+CRUD баланса. САМ подсчёт стоимости (токены→нано) —
// в крейте `metering`; сюда приходит уже готовая сумма списания в нано. Границы держим:
// registry не знает про цены/токены, только про целые нанодоллары на ключе.

/// Строка ключа (баланс/потрачено — в нанодолларах; mult_bp — наценка × 10000).
#[derive(Clone, Debug)]
pub struct KeyRow {
    pub key: String,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub mult_bp: i64,
    pub status: String,
}

/// Выпустить/перевыпустить ключ с балансом (перевыпуск СБРАСЫВАЕТ баланс на новый).
pub fn key_issue(conn: &Connection, key: &str, balance_nano: i64, mult_bp: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO api_keys(key, balance_nano, spent_nano, mult_bp, status, created_ts, created) \
         VALUES(?1, ?2, 0, ?3, 'active', ?4, ?5) \
         ON CONFLICT(key) DO UPDATE SET balance_nano=excluded.balance_nano, mult_bp=excluded.mult_bp, status='active'",
        rusqlite::params![key, balance_nano, mult_bp, now(), chrono_like(now())],
    )?;
    Ok(())
}

/// Пополнить баланс (add_nano может быть отрицательным для ручной коррекции).
/// Возвращает новый баланс или None, если ключа нет.
pub fn key_topup(conn: &Connection, key: &str, add_nano: i64) -> Result<Option<i64>> {
    let n = conn.execute(
        "UPDATE api_keys SET balance_nano = balance_nano + ?1 WHERE key = ?2",
        rusqlite::params![add_nano, key],
    )?;
    if n == 0 { return Ok(None); }
    Ok(Some(conn.query_row(
        "SELECT balance_nano FROM api_keys WHERE key=?1",
        rusqlite::params![key], |r| r.get::<_, i64>(0))?))
}

/// Списать стоимость запроса: баланс −charge, потрачено +charge (одной атомарной командой).
/// Возвращает новый баланс или None, если ключа нет. Баланс может уйти в минус — тогда
/// следующий запрос отбивается 402 (гонка двух запросов на грани нуля даёт лёгкий минус).
pub fn key_deduct(conn: &Connection, key: &str, charge_nano: i64) -> Result<Option<i64>> {
    let n = conn.execute(
        "UPDATE api_keys SET balance_nano = balance_nano - ?1, spent_nano = spent_nano + ?1 WHERE key = ?2",
        rusqlite::params![charge_nano, key],
    )?;
    if n == 0 { return Ok(None); }
    Ok(Some(conn.query_row(
        "SELECT balance_nano FROM api_keys WHERE key=?1",
        rusqlite::params![key], |r| r.get::<_, i64>(0))?))
}

/// Прочитать ключ (для авторизации/`/balance`).
pub fn key_get(conn: &Connection, key: &str) -> Result<Option<KeyRow>> {
    let row = conn.query_row(
        "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') FROM api_keys WHERE key=?1",
        rusqlite::params![key],
        |r| Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            balance_nano: r.get::<_, i64>(1)?,
            spent_nano: r.get::<_, i64>(2)?,
            mult_bp: r.get::<_, i64>(3)?,
            status: r.get::<_, String>(4)?,
        }),
    );
    match row {
        Ok(k) => Ok(Some(k)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn key_set_status(conn: &Connection, key: &str, status: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE api_keys SET status=?1 WHERE key=?2", rusqlite::params![status, key])?)
}

/// Все ключи (для CLI-листинга; ключ маскируется на стороне вывода).
pub fn key_list(conn: &Connection) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') \
         FROM api_keys ORDER BY COALESCE(created_ts,0)")?;
    let rows = stmt.query_map([], |r| Ok(KeyRow {
        key: r.get::<_, String>(0)?,
        balance_nano: r.get::<_, i64>(1)?,
        spent_nano: r.get::<_, i64>(2)?,
        mult_bp: r.get::<_, i64>(3)?,
        status: r.get::<_, String>(4)?,
    }))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Обёртка для запросного пути сервера: одно долгоживущее соединение под мьютексом.
/// Списание идёт синхронно (быстрый UPDATE по WAL, без удержания через await).
pub struct Billing {
    conn: Mutex<Connection>,
}

impl Billing {
    pub fn open(path: &str) -> Result<Billing> {
        Ok(Billing { conn: Mutex::new(open(path)?) })
    }
    /// Прочитать ключ (None при ошибке/отсутствии).
    pub fn get(&self, key: &str) -> Option<KeyRow> {
        self.conn.lock().ok().and_then(|c| key_get(&c, key).ok().flatten())
    }
    /// Списать сумму, вернуть новый баланс (None если ключа нет/ошибка).
    pub fn deduct(&self, key: &str, charge_nano: i64) -> Option<i64> {
        self.conn.lock().ok().and_then(|c| key_deduct(&c, key, charge_nano).ok().flatten())
    }
}

/// Простая UTC-строка YYYY-MM-DD HH:MM без внешних крейтов (для колонки `added`).
fn chrono_like(ts: i64) -> String {
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
