//! claude-api — пул Claude-подписок как ПРОЗРАЧНЫЙ /v1 API.
//!
//! Крейт `server` — КОМПОЗИЦИЯ: читает окружение, поднимает пул из реестра, стартует
//! фоновые циклы и HTTP-роутер. Логика по слоям: registry ← pool ← forward ← server.
//!
//!   claude-api serve                 # поднять сервер (переменные окружения см. config.rs)
//!   claude-api sub add-file <email> --token-file <0600-path> [--proxy-file <0600-path>] [--fleet ...]
//!   claude-api sub list | rm <email> | status <email> <s> | proxy <email> --proxy-file <0600-path>
//!   claude-api sub fleet <email> <fleet>

mod admin;
mod config;
mod http;
mod metrics_store;
mod poller;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use config::Settings;
use forward::{detect_plan, AppState, Clients, PlanDetect};
use pool::Pool;
use std::future::IntoFuture;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

// Fail-closed bounded defaults until these knobs are represented by validated Settings fields.
// AUDIT-TODO(C60/C89): add typed, range-checked fields in config.rs before restoring env overrides.
// A non-SSE response may require a 32 MiB usage document copy. Keep the aggregate buffer envelope
// comfortably below systemd MemoryMax=2G, leaving room for request JSON, TLS and runtime overhead.
const MAX_CONCURRENT: usize = 24;
const LEDGER_RETENTION_DAYS: i64 = 30;
/// Terminal reservations and their actual/shadow children outlive the maximum supported replay
/// age by a wide margin. Keep this independent from analytics retention.
const REQUEST_LIFECYCLE_RETENTION_DAYS: i64 = 30;
const METRICS_RETENTION_DAYS: i64 = 90;
const _: () = assert!(
    REQUEST_LIFECYCLE_RETENTION_DAYS * 86_400
        >= registry::pricing::PRICING_REQUEST_LIFECYCLE_MIN_RETENTION_SECS
);

/// Acquire and hold an OS advisory lock for the full server lifetime. A PID file cannot prove
/// exclusivity across rolling deploys, default-without-`serve` invocation, or PID namespaces.
#[cfg(unix)]
fn acquire_instance_lock(db_path: &str) -> Result<std::fs::File> {
    #[cfg(target_os = "linux")]
    use std::io::Read;
    use std::io::{Seek, SeekFrom, Write};
    use std::os::fd::AsRawFd;

    const LOCK_EX: i32 = 2;
    const LOCK_NB: i32 = 4;
    extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }

    let lock_path = format!("{db_path}.lock");
    if let Some(parent) = std::path::Path::new(&lock_path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("создать каталог lock-файла {lock_path}"))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("открыть lock-файл {lock_path}"))?;

    // SAFETY: flock only observes the valid, open file descriptor owned by `file`; the descriptor
    // remains open until `serve` has drained billing and persisted pool state.
    if unsafe { flock(file.as_raw_fd(), LOCK_EX | LOCK_NB) } != 0 {
        bail!(
            "другой инстанс уже держит эксклюзивный lock {}: {}",
            lock_path,
            std::io::Error::last_os_error()
        );
    }

    // Rolling-upgrade bridge: the previous binary wrote a PID but did not hold flock. Refuse to
    // reconcile while that PID is still visible, regardless of whether it used an explicit `serve`.
    #[cfg(target_os = "linux")]
    {
        let mut prior_pid = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut prior_pid)?;
        if let Ok(pid) = prior_pid.trim().parse::<u32>() {
            if pid != std::process::id() && std::path::Path::new(&format!("/proc/{pid}")).exists() {
                bail!("предыдущий инстанс с PID {pid} ещё жив; reconcile запрещён");
            }
        }
    }

    file.set_len(0)?;
    file.seek(SeekFrom::Start(0))?;
    writeln!(file, "{}", std::process::id())?;
    Ok(file)
}

#[cfg(not(unix))]
fn acquire_instance_lock(_db_path: &str) -> Result<std::fs::File> {
    bail!("безопасный single-instance lock для этой платформы не реализован")
}

#[derive(Parser)]
#[command(
    name = "claude-api",
    about = "Пул Claude-подписок как прозрачный /v1 API"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Поднять HTTP-сервер (форвардинг-прокси над пулом)
    Serve,
    /// Управление реестром подписок
    Sub {
        #[command(subcommand)]
        op: SubOp,
    },
    /// Аккаунты клиентов (профиль + ЕДИНЫЙ баланс, к которому цепляются ключи)
    Account {
        #[command(subcommand)]
        op: AccountOp,
    },
    /// Ключи клиентов (доступы к аккаунту; баланс — на аккаунте)
    Key {
        #[command(subcommand)]
        op: KeyOp,
    },
    /// Консистентный бэкап БД (VACUUM INTO) с ротацией. Ставится по systemd-таймеру.
    Backup {
        /// Каталог для снимков (деф: <cfg>/backups).
        #[arg(long)]
        out: Option<String>,
        /// Сколько последних снимков хранить (деф 24).
        #[arg(long, default_value = "24")]
        keep: usize,
    },
    /// Engine PostgreSQL schema/import operations. These never print the database DSN.
    Db {
        #[command(subcommand)]
        op: DbOp,
    },
}

#[derive(Subcommand)]
enum DbOp {
    /// Apply engine schema then atomically import a fully drained SQLite authority.
    MigratePostgres {
        #[arg(long)]
        sqlite: Option<String>,
    },
    /// Apply pending engine PostgreSQL migrations without importing data.
    MigrateEngine,
    /// Verify the already-installed PostgreSQL schema without issuing DDL.
    VerifyPostgres,
}

#[derive(Subcommand)]
enum AccountOp {
    /// Создать аккаунт (баланс 0). Печатает id. --handle — внешняя идентичность (TG id/email).
    Create {
        #[arg(long)]
        handle: Option<String>,
        /// Наценка × 10000 (2000 = ×0.20). По умолчанию — из CLAUDE_API_MULT_BP.
        #[arg(long)]
        mult_bp: Option<i64>,
        /// Задать id явно (иначе сгенерируется acct_…).
        #[arg(long)]
        id: Option<String>,
    },
    /// Пополнить баланс аккаунта на USD (можно отрицательное — коррекция). --ref — метка платежа.
    #[command(allow_negative_numbers = true)]
    // чтобы `--usd -100` (коррекция) не парсился как флаг
    Topup {
        id: String,
        #[arg(long)]
        usd: f64,
        #[arg(long)]
        r#ref: Option<String>,
    },
    /// Показать баланс/расход аккаунта.
    Balance {
        id: String,
    },
    /// Список аккаунтов.
    List,
    /// Заблокировать/разблокировать аккаунт (все его ключи тоже перестают/начинают работать).
    Disable {
        id: String,
    },
    Enable {
        id: String,
    },
    /// Удалить аккаунт НАВСЕГДА вместе с ключами и историей (нужен --yes).
    Rm {
        id: String,
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum KeyOp {
    /// Выпустить ключ ПОД аккаунт. Без --key-file генерится и печатается ОДИН раз.
    Issue {
        #[arg(long)]
        account: String,
        /// Имя ключа (проект/член команды) — для атрибуции расхода.
        #[arg(long)]
        label: Option<String>,
        /// Прочитать заданный ключ из mode-0600 файла; без флага ключ генерируется.
        #[arg(long)]
        key_file: Option<String>,
    },
    /// Показать ключ: аккаунт, метка, расход по ключу, баланс аккаунта.
    Balance {
        #[arg(long)]
        key_file: String,
    },
    /// Список ключей (ключ маскируется).
    List,
    /// Заблокировать ключ.
    Disable {
        #[arg(long)]
        key_file: String,
    },
    /// Разблокировать ключ.
    Enable {
        #[arg(long)]
        key_file: String,
    },
    /// Удалить ключ НАВСЕГДА (баланс аккаунта не трогается).
    Rm {
        #[arg(long)]
        key_file: String,
    },
    /// Удалить ВСЕ ключи (нужен --yes). Балансы аккаунтов НЕ трогаются.
    Clear {
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum SubOp {
    /// Добавить подписку, читая секреты из mode-0600 файлов.
    AddFile {
        email: String,
        #[arg(long)]
        token_file: String,
        /// Файл с proxy URL; не передавайте credentialed URL через argv.
        #[arg(long)]
        proxy_file: Option<String>,
        #[arg(long, default_value = "prod")]
        fleet: String,
    },
    /// Список подписок
    List,
    /// Удалить подписку
    Rm { email: String },
    /// Удалить ВСЕ подписки (нужен --yes; --fleet ограничивает флотом)
    Clear {
        #[arg(long)]
        yes: bool,
        #[arg(long)]
        fleet: Option<String>,
    },
    /// Сменить статус (active | paused | disabled)
    Status { email: String, status: String },
    /// Сменить прокси, читая URL из mode-0600 файла.
    Proxy {
        email: String,
        #[arg(long)]
        proxy_file: String,
    },
    /// Сменить флот (dev | prod | ...)
    Fleet { email: String, fleet: String },
    /// Задать тариф вручную (pro | max5 | max20)
    SetPlan { email: String, plan: String },
    /// Определить тариф из /api/oauth/profile (без email — все без тарифа)
    DetectPlan { email: Option<String> },
    /// Durable auth-health подписок (healthy | suspect | dead) — что движок узнал по probe.
    Health,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("create Tokio runtime")?
            .block_on(serve()),
        Cmd::Sub { op } => sub_cmd(op),
        Cmd::Account { op } => account_cmd(op),
        Cmd::Key { op } => key_cmd(op),
        Cmd::Backup { out, keep } => backup_cmd(out, keep),
        Cmd::Db { op } => db_cmd(op),
    }
}

fn authority_config(s: &Settings) -> registry::authority::AuthorityConfig {
    registry::authority::AuthorityConfig::new(s.db_path.clone(), s.database_url.clone())
}

fn db_cmd(op: DbOp) -> Result<()> {
    let s = Settings::from_env();
    let url = s
        .database_url
        .as_deref()
        .context("CLAUDE_API_DATABASE_URL is required for PostgreSQL database commands")?;
    let mut pg = registry::pg::PgStore::connect(url)?;
    match op {
        DbOp::MigratePostgres { sqlite } => {
            pg.migrate()?;
            let source = sqlite.unwrap_or(s.db_path);
            let report = pg.import_sqlite(&source)?;
            println!(
                "✓ PostgreSQL import verified: subs={} accounts={} keys={} ledger={} usage={} pool={} balance_nano={} spent_nano={} reserved_nano={}",
                report.subscriptions, report.accounts, report.keys, report.ledger_rows,
                report.usage_rows, report.pool_rows, report.balance_nano, report.spent_nano,
                report.reserved_nano,
            );
        }
        DbOp::MigrateEngine => {
            pg.migrate()?;
            pg.verify_schema()?;
            println!(
                "✓ engine PostgreSQL schema migrated to version {}",
                pg.schema_version()?
            );
        }
        DbOp::VerifyPostgres => {
            pg.verify_schema()?;
            let version = pg.schema_version()?;
            let totals = pg.billing_totals()?;
            println!(
                "✓ engine PostgreSQL schema={} balance_nano={} spent_nano={} reserved_nano={}",
                version, totals.balance_nano, totals.spent_nano, totals.reserved_nano,
            );
        }
    }
    Ok(())
}

/// Консистентный бэкап БД (`VACUUM INTO`) в timestamped-файл + ротация (храним `keep` последних).
/// НЕ копируем .db напрямую (WAL → битая копия). Рекомендация прод: снимки складывать И на ОТДЕЛЬНЫЙ
/// носитель/офсайт (напр. rclone/scp по тому же таймеру) — иначе отказ диска унесёт и БД, и бэкапы.
fn backup_cmd(out: Option<String>, keep: usize) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = Settings::from_env();
    if s.database_url.is_some() {
        bail!("PostgreSQL authority requires pgBackRest/WAL-G or managed PITR; SQLite VACUUM backup is disabled");
    }
    let dir = out.unwrap_or_else(|| {
        let p = std::path::Path::new(&s.db_path)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        format!("{p}/backups")
    });
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let path = format!("{dir}/subscriptions.db.bak-{ts}");
    let conn = registry::open(&s.db_path)?;
    registry::backup_to(&conn, &path)?;
    let sz = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("✓ бэкап: {path} ({} КиБ)", sz / 1024);
    // ротация: оставляем `keep` самых свежих snapshot'ов, старые удаляем
    let mut snaps: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with("subscriptions.db.bak-")
        })
        .map(|e| e.path())
        .collect();
    snaps.sort(); // имена с epoch-ts → лексикографически = хронологически
    if snaps.len() > keep {
        for old in &snaps[..snaps.len() - keep] {
            let _ = std::fs::remove_file(old);
        }
        println!("  ротация: удалено {} старых", snaps.len() - keep);
    }
    Ok(())
}

/// Сгенерировать id аккаунта: acct_<24hex> (из /dev/urandom, как ключ).
pub(crate) fn gen_account_id() -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom").context("открыть /dev/urandom")?;
    let mut buf = [0u8; 12];
    f.read_exact(&mut buf).context("прочитать /dev/urandom")?;
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("acct_{hex}"))
}

fn account_cmd(op: AccountOp) -> Result<()> {
    let s = Settings::from_env();
    let config = authority_config(&s);
    let mut db = config.connect()?;
    match op {
        AccountOp::Create {
            handle,
            mult_bp,
            id,
        } => {
            let id = match id {
                Some(i) => i,
                None => gen_account_id()?,
            };
            let mult = mult_bp.unwrap_or(s.mult_bp);
            db.account_create(&id, handle.as_deref(), mult)?;
            println!(
                "✓ аккаунт создан: {id}  ·  наценка ×{}  ·  handle={}",
                mult as f64 / 10000.0,
                handle.as_deref().unwrap_or("—")
            );
            println!("  выпустить ключ:  claude-api key issue --account {id}");
        }
        AccountOp::Topup { id, usd, r#ref } => {
            match db.account_topup(&id, usd_to_nano(usd), r#ref.as_deref())? {
                Some(bal) => println!("✓ баланс аккаунта {id}: {}", usd_str(bal)),
                None => println!("аккаунт не найден: {id}"),
            }
        }
        AccountOp::Balance { id } => match db.account_get(&id)? {
            Some(a) => println!(
                "{}\tбаланс={}\tпотрачено={}\tрезерв={}\tнаценка=×{}\tstatus={}\thandle={}",
                a.id,
                usd_str(a.balance_nano),
                usd_str(a.spent_nano),
                usd_str(a.reserved_nano),
                a.mult_bp as f64 / 10000.0,
                a.status,
                a.handle.as_deref().unwrap_or("—")
            ),
            None => println!("аккаунт не найден: {id}"),
        },
        AccountOp::List => {
            let rows = db.account_list()?;
            if rows.is_empty() {
                println!("(аккаунтов нет) · БД: {}", config.label());
                return Ok(());
            }
            for a in rows {
                println!(
                    "{}\tбаланс={}\tпотрачено={}\tнаценка=×{}\tstatus={}\thandle={}",
                    a.id,
                    usd_str(a.balance_nano),
                    usd_str(a.spent_nano),
                    a.mult_bp as f64 / 10000.0,
                    a.status,
                    a.handle.as_deref().unwrap_or("—")
                );
            }
        }
        AccountOp::Disable { id } => {
            println!("обновлено: {}", db.account_set_status(&id, "disabled")?)
        }
        AccountOp::Enable { id } => {
            println!("обновлено: {}", db.account_set_status(&id, "active")?)
        }
        AccountOp::Rm { id, yes } => {
            if !yes {
                println!("нужен --yes: удалит аккаунт {id} с ключами и историей");
            } else {
                println!("деактивировано аккаунтов: {}", db.account_remove(&id)?);
            }
        }
    }
    Ok(())
}

/// USD → нанодоллары (округление до ближайшего нано; f64 достаточно на этапе ВЫПУСКА,
/// в отличие от поштучного подсчёта, где всё целочисленно).
fn usd_to_nano(usd: f64) -> i64 {
    (usd * 1e9).round() as i64
}

/// Нанодоллары (i64) → "$X.XXXXXX" через движок metering.
fn usd_str(nano: i64) -> String {
    metering::nano_to_usd_string(nano as i128)
}

/// Маска ключа для листинга: sk-pool-abc…wxyz (не печатаем целиком).
fn mask_key(k: &str) -> String {
    if !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return "<key-redacted>".into();
    }
    if k.len() <= 12 {
        return format!("{}…", &k[..k.len().min(4)]);
    }
    format!("{}…{}", &k[..8], &k[k.len() - 4..])
}

/// Безопасное представление proxy для CLI: схема + маскированный host + необязательный порт.
/// Никогда не возвращает userinfo, path, query или исходную строку при ошибке разбора.
fn mask_proxy(proxy: &str) -> String {
    if proxy.is_empty() {
        return "—".into();
    }
    let Some((scheme, rest)) = proxy.split_once("://") else {
        return "<proxy-redacted>".into();
    };
    if scheme.is_empty()
        || !scheme
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return "<proxy-redacted>".into();
    }

    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let port = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed
            .split_once("]:")
            .and_then(|(_, p)| p.parse::<u16>().ok())
    } else {
        host_port
            .rsplit_once(':')
            .and_then(|(host, p)| (!host.is_empty()).then_some(p))
            .and_then(|p| p.parse::<u16>().ok())
    };

    match port {
        Some(port) => format!("{scheme}://***:{port}"),
        None => format!("{scheme}://***"),
    }
}

/// Read a single-line secret without exposing it through argv. On Unix, group/other permissions
/// are rejected so tokens, customer keys, and credentialed proxy URLs cannot sit in a broad file.
fn read_secret_file(path: &str, kind: &str) -> Result<String> {
    use std::io::Read;

    let mut file =
        std::fs::File::open(path).with_context(|| format!("открыть файл секрета {kind}"))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("прочитать metadata файла секрета {kind}"))?;
    if !metadata.is_file() {
        bail!("файл секрета {kind} не является обычным файлом");
    }
    if metadata.len() > 64 * 1024 {
        bail!("файл секрета {kind} слишком большой");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("файл секрета {kind} должен быть закрыт от group/other (chmod 600 {path})");
        }
    }

    let mut value = String::new();
    file.read_to_string(&mut value)
        .with_context(|| format!("прочитать файл секрета {kind}"))?;
    let value = value.trim_end_matches(['\r', '\n']).to_string();
    if value.is_empty() {
        bail!("файл секрета {kind} пуст");
    }
    if value.contains('\r') || value.contains('\n') {
        bail!("файл секрета {kind} должен содержать ровно одну строку");
    }
    Ok(value)
}

/// Сгенерировать ключ из /dev/urandom (ровно 24 байта → hex): sk-pool-<48hex>.
/// ВАЖНО: читаем фиксированный буфер (`read_exact`), а не весь файл — /dev/urandom бесконечен.
pub(crate) fn gen_key() -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom").context("открыть /dev/urandom")?;
    let mut buf = [0u8; 24];
    f.read_exact(&mut buf).context("прочитать /dev/urandom")?;
    let hex: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("sk-pool-{hex}"))
}

fn key_cmd(op: KeyOp) -> Result<()> {
    let s = Settings::from_env();
    let config = authority_config(&s);
    let mut db = config.connect()?;
    match op {
        KeyOp::Issue {
            account,
            label,
            key_file,
        } => {
            // проверяем, что аккаунт есть (иначе ключ никуда не резолвится)
            if db.account_get(&account)?.is_none() {
                println!("аккаунт не найден: {account} (создай: claude-api account create)");
                return Ok(());
            }
            let generated = key_file.is_none();
            let key = match key_file {
                Some(path) => read_secret_file(&path, "customer API key")?,
                None => gen_key()?,
            };
            db.key_issue(&key, &account, label.as_deref())?;
            if generated {
                println!("✓ ключ выпущен: {key}");
            } else {
                println!("✓ ключ выпущен: {}", mask_key(&key));
            }
            println!(
                "  аккаунт: {account}  ·  метка: {}  (баланс общий с аккаунтом)",
                label.as_deref().unwrap_or("—")
            );
        }
        KeyOp::Balance { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            match db.key_get(&key)? {
                Some(r) => {
                    let acct = r.account_id.clone().unwrap_or_default();
                    let abal = db
                        .account_get(&acct)?
                        .map(|a| usd_str(a.balance_nano))
                        .unwrap_or_else(|| "?".into());
                    println!(
                        "{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tбаланс_аккаунта={}\tstatus={}",
                        mask_key(&r.key),
                        acct,
                        r.label.as_deref().unwrap_or("—"),
                        usd_str(r.spent_nano),
                        abal,
                        r.status
                    );
                }
                None => println!("ключ не найден: {}", mask_key(&key)),
            }
        }
        KeyOp::List => {
            let rows = db.key_list()?;
            if rows.is_empty() {
                println!("(ключей нет) · БД: {}", config.label());
                return Ok(());
            }
            for r in rows {
                println!(
                    "{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tstatus={}",
                    mask_key(&r.key),
                    r.account_id.as_deref().unwrap_or("—"),
                    r.label.as_deref().unwrap_or("—"),
                    usd_str(r.spent_nano),
                    r.status
                );
            }
        }
        KeyOp::Disable { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("обновлено: {}", db.key_set_status(&key, "disabled")?);
        }
        KeyOp::Enable { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("обновлено: {}", db.key_set_status(&key, "active")?);
        }
        KeyOp::Rm { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("удалено ключей: {}", db.key_remove(&key)?);
        }
        KeyOp::Clear { yes } => {
            if !yes {
                println!("нужен --yes: удалит ВСЕ ключи (балансы клиентов пропадут)");
            } else {
                println!("удалено ключей: {}", db.key_clear()?);
            }
        }
    }
    Ok(())
}

/// Определить тариф подписки (профиль Anthropic через её прокси) и записать в реестр.
/// Возвращает человекочитаемую строку итога.
fn detect_and_store(s: &Settings, email: &str) -> String {
    let (tok, proxy) = match authority_config(s)
        .connect()
        .and_then(|mut db| db.get_creds(email))
    {
        Ok(Some(c)) => c,
        Ok(None) => return "нет токена".into(),
        Err(e) => return format!("db: {e}"),
    };
    let clients = Clients::new(&s.proxy);
    let client = match clients.get(&proxy, email) {
        Ok(c) => c,
        Err(_) => return format!("proxy {}: не удалось создать клиент", mask_proxy(&proxy)),
    };
    let ua = forward::persona_ua(&s.proxy, email);
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => return format!("runtime: {e}"),
    };
    match runtime.block_on(detect_plan(&client, &s.proxy, &tok, &ua)) {
        PlanDetect::Plan(p) => {
            match authority_config(s)
                .connect()
                .and_then(|mut db| db.set_plan(email, &p))
            {
                Ok(_) => format!("тариф: {p}"),
                Err(e) => format!("тариф определён ({p}), но запись в db не удалась: {e}"),
            }
        }
        PlanDetect::NoScope => {
            "тариф: noscope (у токена нет scope user:profile — задай вручную: sub set-plan)".into()
        }
        PlanDetect::Err(e) => format!("тариф: не определён ({e})"),
    }
}

fn sub_cmd(op: SubOp) -> Result<()> {
    let s = Settings::from_env();
    let config = authority_config(&s);
    let mut db = config.connect()?;
    match op {
        SubOp::AddFile {
            email,
            token_file,
            proxy_file,
            fleet,
        } => {
            let token = read_secret_file(&token_file, "subscription token")?;
            let proxy = match proxy_file {
                Some(path) => read_secret_file(&path, "proxy URL")?,
                None => String::new(),
            };
            db.add(&email, &token, &proxy, &fleet)?;
            // Тариф НЕ детектим синтетическим запросом к Anthropic: у OAuth-токенов нет scope
            // user:profile → всё равно NoScope, а лишний запрос = фингерпринт. Ёмкость калибруется
            // из боевого трафика; при необходимости тариф задаётся вручную (`sub set-plan`).
            println!(
                "✓ добавлена {email} (fleet={fleet}, proxy={})",
                mask_proxy(&proxy)
            );
        }
        SubOp::List => {
            let rows = db.list_subs()?;
            if rows.is_empty() {
                println!("(подписок нет) · БД: {}", config.label());
                return Ok(());
            }
            for r in rows {
                println!(
                    "{}\tstatus={}\tfleet={}\tplan={}\ttoken={}\tproxy={}",
                    r.email,
                    r.status,
                    r.fleet,
                    if r.plan.is_empty() { "—" } else { &r.plan },
                    if r.has_token { "есть" } else { "НЕТ" },
                    mask_proxy(&r.proxy)
                );
            }
        }
        SubOp::Rm { email } => {
            println!("деактивировано подписок: {}", db.remove_sub(&email)?);
        }
        SubOp::Clear { yes, fleet } => {
            if !yes {
                let n = db.list_subs()?.len();
                println!(
                    "⚠️  удалит ВСЕ подписки{} — {n} шт. Повтори с --yes для подтверждения.",
                    fleet
                        .as_deref()
                        .map(|f| format!(" флота {f}"))
                        .unwrap_or_default()
                );
            } else {
                let n = db.clear_subs(fleet.as_deref())?;
                println!(
                    "✓ удалено подписок: {n}{}",
                    fleet
                        .as_deref()
                        .map(|f| format!(" (флот {f})"))
                        .unwrap_or_default()
                );
            }
        }
        SubOp::Status { email, status } => {
            println!("обновлено: {}", db.set_sub_status(&email, &status)?);
        }
        SubOp::Proxy { email, proxy_file } => {
            let proxy = read_secret_file(&proxy_file, "proxy URL")?;
            println!("обновлено: {}", db.set_proxy(&email, &proxy)?);
        }
        SubOp::Fleet { email, fleet } => {
            println!("обновлено: {}", db.set_fleet(&email, &fleet)?);
        }
        SubOp::SetPlan { email, plan } => {
            println!("обновлено: {} (plan={plan})", db.set_plan(&email, &plan)?);
        }
        SubOp::DetectPlan { email } => {
            let targets: Vec<String> = match email {
                Some(e) => vec![e],
                None => db
                    .list_subs()?
                    .into_iter()
                    .filter(|r| r.plan.is_empty())
                    .map(|r| r.email)
                    .collect(),
            };
            if targets.is_empty() {
                println!("нечего определять (у всех тариф уже задан)");
            }
            drop(db);
            for e in targets {
                println!("{e} → {}", detect_and_store(&s, &e));
            }
        }
        SubOp::Health => {
            let rows = db.load_sub_health(None)?;
            let mut dead = 0;
            for h in &rows {
                if h.auth_state == "healthy" {
                    continue;
                } // печатаем только небанальное
                if h.auth_state == "dead" {
                    dead += 1;
                }
                let reason = match h.dead_reason.as_str() {
                    "permission_error" => "banned",
                    "authentication_error" => "re-auth",
                    "" => "—",
                    other => other,
                };
                println!(
                    "{}\t{}\tstreak={}\tlast_http={}\treason={}\tdead_since={}",
                    h.email,
                    h.auth_state.to_uppercase(),
                    h.auth_fail_streak,
                    if h.last_auth_http == 0 {
                        "—".into()
                    } else {
                        h.last_auth_http.to_string()
                    },
                    reason,
                    if h.dead_since_ts == 0 {
                        "—".into()
                    } else {
                        h.dead_since_ts.to_string()
                    }
                );
            }
            println!(
                "— итого: {} мёртвых, {} под наблюдением (из {} подписок) · БД: {}",
                dead,
                rows.iter().filter(|h| h.auth_state == "suspect").count(),
                rows.len(),
                config.label()
            );
        }
    }
    Ok(())
}

async fn serve() -> Result<()> {
    let s = Settings::from_env();
    let serves_anthropic = s.provider.serves_anthropic();
    // Потолок параллельных запросов на подписку (env CLAUDE_API_MAX_INFLIGHT) — ставим ДО
    // создания пула, чтобы route/pick/capacity видели актуальное значение.
    if serves_anthropic {
        pool::set_max_inflight(s.max_inflight);
    }
    let authority = authority_config(&s);
    if authority.is_postgres() && !s.billing {
        bail!("PostgreSQL authority requires CLAUDE_API_BILLING=1 because capacity leases share the durable actor");
    }
    if matches!(
        s.provider,
        forward::ProviderMode::OpenAi | forward::ProviderMode::Gemini
    ) && !authority.is_postgres()
    {
        bail!(
            "CLAUDE_API_PROVIDER={} requires the PostgreSQL engine authority",
            s.provider.as_str()
        );
    }
    // SQLite has anonymous aggregate reservations and therefore remains strictly single-process.
    // PostgreSQL has owner epochs plus request/capacity leases; its completed fault matrix permits
    // blue-green overlap, so no host-local flock is taken in that mode.
    let instance_lock = if authority.is_postgres() {
        None
    } else {
        Some(acquire_instance_lock(&s.db_path)?)
    };
    // `postgres` is a synchronous client with its own Tokio runtime. Every call must stay outside
    // this async runtime, just like SQLite I/O, or it panics with a nested-runtime error.
    let startup_authority = authority.clone();
    let startup_instance = s.instance_id.clone();
    let startup_fleet = s.fleet.clone();
    let startup_billing = s.billing;
    let startup_is_postgres = authority.is_postgres();
    let startup_load_anthropic = serves_anthropic;
    let (owner, subs, recovery, pool_rows, health_rows, sqlite_reconcile) =
        tokio::task::spawn_blocking(move || {
            let mut db = startup_authority.connect()?;
            db.verify_schema()?;
            let owner = db.claim_instance(&startup_instance, 30)?;
            let mut recovery = registry::pg::ReconcileReport::default();
            if let Some(current) = owner.as_ref() {
                let pg = db.postgres()?;
                recovery = pg.reconcile_expired(10_000)?;
                if !pg.heartbeat_instance(current, 30)? {
                    bail!("engine PostgreSQL owner epoch was fenced during startup");
                }
            }
            let subs = if startup_load_anthropic {
                db.load_active(startup_fleet.as_deref())?
            } else {
                Vec::new()
            };
            let pool_rows = if startup_load_anthropic {
                db.load_pool_state()?
            } else {
                Vec::new()
            };
            // Durable auth-health: чтобы мёртвые (забаненные) подписки сразу были вне ротации и помечены
            // на панели, а не «воскресали» здоровыми на каждый рестарт (старый эфемерный auth_dead-баг).
            let health_rows = if startup_load_anthropic {
                db.load_sub_health(startup_fleet.as_deref())?
            } else {
                Vec::new()
            };
            let sqlite_reconcile = if startup_billing && !startup_is_postgres {
                Some(
                    db.sqlite_connection()
                        .and_then(registry::reconcile_reservations)
                        .map_err(|e| e.to_string()),
                )
            } else {
                None
            };
            Ok::<_, anyhow::Error>((
                owner,
                subs,
                recovery,
                pool_rows,
                health_rows,
                sqlite_reconcile,
            ))
        })
        .await
        .context("engine authority startup task failed")??;
    if recovery != registry::pg::ReconcileReport::default() {
        eprintln!(
            "PostgreSQL recovery: canceled={} full_hold={} outbox={}",
            recovery.canceled_before_delivery,
            recovery.charged_after_delivery,
            recovery.processed_outbox,
        );
    }
    if let Some(result) = sqlite_reconcile {
        match result {
            Ok(n) if n > 0 => {
                eprintln!("реконсиляция резервов: возвращено {n} ключам (краш-остатки)")
            }
            Err(e) => eprintln!("⚠ reconcile резервов не удался: {e}"),
            _ => {}
        }
    }
    let n = subs.len();

    if s.proxy.pricing_shadow.enabled() {
        if !authority.is_postgres() {
            bail!("pricing shadow producer requires PostgreSQL authority");
        }
        if !s.billing {
            bail!("pricing shadow producer requires billing to be enabled");
        }
        if !matches!(
            s.provider,
            forward::ProviderMode::Anthropic | forward::ProviderMode::OpenAi
        ) {
            bail!("pricing shadow producer requires a fixed Anthropic or OpenAI provider plane");
        }
    }

    // Биллинг — async DB-акторы (синхронный SQLite на выделенных потоках, не на воркерах рантайма):
    // 1 writer + N reader-потоков (WAL параллелит чтения). N ограничен диапазоном 4..=16.
    let billing = if s.billing {
        // Sized from the host by default, but capped by an explicit budget because the engine is
        // not the only tenant of the authority database.
        //
        // Each slot opens one writer plus this many readers, and blue-green deliberately runs two
        // generations at once, so the real demand is (slots + 1) * (readers + 1). With four slots on
        // a 16-core host that is 85 of a 100-connection server before commerce, CRM, sales and
        // openkeys are counted — and a cutover that cannot open its connections fails closed with
        // `remaining connection slots are reserved for roles with the SUPERUSER attribute`, which
        // reads like a database outage rather than a capacity plan that no longer fits.
        let readers = s.billing_readers;
        // TTL-кэш key_auth (мс): срезает read/запрос под нагрузкой; reserve перечитывает баланс
        // атомарно, а кэш чистится при смене статуса ключа/аккаунта.
        let billing_authority = authority.clone();
        let billing_owner = owner.clone();
        let pricing_shadow_readers = if s.proxy.pricing_shadow.enabled() {
            s.proxy.pricing_shadow.db_read_connections()
        } else {
            0
        };
        let actor = tokio::task::spawn_blocking(move || {
            forward::AsyncBilling::start_authority_with_pricing_shadow(
                billing_authority,
                billing_owner,
                readers,
                pricing_shadow_readers,
                0,
            )
        })
        .await
        .context("billing authority startup task failed")??;
        Some(Arc::new(actor))
    } else {
        None
    };

    // Poke поллера: и расписание (reload/poll_loop), и probe-по-требованию из forward (401/403 →
    // ранний clean-probe). Создаём ДО AppState, чтобы отдать хэндл в него. Без поллера — probe не нужен.
    let poke = std::sync::Arc::new(tokio::sync::Notify::new());
    let accepting = Arc::new(AtomicBool::new(true));
    let authority_ready = Arc::new(AtomicBool::new(true));
    spawn_pre_drain_signal(accepting.clone());
    let fleet_size = subs.len();
    let affinity_secret = s
        .affinity_secret
        .as_ref()
        .map(|secret| format!("{secret}:{}", s.provider.as_str()));
    let affinity = Arc::new(
        forward::AffinityStore::new(
            s.redis_url.as_deref(),
            affinity_secret.as_deref(),
            s.affinity_ttl_secs,
            s.affinity_local_ttl_secs,
            s.affinity_redis_timeout_ms,
        )
        .context("initialize cache affinity")?,
    );
    eprintln!(
        "cache affinity [{}]: local L1 + {} L2",
        s.provider.as_str(),
        if affinity.redis_configured() {
            "Redis"
        } else {
            "no shared"
        }
    );
    let codex = if let Some(config) = s.codex.clone() {
        let calibration_store = billing
            .clone()
            .context("Codex provider requires the durable billing authority for calibration")?;
        let gateway = Arc::new(
            forward::CodexGateway::new_with_calibration(config, Some(calibration_store))
                .context("initialize Codex provider")?,
        );
        gateway
            .preflight()
            .await
            .context("validate Codex provider")?;
        eprintln!("Codex app-server provider preflight passed");
        tokio::spawn(poller::codex_health_loop(gateway.clone()));
        spawn_codex_reconcile_signal(gateway.clone());
        Some(gateway)
    } else {
        None
    };
    let gemini = if let Some(config) = s.gemini.clone() {
        let calibration_store = billing
            .clone()
            .context("Gemini provider requires the durable billing authority for calibration")?;
        let gateway = Arc::new(
            forward::GeminiGateway::new_with_calibration(config, Some(calibration_store))
                .context("initialize Gemini provider")?,
        );
        gateway
            .preflight()
            .await
            .context("validate Gemini provider")?;
        eprintln!("Gemini OAuth subscription provider preflight passed");
        tokio::spawn(poller::gemini_health_loop(gateway.clone()));
        Some(gateway)
    } else {
        None
    };
    let metrics = Arc::new(forward::Metrics::new());
    let pricing_shadow = Some(forward::PricingShadowRuntime::start(
        s.proxy.pricing_shadow,
        s.pricing_shadow_manifest.clone(),
        billing.clone(),
        metrics.clone(),
    )?);
    if s.proxy.pricing_shadow.enabled() {
        eprintln!(
            "pricing shadow: enabled sample={}bp queue={} workers={} timeout={}ms max_age={}s rate={}/s burst={} db_readers={}",
            s.proxy.pricing_shadow.sample_bp(),
            s.proxy.pricing_shadow.queue_capacity(),
            s.proxy.pricing_shadow.worker_concurrency(),
            s.proxy.pricing_shadow.timeout_ms(),
            s.proxy.pricing_shadow.max_queue_age_secs(),
            s.proxy.pricing_shadow.rate_per_sec(),
            s.proxy.pricing_shadow.rate_burst(),
            s.proxy.pricing_shadow.db_read_connections(),
        );
    } else {
        eprintln!("pricing shadow: disabled (default-off)");
    }
    let app = AppState {
        provider: s.provider,
        cfg: Arc::new(s.proxy.clone()),
        authority: Arc::new(authority.clone()),
        data_db_path: Arc::new(s.db_path.clone()),
        pool: Arc::new(Pool::new(
            subs,
            pool::Reserve::new(s.reserve5h, s.reserve7d, s.reserve_jitter),
            s.cap5h_usd,
            s.cap7d_usd,
        )),
        affinity,
        clients: Arc::new(Clients::new(&s.proxy)),
        codex,
        gemini,
        billing,
        pricing_shadow,
        authority_ready: authority_ready.clone(),
        breaker: Arc::new(forward::Breaker::new(fleet_size)),
        metrics,
        key_limiter: Arc::new(forward::KeyLimiter::new()),
        // Глобальный потолок одновременной обработки (анти-DoS). Permit живёт до EOF/drop response;
        // 24 максимальных 32 MiB JSON-ответа держат accumulator-envelope не выше 768 MiB.
        concurrency: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
        probe_poke: if s.proxy.poll {
            Some(poke.clone())
        } else {
            None
        },
    };
    let restored = pool_rows.len();
    let repaired_calibrations = if serves_anthropic {
        app.pool.import_state(pool_rows)
    } else {
        0
    };
    if restored > 0 {
        eprintln!("восстановлено состояние пула: {restored} подписок");
    }
    if repaired_calibrations > 0 {
        eprintln!(
            "pool calibration: quarantined {repaired_calibrations} implausible persisted estimate(s)"
        );
    }
    let dead_restored = health_rows
        .iter()
        .filter(|h| h.auth_state == "dead")
        .count();
    if serves_anthropic {
        app.pool.import_health(health_rows);
    }
    if dead_restored > 0 {
        eprintln!("восстановлено auth-health: {dead_restored} мёртвых подписок (вне ротации)");
    }
    if let Some(owner) = owner.clone() {
        tokio::spawn(poller::owner_heartbeat_loop(
            authority.clone(),
            owner.clone(),
            authority_ready.clone(),
        ));
    }
    if app.billing.is_some() {
        tokio::spawn(poller::billing_recovery_loop(
            authority.clone(),
            owner.clone(),
            authority_ready.clone(),
        ));
    }

    if serves_anthropic {
        // Write-through персист: pool сигналит cooling → poke → persist_loop пишет (плюс safety-flush).
        let persist_poke = std::sync::Arc::new(tokio::sync::Notify::new());
        {
            let pk = persist_poke.clone();
            app.pool
                .set_on_change(std::sync::Arc::new(move || pk.notify_one()));
        }
        let repair_persist_poke = persist_poke.clone();
        tokio::spawn(poller::persist_loop(
            app.clone(),
            authority.clone(),
            owner.clone(),
            persist_poke,
        ));
        if repaired_calibrations > 0 {
            // `Notify` retains one permit even if the task has not reached `notified()` yet.
            repair_persist_poke.notify_one();
        }

        tokio::spawn(poller::reload_loop(
            app.clone(),
            authority.clone(),
            s.fleet.clone(),
            poke.clone(),
        ));
        // One provider process owns shared retention; the recovery loop itself remains leader-elected.
        if s.billing {
            tokio::spawn(poller::ledger_prune_loop(
                authority.clone(),
                LEDGER_RETENTION_DAYS,
                REQUEST_LIFECYCLE_RETENTION_DAYS,
            ));
        }
        if s.proxy.poll {
            tokio::spawn(poller::poll_loop(
                app.clone(),
                authority.clone(),
                owner.clone(),
                poke.clone(),
            ));
            eprintln!("поллер лимитов: событийный (liveness-only)");
        }
        // Коллектор истории метрик: снапшоты агрегата (спрос/предложение/headroom) в отдельную metrics.db.
        let mdir = std::path::Path::new(&s.db_path)
            .parent()
            .map(|d| d.to_string_lossy().into_owned())
            .unwrap_or_else(|| ".".into());
        let metrics_db = format!("{mdir}/metrics.db");
        tokio::spawn(poller::metrics_loop(
            app.clone(),
            metrics_db,
            METRICS_RETENTION_DAYS,
        ));
        eprintln!(
            "коллектор истории: metrics.db (снапшот/60с, retention {METRICS_RETENTION_DAYS}д)"
        );
    }
    if s.proxy.api_keys.is_empty() {
        if s.proxy.trust_loopback {
            eprintln!(
                "⚠️  CLAUDE_API_KEYS не заданы — админ ТОЛЬКО с loopback (bind {})",
                s.bind
            );
        } else {
            eprintln!(
                "🛑 CLAUDE_API_KEYS не заданы, а bind {} НЕ loopback — сервер ОТКЛОНЯЕТ все \
                       запросы (за реверс-прокси peer виден как 127.0.0.1). Задай CLAUDE_API_KEYS.",
                s.bind
            );
        }
    }

    let listener = tokio::net::TcpListener::bind(&s.bind).await?;
    eprintln!(
        "claude-api слушает http://{}  (provider {}, подписок: {n}, апстрим {}, реестр {})",
        s.bind,
        s.provider.as_str(),
        s.proxy.upstream,
        authority.label()
    );
    // Graceful shutdown: сначала снимаем readiness и даём балансировщику убрать инстанс, затем axum
    // дренирует in-flight стримы. Общий deadline включает propagation-delay, дренаж и billing FIFO-flush.
    let flush_app = app.clone();
    let flush_authority = authority.clone();
    let flush_owner = owner.clone();
    let shutdown_accepting = accepting.clone();
    let readiness_delay = Duration::from_secs(s.readiness_delay_secs);
    let drain_deadline = Duration::from_secs(s.drain_deadline_secs);
    let (shutdown_started_tx, shutdown_started_rx) = tokio::sync::oneshot::channel();
    let (serve_result, shutdown_deadline, forced_stream_cut) = {
        let shutdown = async move {
            shutdown_signal().await;
            shutdown_accepting.store(false, Ordering::Release);
            eprintln!(
                "graceful shutdown: readiness снята, жду {}с перед дренажем",
                readiness_delay.as_secs()
            );
            let _ = shutdown_started_tx.send(());
            tokio::time::sleep(readiness_delay).await;
        };
        let server = axum::serve(
            listener,
            http::router(app, accepting).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .into_future();
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => (result, None, false),
            started = shutdown_started_rx => match started {
                Ok(()) => {
                    let deadline = tokio::time::Instant::now() + drain_deadline;
                    match tokio::time::timeout_at(deadline, &mut server).await {
                        Ok(result) => (result, Some(deadline), false),
                        Err(_) => (Ok(()), Some(deadline), true),
                    }
                }
                Err(_) => ((&mut server).await, None, false),
            }
        }
    };
    if forced_stream_cut {
        eprintln!(
            "⚠ graceful shutdown: deadline {}с исчерпан — принудительно обрываю оставшиеся стримы",
            s.drain_deadline_secs
        );
    } else if shutdown_deadline.is_some() {
        eprintln!("graceful shutdown: дренаж стримов завершён");
    }
    if let Some(codex) = &flush_app.codex {
        eprintln!("graceful shutdown: жду Codex settlement tasks и останавливаю children");
        codex.shutdown_until(shutdown_deadline).await;
    }
    if let Some(gemini) = &flush_app.gemini {
        // Detached Gemini streams continue draining to the authoritative final usageMetadata after
        // a client disconnect. Hold the billing FIFO open until those settlements are queued.
        gemini.shutdown_until(shutdown_deadline).await;
    }
    if let Some(pricing_shadow) = &flush_app.pricing_shadow {
        pricing_shadow.shutdown_until(shutdown_deadline).await;
    }
    eprintln!("graceful shutdown: дренирую очередь биллинга + флаш пула");
    // Завершённые/оборванные стримы поставили settle в очередь DB-актора. Даже после deadline ждём
    // обязательный FIFO-барьер: ограничение дренажа не должно превращаться в потерянную выручку.
    if let Some(b) = &flush_app.billing {
        let flushed_in_deadline = match shutdown_deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, b.flush()).await {
                Ok(result) => {
                    result.context("billing writer failed during graceful shutdown")?;
                    true
                }
                Err(_) => false,
            },
            None => {
                b.flush()
                    .await
                    .context("billing writer failed during graceful shutdown")?;
                true
            }
        };
        if !flushed_in_deadline {
            eprintln!(
                "⚠ graceful shutdown: billing flush не уложился в deadline; продолжаю обязательный flush"
            );
            // Never convert a slow money-writer into lost revenue by exiting on a local timeout.
            b.flush()
                .await
                .context("billing writer failed after shutdown deadline")?;
        }
    }
    if serves_anthropic {
        if let Err(e) =
            poller::persist_state_cas(&flush_app, &flush_authority, flush_owner.as_ref(), 8).await
        {
            eprintln!("⚠ финальный флаш не удался после CAS retry: {e}");
        }
    }
    drop(instance_lock);
    serve_result?;
    Ok(())
}

/// Ждать сигнала завершения: SIGINT (Ctrl-C) или SIGTERM (деплой/systemd stop).
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => {
                s.recv().await;
            }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}

/// SIGUSR1 is a load-balancer pre-drain: readiness flips immediately, but the listener and existing
/// streams remain alive until the deployer later sends SIGTERM through `systemctl stop`.
#[cfg(unix)]
fn spawn_pre_drain_signal(accepting: Arc<AtomicBool>) {
    tokio::spawn(async move {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1()) {
            Ok(mut signal) => {
                if signal.recv().await.is_some() {
                    accepting.store(false, Ordering::Release);
                    eprintln!(
                        "pre-drain: readiness снята по SIGUSR1; listener остаётся для in-flight"
                    );
                }
            }
            Err(e) => eprintln!("⚠ SIGUSR1 pre-drain handler unavailable: {e}"),
        }
    });
}

#[cfg(not(unix))]
fn spawn_pre_drain_signal(_accepting: Arc<AtomicBool>) {}

/// SIGUSR2 is the per-home drain handshake with the Codex app-server supervisor. Rediscovery first
/// removes a fenced home from selection, then waits for its admitted turns and closes that slot's
/// official websocket bridge. The supervisor does not restart the daemon until those proxies are
/// gone.
#[cfg(unix)]
fn spawn_codex_reconcile_signal(gateway: Arc<forward::CodexGateway>) {
    tokio::spawn(async move {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined2()) {
            Ok(mut signal) => {
                while signal.recv().await.is_some() {
                    gateway.probe_health().await;
                }
            }
            Err(e) => eprintln!("⚠ SIGUSR2 Codex reconcile handler unavailable: {e}"),
        }
    });
}

#[cfg(not(unix))]
fn spawn_codex_reconcile_signal(_gateway: Arc<forward::CodexGateway>) {}
