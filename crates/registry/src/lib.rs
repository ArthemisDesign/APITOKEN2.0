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

/// Рантайм-запись подписки с УЖЕ разрешённым токеном (inline или из файла).
#[derive(Clone, Debug)]
pub struct Sub {
    pub email: String,
    pub token: String, // OAuth Bearer подписки (секрет)
    pub proxy: String, // http://user:pass@ip:port ("" = без прокси)
    pub fleet: String,
    pub plan: String,  // pro|max5|max20 (детект) — для per-sub прайора ёмкости в pool ("" = неизвестно)
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
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT, \
         reserved_nano INTEGER NOT NULL DEFAULT 0)",
        [],
    )?;
    // Мягкая миграция: колонка учёта незакрытых резервов (леджер крах-безопасности).
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN reserved_nano INTEGER NOT NULL DEFAULT 0", []);

    // АККАУНТЫ клиентов: ЕДИНЫЙ баланс на профиль; ключи (api_keys) — доступы к нему (1:N).
    // Баланс/резерв/наценка живут ЗДЕСЬ, не на ключе. Ключ теперь несёт account_id + label +
    // per-key spent (атрибуция расхода по ключу без разделения баланса).
    c.execute(
        "CREATE TABLE IF NOT EXISTS accounts(id TEXT PRIMARY KEY, handle TEXT, \
         balance_nano INTEGER NOT NULL DEFAULT 0, spent_nano INTEGER NOT NULL DEFAULT 0, \
         reserved_nano INTEGER NOT NULL DEFAULT 0, mult_bp INTEGER NOT NULL DEFAULT 2000, \
         status TEXT NOT NULL DEFAULT 'active', created_ts INTEGER, created TEXT)",
        [],
    )?;
    // handle (внешняя идентичность: TG id / email) уникален, когда задан.
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS accounts_handle ON accounts(handle) WHERE handle IS NOT NULL", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN account_id TEXT", []);
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN label TEXT", []);
    let _ = c.execute("CREATE INDEX IF NOT EXISTS api_keys_account ON api_keys(account_id)", []);

    // ЛЕДЖЕР: append-only история движений баланса (пополнения/списания/возвраты) — для точного
    // учёта, споров и дашбордов. Текущий баланс = accounts.balance_nano; ledger — журнал КАК он менялся.
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
         key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
         balance_after_nano INTEGER, ts INTEGER)",
        [],
    )?;
    let _ = c.execute("CREATE INDEX IF NOT EXISTS ledger_acct ON ledger(account_id, id)", []);
    // ИДЕМПОТЕНТНОСТЬ пополнений: payment-ref уникален среди topup-строк → повторный вебхук с тем же
    // tx-id физически не начислит дважды (партиальный UNIQUE; на charge/adjust не распространяется).
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_topup_ref ON ledger(ref) \
         WHERE kind='topup' AND ref IS NOT NULL", []);

    // Миграция старой модели (key=кошелёк): ключам без account_id заводим аккаунт и переносим баланс.
    migrate_legacy_keys(&c)?;
    // Персист волатильного состояния пула (переживание рестарта): cooling (бан на дни не должен
    // забываться при деплое) + калибровка ёмкости (дорого переучивать) + spent/util/reset.
    c.execute(
        "CREATE TABLE IF NOT EXISTS pool_state(email TEXT PRIMARY KEY, cooling_until INTEGER, \
         cap5h REAL, cap7d REAL, spent_total REAL, util5 REAL, util7 REAL, \
         reset5 INTEGER, reset7 INTEGER, calib_n INTEGER, updated_ts INTEGER)",
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

/// Миграция старой модели «ключ = кошелёк» → «аккаунт (баланс) + ключи (доступы)». Для каждого ключа
/// без `account_id` (легаси) заводим отдельный аккаунт, переносим баланс/расход/наценку, линкуем ключ.
/// Идемпотентно (INSERT OR IGNORE + фильтр по NULL). В проде ключей нет → no-op, но безопасно вообще.
fn migrate_legacy_keys(c: &Connection) -> Result<()> {
    let legacy: Vec<(String, i64, i64, i64, String)> = {
        let mut stmt = c.prepare(
            "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') \
             FROM api_keys WHERE account_id IS NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    for (key, bal, spent, mult, status) in legacy {
        // детерминированный id аккаунта из ключа (хвост) — миграция повторяема без дублей
        let tail: String = key.chars().rev().take(12).collect::<String>().chars().rev().collect();
        let acct = format!("acct_{tail}");
        c.execute(
            "INSERT OR IGNORE INTO accounts(id, balance_nano, spent_nano, mult_bp, status, created_ts, created) \
             VALUES(?1,?2,?3,?4,?5,?6,?7)",
            rusqlite::params![acct, bal, spent, mult, status, now(), chrono_like(now())])?;
        c.execute("UPDATE api_keys SET account_id=?1 WHERE key=?2 AND account_id IS NULL",
                  rusqlite::params![acct, key])?;
    }
    Ok(())
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
        "SELECT email, token, token_file, proxy, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(plan,'') FROM subs",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (email, token, token_file, proxy, status, sfleet, plan) = row?;
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
        out.push(Sub { email, token: tok, proxy: proxy.unwrap_or_default(), fleet: sfleet, plan });
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

/// Строка ключа. Баланс — НЕ здесь (он на аккаунте); ключ = доступ + метка + атрибуция расхода.
#[derive(Clone, Debug)]
pub struct KeyRow {
    pub key: String,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub spent_nano: i64, // расход по ЭТОМУ ключу (атрибуция; баланс общий на аккаунте)
    pub status: String,
}

/// Выпустить ключ ПОД аккаунт (баланс — на аккаунте, ключ лишь ссылается). `label` — имя ключа.
pub fn key_issue(conn: &Connection, key: &str, account_id: &str, label: Option<&str>) -> Result<()> {
    conn.execute(
        "INSERT INTO api_keys(key, account_id, label, spent_nano, status, created_ts, created) \
         VALUES(?1, ?2, ?3, 0, 'active', ?4, ?5) \
         ON CONFLICT(key) DO UPDATE SET account_id=excluded.account_id, label=excluded.label, status='active'",
        rusqlite::params![key, account_id, label, now(), chrono_like(now())],
    )?;
    Ok(())
}

/// Реконсиляция незакрытых резервов на СТАРТЕ. Краш процесса между `reserve` и `settle` оставляет
/// hold вычтенным из баланса, но `reserved_nano` его помнит. На свежем старте запросов в полёте нет,
/// поэтому любой ненулевой `reserved_nano` = осиротевший hold → возвращаем в баланс. Возвращает
/// число ключей, которым вернули резерв. Вызывать ОДИН раз до начала обслуживания.
pub fn reconcile_reservations(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET balance_nano = balance_nano + reserved_nano, reserved_nano = 0 \
         WHERE reserved_nano != 0",
        [],
    )?)
}

// ── Аккаунты (профиль клиента: ЕДИНЫЙ баланс, N ключей-доступов) ─────────────────────

/// Строка аккаунта. Баланс/резерв/наценка — ЗДЕСЬ (не на ключе). `handle` — внешняя идентичность.
#[derive(Clone, Debug)]
pub struct AccountRow {
    pub id: String,
    pub handle: Option<String>,
    pub balance_nano: i64,
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub mult_bp: i64,
    pub status: String,
}

/// Резолв ключа → аккаунт (горячий путь авторизации). Активны должны быть И ключ, И аккаунт.
#[derive(Clone, Debug)]
pub struct KeyAuth {
    pub account_id: String,
    pub mult_bp: i64,
    pub balance_nano: i64,
    pub active: bool, // ключ активен И аккаунт активен
}

/// Создать аккаунт. `handle` (TG id/email) опционален и уникален (когда задан).
pub fn account_create(conn: &Connection, id: &str, handle: Option<&str>, mult_bp: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO accounts(id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, status, created_ts, created) \
         VALUES(?1, ?2, 0, 0, 0, ?3, 'active', ?4, ?5)",
        rusqlite::params![id, handle, mult_bp, now(), chrono_like(now())])?;
    Ok(())
}

pub fn account_get(conn: &Connection, id: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "id", id)
}
/// Найти аккаунт по внешней идентичности (для входа юзера из TG/web).
pub fn account_by_handle(conn: &Connection, handle: &str) -> Result<Option<AccountRow>> {
    one_account(conn, "handle", handle)
}
fn one_account(conn: &Connection, col: &str, val: &str) -> Result<Option<AccountRow>> {
    let sql = format!(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts WHERE {col}=?1");
    match conn.query_row(&sql, rusqlite::params![val], |r| Ok(AccountRow {
        id: r.get(0)?, handle: r.get(1)?, balance_nano: r.get(2)?, spent_nano: r.get(3)?,
        reserved_nano: r.get(4)?, mult_bp: r.get(5)?, status: r.get(6)?,
    })) {
        Ok(a) => Ok(Some(a)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Список аккаунтов (для админ-CLI).
pub fn account_list(conn: &Connection) -> Result<Vec<AccountRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, handle, balance_nano, spent_nano, reserved_nano, mult_bp, COALESCE(status,'active') \
         FROM accounts ORDER BY COALESCE(created_ts,0)")?;
    let rows = stmt.query_map([], |r| Ok(AccountRow {
        id: r.get(0)?, handle: r.get(1)?, balance_nano: r.get(2)?, spent_nano: r.get(3)?,
        reserved_nano: r.get(4)?, mult_bp: r.get(5)?, status: r.get(6)?,
    }))?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn account_set_status(conn: &Connection, id: &str, status: &str) -> Result<usize> {
    Ok(conn.execute("UPDATE accounts SET status=?1 WHERE id=?2", rusqlite::params![status, id])?)
}

/// Удалить аккаунт НАВСЕГДА вместе с его ключами и ledger (каскад, одной транзакцией).
pub fn account_remove(conn: &Connection, id: &str) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM api_keys WHERE account_id=?1", rusqlite::params![id])?;
    tx.execute("DELETE FROM ledger WHERE account_id=?1", rusqlite::params![id])?;
    let n = tx.execute("DELETE FROM accounts WHERE id=?1", rusqlite::params![id])?;
    tx.commit()?;
    Ok(n)
}

/// Пополнить баланс аккаунта (`amount` может быть отрицательным = коррекция) + запись в ledger.
/// Возвращает новый баланс или None, если аккаунта нет. Атомарно (UPDATE…RETURNING + ledger).
pub fn account_topup(conn: &Connection, id: &str, amount_nano: i64, reference: Option<&str>) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    // Начисляем баланс...
    let bal = match tx.query_row(
        "UPDATE accounts SET balance_nano = balance_nano + ?1 WHERE id = ?2 RETURNING balance_nano",
        rusqlite::params![amount_nano, id], |r| r.get::<_, i64>(0)) {
        Ok(b) => b,
        Err(rusqlite::Error::QueryReturnedNoRows) => { return Ok(None); } // нет аккаунта
        Err(e) => return Err(e.into()),
    };
    let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
    // ...и пишем ledger. ИДЕМПОТЕНТНОСТЬ: если payment-ref (topup) уже был — UNIQUE-индекс отклонит
    // INSERT → откатываем НАЧИСЛЕНИЕ и возвращаем текущий баланс (повторный вебхук не задваивает).
    match tx.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts) \
         VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, kind, amount_nano, reference, bal, now()]) {
        Ok(_) => { tx.commit()?; Ok(Some(bal)) }
        Err(rusqlite::Error::SqliteFailure(e, _)) if e.code == rusqlite::ErrorCode::ConstraintViolation => {
            drop(tx); // ROLLBACK: начисление отменено
            eprintln!("↩ topup идемпотентно пропущен (ref уже начислен): {}", reference.unwrap_or("?"));
            Ok(account_get(conn, id)?.map(|a| a.balance_nano))
        }
        Err(e) => Err(e.into()),
    }
}

/// АТОМАРНО зарезервировать `hold` по АККАУНТУ, если баланс покрывает и аккаунт активен. Та же
/// семантика, что была на ключе, но кошелёк — общий на профиль (все ключи юзера тратят из него).
pub fn account_reserve(conn: &Connection, id: &str, hold_nano: i64) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    match conn.query_row(
        "UPDATE accounts SET balance_nano = balance_nano - ?1, reserved_nano = reserved_nano + ?1 \
         WHERE id = ?2 AND status = 'active' AND balance_nano >= ?1 RETURNING balance_nano",
        rusqlite::params![hold, id], |r| r.get::<_, i64>(0)) {
        Ok(bal) => Ok(Some(bal)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Закрыть резерв аккаунта: баланс += hold − actual, spent += actual, reserved −= hold; per-key
/// `spent` += actual (атрибуция расхода по ключу); строка в ledger (charge). ВСЁ в ОДНОЙ транзакции.
pub fn account_settle(conn: &Connection, id: &str, key: &str, hold_nano: i64, actual_nano: i64,
                      reference: Option<&str>) -> Result<Option<i64>> {
    let (hold, actual) = (hold_nano.max(0), actual_nano.max(0));
    let tx = conn.unchecked_transaction()?;
    // Возвращаем hold, но НЕ БОЛЬШЕ, чем реально числится в reserved_nano: MIN(hold, reserved).
    // Защита от двойного settle (перекрытие деплоя: reconcile уже вернул резерв, затем прилетел
    // settle старого инстанса) — иначе balance получил бы +hold дважды (over-credit) и reserved
    // ушёл бы в минус. MAX(0, …) держит reserved ≥ 0. В норме (reserved≥hold) поведение прежнее.
    let bal = match tx.query_row(
        "UPDATE accounts SET \
         balance_nano = balance_nano + MIN(?1, reserved_nano) - ?2, \
         spent_nano = spent_nano + ?2, \
         reserved_nano = MAX(0, reserved_nano - ?1) WHERE id = ?3 RETURNING balance_nano",
        rusqlite::params![hold, actual, id], |r| r.get::<_, i64>(0)) {
        Ok(b) => Some(b),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(b) = bal {
        tx.execute("UPDATE api_keys SET spent_nano = spent_nano + ?1 WHERE key = ?2",
                   rusqlite::params![actual, key])?;
        if actual > 0 { ledger_add(&tx, id, Some(key), "charge", actual, reference, b)?; }
    }
    tx.commit()?;
    Ok(bal)
}

/// Резолв ключа в аккаунт для авторизации запроса (JOIN api_keys→accounts).
pub fn key_account(conn: &Connection, key: &str) -> Result<Option<KeyAuth>> {
    match conn.query_row(
        "SELECT a.id, a.mult_bp, a.balance_nano, \
         (COALESCE(k.status,'active')='active' AND COALESCE(a.status,'active')='active') \
         FROM api_keys k JOIN accounts a ON a.id = k.account_id WHERE k.key = ?1",
        rusqlite::params![key],
        |r| Ok(KeyAuth {
            account_id: r.get(0)?, mult_bp: r.get(1)?, balance_nano: r.get(2)?,
            active: r.get::<_, i64>(3)? != 0,
        })) {
        Ok(a) => Ok(Some(a)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Консистентный ОНЛАЙН-бэкап всей БД в `out_path` через `VACUUM INTO` (best-practice для живого
/// SQLite): создаёт целостный снимок, безопасный при WAL и параллельной работе. НЕЛЬЗЯ просто
/// копировать `.db` — без `-wal`/`-shm` копия рассинхронизирована/битая. `out_path` должен НЕ
/// существовать (VACUUM INTO создаёт файл). Восстановление: остановить сервис, положить снимок на
/// место `subscriptions.db`, удалить `-wal`/`-shm`, запустить.
pub fn backup_to(conn: &Connection, out_path: &str) -> Result<()> {
    let esc = out_path.replace('\'', "''"); // путь наш, но экранируем кавычку
    conn.execute_batch(&format!("VACUUM INTO '{esc}'"))?;
    Ok(())
}

/// Обрезать ledger под масштаб: удалить bulk-строки списаний (`charge`) старше `older_than_ts`.
/// Редкие финансовые события (`topup`/`adjust` — пополнения/коррекции) НЕ трогаем: их немного, а
/// история «кто сколько занёс» важна для споров. Текущие суммы (`accounts.spent_nano`/per-key) —
/// это отдельный монотонный источник истины, обрезка ledger их не затрагивает. Возвращает удалённое.
pub fn ledger_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM ledger WHERE kind='charge' AND ts < ?1",
        rusqlite::params![older_than_ts])?)
}

/// Добавить строку в append-only ledger (журнал движений баланса).
fn ledger_add(conn: &Connection, account_id: &str, key: Option<&str>, kind: &str,
              amount_nano: i64, reference: Option<&str>, balance_after: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![account_id, key, kind, amount_nano, reference, balance_after, now()])?;
    Ok(())
}

/// Прочитать ключ (для авторизации/`/balance`).
pub fn key_get(conn: &Connection, key: &str) -> Result<Option<KeyRow>> {
    let row = conn.query_row(
        "SELECT key, account_id, label, spent_nano, COALESCE(status,'active') FROM api_keys WHERE key=?1",
        rusqlite::params![key],
        |r| Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            account_id: r.get::<_, Option<String>>(1)?,
            label: r.get::<_, Option<String>>(2)?,
            spent_nano: r.get::<_, i64>(3)?,
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

/// Удалить ключ НАВСЕГДА (в отличие от set_status 'disabled' — строка исчезает).
pub fn key_remove(conn: &Connection, key: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys WHERE key=?1", rusqlite::params![key])?)
}

/// Удалить ВСЕ ключи (для очистки/тестов). Возвращает число удалённых.
pub fn key_clear(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys", [])?)
}

/// Все ключи (для CLI-листинга; ключ маскируется на стороне вывода).
pub fn key_list(conn: &Connection) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT key, account_id, label, spent_nano, COALESCE(status,'active') \
         FROM api_keys ORDER BY COALESCE(created_ts,0)")?;
    let rows = stmt.query_map([], |r| Ok(KeyRow {
        key: r.get::<_, String>(0)?,
        account_id: r.get::<_, Option<String>>(1)?,
        label: r.get::<_, Option<String>>(2)?,
        spent_nano: r.get::<_, i64>(3)?,
        status: r.get::<_, String>(4)?,
    }))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка персиста состояния пула (по подписке). Примитивы — registry не знает типов `pool`.
#[derive(Clone, Debug, Default)]
pub struct PoolStateRow {
    pub email: String,
    pub cooling_until: i64,
    pub cap5h_usd: f64,
    pub cap7d_usd: f64,
    pub spent_total_usd: f64,
    pub util5h: f64,
    pub util7d: f64,
    pub reset5h: i64,
    pub reset7d: i64,
    pub calib_n: i64,
}

/// Сохранить снимок состояния пула (upsert по email). Одной транзакцией — атомарно и быстро.
pub fn save_pool_state(conn: &Connection, rows: &[PoolStateRow]) -> Result<()> {
    let ts = now();
    conn.execute_batch("BEGIN")?;
    {
        let mut stmt = conn.prepare(
            "INSERT INTO pool_state(email, cooling_until, cap5h, cap7d, spent_total, util5, util7, \
             reset5, reset7, calib_n, updated_ts) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11) \
             ON CONFLICT(email) DO UPDATE SET cooling_until=excluded.cooling_until, cap5h=excluded.cap5h, \
             cap7d=excluded.cap7d, spent_total=excluded.spent_total, util5=excluded.util5, \
             util7=excluded.util7, reset5=excluded.reset5, reset7=excluded.reset7, \
             calib_n=excluded.calib_n, updated_ts=excluded.updated_ts")?;
        for r in rows {
            stmt.execute(rusqlite::params![r.email, r.cooling_until, r.cap5h_usd, r.cap7d_usd,
                r.spent_total_usd, r.util5h, r.util7d, r.reset5h, r.reset7d, r.calib_n, ts])?;
        }
    }
    conn.execute_batch("COMMIT")?;
    Ok(())
}

/// Прочитать сохранённое состояние пула (для восстановления на старте).
pub fn load_pool_state(conn: &Connection) -> Result<Vec<PoolStateRow>> {
    let mut stmt = conn.prepare(
        "SELECT email, cooling_until, cap5h, cap7d, spent_total, util5, util7, reset5, reset7, calib_n \
         FROM pool_state")?;
    let rows = stmt.query_map([], |r| Ok(PoolStateRow {
        email: r.get(0)?,
        cooling_until: r.get(1)?,
        cap5h_usd: r.get(2)?,
        cap7d_usd: r.get(3)?,
        spent_total_usd: r.get(4)?,
        util5h: r.get(5)?,
        util7d: r.get(6)?,
        reset5h: r.get(7)?,
        reset7d: r.get(8)?,
        calib_n: r.get(9)?,
    }))?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

// (Синхронная обёртка `Billing` удалена: запросный путь теперь через `forward::AsyncBilling`
//  — DB-акторы, ноль синхронных вызовов на async-воркерах. registry остаётся чистым sync-ядром.)

/// Агрегаты биллинга по всем аккаунтам клиентов (нанодоллары).
#[derive(Clone, Debug, Default)]
pub struct BillingTotals {
    pub balance_nano: i64,   // суммарный остаток на аккаунтах (клиентский флоат)
    pub spent_nano: i64,     // суммарно списано за всё время
    pub reserved_nano: i64,  // сейчас в незакрытых резервах (in-flight холды)
    pub active_keys: i64,    // активных аккаунтов
}

/// Суммы по accounts одним запросом (источник истины — БД). Ошибка → нули.
pub fn billing_totals(conn: &Connection) -> BillingTotals {
    conn.query_row(
        "SELECT COALESCE(SUM(balance_nano),0), COALESCE(SUM(spent_nano),0), \
         COALESCE(SUM(reserved_nano),0), COALESCE(SUM(CASE WHEN COALESCE(status,'active')='active' \
         THEN 1 ELSE 0 END),0) FROM accounts",
        [], |r| Ok(BillingTotals {
            balance_nano: r.get(0)?, spent_nano: r.get(1)?,
            reserved_nano: r.get(2)?, active_keys: r.get(3)?,
        }),
    ).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection { open(":memory:").unwrap() }

    /// Персист состояния пула: save→load переносит cooling/калибровку (upsert по email).
    #[test]
    fn pool_state_save_load_roundtrip() {
        let c = db();
        let rows = vec![PoolStateRow {
            email: "a@x.io".into(), cooling_until: 123456, cap5h_usd: 50.0, cap7d_usd: 1500.0,
            spent_total_usd: 12.5, util5h: 0.3, util7d: 0.1, reset5h: 999, reset7d: 888, calib_n: 4,
        }];
        save_pool_state(&c, &rows).unwrap();
        // повторный save (upsert) не дублирует и обновляет
        let mut r2 = rows.clone();
        r2[0].cooling_until = 222222;
        save_pool_state(&c, &r2).unwrap();
        let got = load_pool_state(&c).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].email, "a@x.io");
        assert_eq!(got[0].cooling_until, 222222);
        assert!((got[0].cap5h_usd - 50.0).abs() < 1e-9);
        assert_eq!(got[0].calib_n, 4);
    }

    // хелпер: аккаунт с балансом + ключ под ним (ref=None — админ-сид, не платёж, без дедупа)
    fn acct_with_key(c: &Connection, acct: &str, key: &str, usd_nano: i64, mult: i64) {
        account_create(c, acct, None, mult).unwrap();
        account_topup(c, acct, usd_nano, None).unwrap();
        key_issue(c, key, acct, None).unwrap();
    }

    /// Агрегаты трат (для /metrics): суммы по аккаунтам + число активных.
    #[test]
    fn billing_totals_aggregates_across_accounts() {
        let c = db();
        acct_with_key(&c, "acct_1", "sk-1", 5_000_000_000, 10000); // $5
        acct_with_key(&c, "acct_2", "sk-2", 3_000_000_000, 10000); // $3
        account_reserve(&c, "acct_1", 1_000_000_000).unwrap();
        account_settle(&c, "acct_1", "sk-1", 1_000_000_000, 400_000_000, None).unwrap(); // spent $0.4
        account_reserve(&c, "acct_2", 500_000_000).unwrap(); // висящий резерв $0.5
        account_set_status(&c, "acct_2", "disabled").unwrap();
        let t = billing_totals(&c);
        assert_eq!(t.balance_nano, 4_600_000_000 + 2_500_000_000); // $4.6 + $2.5
        assert_eq!(t.spent_nano, 400_000_000);
        assert_eq!(t.reserved_nano, 500_000_000);
        assert_eq!(t.active_keys, 1); // активных аккаунтов
    }

    /// Реконсиляция возвращает осиротевший при краше резерв в баланс АККАУНТА (идемпотентно).
    #[test]
    fn reconcile_refunds_orphaned_reservations() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
        account_reserve(&c, "a", 600_000_000).unwrap(); // «краш» до settle: balance 0.40, reserved 0.60
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 400_000_000);
        assert_eq!(reconcile_reservations(&c).unwrap(), 1);
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 1_000_000_000);
        assert_eq!(reconcile_reservations(&c).unwrap(), 0); // повторно — no-op
    }

    /// reserve атомарно гейтит по балансу аккаунта; settle сводит пару к −actual; per-key spent + ledger.
    #[test]
    fn reserve_gates_and_settle_nets_to_actual() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 2000); // $1.00
        assert_eq!(account_reserve(&c, "a", 600_000_000).unwrap(), Some(400_000_000));
        assert_eq!(account_reserve(&c, "a", 600_000_000).unwrap(), None); // $0.40 < $0.60 → отказ
        assert_eq!(account_settle(&c, "a", "k", 600_000_000, 100_000_000, Some("req1")).unwrap(), Some(900_000_000));
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 900_000_000);
        assert_eq!(acc.spent_nano, 100_000_000);
        // per-key атрибуция: spent по ключу тоже $0.10
        assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 100_000_000);
        // ledger: строка topup ($1) + строка charge ($0.10)
        let cnt: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE account_id='a'", [], |r| r.get(0)).unwrap();
        assert_eq!(cnt, 2);
    }

    /// Двойной settle (перекрытие деплоя: reconcile уже вернул резерв, затем settle старого инстанса)
    /// НЕ переначисляет и НЕ уводит reserved в минус — кламп MIN(hold,reserved)/MAX(0,…).
    #[test]
    fn double_settle_no_overcredit() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 10000); // $1
        account_reserve(&c, "a", 400_000_000).unwrap();     // резерв $0.4 → баланс $0.6, reserved $0.4
        // «reconcile» вернул резерв (краш до settle): руками эмулируем возврат
        reconcile_reservations(&c).unwrap();                 // баланс $1.0, reserved 0
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 1_000_000_000);
        // теперь прилетает settle СТАРОГО инстанса на тот же hold (actual $0.1)
        account_settle(&c, "a", "k", 400_000_000, 100_000_000, None).unwrap();
        let acc = account_get(&c, "a").unwrap().unwrap();
        // без клампа было бы: +$0.4 (второй раз!) − $0.1 = $1.3 (over-credit) и reserved=−$0.4.
        // с клампом: MIN(0.4, reserved=0)=0 → баланс += 0 − $0.1 = $0.9; reserved MAX(0,−0.4)=0.
        assert_eq!(acc.balance_nano, 900_000_000, "нет over-credit: списан только actual");
        assert_eq!(acc.reserved_nano, 0, "reserved не ушёл в минус");
    }

    /// release (settle с actual=0) возвращает резерв полностью, ledger-charge НЕ пишется.
    #[test]
    fn reserve_release_refunds_fully() {
        let c = db();
        acct_with_key(&c, "a", "k", 500_000_000, 2000);
        account_reserve(&c, "a", 200_000_000).unwrap();
        account_settle(&c, "a", "k", 200_000_000, 0, None).unwrap();
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 500_000_000);
        let charges: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE kind='charge'", [], |r| r.get(0)).unwrap();
        assert_eq!(charges, 0);
    }

    /// заблокированный аккаунт не резервируется; резолв ключа отражает активность обоих.
    #[test]
    fn reserve_rejects_disabled_account() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
        assert!(key_account(&c, "k").unwrap().unwrap().active);
        account_set_status(&c, "a", "disabled").unwrap();
        assert_eq!(account_reserve(&c, "a", 1).unwrap(), None);
        assert!(!key_account(&c, "k").unwrap().unwrap().active); // аккаунт неактивен → ключ тоже
    }

    /// Идемпотентный topup: повтор вебхука с тем же payment-ref НЕ начисляет дважды.
    #[test]
    fn topup_is_idempotent_by_ref() {
        let c = db();
        account_create(&c, "a", None, 2000).unwrap();
        // первый вебхук: +$10, ref=tx_ABC
        assert_eq!(account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(), Some(10_000_000_000));
        // ПОВТОР того же вебхука (ретрай) — баланс НЕ должен вырасти
        assert_eq!(account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(), Some(10_000_000_000));
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 10_000_000_000); // ровно $10
        // ДРУГОЙ ref начисляет нормально
        assert_eq!(account_topup(&c, "a", 5_000_000_000, Some("tx_XYZ")).unwrap(), Some(15_000_000_000));
        // без ref (админ-коррекция) — не дедупится, всегда применяется
        account_topup(&c, "a", 1_000_000_000, None).unwrap();
        account_topup(&c, "a", 1_000_000_000, None).unwrap();
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 17_000_000_000);
        // в ledger ровно один topup на каждый уникальный ref (+ 2 без ref)
        let topups: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE kind='topup'", [], |r| r.get(0)).unwrap();
        assert_eq!(topups, 4); // tx_ABC, tx_XYZ, и 2 без ref
    }

    /// ledger_prune режет старые списания (charge), но хранит пополнения (topup) и текущие суммы.
    #[test]
    fn ledger_prune_keeps_topups_and_totals() {
        let c = db();
        acct_with_key(&c, "a", "k", 5_000_000_000, 10000); // topup $5 (ledger topup)
        account_reserve(&c, "a", 1_000_000_000).unwrap();
        account_settle(&c, "a", "k", 1_000_000_000, 400_000_000, Some("old")).unwrap(); // charge $0.4
        // состарим ВСЕ строки (ts в прошлом)
        c.execute("UPDATE ledger SET ts = 1000", []).unwrap();
        let removed = ledger_prune(&c, 2000).unwrap(); // cutoff позже строк
        assert_eq!(removed, 1, "удалён 1 charge");
        let topups: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE kind='topup'", [], |r| r.get(0)).unwrap();
        let charges: i64 = c.query_row("SELECT COUNT(*) FROM ledger WHERE kind='charge'", [], |r| r.get(0)).unwrap();
        assert_eq!(topups, 1, "topup сохранён");
        assert_eq!(charges, 0, "charge обрезан");
        // текущие суммы аккаунта НЕ тронуты обрезкой (отдельный источник истины)
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 4_600_000_000);
        assert_eq!(acc.spent_nano, 400_000_000);
    }

    /// N ключей под ОДНИМ аккаунтом тратят из ОБЩЕГО баланса (ключевая модель).
    #[test]
    fn multiple_keys_share_one_account_balance() {
        let c = db();
        account_create(&c, "team", Some("tg:123"), 2000).unwrap();
        account_topup(&c, "team", 1_000_000_000, None).unwrap(); // $1 на команду
        key_issue(&c, "k-alice", "team", Some("alice")).unwrap();
        key_issue(&c, "k-bob", "team", Some("bob")).unwrap();
        // оба ключа резолвятся в тот же аккаунт
        assert_eq!(key_account(&c, "k-alice").unwrap().unwrap().account_id, "team");
        assert_eq!(key_account(&c, "k-bob").unwrap().unwrap().account_id, "team");
        // alice тратит $0.30, bob $0.20 — из общего баланса
        account_reserve(&c, "team", 300_000_000).unwrap();
        account_settle(&c, "team", "k-alice", 300_000_000, 300_000_000, None).unwrap();
        account_reserve(&c, "team", 200_000_000).unwrap();
        account_settle(&c, "team", "k-bob", 200_000_000, 200_000_000, None).unwrap();
        assert_eq!(account_get(&c, "team").unwrap().unwrap().balance_nano, 500_000_000); // $0.50 осталось
        // атрибуция по ключам раздельная
        assert_eq!(key_get(&c, "k-alice").unwrap().unwrap().spent_nano, 300_000_000);
        assert_eq!(key_get(&c, "k-bob").unwrap().unwrap().spent_nano, 200_000_000);
        // вход по handle
        assert_eq!(account_by_handle(&c, "tg:123").unwrap().unwrap().id, "team");
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
