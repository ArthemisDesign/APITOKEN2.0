//! # registry — реестр подписок (пункт 1)
//!
//! Источник истины пула: engine-owned PostgreSQL. SQLite remains a migration/rollback source. Для форвардинг-прокси подписке нужны
//! только OAuth-токен + прокси (+ статус/флот). Токен берётся из колонки `token` (inline)
//! либо из файла `token_file`. Совместим с исторической subscriptions.db (мягкая миграция).
//!
//! **Границы крейта:** только хранение/чтение подписок. НИКАКОЙ HTTP/логики пула.
//! Ниже по стеку зависеть не от кого.

pub mod authority;
pub mod pg;

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
    pub plan: String, // pro|max5|max20 (детект) — для per-sub прайора ёмкости в pool ("" = неизвестно)
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
    // Метаданные прокси (заполняет authbot — владелец жизненного цикла; движок лишь читает/показывает):
    ("proxy_expire", "TEXT"), // дата истечения прокси из IPRoyal (ISO), "" = неизвестно
    ("proxy_checked_ts", "INTEGER"), // ts последней health-проверки прокси (fingerprint-free)
    ("proxy_ok", "INTEGER"),  // 1=жив / 0=мёртв на последней проверке (NULL=не проверялся)
    // Durable auth-health (движок пишет из коррелированных probe; переживает рестарт). Зеркало
    // engine PostgreSQL migration 0003. Токен-fingerprint даёт авто-ревайв при замене токена.
    ("auth_state", "TEXT"), // 'healthy' | 'suspect' | 'dead'
    ("auth_fail_streak", "INTEGER"),
    ("first_auth_fail_ts", "INTEGER"),
    ("last_auth_fail_ts", "INTEGER"),
    ("last_auth_http", "INTEGER"),
    ("dead_since_ts", "INTEGER"),
    ("dead_reason", "TEXT"),
    ("auth_token_fp", "TEXT"),
];

pub fn open(path: &str) -> Result<Connection> {
    if let Some(dir) = std::path::Path::new(path).parent() {
        if !dir.as_os_str().is_empty() {
            let _ = fs::create_dir_all(dir);
        }
    }
    let c = Connection::open(path).with_context(|| format!("открыть БД {path}"))?;
    // AUDIT(C38): this database is authoritative for balances and ledger entries. In WAL mode,
    // synchronous=FULL makes an acknowledged commit durable across OS crashes and power loss.
    // Performance-sensitive nonfinancial state should move to a separate database if needed.
    c.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; \
                     PRAGMA busy_timeout=5000; PRAGMA wal_autocheckpoint=1000;",
    )?;
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
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN reserved_nano INTEGER NOT NULL DEFAULT 0",
        [],
    );

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
    // Stable public identifier for control-plane key management. The usable `key` remains secret;
    // dashboards and the commercial backend can revoke by `key_id` without persisting that secret.
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN key_id TEXT", []);
    let _ = c.execute(
        "ALTER TABLE api_keys ADD COLUMN spend_limit_nano INTEGER",
        [],
    );
    let _ = c.execute("ALTER TABLE api_keys ADD COLUMN expires_ts INTEGER", []);
    let _ = c.execute(
        "UPDATE api_keys SET key_id = 'key_' || lower(hex(randomblob(16))) WHERE key_id IS NULL",
        [],
    );
    let _ = c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS api_keys_key_id ON api_keys(key_id) WHERE key_id IS NOT NULL", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS api_keys_account ON api_keys(account_id)",
        [],
    );

    // ЛЕДЖЕР: append-only история движений баланса (пополнения/списания/возвраты) — для точного
    // учёта, споров и дашбордов. Текущий баланс = accounts.balance_nano; ledger — журнал КАК он менялся.
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
         key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
         balance_after_nano INTEGER, ts INTEGER, model TEXT)",
        [],
    )?;
    // Атрибуция charge-строк к Claude-модели (для точного per-model дневного графика). Модель известна
    // в момент settle (тот же запрос, что и usage_event). topup/adjust модели не имеют → NULL. Идемпотентно.
    let _ = c.execute("ALTER TABLE ledger ADD COLUMN model TEXT", []);
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_acct ON ledger(account_id, id)",
        [],
    );
    // AUDIT(C2): correctness-critical idempotency indexes must fail closed. A legacy database with
    // duplicate references must not open for billing traffic without explicit operator repair.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_topup_ref ON ledger(ref) \
         WHERE kind='topup' AND ref IS NOT NULL",
        [],
    )
    .context("create required unique top-up reference index")?;
    // AUDIT(C40): negative adjustments are retryable monetary mutations too, so their supplied
    // references share the same global idempotency namespace as top-ups.
    c.execute(
        "CREATE UNIQUE INDEX IF NOT EXISTS ledger_money_ref ON ledger(ref) \
         WHERE kind IN ('topup','adjust') AND ref IS NOT NULL",
        [],
    )
    .context("create required unique monetary reference index")?;
    c.execute(
        "CREATE TABLE IF NOT EXISTS ledger_consumer_checkpoints(consumer TEXT NOT NULL, \
         account_id TEXT NOT NULL, last_ledger_id INTEGER NOT NULL, updated_ts INTEGER NOT NULL, \
         PRIMARY KEY(consumer,account_id))",
        [],
    )?;
    // AUDIT-TODO(C2): move schema upgrades into versioned transactions with explicit duplicate repair.
    // Retained for the future checkpoint-aware pruning path; current timestamp-only prune is disabled.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS ledger_charge_ts ON ledger(ts) WHERE kind='charge'",
        [],
    );

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
    // Разбивка расхода по токенам/моделям для клиентских дашбордов (per-request). НЕ money-БД:
    // авторитет денег — accounts.balance_nano + ledger. Эта таблица — аналитика (что реально
    // потрачено по корзинам токенов и моделям), пишется рядом с charge, обрезается по ретенции.
    c.execute(
        "CREATE TABLE IF NOT EXISTS usage_events(id INTEGER PRIMARY KEY AUTOINCREMENT, \
         account_id TEXT NOT NULL, key TEXT, model TEXT, \
         input_tokens INTEGER NOT NULL DEFAULT 0, output_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_read_tokens INTEGER NOT NULL DEFAULT 0, cache_write_5m_tokens INTEGER NOT NULL DEFAULT 0, \
         cache_write_1h_tokens INTEGER NOT NULL DEFAULT 0, web_search_requests INTEGER NOT NULL DEFAULT 0, \
         real_nano INTEGER NOT NULL DEFAULT 0, charge_nano INTEGER NOT NULL DEFAULT 0, ref TEXT, ts INTEGER)",
        [],
    )?;
    for (name, ty) in [
        ("speed", "TEXT NOT NULL DEFAULT 'standard'"),
        ("inference_geo", "TEXT NOT NULL DEFAULT ''"),
        ("input_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("output_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_read_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_5m_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("cache_write_1h_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("web_search_nano", "INTEGER NOT NULL DEFAULT 0"),
        ("priced_ts", "INTEGER NOT NULL DEFAULT 0"),
        ("provider", "TEXT NOT NULL DEFAULT 'anthropic'"),
    ] {
        let _ = c.execute(
            &format!("ALTER TABLE usage_events ADD COLUMN {name} {ty}"),
            [],
        );
    }
    // Индекс под агрегацию по окну (account_id + время) и под фоновую обрезку по ts.
    let _ = c.execute(
        "CREATE INDEX IF NOT EXISTS usage_events_acct_ts ON usage_events(account_id, ts)",
        [],
    );
    // SQLite money durability mirrors the PostgreSQL request lifecycle: every hold has an exact
    // request identity and lease, while settlement intent is committed to an outbox before the
    // balance mutation. Recovery can therefore distinguish pre-delivery cancellation from a
    // delivered request and can retry the exact settlement after process/database failures.
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS billing_reservations( \
           request_id TEXT PRIMARY KEY, account_id TEXT NOT NULL, key TEXT NOT NULL, \
           hold_nano INTEGER NOT NULL, state TEXT NOT NULL, \
           balance_after_reserve_nano INTEGER NOT NULL, actual_nano INTEGER, \
           balance_after_settle_nano INTEGER, reference TEXT, lease_until INTEGER NOT NULL, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, settled_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_reservations_lease \
           ON billing_reservations(state,lease_until); \
         CREATE TABLE IF NOT EXISTS billing_settlement_outbox( \
           request_id TEXT PRIMARY KEY, actual_nano INTEGER NOT NULL, reference TEXT, \
           usage_json TEXT, state TEXT NOT NULL DEFAULT 'pending', attempts INTEGER NOT NULL DEFAULT 0, \
           next_attempt_ts INTEGER NOT NULL DEFAULT 0, last_error TEXT, \
           created_ts INTEGER NOT NULL, updated_ts INTEGER NOT NULL, committed_ts INTEGER); \
         CREATE INDEX IF NOT EXISTS billing_outbox_pending \
           ON billing_settlement_outbox(state,next_attempt_ts,created_ts);"
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
/// без `account_id` (легаси) атомарно заводим отдельный случайный аккаунт, переносим баланс/расход/
/// наценку и линкуем ключ. Повторный запуск пропускает уже связанные строки.
fn migrate_legacy_keys(c: &Connection) -> Result<()> {
    let legacy: Vec<(String, i64, i64, i64, String)> = {
        let mut stmt = c.prepare(
            "SELECT key, balance_nano, spent_nano, mult_bp, COALESCE(status,'active') \
             FROM api_keys WHERE account_id IS NULL",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let tx = c.unchecked_transaction()?;
    for (key, bal, spent, mult, status) in legacy {
        // AUDIT(C39): never derive wallet identity from a short key suffix. Generate a full random
        // account ID and persist the key→account mapping atomically with the migrated balance.
        let ts = now();
        let acct: String = tx.query_row(
            "INSERT INTO accounts(id, balance_nano, spent_nano, mult_bp, status, created_ts, created) \
             VALUES('acct_' || lower(hex(randomblob(16))),?1,?2,?3,?4,?5,?6) RETURNING id",
            rusqlite::params![bal, spent, mult, status, ts, chrono_like(ts)],
            |r| r.get(0),
        )?;
        let updated = tx.execute(
            "UPDATE api_keys SET account_id=?1, reserved_nano=0 WHERE key=?2 AND account_id IS NULL",
            rusqlite::params![acct, key],
        )?;
        if updated != 1 {
            anyhow::bail!("legacy key migration lost its target row");
        }
    }
    tx.commit()?;
    // AUDIT-TODO(C39): detect and manually split wallets already merged by the historical suffix migration.
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
        out.push(Sub {
            email,
            token: tok,
            proxy: proxy.unwrap_or_default(),
            fleet: sfleet,
            plan,
        });
    }
    Ok(out)
}

// ── CLI-операции реестра ────────────────────────────────────────────────────
pub fn add(conn: &Connection, email: &str, token: &str, proxy: &str, fleet: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, ?2, NULL, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=excluded.token, token_file=NULL, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>excluded.token OR COALESCE(subs.token_file,'')<>'' \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn add_file(
    conn: &Connection,
    email: &str,
    token_file: &str,
    proxy: &str,
    fleet: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO subs(email, token, token_file, proxy, status, fleet, added_ts, added) \
         VALUES(?1, NULL, ?2, ?3, 'active', ?4, ?5, ?6) \
         ON CONFLICT(email) DO UPDATE SET token=NULL, token_file=excluded.token_file, \
         proxy=excluded.proxy, status='active', fleet=excluded.fleet, \
         auth_state=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 'healthy' ELSE subs.auth_state END, \
         auth_fail_streak=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.auth_fail_streak END, \
         first_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.first_auth_fail_ts END, \
         last_auth_fail_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_fail_ts END, \
         last_auth_http=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.last_auth_http END, \
         dead_since_ts=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN 0 ELSE subs.dead_since_ts END, \
         dead_reason=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.dead_reason END, \
         auth_token_fp=CASE WHEN COALESCE(subs.token,'')<>'' OR COALESCE(subs.token_file,'')<>excluded.token_file \
           THEN '' ELSE subs.auth_token_fp END",
        rusqlite::params![email, token_file, proxy, fleet, now(), chrono_like(now())],
    )?;
    Ok(())
}

pub fn set_status(conn: &Connection, email: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status=?1 WHERE email=?2",
        rusqlite::params![status, email],
    )?)
}
pub fn set_plan(conn: &Connection, email: &str, plan: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET plan=?1 WHERE email=?2",
        rusqlite::params![plan, email],
    )?)
}

/// (разрешённый токен, proxy) для одной подписки (любого статуса) — для детекта тарифа.
pub fn get_creds(conn: &Connection, email: &str) -> Result<Option<(String, String)>> {
    let row = conn.query_row(
        "SELECT token, token_file, proxy FROM subs WHERE email=?1",
        rusqlite::params![email],
        |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        },
    );
    match row {
        Ok((token, token_file, proxy)) => {
            let tok = resolve_token(token, token_file);
            if tok.is_empty() {
                Ok(None)
            } else {
                Ok(Some((tok, proxy.unwrap_or_default())))
            }
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}
pub fn set_proxy(conn: &Connection, email: &str, proxy: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy=?1 WHERE email=?2",
        rusqlite::params![proxy, email],
    )?)
}
pub fn set_fleet(conn: &Connection, email: &str, fleet: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET fleet=?1 WHERE email=?2",
        rusqlite::params![fleet, email],
    )?)
}
/// Обновить прокси-метаданные (пишет authbot — владелец жизненного цикла прокси). `expire` — дата
/// истечения из IPRoyal (ISO, "" если неизвестно); `ok` — жив ли прокси на fingerprint-free проверке.
pub fn set_proxy_meta(
    conn: &Connection,
    email: &str,
    expire: &str,
    checked_ts: i64,
    ok: bool,
) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET proxy_expire=?1, proxy_checked_ts=?2, proxy_ok=?3 WHERE email=?4",
        rusqlite::params![expire, checked_ts, ok as i64, email],
    )?)
}
/// host:port из строки прокси (без user:pass) — для показа в панели/логах.
pub fn mask_proxy(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let no_scheme = p.split("://").last().unwrap_or(p);
    no_scheme
        .rsplit('@')
        .next()
        .unwrap_or(no_scheme)
        .to_string()
}
pub fn remove(conn: &Connection, email: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET status='deleted' WHERE email=?1",
        rusqlite::params![email],
    )?)
}

/// Disable every subscription (optionally in one fleet) without destroying active lease history.
pub fn clear(conn: &Connection, fleet: Option<&str>) -> Result<usize> {
    Ok(match fleet {
        Some(f) => conn.execute(
            "UPDATE subs SET status='deleted' WHERE COALESCE(fleet,'prod')=?1 AND status<>'deleted'",
            rusqlite::params![f],
        )?,
        None => conn.execute("UPDATE subs SET status='deleted' WHERE status<>'deleted'", [])?,
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
            has_token: r
                .get::<_, Option<String>>(4)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy: r.get::<_, String>(5)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка админ-обзора подписок (движок → панель): БЕЗ токена, прокси — маска host:port + метаданные.
pub struct SubAdmin {
    pub email: String,
    pub status: String,
    pub fleet: String,
    pub has_token: bool,
    pub proxy_host: String,     // host:port (без user:pass)
    pub proxy_expire: String,   // ISO из IPRoyal / ""
    pub proxy_ok: Option<bool>, // None = не проверялся (здоровье в осн. из движка/органики)
    pub added_ts: i64,          // момент добавления токена (срок жизни = added_ts + N дней)
    pub added: String,
    /// Durable auth-health (авторитетно из БД, переживает рестарт): 'healthy'|'suspect'|'dead'.
    pub auth_state: String,
    pub dead_reason: String, // '' если не dead
    pub dead_since_ts: i64,  // 0 если не dead
}

/// Durable auth-health одной подписки. Движок (поллер) пишет это из КОРРЕЛИРОВАННЫХ чистых probe:
/// один 401/403 не приговор (может быть транзиент/битый запрос), но N подряд за ≥T минут = мёртвый
/// токен/бан. Переживает рестарт и blue/green (в отличие от эфемерного in-memory `auth_dead`).
/// Примитивы-сентинелы (0/"" = «нет») — чтобы `pool` не тащил Option через слой.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubHealth {
    pub email: String,
    pub auth_state: String, // 'healthy' | 'suspect' | 'dead'
    pub auth_fail_streak: i64,
    pub first_auth_fail_ts: i64, // 0 = нет текущей серии отказов
    pub last_auth_fail_ts: i64,
    pub last_auth_http: i64,   // 0 = нет
    pub dead_since_ts: i64,    // 0 = не dead
    pub dead_reason: String,   // '' = нет
    pub auth_token_fp: String, // отпечаток токена, к которому относится вердикт (смена → авто-ревайв)
}

pub fn subs_admin(conn: &Connection) -> Result<Vec<SubAdmin>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(status,'active'), COALESCE(fleet,'prod'), \
         COALESCE(NULLIF(token,''), NULLIF(token_file,'')), COALESCE(proxy,''), \
         COALESCE(proxy_expire,''), proxy_ok, COALESCE(added_ts,0), COALESCE(added,''), \
         COALESCE(auth_state,'healthy'), COALESCE(dead_reason,''), COALESCE(dead_since_ts,0) \
         FROM subs ORDER BY COALESCE(added_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        let proxy: String = r.get(4)?;
        Ok(SubAdmin {
            email: r.get(0)?,
            status: r.get(1)?,
            fleet: r.get(2)?,
            has_token: r
                .get::<_, Option<String>>(3)?
                .map(|s| !s.is_empty())
                .unwrap_or(false),
            proxy_host: mask_proxy(&proxy),
            proxy_expire: r.get(5)?,
            proxy_ok: r.get::<_, Option<i64>>(6)?.map(|n| n != 0),
            added_ts: r.get(7)?,
            added: r.get(8)?,
            auth_state: r.get(9)?,
            dead_reason: r.get(10)?,
            dead_since_ts: r.get(11)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Загрузить durable auth-health всех подписок (движок сеет им in-memory состояние на старте).
pub fn load_sub_health(conn: &Connection, fleet: Option<&str>) -> Result<Vec<SubHealth>> {
    let mut stmt = conn.prepare(
        "SELECT email, COALESCE(auth_state,'healthy'), COALESCE(auth_fail_streak,0), \
         COALESCE(first_auth_fail_ts,0), COALESCE(last_auth_fail_ts,0), COALESCE(last_auth_http,0), \
         COALESCE(dead_since_ts,0), COALESCE(dead_reason,''), COALESCE(auth_token_fp,''), \
         COALESCE(fleet,'prod') FROM subs")?;
    let rows = stmt.query_map([], |r| {
        Ok((
            SubHealth {
                email: r.get(0)?,
                auth_state: r.get(1)?,
                auth_fail_streak: r.get(2)?,
                first_auth_fail_ts: r.get(3)?,
                last_auth_fail_ts: r.get(4)?,
                last_auth_http: r.get(5)?,
                dead_since_ts: r.get(6)?,
                dead_reason: r.get(7)?,
                auth_token_fp: r.get(8)?,
            },
            r.get::<_, String>(9)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (h, sfleet) = row?;
        if let Some(f) = fleet {
            if f != sfleet {
                continue;
            }
        }
        out.push(h);
    }
    Ok(out)
}

/// Записать durable auth-health одной подписки (движок → БД). Идемпотентный upsert по email.
pub fn save_sub_health(conn: &Connection, h: &SubHealth) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE subs SET auth_state=?1, auth_fail_streak=?2, first_auth_fail_ts=?3, \
         last_auth_fail_ts=?4, last_auth_http=?5, dead_since_ts=?6, dead_reason=?7, auth_token_fp=?8 \
         WHERE email=?9",
        rusqlite::params![
            h.auth_state, h.auth_fail_streak, h.first_auth_fail_ts, h.last_auth_fail_ts,
            h.last_auth_http, h.dead_since_ts, h.dead_reason, h.auth_token_fp, h.email
        ],
    )?)
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
    pub key_id: String,
    pub account_id: Option<String>,
    pub label: Option<String>,
    pub spent_nano: i64, // расход по ЭТОМУ ключу (атрибуция; баланс общий на аккаунте)
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub created_ts: i64,
    pub last_used_ts: Option<i64>,
    pub status: String,
}

/// Result of atomically replacing a key's mutable spending policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPolicyUpdate {
    Updated,
    NotFound,
    LimitBelowUsage,
    ExpiryNotFuture,
}

/// Выпустить ключ ПОД аккаунт (баланс — на аккаунте, ключ лишь ссылается). `label` — имя ключа.
pub fn key_issue(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
) -> Result<()> {
    key_issue_with_policy(conn, key, account_id, label, None, None)
}

pub fn key_issue_with_policy(
    conn: &Connection,
    key: &str,
    account_id: &str,
    label: Option<&str>,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<()> {
    if key.trim().is_empty() || account_id.trim().is_empty() {
        anyhow::bail!("key and account id must not be empty");
    }
    let changed = conn.execute(
        "INSERT INTO api_keys(key, key_id, account_id, label, spent_nano, reserved_nano, \
         spend_limit_nano, expires_ts, status, created_ts, created) \
         VALUES(?1, 'key_' || lower(hex(randomblob(16))), ?2, ?3, 0, 0, ?4, ?5, 'active', ?6, ?7) \
         ON CONFLICT(key) DO UPDATE SET label=excluded.label, \
         spend_limit_nano=excluded.spend_limit_nano, expires_ts=excluded.expires_ts \
         WHERE api_keys.account_id=excluded.account_id",
        rusqlite::params![
            key,
            account_id,
            label,
            spend_limit_nano,
            expires_ts,
            now(),
            chrono_like(now())
        ],
    )?;
    if changed == 0 {
        anyhow::bail!("key is already owned by another account");
    }
    Ok(())
}

/// Backward-compatible startup entry point. Only expired request-scoped leases are touched; legacy
/// aggregate holds remain fail-closed because they still have no provable owner or age.
pub fn reconcile_reservations(conn: &Connection) -> Result<usize> {
    let report = sqlite_reconcile_expired(conn, 10_000)?;
    Ok(report.canceled_before_delivery + report.charged_after_delivery + report.processed_outbox)
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
    pub spent_nano: i64,
    pub reserved_nano: i64,
    pub spend_limit_nano: Option<i64>,
    pub expires_ts: Option<i64>,
    pub active: bool, // ключ активен И аккаунт активен
}

impl KeyAuth {
    pub fn active_at(&self, ts: i64) -> bool {
        self.active && self.expires_ts.is_none_or(|expires| expires > ts)
    }
}

/// Создать аккаунт. `handle` (TG id/email) опционален и уникален (когда задан).
pub fn account_create(
    conn: &Connection,
    id: &str,
    handle: Option<&str>,
    mult_bp: i64,
) -> Result<()> {
    if id.trim().is_empty() || handle.is_some_and(|value| value.trim().is_empty()) {
        anyhow::bail!("account id and supplied handle must not be empty");
    }
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("account multiplier must be within 0..=10000 basis points");
    }
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
    match conn.query_row(&sql, rusqlite::params![val], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    }) {
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
    let rows = stmt.query_map([], |r| {
        Ok(AccountRow {
            id: r.get(0)?,
            handle: r.get(1)?,
            balance_nano: r.get(2)?,
            spent_nano: r.get(3)?,
            reserved_nano: r.get(4)?,
            mult_bp: r.get(5)?,
            status: r.get(6)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

pub fn account_set_status(conn: &Connection, id: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status=?1 WHERE id=?2",
        rusqlite::params![status, id],
    )?)
}

/// Change the price multiplier for future requests. Existing ledger rows remain immutable.
pub fn account_set_mult_bp(conn: &Connection, id: &str, mult_bp: i64) -> Result<usize> {
    if !(0..=10_000).contains(&mult_bp) {
        anyhow::bail!("invalid account multiplier");
    }
    Ok(conn.execute(
        "UPDATE accounts SET mult_bp=?1 WHERE id=?2",
        rusqlite::params![mult_bp, id],
    )?)
}

/// Tombstone an account. Financial history and in-flight reservations remain settleable/auditable.
pub fn account_remove(conn: &Connection, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE accounts SET status='deleted' WHERE id=?1 AND status<>'deleted'",
        rusqlite::params![id],
    )?)
}

/// Пополнить баланс аккаунта (`amount` может быть отрицательным = коррекция) + запись в ledger.
/// Возвращает новый баланс, сохранённый исходный баланс точного idempotent replay, либо None, если
/// аккаунта нет. Повтор reference с другими параметрами — ошибка. Атомарно (UPDATE…RETURNING + ledger).
pub fn account_topup(
    conn: &Connection,
    id: &str,
    amount_nano: i64,
    reference: Option<&str>,
) -> Result<Option<i64>> {
    if matches!(reference, Some(r) if r.trim().is_empty()) {
        anyhow::bail!("monetary idempotency reference must not be empty");
    }
    let tx = conn.unchecked_transaction()?;
    // Начисляем баланс...
    let bal = match tx.query_row(
        "UPDATE accounts SET balance_nano = balance_nano + ?1 WHERE id = ?2 RETURNING balance_nano",
        rusqlite::params![amount_nano, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => b,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            return Ok(None);
        } // нет аккаунта
        Err(e) => return Err(e.into()),
    };
    let kind = if amount_nano >= 0 { "topup" } else { "adjust" };
    // ...и пишем ledger. UNIQUE откатывает предварительный UPDATE. После конфликта считаем операцию
    // идемпотентным повтором ТОЛЬКО при точном совпадении account + amount + kind.
    match tx.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts) \
         VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, kind, amount_nano, reference, bal, now()],
    ) {
        Ok(_) => {
            tx.commit()?;
            Ok(Some(bal))
        }
        Err(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            drop(tx); // ROLLBACK before inspecting the existing operation.
            let Some(reference) = reference else {
                return Err(rusqlite::Error::SqliteFailure(e, None).into());
            };
            let existing = conn.query_row(
                "SELECT account_id, kind, amount_nano, balance_after_nano FROM ledger \
                 WHERE ref=?1 AND kind IN ('topup','adjust')",
                rusqlite::params![reference],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<i64>>(3)?,
                    ))
                },
            );
            match existing {
                Ok((existing_id, existing_kind, existing_amount, Some(original_balance)))
                    if existing_id == id
                        && existing_kind == kind
                        && existing_amount == amount_nano =>
                {
                    Ok(Some(original_balance))
                }
                Ok(_) => {
                    eprintln!(
                        "billing idempotency conflict: parameters differ from the stored operation"
                    );
                    // AUDIT-TODO(C42/C80): expose a typed idempotency conflict through Control API as HTTP 409.
                    anyhow::bail!(
                        "idempotency reference already belongs to a different monetary operation"
                    )
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    Err(rusqlite::Error::SqliteFailure(e, None).into())
                }
                Err(query_err) => Err(query_err.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Горячая money-операция для group-commit (reserve/settle) — writer'а. Ссылки (не owned): вызывающий
/// (billing) держит команды, registry лишь применяет их SQL в ОДНОЙ транзакции.
pub enum HotOp<'a> {
    Reserve {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
    },
    Settle {
        account_id: &'a str,
        key: &'a str,
        hold: i64,
        actual: i64,
        reference: Option<&'a str>,
        usage: Option<&'a UsageEventInput>,
    },
}

/// Применить пачку reserve/settle в ОДНОЙ транзакции (group-commit): амортизирует стоимость коммита
/// под нагрузкой. Команды применяются ПОСЛЕДОВАТЕЛЬНО — атомарный reserve (`WHERE balance>=hold`)
/// видит эффекты предыдущих в этой же транзакции ⇒ инвариант `charge≤hold≤balance` сохранён, как при
/// по-одному. Возвращает результаты в порядке `ops` (индекс-в-индекс). Ошибка BEGIN/COMMIT → Err
/// (вызывающий откатывается на обработку по-одному). Per-op ошибки глушатся в None (как в прежнем
/// writer'е: `.ok().flatten()`).
pub fn apply_hot_batch(conn: &Connection, ops: &[HotOp]) -> Result<Vec<Option<i64>>> {
    let tx = conn.unchecked_transaction()?;
    let mut out = Vec::with_capacity(ops.len());
    for op in ops {
        out.push(match op {
            HotOp::Reserve {
                account_id,
                key,
                hold,
            } => account_reserve_for_key(&tx, account_id, key, *hold)
                .ok()
                .flatten(),
            HotOp::Settle {
                account_id,
                key,
                hold,
                actual,
                reference,
                usage,
            } => account_settle_in(&tx, account_id, key, *hold, *actual, *reference, *usage)
                .ok()
                .flatten(),
        });
    }
    tx.commit()?;
    Ok(out)
}

/// АТОМАРНО зарезервировать `hold` по АККАУНТУ, если баланс покрывает и аккаунт активен. Та же
/// семантика, что была на ключе, но кошелёк — общий на профиль (все ключи юзера тратят из него).
pub fn account_reserve(conn: &Connection, id: &str, hold_nano: i64) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    match conn.query_row(
        "UPDATE accounts SET balance_nano = balance_nano - ?1, reserved_nano = reserved_nano + ?1 \
         WHERE id = ?2 AND status = 'active' AND balance_nano >= ?1 RETURNING balance_nano",
        rusqlite::params![hold, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(bal) => Ok(Some(bal)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Reserve against both the shared account balance and one key's lifetime spending policy.
pub fn account_reserve_for_key(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    conn.execute_batch("SAVEPOINT key_policy_reserve")?;
    let balance = match conn.query_row(
        "UPDATE accounts SET balance_nano=balance_nano-?1, reserved_nano=reserved_nano+?1 \
         WHERE id=?2 AND status='active' AND balance_nano>=?1 RETURNING balance_nano",
        rusqlite::params![hold, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(value) => value,
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
            return Ok(None);
        }
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    let updated = match conn.execute(
        "UPDATE api_keys SET reserved_nano=reserved_nano+?1 \
         WHERE key=?2 AND account_id=?3 AND COALESCE(status,'active')='active' \
           AND (expires_ts IS NULL OR expires_ts>CAST(strftime('%s','now') AS INTEGER)) \
           AND (spend_limit_nano IS NULL OR spent_nano+reserved_nano+?1<=spend_limit_nano)",
        rusqlite::params![hold, key, id],
    ) {
        Ok(value) => value,
        Err(error) => {
            let _ =
                conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve");
            return Err(error.into());
        }
    };
    if updated != 1 {
        conn.execute_batch("ROLLBACK TO key_policy_reserve; RELEASE key_policy_reserve")?;
        return Ok(None);
    }
    conn.execute_batch("RELEASE key_policy_reserve")?;
    Ok(Some(balance))
}

/// Закрыть резерв аккаунта: баланс += hold − actual, spent += actual, reserved −= hold; per-key
/// `spent` += actual (атрибуция расхода по ключу); строка в ledger (charge). ВСЁ в ОДНОЙ транзакции.
pub fn account_settle(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    let bal = account_settle_in(&tx, id, key, hold_nano, actual_nano, reference, usage)?;
    tx.commit()?;
    Ok(bal)
}

/// SQL-тело settle БЕЗ BEGIN/COMMIT — для group-commit writer'а (несколько settle в одной транзакции).
/// Вызывающий обязан обернуть в транзакцию (`account_settle` — тонкая обёртка). `conn` может быть
/// `&Transaction` (Deref в `&Connection`). Семантика идентична `account_settle`.
pub fn account_settle_in(
    conn: &Connection,
    id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let hold = hold_nano.max(0);
    // Exact provider usage may exceed a conservative hold because of provider-added tokens or a
    // response-reported pricing modifier. Forward caps this at hold+$1, matching the account-level
    // overdraft floor; silently clamping here would turn delivered provider usage into lost revenue.
    let actual = actual_nano.max(0);
    // Возвращаем hold, но НЕ БОЛЬШЕ, чем реально числится в reserved_nano: MIN(hold, reserved).
    // Защита от двойного settle (перекрытие деплоя: reconcile уже вернул резерв, затем прилетел
    // settle старого инстанса) — иначе balance получил бы +hold дважды (over-credit) и reserved
    // ушёл бы в минус. MAX(0, …) держит reserved ≥ 0. В норме (reserved≥hold) поведение прежнее.
    let bal = match conn.query_row(
        "UPDATE accounts SET \
         balance_nano = balance_nano + MIN(?1, reserved_nano) - ?2, \
         spent_nano = spent_nano + ?2, \
         reserved_nano = MAX(0, reserved_nano - ?1) WHERE id = ?3 RETURNING balance_nano",
        rusqlite::params![hold, actual, id],
        |r| r.get::<_, i64>(0),
    ) {
        Ok(b) => Some(b),
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(b) = bal {
        conn.execute(
            "UPDATE api_keys SET spent_nano=spent_nano+?1, \
             reserved_nano=MAX(0,reserved_nano-?2) WHERE key=?3",
            rusqlite::params![actual, hold, key],
        )?;
        if actual > 0 {
            // Модель за списанием — из usage того же запроса (пустую строку не пишем → NULL).
            let model = usage.map(|u| u.model.as_str()).filter(|m| !m.is_empty());
            ledger_add(conn, id, Some(key), "charge", actual, reference, b, model)?;
            // usage_events (аналитика) — в ТОЙ ЖЕ транзакции, что и charge (экономим коммит на запрос).
            // Best-effort: ошибка вставки usage НЕ роняет money-коммит (аналитика не критична).
            if let Some(u) = usage {
                let _ = usage_event_add(conn, id, Some(key), u, actual, reference);
            }
        }
    }
    Ok(bal)
}

/// Резолв ключа в аккаунт для авторизации запроса (JOIN api_keys→accounts).
pub fn key_account(conn: &Connection, key: &str) -> Result<Option<KeyAuth>> {
    match conn.query_row(
        "SELECT a.id, a.mult_bp, a.balance_nano, k.spent_nano, k.reserved_nano, \
         k.spend_limit_nano, k.expires_ts, \
         (COALESCE(k.status,'active')='active' AND COALESCE(a.status,'active')='active') \
         FROM api_keys k JOIN accounts a ON a.id = k.account_id WHERE k.key = ?1",
        rusqlite::params![key],
        |r| {
            Ok(KeyAuth {
                account_id: r.get(0)?,
                mult_bp: r.get(1)?,
                balance_nano: r.get(2)?,
                spent_nano: r.get(3)?,
                reserved_nano: r.get(4)?,
                spend_limit_nano: r.get(5)?,
                expires_ts: r.get(6)?,
                active: r.get::<_, i64>(7)? != 0,
            })
        },
    ) {
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

/// Свернуть WAL в основную БД и обрезать файл (TRUNCATE). Авто-checkpoint SQLite (порог ~1000 стр.)
/// обычно держит WAL в узде, но под НЕПРЕРЫВНОЙ записью + постоянными читателями (наш случай:
/// reserve/settle на каждом запросе + N read-соединений) чекпоинт может откладываться и WAL растёт.
/// Периодический явный TRUNCATE-чекпоинт держит файл ограниченным. PASSIVE не нужен — вызываем редко
/// из persist_loop; занятость нормальна, вернём Ok даже если часть страниц осталась (не критично).
pub fn wal_checkpoint(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}

pub fn ledger_ack(
    conn: &Connection,
    consumer: &str,
    account_id: &str,
    last_ledger_id: i64,
) -> Result<usize> {
    if consumer.trim().is_empty() || last_ledger_id < 0 {
        anyhow::bail!("invalid ledger checkpoint");
    }
    Ok(conn.execute(
        "INSERT INTO ledger_consumer_checkpoints(consumer,account_id,last_ledger_id,updated_ts) \
         VALUES(?1,?2,?3,?4) ON CONFLICT(consumer,account_id) DO UPDATE SET \
         last_ledger_id=MAX(last_ledger_id,excluded.last_ledger_id),updated_ts=excluded.updated_ts",
        rusqlite::params![consumer, account_id, last_ledger_id, now()],
    )?)
}

/// Delete charge detail only after the required pricing consumer has durably acknowledged it.
/// Top-ups/adjustments remain as the long-term accounting record.
pub fn ledger_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM ledger WHERE id IN ( \
           SELECT l.id FROM ledger l JOIN ledger_consumer_checkpoints c \
             ON c.account_id=l.account_id AND c.consumer='pricing' \
           WHERE l.kind='charge' AND l.ts < ?1 AND l.id <= c.last_ledger_id \
           ORDER BY l.id LIMIT 5000 \
         )",
        rusqlite::params![older_than_ts],
    )?)
}

/// Добавить строку в append-only ledger (журнал движений баланса). `model` — Claude-модель за
/// charge-строкой (для точного per-model графика); у topup/adjust модели нет → None.
#[allow(clippy::too_many_arguments)]
fn ledger_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    kind: &str,
    amount_nano: i64,
    reference: Option<&str>,
    balance_after: i64,
    model: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ledger(account_id, key, kind, amount_nano, ref, balance_after_nano, ts, model) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![account_id, key, kind, amount_nano, reference, balance_after, now(), model])?;
    Ok(())
}

/// Traffic predates provider attribution, or was queued by an engine release that only served
/// Claude. Either way the Claude fleet is the only upstream it could have used.
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
/// The OpenAI-compatible Codex home pool.
pub const PROVIDER_OPENAI: &str = "openai";

fn default_provider() -> String {
    PROVIDER_ANTHROPIC.to_string()
}

/// Разбивка одного оплаченного запроса по корзинам токенов + модель (owned — переживает канал
/// биллинг-актора). `real_nano` — стоимость по официальным ценам (×1.0, до наценки).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct UsageEventInput {
    pub model: String,
    /// Upstream that served the request: `anthropic` for the Claude fleet, `openai` for the
    /// Codex home pool. Defaulted on deserialization so settlement rows queued by a previous
    /// engine release stay readable across a blue-green promotion.
    #[serde(default = "default_provider")]
    pub provider: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    pub real_nano: i64,
    pub speed: String,
    pub inference_geo: String,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
    pub priced_ts: i64,
}

/// Create one durable SQLite reservation. Exact active replays return the original post-reserve
/// balance; a reused request ID with different parameters or a terminal request fails closed.
pub fn sqlite_reserve_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    lease_secs: i64,
) -> Result<Option<i64>> {
    if request_id.trim().is_empty()
        || account_id.trim().is_empty()
        || key.trim().is_empty()
        || hold_nano < 0
        || lease_secs <= 0
    {
        anyhow::bail!("invalid SQLite reservation parameters");
    }
    let tx = conn.unchecked_transaction()?;
    let existing = tx.query_row(
        "SELECT account_id,key,hold_nano,state,balance_after_reserve_nano \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        },
    );
    match existing {
        Ok((stored_account, stored_key, stored_hold, state, balance)) => {
            if stored_account != account_id || stored_key != key || stored_hold != hold_nano {
                anyhow::bail!("reservation request ID was reused with different parameters");
            }
            if state == "reserved" || state == "delivering" {
                tx.commit()?;
                return Ok(Some(balance));
            }
            anyhow::bail!("reservation request is already terminal");
        }
        Err(rusqlite::Error::QueryReturnedNoRows) => {}
        Err(error) => return Err(error.into()),
    }

    let Some(balance) = account_reserve_for_key(&tx, account_id, key, hold_nano)? else {
        tx.rollback()?;
        return Ok(None);
    };
    let timestamp = now();
    tx.execute(
        "INSERT INTO billing_reservations( \
           request_id,account_id,key,hold_nano,state,balance_after_reserve_nano,lease_until,created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,'reserved',?5,?6,?7,?7)",
        rusqlite::params![request_id, account_id, key, hold_nano, balance,
            timestamp.saturating_add(lease_secs), timestamp],
    )?;
    tx.commit()?;
    Ok(Some(balance))
}

/// Mark a reservation as provider-accepted before handing its response body to the client.
pub fn sqlite_mark_delivering(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid delivery lease");
    }
    let timestamp = now();
    let changed = conn.execute(
        "UPDATE billing_reservations SET state='delivering',lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )?;
    Ok(changed == 1)
}

pub fn sqlite_renew_reservation_lease(
    conn: &Connection,
    request_id: &str,
    lease_secs: i64,
) -> Result<bool> {
    if lease_secs <= 0 {
        anyhow::bail!("invalid reservation lease");
    }
    let timestamp = now();
    Ok(conn.execute(
        "UPDATE billing_reservations SET lease_until=?2,updated_ts=?3 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, timestamp.saturating_add(lease_secs), timestamp],
    )? == 1)
}

#[allow(clippy::too_many_arguments)]
fn sqlite_enqueue_settlement(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    let actual = actual_nano.max(0);
    let usage_json = usage.map(serde_json::to_string).transpose()?;
    let tx = conn.unchecked_transaction()?;
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano,reference \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, Option<String>>(6)?,
            ))
        },
    );
    let (stored_account, stored_key, stored_hold, state, stored_actual, stored_balance, stored_ref) =
        match reservation {
            Ok(row) => row,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                anyhow::bail!("settlement has no durable reservation")
            }
            Err(error) => return Err(error.into()),
        };
    if stored_account != account_id || stored_key != key || stored_hold != hold_nano {
        anyhow::bail!("settlement parameters do not match reservation");
    }
    if state == "settled" {
        if stored_actual == Some(actual) && stored_ref.as_deref() == reference {
            tx.commit()?;
            return Ok(stored_balance);
        }
        anyhow::bail!("settlement request ID was reused with different parameters");
    }

    let timestamp = now();
    let inserted = tx.execute(
        "INSERT INTO billing_settlement_outbox( \
           request_id,actual_nano,reference,usage_json,state,attempts,next_attempt_ts,created_ts,updated_ts) \
         VALUES(?1,?2,?3,?4,'pending',0,0,?5,?5) ON CONFLICT(request_id) DO NOTHING",
        rusqlite::params![request_id, actual, reference, usage_json, timestamp],
    )?;
    if inserted == 0 {
        let existing = tx.query_row(
            "SELECT actual_nano,reference,usage_json FROM billing_settlement_outbox WHERE request_id=?1",
            rusqlite::params![request_id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?)),
        )?;
        if existing != (actual, reference.map(str::to_owned), usage_json) {
            anyhow::bail!("settlement request ID was reused with different parameters");
        }
    }
    tx.commit()?;
    Ok(None)
}

/// Apply one already-durable SQLite settlement intent atomically with its ledger/usage rows.
pub fn sqlite_process_settlement(conn: &Connection, request_id: &str) -> Result<Option<i64>> {
    let tx = conn.unchecked_transaction()?;
    let outbox = tx.query_row(
        "SELECT actual_nano,reference,usage_json,state FROM billing_settlement_outbox WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?,
            row.get::<_, Option<String>>(2)?, row.get::<_, String>(3)?)),
    );
    let (actual, reference, usage_json, outbox_state) = match outbox {
        Ok(row) => row,
        Err(rusqlite::Error::QueryReturnedNoRows) => anyhow::bail!("settlement outbox row missing"),
        Err(error) => return Err(error.into()),
    };
    let reservation = tx.query_row(
        "SELECT account_id,key,hold_nano,state,actual_nano,balance_after_settle_nano \
         FROM billing_reservations WHERE request_id=?1",
        rusqlite::params![request_id],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
            ))
        },
    )?;
    if reservation.3 == "settled" || outbox_state == "done" {
        if reservation.4 != Some(actual) {
            anyhow::bail!("stored settlement differs from outbox");
        }
        tx.execute(
            "UPDATE billing_settlement_outbox SET state='done',committed_ts=COALESCE(committed_ts,?2), \
             updated_ts=?2,last_error=NULL WHERE request_id=?1",
            rusqlite::params![request_id, now()],
        )?;
        tx.commit()?;
        return Ok(reservation.5);
    }
    if reservation.3 != "reserved" && reservation.3 != "delivering" {
        anyhow::bail!("reservation is not settleable");
    }
    let usage = usage_json
        .as_deref()
        .map(serde_json::from_str::<UsageEventInput>)
        .transpose()?;
    let balance = account_settle_in(
        &tx,
        &reservation.0,
        &reservation.1,
        reservation.2,
        actual,
        reference.as_deref(),
        usage.as_ref(),
    )?
    .ok_or_else(|| anyhow::anyhow!("settlement account no longer exists"))?;
    let timestamp = now();
    tx.execute(
        "UPDATE billing_reservations SET state='settled',actual_nano=?2, \
         balance_after_settle_nano=?3,reference=?4,updated_ts=?5,settled_ts=?5 \
         WHERE request_id=?1 AND state IN ('reserved','delivering')",
        rusqlite::params![request_id, actual, balance, reference, timestamp],
    )?;
    tx.execute(
        "UPDATE billing_settlement_outbox SET state='done',attempts=attempts+1, \
         updated_ts=?2,committed_ts=?2,last_error=NULL WHERE request_id=?1",
        rusqlite::params![request_id, timestamp],
    )?;
    tx.commit()?;
    Ok(Some(balance))
}

#[allow(clippy::too_many_arguments)]
pub fn sqlite_settle_request(
    conn: &Connection,
    request_id: &str,
    account_id: &str,
    key: &str,
    hold_nano: i64,
    actual_nano: i64,
    reference: Option<&str>,
    usage: Option<&UsageEventInput>,
) -> Result<Option<i64>> {
    if let Some(balance) = sqlite_enqueue_settlement(
        conn,
        request_id,
        account_id,
        key,
        hold_nano,
        actual_nano,
        reference,
        usage,
    )? {
        return Ok(Some(balance));
    }
    match sqlite_process_settlement(conn, request_id) {
        Ok(result) => Ok(result),
        Err(error) => {
            let message: String = format!("{error:#}").chars().take(1000).collect();
            let timestamp = now();
            let _ = conn.execute(
                "UPDATE billing_settlement_outbox SET attempts=attempts+1,last_error=?2, \
                 updated_ts=?3,next_attempt_ts=?3+MIN(60,MAX(1,attempts+1)) WHERE request_id=?1",
                rusqlite::params![request_id, message, timestamp],
            );
            Err(error)
        }
    }
}

/// Retry persisted intents, then reconcile expired holds. Reserved requests are canceled; requests
/// marked delivering are charged their approved hold when exact usage never arrived.
pub fn sqlite_reconcile_expired(
    conn: &Connection,
    limit: usize,
) -> Result<crate::pg::ReconcileReport> {
    let limit = limit.clamp(1, 10_000) as i64;
    let timestamp = now();
    let pending: Vec<String> = {
        let mut statement = conn.prepare(
            "SELECT request_id FROM billing_settlement_outbox \
             WHERE state='pending' AND next_attempt_ts<=?1 ORDER BY created_ts LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, limit], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    let mut report = crate::pg::ReconcileReport::default();
    for request_id in pending {
        if sqlite_process_settlement(conn, &request_id).is_ok() {
            report.processed_outbox += 1;
        }
    }

    let remaining = (limit as usize).saturating_sub(report.processed_outbox) as i64;
    if remaining == 0 {
        return Ok(report);
    }
    let expired: Vec<(String, String, String, i64, String)> = {
        let mut statement = conn.prepare(
            "SELECT request_id,account_id,key,hold_nano,state FROM billing_reservations \
             WHERE state IN ('reserved','delivering') AND lease_until<=?1 \
             ORDER BY lease_until LIMIT ?2",
        )?;
        let rows = statement
            .query_map(rusqlite::params![timestamp, remaining], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    for (request_id, account_id, key, hold, state) in expired {
        let actual = if state == "delivering" { hold } else { 0 };
        match sqlite_settle_request(
            conn,
            &request_id,
            &account_id,
            &key,
            hold,
            actual,
            Some(if state == "delivering" {
                "lease-expired-delivering"
            } else {
                "lease-expired-reserved"
            }),
            None,
        ) {
            Ok(_) if state == "delivering" => report.charged_after_delivery += 1,
            Ok(_) => report.canceled_before_delivery += 1,
            Err(error) => {
                eprintln!("SQLite reservation recovery failed for {request_id}: {error:#}")
            }
        }
    }
    Ok(report)
}

pub fn sqlite_maintenance_prune(
    conn: &Connection,
    older_than_ts: i64,
) -> Result<crate::pg::MaintenanceReport> {
    let outbox = conn.execute(
        "DELETE FROM billing_settlement_outbox WHERE request_id IN ( \
           SELECT request_id FROM billing_settlement_outbox WHERE state='done' AND committed_ts<?1 \
           ORDER BY committed_ts LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    let reservations = conn.execute(
        "DELETE FROM billing_reservations WHERE request_id IN ( \
           SELECT request_id FROM billing_reservations WHERE state='settled' AND settled_ts<?1 \
             AND request_id NOT IN (SELECT request_id FROM billing_settlement_outbox) \
           ORDER BY settled_ts LIMIT 5000)",
        rusqlite::params![older_than_ts],
    )?;
    Ok(crate::pg::MaintenanceReport {
        outbox,
        reservations,
        ..Default::default()
    })
}

/// Записать usage-событие (аналитика; НЕ money-строка). Вызывается billing-writer'ом сразу после
/// `account_settle` на той же connection. `charge_nano` — фактически списанное (после наценки).
pub fn usage_event_add(
    conn: &Connection,
    account_id: &str,
    key: Option<&str>,
    u: &UsageEventInput,
    charge_nano: i64,
    reference: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO usage_events(account_id, key, model, input_tokens, output_tokens, \
         cache_read_tokens, cache_write_5m_tokens, cache_write_1h_tokens, web_search_requests, \
         real_nano, charge_nano, ref, ts, speed, inference_geo, input_nano, output_nano, \
         cache_read_nano, cache_write_5m_nano, cache_write_1h_nano, web_search_nano, priced_ts, \
         provider) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23)",
        rusqlite::params![
            account_id,
            key,
            u.model,
            u.input_tokens,
            u.output_tokens,
            u.cache_read_tokens,
            u.cache_write_5m_tokens,
            u.cache_write_1h_tokens,
            u.web_search_requests,
            u.real_nano,
            charge_nano,
            reference,
            now(),
            u.speed,
            u.inference_geo,
            u.input_nano,
            u.output_nano,
            u.cache_read_nano,
            u.cache_write_5m_nano,
            u.cache_write_1h_nano,
            u.web_search_nano,
            u.priced_ts,
            u.provider
        ],
    )?;
    Ok(())
}

/// Агрегат usage по модели за окно (ts ≥ `since_ts`). Суммы токенов по корзинам + real/charge nano
/// + число запросов. Долларовый эквивалент по корзинам считает вызывающий (server, через metering).
#[derive(Debug, Clone, Default)]
pub struct UsageModelAgg {
    pub model: String,
    pub requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_5m_tokens: i64,
    pub cache_write_1h_tokens: i64,
    pub web_search_requests: i64,
    pub real_nano: i64,
    pub charge_nano: i64,
    pub input_nano: i64,
    pub output_nano: i64,
    pub cache_read_nano: i64,
    pub cache_write_5m_nano: i64,
    pub cache_write_1h_nano: i64,
    pub web_search_nano: i64,
}

pub fn usage_by_model(
    conn: &Connection,
    account_id: &str,
    since_ts: i64,
) -> Result<Vec<UsageModelAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(model,''), COUNT(*), \
         COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0), \
         COALESCE(SUM(cache_read_tokens),0), COALESCE(SUM(cache_write_5m_tokens),0), \
         COALESCE(SUM(cache_write_1h_tokens),0), COALESCE(SUM(web_search_requests),0), \
         COALESCE(SUM(real_nano),0), COALESCE(SUM(charge_nano),0), \
         COALESCE(SUM(input_nano),0), COALESCE(SUM(output_nano),0), \
         COALESCE(SUM(cache_read_nano),0), COALESCE(SUM(cache_write_5m_nano),0), \
         COALESCE(SUM(cache_write_1h_nano),0), COALESCE(SUM(web_search_nano),0) \
         FROM usage_events WHERE account_id=?1 AND ts>=?2 GROUP BY model ORDER BY SUM(real_nano) DESC")?;
    let rows = stmt.query_map(rusqlite::params![account_id, since_ts], |r| {
        Ok(UsageModelAgg {
            model: r.get(0)?,
            requests: r.get(1)?,
            input_tokens: r.get(2)?,
            output_tokens: r.get(3)?,
            cache_read_tokens: r.get(4)?,
            cache_write_5m_tokens: r.get(5)?,
            cache_write_1h_tokens: r.get(6)?,
            web_search_requests: r.get(7)?,
            real_nano: r.get(8)?,
            charge_nano: r.get(9)?,
            input_nano: r.get(10)?,
            output_nano: r.get(11)?,
            cache_read_nano: r.get(12)?,
            cache_write_5m_nano: r.get(13)?,
            cache_write_1h_nano: r.get(14)?,
            web_search_nano: r.get(15)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Агрегат расхода ПО АККАУНТАМ за окно (ts ≥ `since_ts`): списано клиенту (charge) +
/// real-API стоимость + число запросов. Для панели «кто тратит» (сегодня/7д/30д).
#[derive(Debug, Clone, Default)]
pub struct SpendAccountAgg {
    pub account_id: String,
    pub handle: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
    pub last_ts: i64,
}

pub fn spend_by_account(
    conn: &Connection,
    since_ts: i64,
    limit: i64,
) -> Result<Vec<SpendAccountAgg>> {
    let mut stmt = conn.prepare(
        "SELECT u.account_id, COALESCE(a.handle,''), COUNT(*), \
         COALESCE(SUM(u.charge_nano),0), COALESCE(SUM(u.real_nano),0), COALESCE(MAX(u.ts),0) \
         FROM usage_events u LEFT JOIN accounts a ON a.id=u.account_id \
         WHERE u.ts>=?1 GROUP BY u.account_id, a.handle ORDER BY SUM(u.charge_nano) DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts, limit], |r| {
        Ok(SpendAccountAgg {
            account_id: r.get(0)?,
            handle: r.get(1)?,
            requests: r.get(2)?,
            charge_nano: r.get(3)?,
            real_nano: r.get(4)?,
            last_ts: r.get(5)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Расход по ПРОВАЙДЕРУ за окно (ts ≥ `since_ts`). Claude-флот и Codex-пул сеттлятся в одни и те же
/// денежные таблицы, поэтому «сколько заработал каждый апстрим» читается только из явной колонки.
#[derive(Debug, Clone, Default)]
pub struct SpendProviderAgg {
    pub provider: String,
    pub requests: i64,
    pub charge_nano: i64,
    pub real_nano: i64,
}

pub fn spend_by_provider(conn: &Connection, since_ts: i64) -> Result<Vec<SpendProviderAgg>> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(NULLIF(provider,''),'anthropic'), COUNT(*), \
         COALESCE(SUM(charge_nano),0), COALESCE(SUM(real_nano),0) \
         FROM usage_events WHERE ts>=?1 GROUP BY 1 ORDER BY SUM(charge_nano) DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![since_ts], |r| {
        Ok(SpendProviderAgg {
            provider: r.get(0)?,
            requests: r.get(1)?,
            charge_nano: r.get(2)?,
            real_nano: r.get(3)?,
        })
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Обрезать usage_events под масштаб (как ledger_prune): удалить строки старше `older_than_ts`
/// батчами, отдавая write-lock между ними. Возвращает удалённое.
pub fn usage_prune(conn: &Connection, older_than_ts: i64) -> Result<usize> {
    const BATCH: i64 = 5000;
    let mut total = 0usize;
    loop {
        let n = conn.execute(
            "DELETE FROM usage_events WHERE id IN \
             (SELECT id FROM usage_events WHERE ts < ?1 LIMIT ?2)",
            rusqlite::params![older_than_ts, BATCH],
        )?;
        total += n;
        if (n as i64) < BATCH {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(total)
}

/// Прочитать ключ (для авторизации/`/balance`).
pub fn key_get(conn: &Connection, key: &str) -> Result<Option<KeyRow>> {
    let row = conn.query_row(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0), \
         (SELECT MAX(u.ts) FROM usage_events u WHERE u.account_id=k.account_id AND u.key=k.key), \
         COALESCE(k.status,'active') \
         FROM api_keys k WHERE k.key=?1",
        rusqlite::params![key],
        |r| {
            Ok(KeyRow {
                key: r.get::<_, String>(0)?,
                key_id: r.get::<_, String>(1)?,
                account_id: r.get::<_, Option<String>>(2)?,
                label: r.get::<_, Option<String>>(3)?,
                spent_nano: r.get::<_, i64>(4)?,
                reserved_nano: r.get(5)?,
                spend_limit_nano: r.get(6)?,
                expires_ts: r.get(7)?,
                created_ts: r.get(8)?,
                last_used_ts: r.get(9)?,
                status: r.get(10)?,
            })
        },
    );
    match row {
        Ok(k) => Ok(Some(k)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn key_set_status(conn: &Connection, key: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE api_keys SET status=?1 WHERE key=?2",
        rusqlite::params![status, key],
    )?)
}

/// Change key status through its non-secret control-plane identifier.
pub fn key_set_status_by_id(conn: &Connection, key_id: &str, status: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE api_keys SET status=?1 WHERE key_id=?2",
        rusqlite::params![status, key_id],
    )?)
}

/// Change key label through its non-secret control-plane identifier.
pub fn key_set_label_by_id(conn: &Connection, key_id: &str, label: &str) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE api_keys SET label=?2 WHERE key_id=?1",
        rusqlite::params![key_id, label],
    )?)
}

/// Atomically replace a key policy without allowing a new limit to undercut committed or in-flight
/// usage. `None` clears the corresponding guardrail.
pub fn key_set_policy_by_id(
    conn: &Connection,
    account_id: &str,
    key_id: &str,
    spend_limit_nano: Option<i64>,
    expires_ts: Option<i64>,
) -> Result<KeyPolicyUpdate> {
    let updated = conn.execute(
        "UPDATE api_keys SET spend_limit_nano=?3, expires_ts=?4 \
         WHERE key_id=?1 AND account_id=?2 \
           AND (?3 IS NULL OR (reserved_nano<=?3 AND spent_nano<=?3-reserved_nano)) \
           AND (?4 IS NULL OR ?4>CAST(strftime('%s','now') AS INTEGER))",
        rusqlite::params![key_id, account_id, spend_limit_nano, expires_ts],
    )?;
    if updated == 1 {
        return Ok(KeyPolicyUpdate::Updated);
    }
    let exists = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM api_keys WHERE key_id=?1 AND account_id=?2)",
        rusqlite::params![key_id, account_id],
        |row| row.get::<_, bool>(0),
    )?;
    if !exists {
        return Ok(KeyPolicyUpdate::NotFound);
    }
    if expires_ts.is_some_and(|expires| expires <= now()) {
        return Ok(KeyPolicyUpdate::ExpiryNotFuture);
    }
    Ok(KeyPolicyUpdate::LimitBelowUsage)
}

/// Удалить ключ НАВСЕГДА (в отличие от set_status 'disabled' — строка исчезает).
pub fn key_remove(conn: &Connection, key: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys WHERE key=?1", rusqlite::params![key])?)
}

/// Удалить ВСЕ ключи (для очистки/тестов). Возвращает число удалённых.
pub fn key_clear(conn: &Connection) -> Result<usize> {
    Ok(conn.execute("DELETE FROM api_keys", [])?)
}

/// Ключи КОНКРЕТНОГО аккаунта (для дашборда коммерции: список ключей юзера). Ключ маскируется на выводе.
pub fn keys_by_account(conn: &Connection, account_id: &str) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events WHERE account_id=?1 GROUP BY key \
         ) u ON u.key=k.key WHERE k.account_id=?1 ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Строка журнала движений баланса (для истории трат/пополнений в дашборде).
#[derive(Debug, Clone)]
pub struct LedgerRow {
    pub id: i64,
    pub key: Option<String>,
    pub kind: String,     // topup | charge | adjust
    pub amount_nano: i64, // + пополнение / − списание
    pub reference: Option<String>,
    pub balance_after_nano: Option<i64>,
    pub ts: i64,
    pub model: Option<String>, // Claude-модель за charge (для per-model графика); topup/adjust → None
}

/// Последние `limit` строк ledger аккаунта (свежие сверху). Для дашборда «история/расход».
pub fn ledger_recent(conn: &Connection, account_id: &str, limit: i64) -> Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, key, kind, amount_nano, ref, balance_after_nano, ts, model \
         FROM ledger WHERE account_id=?1 ORDER BY id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(rusqlite::params![account_id, limit.clamp(1, 1000)], |r| {
        Ok(LedgerRow {
            id: r.get::<_, i64>(0)?,
            key: r.get::<_, Option<String>>(1)?,
            kind: r.get::<_, String>(2)?,
            amount_nano: r.get::<_, i64>(3)?,
            reference: r.get::<_, Option<String>>(4)?,
            balance_after_nano: r.get::<_, Option<i64>>(5)?,
            ts: r.get::<_, i64>(6)?,
            model: r.get::<_, Option<String>>(7)?,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Ledger cursor for durable external consumers. Rows are returned oldest-first after `after_id`.
pub fn ledger_after(
    conn: &Connection,
    account_id: &str,
    after_id: i64,
    limit: i64,
) -> Result<Vec<LedgerRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, key, kind, amount_nano, ref, balance_after_nano, ts, model \
         FROM ledger WHERE account_id=?1 AND id>?2 ORDER BY id ASC LIMIT ?3",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![account_id, after_id.max(0), limit.clamp(1, 1000)],
        |r| {
            Ok(LedgerRow {
                id: r.get::<_, i64>(0)?,
                key: r.get::<_, Option<String>>(1)?,
                kind: r.get::<_, String>(2)?,
                amount_nano: r.get::<_, i64>(3)?,
                reference: r.get::<_, Option<String>>(4)?,
                balance_after_nano: r.get::<_, Option<i64>>(5)?,
                ts: r.get::<_, i64>(6)?,
                model: r.get::<_, Option<String>>(7)?,
            })
        },
    )?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

/// Все ключи (для CLI-листинга; ключ маскируется на стороне вывода).
pub fn key_list(conn: &Connection) -> Result<Vec<KeyRow>> {
    let mut stmt = conn.prepare(
        "SELECT k.key,k.key_id,k.account_id,k.label,k.spent_nano,k.reserved_nano, \
         k.spend_limit_nano,k.expires_ts,COALESCE(k.created_ts,0),u.last_used_ts, \
         COALESCE(k.status,'active') FROM api_keys k LEFT JOIN ( \
           SELECT key,MAX(ts) AS last_used_ts FROM usage_events GROUP BY key \
         ) u ON u.key=k.key ORDER BY COALESCE(k.created_ts,0)",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(KeyRow {
            key: r.get::<_, String>(0)?,
            key_id: r.get::<_, String>(1)?,
            account_id: r.get::<_, Option<String>>(2)?,
            label: r.get::<_, Option<String>>(3)?,
            spent_nano: r.get::<_, i64>(4)?,
            reserved_nano: r.get(5)?,
            spend_limit_nano: r.get(6)?,
            expires_ts: r.get(7)?,
            created_ts: r.get(8)?,
            last_used_ts: r.get(9)?,
            status: r.get(10)?,
        })
    })?;
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
    /// Process-local increment since the last successful persistence operation.
    pub spent_delta_usd: f64,
    pub util5h: f64,
    pub util7d: f64,
    pub reset5h: i64,
    pub reset7d: i64,
    pub calib_n: i64,
    /// PostgreSQL CAS version. SQLite compatibility rows use zero.
    pub version: i64,
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
            stmt.execute(rusqlite::params![
                r.email,
                r.cooling_until,
                r.cap5h_usd,
                r.cap7d_usd,
                r.spent_total_usd,
                r.util5h,
                r.util7d,
                r.reset5h,
                r.reset7d,
                r.calib_n,
                ts
            ])?;
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
    let rows = stmt.query_map([], |r| {
        Ok(PoolStateRow {
            email: r.get(0)?,
            cooling_until: r.get(1)?,
            cap5h_usd: r.get(2)?,
            cap7d_usd: r.get(3)?,
            spent_total_usd: r.get(4)?,
            spent_delta_usd: 0.0,
            util5h: r.get(5)?,
            util7d: r.get(6)?,
            reset5h: r.get(7)?,
            reset7d: r.get(8)?,
            calib_n: r.get(9)?,
            version: 0,
        })
    })?;
    Ok(rows.filter_map(|x| x.ok()).collect())
}

// (Синхронная обёртка `Billing` удалена: запросный путь теперь через `forward::AsyncBilling`
//  — DB-акторы, ноль синхронных вызовов на async-воркерах. registry остаётся чистым sync-ядром.)

/// Агрегаты биллинга по всем аккаунтам клиентов (нанодоллары).
#[derive(Clone, Debug, Default)]
pub struct BillingTotals {
    pub balance_nano: i64,  // суммарный остаток на аккаунтах (клиентский флоат)
    pub spent_nano: i64,    // суммарно списано за всё время
    pub reserved_nano: i64, // сейчас в незакрытых резервах (in-flight холды)
    pub active_accounts: i64,
}

/// Суммы по accounts одним запросом (источник истины — БД). Ошибка возвращается вызывающему коду.
pub fn billing_totals(conn: &Connection) -> Result<BillingTotals> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(balance_nano),0), COALESCE(SUM(spent_nano),0), \
         COALESCE(SUM(reserved_nano),0), COALESCE(SUM(CASE WHEN COALESCE(status,'active')='active' \
         THEN 1 ELSE 0 END),0) FROM accounts",
        [],
        |r| {
            Ok(BillingTotals {
                balance_nano: r.get(0)?,
                spent_nano: r.get(1)?,
                reserved_nano: r.get(2)?,
                active_accounts: r.get(3)?,
            })
        },
    )?)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Connection {
        open(":memory:").unwrap()
    }

    /// Персист состояния пула: save→load переносит cooling/калибровку (upsert по email).
    #[test]
    fn pool_state_save_load_roundtrip() {
        let c = db();
        let rows = vec![PoolStateRow {
            email: "a@x.io".into(),
            cooling_until: 123456,
            cap5h_usd: 50.0,
            cap7d_usd: 1500.0,
            spent_total_usd: 12.5,
            util5h: 0.3,
            util7d: 0.1,
            reset5h: 999,
            reset7d: 888,
            calib_n: 4,
            version: 0,
            spent_delta_usd: 0.0,
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

    #[test]
    fn authoritative_database_uses_full_synchronous_durability() {
        let c = db();
        let synchronous: i64 = c.query_row("PRAGMA synchronous", [], |r| r.get(0)).unwrap();
        assert_eq!(synchronous, 2); // SQLite FULL
    }

    #[test]
    fn open_fails_closed_when_legacy_topup_references_are_duplicated() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "registry-duplicate-ref-{}-{unique}.db",
            std::process::id()
        ));
        {
            let c = Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE ledger(id INTEGER PRIMARY KEY AUTOINCREMENT, account_id TEXT NOT NULL, \
                 key TEXT, kind TEXT NOT NULL, amount_nano INTEGER NOT NULL, ref TEXT, \
                 balance_after_nano INTEGER, ts INTEGER, model TEXT); \
                 INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup'); \
                 INSERT INTO ledger(account_id,kind,amount_nano,ref) VALUES('a','topup',1,'dup');",
            ).unwrap();
        }
        assert!(open(path.to_str().unwrap()).is_err());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(format!("{}-wal", path.display()));
        let _ = fs::remove_file(format!("{}-shm", path.display()));
    }

    #[test]
    fn legacy_keys_with_same_suffix_migrate_to_distinct_accounts() {
        let c = db();
        c.execute(
            "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
             VALUES(?1,?2,?3,?4,?5,'active',0)",
            rusqlite::params!["sk-user-a-123456789abc", "legacy_a", 100, 10, 2000],
        ).unwrap();
        c.execute(
            "INSERT INTO api_keys(key,key_id,balance_nano,spent_nano,mult_bp,status,reserved_nano) \
             VALUES(?1,?2,?3,?4,?5,'active',0)",
            rusqlite::params!["sk-user-b-123456789abc", "legacy_b", 200, 20, 3000],
        ).unwrap();
        migrate_legacy_keys(&c).unwrap();
        let a = key_get(&c, "sk-user-a-123456789abc")
            .unwrap()
            .unwrap()
            .account_id
            .unwrap();
        let b = key_get(&c, "sk-user-b-123456789abc")
            .unwrap()
            .unwrap()
            .account_id
            .unwrap();
        assert_ne!(a, b);
        assert_eq!(account_get(&c, &a).unwrap().unwrap().balance_nano, 100);
        assert_eq!(account_get(&c, &b).unwrap().unwrap().balance_nano, 200);
    }

    /// Агрегаты трат (для /metrics): суммы по аккаунтам + число активных.
    #[test]
    fn billing_totals_aggregates_across_accounts() {
        let c = db();
        acct_with_key(&c, "acct_1", "sk-1", 5_000_000_000, 10000); // $5
        acct_with_key(&c, "acct_2", "sk-2", 3_000_000_000, 10000); // $3
        account_reserve(&c, "acct_1", 1_000_000_000).unwrap();
        account_settle(&c, "acct_1", "sk-1", 1_000_000_000, 400_000_000, None, None).unwrap(); // spent $0.4
        account_reserve(&c, "acct_2", 500_000_000).unwrap(); // висящий резерв $0.5
        account_set_status(&c, "acct_2", "disabled").unwrap();
        let t = billing_totals(&c).unwrap();
        assert_eq!(t.balance_nano, 4_600_000_000 + 2_500_000_000); // $4.6 + $2.5
        assert_eq!(t.spent_nano, 400_000_000);
        assert_eq!(t.reserved_nano, 500_000_000);
        assert_eq!(t.active_accounts, 1);
    }

    /// Без per-request identity старт не может доказать, что резерв осиротел: fail-closed оставляет hold.
    #[test]
    fn reconcile_does_not_refund_unowned_aggregate_reservations() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 2000);
        account_reserve(&c, "a", 600_000_000).unwrap();
        assert_eq!(reconcile_reservations(&c).unwrap(), 0);
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 400_000_000);
        assert_eq!(acc.reserved_nano, 600_000_000);
    }

    /// reserve атомарно гейтит по балансу аккаунта; settle сводит пару к −actual; per-key spent + ledger.
    #[test]
    fn reserve_gates_and_settle_nets_to_actual() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 2000); // $1.00
        assert_eq!(
            account_reserve(&c, "a", 600_000_000).unwrap(),
            Some(400_000_000)
        );
        assert_eq!(account_reserve(&c, "a", 600_000_000).unwrap(), None); // $0.40 < $0.60 → отказ
        assert_eq!(
            account_settle(&c, "a", "k", 600_000_000, 100_000_000, Some("req1"), None).unwrap(),
            Some(900_000_000)
        );
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 900_000_000);
        assert_eq!(acc.spent_nano, 100_000_000);
        // per-key атрибуция: spent по ключу тоже $0.10
        assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 100_000_000);
        // ledger: строка topup ($1) + строка charge ($0.10)
        let cnt: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM ledger WHERE account_id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(cnt, 2);
    }

    /// Exact provider usage is never silently clamped to the estimate held before delivery.
    #[test]
    fn settle_records_exact_actual_above_hold() {
        let c = db();
        acct_with_key(&c, "a", "k", 100, 2000);
        assert_eq!(account_reserve(&c, "a", 100).unwrap(), Some(0));
        assert_eq!(
            account_settle(&c, "a", "k", 100, 150, Some("req"), None).unwrap(),
            Some(-50)
        );
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, -50);
        assert_eq!(acc.spent_nano, 150);
        assert_eq!(acc.reserved_nano, 0);
        assert_eq!(key_get(&c, "k").unwrap().unwrap().spent_nano, 150);
        assert_eq!(ledger_recent(&c, "a", 10).unwrap()[0].amount_nano, 150);
    }

    #[test]
    fn sqlite_request_lifecycle_is_exactly_once() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000, 2000);
        assert_eq!(
            sqlite_reserve_request(&c, "req", "a", "k", 400, 60).unwrap(),
            Some(600)
        );
        assert_eq!(
            sqlite_reserve_request(&c, "req", "a", "k", 400, 60).unwrap(),
            Some(600)
        );
        assert!(sqlite_mark_delivering(&c, "req", 60).unwrap());
        assert_eq!(
            sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None)
                .unwrap(),
            Some(850),
        );
        assert_eq!(
            sqlite_settle_request(&c, "req", "a", "k", 400, 150, Some("provider:req"), None)
                .unwrap(),
            Some(850),
        );
        let account = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(
            (
                account.balance_nano,
                account.spent_nano,
                account.reserved_nano
            ),
            (850, 150, 0)
        );
        assert_eq!(
            c.query_row(
                "SELECT COUNT(*) FROM ledger WHERE kind='charge'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
            1,
        );
    }

    #[test]
    fn sqlite_pending_settlement_survives_until_recovery() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000, 2000);
        sqlite_reserve_request(&c, "req", "a", "k", 500, 60).unwrap();
        sqlite_mark_delivering(&c, "req", 60).unwrap();
        // Simulate a process crash after durable intent commit but before the balance transaction.
        assert_eq!(
            sqlite_enqueue_settlement(&c, "req", "a", "k", 500, 175, Some("provider:req"), None)
                .unwrap(),
            None,
        );
        let before = account_get(&c, "a").unwrap().unwrap();
        assert_eq!((before.balance_nano, before.reserved_nano), (500, 500));
        let report = sqlite_reconcile_expired(&c, 100).unwrap();
        assert_eq!(report.processed_outbox, 1);
        let after = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(
            (after.balance_nano, after.spent_nano, after.reserved_nano),
            (825, 175, 0)
        );
    }

    #[test]
    fn sqlite_expired_reservations_follow_delivery_state() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000, 2000);
        sqlite_reserve_request(&c, "pre", "a", "k", 200, 60).unwrap();
        sqlite_reserve_request(&c, "delivered", "a", "k", 300, 60).unwrap();
        sqlite_mark_delivering(&c, "delivered", 60).unwrap();
        c.execute("UPDATE billing_reservations SET lease_until=0", [])
            .unwrap();
        let report = sqlite_reconcile_expired(&c, 100).unwrap();
        assert_eq!(report.canceled_before_delivery, 1);
        assert_eq!(report.charged_after_delivery, 1);
        let account = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(
            (
                account.balance_nano,
                account.spent_nano,
                account.reserved_nano
            ),
            (700, 300, 0)
        );
    }

    #[test]
    fn token_source_switch_resets_only_stale_health() {
        let c = db();
        add(&c, "sub@example.com", "token-a", "", "prod").unwrap();
        let dead = SubHealth {
            email: "sub@example.com".into(),
            auth_state: "dead".into(),
            auth_fail_streak: 3,
            first_auth_fail_ts: 1,
            last_auth_fail_ts: 2,
            last_auth_http: 401,
            dead_since_ts: 2,
            dead_reason: "authentication_error".into(),
            auth_token_fp: "old-fingerprint".into(),
        };
        save_sub_health(&c, &dead).unwrap();

        add(&c, "sub@example.com", "token-a", "proxy", "prod").unwrap();
        assert_eq!(load_sub_health(&c, None).unwrap()[0].auth_state, "dead");

        add(&c, "sub@example.com", "token-b", "proxy", "prod").unwrap();
        let changed = &load_sub_health(&c, None).unwrap()[0];
        assert_eq!(
            (changed.auth_state.as_str(), changed.auth_fail_streak),
            ("healthy", 0)
        );
        assert!(changed.auth_token_fp.is_empty());
        let sources: (Option<String>, Option<String>) = c
            .query_row(
                "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(sources, (Some("token-b".into()), None));

        add_file(&c, "sub@example.com", "/tmp/token", "proxy", "prod").unwrap();
        let sources: (Option<String>, Option<String>) = c
            .query_row(
                "SELECT token,token_file FROM subs WHERE email='sub@example.com'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(sources, (None, Some("/tmp/token".into())));
    }

    /// Двойной settle (перекрытие деплоя: reconcile уже вернул резерв, затем settle старого инстанса)
    /// НЕ переначисляет и НЕ уводит reserved в минус — кламп MIN(hold,reserved)/MAX(0,…).
    #[test]
    fn double_settle_no_overcredit() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 10000);
        account_reserve(&c, "a", 400_000_000).unwrap();
        // Эмулируем внешний/исторический возврат hold до прихода старого settle.
        c.execute(
            "UPDATE accounts SET balance_nano=balance_nano+reserved_nano, reserved_nano=0 WHERE id='a'",
            [],
        ).unwrap();
        assert_eq!(
            account_get(&c, "a").unwrap().unwrap().balance_nano,
            1_000_000_000
        );
        // теперь прилетает settle СТАРОГО инстанса на тот же hold (actual $0.1)
        account_settle(&c, "a", "k", 400_000_000, 100_000_000, None, None).unwrap();
        let acc = account_get(&c, "a").unwrap().unwrap();
        // без клампа было бы: +$0.4 (второй раз!) − $0.1 = $1.3 (over-credit) и reserved=−$0.4.
        // с клампом: MIN(0.4, reserved=0)=0 → баланс += 0 − $0.1 = $0.9; reserved MAX(0,−0.4)=0.
        assert_eq!(
            acc.balance_nano, 900_000_000,
            "нет over-credit: списан только actual"
        );
        assert_eq!(acc.reserved_nano, 0, "reserved не ушёл в минус");
    }

    /// release (settle с actual=0) возвращает резерв полностью, ledger-charge НЕ пишется.
    #[test]
    fn reserve_release_refunds_fully() {
        let c = db();
        acct_with_key(&c, "a", "k", 500_000_000, 2000);
        account_reserve(&c, "a", 200_000_000).unwrap();
        account_settle(&c, "a", "k", 200_000_000, 0, None, None).unwrap();
        assert_eq!(
            account_get(&c, "a").unwrap().unwrap().balance_nano,
            500_000_000
        );
        let charges: i64 = c
            .query_row("SELECT COUNT(*) FROM ledger WHERE kind='charge'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(charges, 0);
    }

    /// usage_events: запись по корзинам и агрегат по модели (суммы + real/charge nano + requests).
    #[test]
    fn usage_events_aggregate_by_model() {
        let c = db();
        acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
        let opus = UsageEventInput {
            model: "claude-opus-4-8".into(),
            input_tokens: 1000,
            output_tokens: 500,
            cache_read_tokens: 200,
            cache_write_5m_tokens: 100,
            cache_write_1h_tokens: 50,
            web_search_requests: 2,
            real_nano: 20_000_000,
            ..Default::default()
        };
        usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req1")).unwrap();
        usage_event_add(&c, "a", Some("k"), &opus, 8_000_000, Some("req2")).unwrap();
        let sonnet = UsageEventInput {
            model: "claude-sonnet-5".into(),
            input_tokens: 300,
            output_tokens: 100,
            real_nano: 5_000_000,
            ..Default::default()
        };
        usage_event_add(&c, "a", Some("k"), &sonnet, 2_000_000, Some("req3")).unwrap();

        let aggs = usage_by_model(&c, "a", 0).unwrap();
        assert_eq!(aggs.len(), 2);
        // сортировка по SUM(real_nano) DESC → opus первый (2×20M > 5M)
        let o = &aggs[0];
        assert_eq!(o.model, "claude-opus-4-8");
        assert_eq!(o.requests, 2);
        assert_eq!(o.input_tokens, 2000); // 2×1000
        assert_eq!(o.output_tokens, 1000);
        assert_eq!(o.cache_read_tokens, 400);
        assert_eq!(o.cache_write_5m_tokens, 200);
        assert_eq!(o.cache_write_1h_tokens, 100);
        assert_eq!(o.web_search_requests, 4);
        assert_eq!(o.real_nano, 40_000_000);
        assert_eq!(o.charge_nano, 16_000_000);
        assert_eq!(aggs[1].model, "claude-sonnet-5");
        assert_eq!(aggs[1].requests, 1);
        // окно отсекает по ts: since в будущем → пусто
        assert!(usage_by_model(&c, "a", now() + 10_000).unwrap().is_empty());
        // prune всего → таблица пуста
        assert!(usage_prune(&c, now() + 10_000).unwrap() >= 3);
        assert!(usage_by_model(&c, "a", 0).unwrap().is_empty());
    }

    /// Оба апстрима сеттлятся в одни и те же денежные таблицы, поэтому «кто заработал» должно
    /// читаться из явной колонки, а не угадываться по имени модели.
    #[test]
    fn spend_is_attributed_to_the_serving_provider() {
        let c = db();
        acct_with_key(&c, "a", "k", 100_000_000_000, 4000);
        let claude = UsageEventInput {
            model: "claude-opus-5".into(),
            provider: PROVIDER_ANTHROPIC.into(),
            real_nano: 20_000_000,
            ..Default::default()
        };
        let codex = UsageEventInput {
            model: "gpt-5.6".into(),
            provider: PROVIDER_OPENAI.into(),
            real_nano: 5_000_000,
            ..Default::default()
        };
        usage_event_add(&c, "a", Some("k"), &claude, 8_000_000, Some("req1")).unwrap();
        usage_event_add(&c, "a", Some("k"), &codex, 2_000_000, Some("req2")).unwrap();
        usage_event_add(&c, "a", Some("k"), &codex, 3_000_000, Some("req3")).unwrap();

        let rows = spend_by_provider(&c, 0).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);
        assert_eq!(rows[0].requests, 1);
        assert_eq!(rows[0].charge_nano, 8_000_000);
        assert_eq!(rows[1].provider, PROVIDER_OPENAI);
        assert_eq!(rows[1].requests, 2);
        assert_eq!(rows[1].charge_nano, 5_000_000);
        assert_eq!(rows[1].real_nano, 10_000_000);
        // Окно отсекает по ts, как и остальные агрегаты панели.
        assert!(spend_by_provider(&c, now() + 10_000).unwrap().is_empty());
    }

    /// Строка, записанная релизом без атрибуции, должна читаться как Claude, а не выпадать из
    /// разбивки: blue-green оставляет предыдущий слот пишущим во время промоушена.
    #[test]
    fn usage_written_before_attribution_reads_as_the_claude_fleet() {
        let c = db();
        acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
        let legacy = UsageEventInput {
            model: "claude-opus-5".into(),
            real_nano: 1_000_000,
            ..Default::default()
        };
        usage_event_add(&c, "a", Some("k"), &legacy, 1_000_000, Some("req1")).unwrap();
        c.execute("UPDATE usage_events SET provider=''", []).unwrap();
        let rows = spend_by_provider(&c, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].provider, PROVIDER_ANTHROPIC);

        // The queued settlement payload is JSON: a row serialized by the previous release carries
        // every field except this one, and must still decode instead of poisoning the outbox.
        let mut payload: serde_json::Value = serde_json::to_value(&legacy).unwrap();
        payload.as_object_mut().unwrap().remove("provider");
        let decoded: UsageEventInput = serde_json::from_value(payload).unwrap();
        assert_eq!(decoded.provider, PROVIDER_ANTHROPIC);
        assert_eq!(decoded.model, "claude-opus-5");
    }

    /// settle пишет usage_event В ТОЙ ЖЕ операции (один коммит); при actual=0 usage НЕ пишется.
    #[test]
    fn settle_writes_usage_event_in_same_tx() {
        let c = db();
        acct_with_key(&c, "a", "k", 10_000_000_000, 4000);
        account_reserve(&c, "a", 1_000_000_000).unwrap();
        let u = UsageEventInput {
            model: "claude-opus-4-8".into(),
            input_tokens: 100,
            output_tokens: 50,
            real_nano: 5_000_000,
            ..Default::default()
        };
        account_settle(
            &c,
            "a",
            "k",
            1_000_000_000,
            400_000_000,
            Some("req1"),
            Some(&u),
        )
        .unwrap();
        let charges: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM ledger WHERE kind='charge' AND account_id='a'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(charges, 1, "charge записан");
        // charge-строка несёт модель (для точного per-model графика); topup/adjust — NULL.
        assert_eq!(
            ledger_recent(&c, "a", 10).unwrap()[0].model.as_deref(),
            Some("claude-opus-4-8"),
            "модель проставлена в ledger-charge"
        );
        let agg = usage_by_model(&c, "a", 0).unwrap();
        assert_eq!(agg.len(), 1);
        assert_eq!(agg[0].model, "claude-opus-4-8");
        assert_eq!(agg[0].input_tokens, 100);
        assert_eq!(agg[0].charge_nano, 400_000_000);
        // actual=0 (release/refund) → usage НЕ добавляется (charge не было)
        account_reserve(&c, "a", 500_000_000).unwrap();
        account_settle(&c, "a", "k", 500_000_000, 0, None, Some(&u)).unwrap();
        assert_eq!(
            usage_by_model(&c, "a", 0).unwrap()[0].requests,
            1,
            "usage не прибавился при actual=0"
        );
    }

    /// group-commit: reserve/settle в ОДНОЙ транзакции видят эффекты предыдущих (атомарность
    /// `charge≤hold≤balance` сохранена), результаты в порядке ops, settle пишет usage.
    #[test]
    fn hot_batch_sequential_and_atomic() {
        let c = db();
        acct_with_key(&c, "a", "k", 1_000_000_000, 4000);
        // 3 резерва по 400M в одной пачке: 3-й видит списания первых двух → отказ (None).
        let ops = vec![
            HotOp::Reserve {
                account_id: "a",
                key: "k",
                hold: 400_000_000,
            },
            HotOp::Reserve {
                account_id: "a",
                key: "k",
                hold: 400_000_000,
            },
            HotOp::Reserve {
                account_id: "a",
                key: "k",
                hold: 400_000_000,
            },
        ];
        let r = apply_hot_batch(&c, &ops).unwrap();
        assert_eq!(r[0], Some(600_000_000));
        assert_eq!(r[1], Some(200_000_000));
        assert_eq!(
            r[2], None,
            "3-й резерв видит эффекты предыдущих в той же tx → отказ"
        );
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 200_000_000);
        assert_eq!(acc.reserved_nano, 800_000_000);
        // settle в пачке: возвращает hold − actual, пишет usage; release (actual=0) возвращает hold.
        let u = UsageEventInput {
            model: "claude-opus-4-8".into(),
            input_tokens: 10,
            real_nano: 1000,
            ..Default::default()
        };
        let ops2 = vec![
            HotOp::Settle {
                account_id: "a",
                key: "k",
                hold: 400_000_000,
                actual: 100_000_000,
                reference: Some("r1"),
                usage: Some(&u),
            },
            HotOp::Settle {
                account_id: "a",
                key: "k",
                hold: 400_000_000,
                actual: 0,
                reference: None,
                usage: None,
            },
        ];
        apply_hot_batch(&c, &ops2).unwrap();
        let acc = account_get(&c, "a").unwrap().unwrap();
        assert_eq!(acc.balance_nano, 900_000_000); // 200 +300(settle1) +400(settle2)
        assert_eq!(acc.reserved_nano, 0);
        assert_eq!(acc.spent_nano, 100_000_000);
        assert_eq!(
            usage_by_model(&c, "a", 0).unwrap().len(),
            1,
            "usage записан из батча"
        );
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
        assert_eq!(
            account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
            Some(10_000_000_000)
        );
        // ПОВТОР того же вебхука (ретрай) — баланс НЕ должен вырасти
        assert_eq!(
            account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
            Some(10_000_000_000)
        );
        assert_eq!(
            account_get(&c, "a").unwrap().unwrap().balance_nano,
            10_000_000_000
        ); // ровно $10
           // ДРУГОЙ ref начисляет нормально
        assert_eq!(
            account_topup(&c, "a", 5_000_000_000, Some("tx_XYZ")).unwrap(),
            Some(15_000_000_000)
        );
        // без ref (админ-коррекция) — не дедупится, всегда применяется
        account_topup(&c, "a", 1_000_000_000, None).unwrap();
        account_topup(&c, "a", 1_000_000_000, None).unwrap();
        assert_eq!(
            account_get(&c, "a").unwrap().unwrap().balance_nano,
            17_000_000_000
        );
        // в ledger ровно один topup на каждый уникальный ref (+ 2 без ref)
        let topups: i64 = c
            .query_row("SELECT COUNT(*) FROM ledger WHERE kind='topup'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(topups, 4); // tx_ABC, tx_XYZ, и 2 без ref
                               // Поздний точный replay возвращает сохранённый исходный результат, не текущий баланс.
        assert_eq!(
            account_topup(&c, "a", 10_000_000_000, Some("tx_ABC")).unwrap(),
            Some(10_000_000_000)
        );
    }

    /// A duplicate monetary reference succeeds only for the exact original operation.
    #[test]
    fn monetary_reference_rejects_parameter_mismatch_and_deduplicates_adjustments() {
        let c = db();
        account_create(&c, "a", None, 2000).unwrap();
        account_create(&c, "b", None, 2000).unwrap();
        assert_eq!(
            account_topup(&c, "a", 100, Some("payment:1")).unwrap(),
            Some(100)
        );
        assert!(account_topup(&c, "a", 200, Some("payment:1")).is_err());
        assert!(account_topup(&c, "b", 100, Some("payment:1")).is_err());
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 100);
        assert_eq!(account_get(&c, "b").unwrap().unwrap().balance_nano, 0);
        assert_eq!(
            account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
            Some(75)
        );
        assert_eq!(
            account_topup(&c, "a", -25, Some("adjust:1")).unwrap(),
            Some(75)
        );
        assert!(account_topup(&c, "a", -30, Some("adjust:1")).is_err());
        assert_eq!(account_get(&c, "a").unwrap().unwrap().balance_nano, 75);
        assert!(account_topup(&c, "a", 1, Some("   ")).is_err());
    }

    /// Без consumer acknowledgement watermark charge-строки нельзя безопасно удалять.
    #[test]
    fn ledger_prune_is_disabled_without_consumer_watermarks() {
        let c = db();
        acct_with_key(&c, "a", "k", 5_000_000_000, 10000);
        account_reserve(&c, "a", 1_000_000_000).unwrap();
        account_settle(&c, "a", "k", 1_000_000_000, 400_000_000, Some("old"), None).unwrap();
        c.execute("UPDATE ledger SET ts = 1000", []).unwrap();
        assert_eq!(ledger_prune(&c, 2000).unwrap(), 0);
        let rows = ledger_after(&c, "a", 0, 10).unwrap();
        assert_eq!(
            rows.len(),
            2,
            "topup and unacknowledged charge remain cursor-visible"
        );
        assert!(rows.iter().any(|row| row.kind == "charge"));
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
        assert_eq!(
            key_account(&c, "k-alice").unwrap().unwrap().account_id,
            "team"
        );
        assert_eq!(
            key_account(&c, "k-bob").unwrap().unwrap().account_id,
            "team"
        );
        // alice тратит $0.30, bob $0.20 — из общего баланса
        account_reserve(&c, "team", 300_000_000).unwrap();
        account_settle(&c, "team", "k-alice", 300_000_000, 300_000_000, None, None).unwrap();
        account_reserve(&c, "team", 200_000_000).unwrap();
        account_settle(&c, "team", "k-bob", 200_000_000, 200_000_000, None, None).unwrap();
        assert_eq!(
            account_get(&c, "team").unwrap().unwrap().balance_nano,
            500_000_000
        ); // $0.50 осталось
           // атрибуция по ключам раздельная
        assert_eq!(
            key_get(&c, "k-alice").unwrap().unwrap().spent_nano,
            300_000_000
        );
        assert_eq!(
            key_get(&c, "k-bob").unwrap().unwrap().spent_nano,
            200_000_000
        );
        // вход по handle
        assert_eq!(account_by_handle(&c, "tg:123").unwrap().unwrap().id, "team");
    }

    /// Control-plane management uses a stable public ID and never needs to persist the raw key.
    #[test]
    fn key_can_be_disabled_by_non_secret_id() {
        let c = db();
        account_create(&c, "acct", None, 2000).unwrap();
        key_issue(&c, "sk-pool-super-secret", "acct", Some("prod")).unwrap();
        let issued = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
        assert!(issued.key_id.starts_with("key_"));
        assert_eq!(
            key_set_status_by_id(&c, &issued.key_id, "disabled").unwrap(),
            1
        );
        assert_eq!(
            key_set_label_by_id(&c, &issued.key_id, "renamed").unwrap(),
            1
        );
        let updated = key_get(&c, "sk-pool-super-secret").unwrap().unwrap();
        assert_eq!(updated.status, "disabled");
        assert_eq!(updated.label.as_deref(), Some("renamed"));
        assert_eq!(key_set_label_by_id(&c, "key_missing", "unused").unwrap(), 0);
    }

    #[test]
    fn per_key_policy_gates_reservations_and_releases_allowance() {
        let c = db();
        account_create(&c, "acct", None, 10_000).unwrap();
        account_topup(&c, "acct", 1_000, None).unwrap();
        key_issue_with_policy(
            &c,
            "limited",
            "acct",
            Some("limited"),
            Some(700),
            Some(now() + 60),
        )
        .unwrap();

        assert_eq!(
            account_reserve_for_key(&c, "acct", "limited", 500).unwrap(),
            Some(500)
        );
        assert_eq!(
            account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
            None
        );
        let account = account_get(&c, "acct").unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (500, 500));

        account_settle(&c, "acct", "limited", 500, 400, None, None).unwrap();
        let key = key_get(&c, "limited").unwrap().unwrap();
        assert_eq!(
            (key.spent_nano, key.reserved_nano, key.spend_limit_nano),
            (400, 0, Some(700))
        );

        assert_eq!(
            account_reserve_for_key(&c, "acct", "limited", 300).unwrap(),
            Some(300)
        );
        account_settle(&c, "acct", "limited", 300, 0, None, None).unwrap();
        assert_eq!(key_get(&c, "limited").unwrap().unwrap().reserved_nano, 0);

        key_issue_with_policy(&c, "expired", "acct", None, None, Some(now())).unwrap();
        assert_eq!(
            account_reserve_for_key(&c, "acct", "expired", 1).unwrap(),
            None
        );
        assert_eq!(account_get(&c, "acct").unwrap().unwrap().reserved_nano, 0);
        let expired_auth = key_account(&c, "expired").unwrap().unwrap();
        assert!(expired_auth.active);
        assert!(
            !expired_auth.active_at(now()),
            "expiry is exclusive at the exact second"
        );

        key_set_status(&c, "limited", "disabled").unwrap();
        assert_eq!(
            account_reserve_for_key(&c, "acct", "limited", 1).unwrap(),
            None
        );
        assert!(!key_account(&c, "limited")
            .unwrap()
            .unwrap()
            .active_at(now()));
    }

    #[test]
    fn key_policy_can_be_replaced_without_undercutting_live_usage() {
        let c = db();
        account_create(&c, "acct", None, 10_000).unwrap();
        account_topup(&c, "acct", 2_000, None).unwrap();
        key_issue_with_policy(&c, "mutable", "acct", None, Some(1_000), Some(now() + 60)).unwrap();
        let key_id = key_get(&c, "mutable").unwrap().unwrap().key_id;

        assert_eq!(
            account_reserve_for_key(&c, "acct", "mutable", 600).unwrap(),
            Some(1_400)
        );
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, Some(599), None).unwrap(),
            KeyPolicyUpdate::LimitBelowUsage,
        );
        assert_eq!(
            key_get(&c, "mutable").unwrap().unwrap().spend_limit_nano,
            Some(1_000)
        );
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, Some(600), None).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        account_settle(&c, "acct", "mutable", 600, 500, None, None).unwrap();
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, Some(499), None).unwrap(),
            KeyPolicyUpdate::LimitBelowUsage,
        );

        let future = now() + 3_600;
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, None, Some(future)).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        let updated = key_get(&c, "mutable").unwrap().unwrap();
        assert_eq!(
            (updated.spend_limit_nano, updated.expires_ts),
            (None, Some(future))
        );
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, None, None).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        key_set_status_by_id(&c, &key_id, "disabled").unwrap();
        assert_eq!(
            key_set_policy_by_id(&c, "acct", &key_id, None, Some(now() + 7_200)).unwrap(),
            KeyPolicyUpdate::Updated,
        );
        assert!(!key_account(&c, "mutable")
            .unwrap()
            .unwrap()
            .active_at(now()));
        assert_eq!(
            key_set_policy_by_id(&c, "other-account", &key_id, None, None).unwrap(),
            KeyPolicyUpdate::NotFound,
        );
        assert_eq!(
            key_set_policy_by_id(&c, "acct", "key_missing", None, None).unwrap(),
            KeyPolicyUpdate::NotFound,
        );
    }

    #[test]
    fn ledger_cursor_is_oldest_first_and_multiplier_is_mutable() {
        let c = db();
        acct_with_key(&c, "acct", "key", 2_000_000_000, 4000);
        account_reserve(&c, "acct", 100_000_000).unwrap();
        account_settle(
            &c,
            "acct",
            "key",
            100_000_000,
            50_000_000,
            Some("request"),
            None,
        )
        .unwrap();
        let first = ledger_after(&c, "acct", 0, 1).unwrap();
        assert_eq!(first.len(), 1);
        let rest = ledger_after(&c, "acct", first[0].id, 10).unwrap();
        assert_eq!(rest.len(), 1);
        assert!(rest[0].id > first[0].id);
        assert_eq!(account_set_mult_bp(&c, "acct", 3500).unwrap(), 1);
        assert_eq!(account_get(&c, "acct").unwrap().unwrap().mult_bp, 3500);
    }
}
