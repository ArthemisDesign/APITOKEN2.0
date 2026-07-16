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
use std::net::SocketAddr;
use std::sync::Arc;

// Fail-closed bounded defaults until these knobs are represented by validated Settings fields.
// AUDIT-TODO(C60/C89): add typed, range-checked fields in config.rs before restoring env overrides.
const KEY_AUTH_TTL_MS: u64 = 1000;
const MAX_CONCURRENT: usize = 1024;
const LEDGER_RETENTION_DAYS: i64 = 30;
const METRICS_RETENTION_DAYS: i64 = 90;

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
    if let Some(parent) = std::path::Path::new(&lock_path).parent()
        .filter(|p| !p.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("создать каталог lock-файла {lock_path}"))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
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
    write!(file, "{}\n", std::process::id())?;
    Ok(file)
}

#[cfg(not(unix))]
fn acquire_instance_lock(_db_path: &str) -> Result<std::fs::File> {
    bail!("безопасный single-instance lock для этой платформы не реализован")
}

#[derive(Parser)]
#[command(name = "claude-api", about = "Пул Claude-подписок как прозрачный /v1 API")]
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
        #[arg(long)] out: Option<String>,
        /// Сколько последних снимков хранить (деф 24).
        #[arg(long, default_value = "24")] keep: usize,
    },
}

#[derive(Subcommand)]
enum AccountOp {
    /// Создать аккаунт (баланс 0). Печатает id. --handle — внешняя идентичность (TG id/email).
    Create {
        #[arg(long)] handle: Option<String>,
        /// Наценка × 10000 (2000 = ×0.20). По умолчанию — из CLAUDE_API_MULT_BP.
        #[arg(long)] mult_bp: Option<i64>,
        /// Задать id явно (иначе сгенерируется acct_…).
        #[arg(long)] id: Option<String>,
    },
    /// Пополнить баланс аккаунта на USD (можно отрицательное — коррекция). --ref — метка платежа.
    #[command(allow_negative_numbers = true)] // чтобы `--usd -100` (коррекция) не парсился как флаг
    Topup { id: String, #[arg(long)] usd: f64, #[arg(long)] r#ref: Option<String> },
    /// Показать баланс/расход аккаунта.
    Balance { id: String },
    /// Список аккаунтов.
    List,
    /// Заблокировать/разблокировать аккаунт (все его ключи тоже перестают/начинают работать).
    Disable { id: String },
    Enable { id: String },
    /// Удалить аккаунт НАВСЕГДА вместе с ключами и историей (нужен --yes).
    Rm { id: String, #[arg(long)] yes: bool },
}

#[derive(Subcommand)]
enum KeyOp {
    /// Выпустить ключ ПОД аккаунт. Без --key-file генерится и печатается ОДИН раз.
    Issue {
        #[arg(long)] account: String,
        /// Имя ключа (проект/член команды) — для атрибуции расхода.
        #[arg(long)] label: Option<String>,
        /// Прочитать заданный ключ из mode-0600 файла; без флага ключ генерируется.
        #[arg(long)] key_file: Option<String>,
    },
    /// Показать ключ: аккаунт, метка, расход по ключу, баланс аккаунта.
    Balance { #[arg(long)] key_file: String },
    /// Список ключей (ключ маскируется).
    List,
    /// Заблокировать ключ.
    Disable { #[arg(long)] key_file: String },
    /// Разблокировать ключ.
    Enable { #[arg(long)] key_file: String },
    /// Удалить ключ НАВСЕГДА (баланс аккаунта не трогается).
    Rm { #[arg(long)] key_file: String },
    /// Удалить ВСЕ ключи (нужен --yes). Балансы аккаунтов НЕ трогаются.
    Clear {
        #[arg(long)] yes: bool,
    },
}

#[derive(Subcommand)]
enum SubOp {
    /// Добавить подписку, читая секреты из mode-0600 файлов.
    AddFile {
        email: String,
        #[arg(long)] token_file: String,
        /// Файл с proxy URL; не передавайте credentialed URL через argv.
        #[arg(long)] proxy_file: Option<String>,
        #[arg(long, default_value = "prod")] fleet: String,
    },
    /// Список подписок
    List,
    /// Удалить подписку
    Rm { email: String },
    /// Удалить ВСЕ подписки (нужен --yes; --fleet ограничивает флотом)
    Clear {
        #[arg(long)] yes: bool,
        #[arg(long)] fleet: Option<String>,
    },
    /// Сменить статус (active | paused | disabled)
    Status { email: String, status: String },
    /// Сменить прокси, читая URL из mode-0600 файла.
    Proxy { email: String, #[arg(long)] proxy_file: String },
    /// Сменить флот (dev | prod | ...)
    Fleet { email: String, fleet: String },
    /// Задать тариф вручную (pro | max5 | max20)
    SetPlan { email: String, plan: String },
    /// Определить тариф из /api/oauth/profile (без email — все без тарифа)
    DetectPlan { email: Option<String> },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => serve().await,
        Cmd::Sub { op } => sub_cmd(op).await,
        Cmd::Account { op } => account_cmd(op),
        Cmd::Key { op } => key_cmd(op),
        Cmd::Backup { out, keep } => backup_cmd(out, keep),
    }
}

/// Консистентный бэкап БД (`VACUUM INTO`) в timestamped-файл + ротация (храним `keep` последних).
/// НЕ копируем .db напрямую (WAL → битая копия). Рекомендация прод: снимки складывать И на ОТДЕЛЬНЫЙ
/// носитель/офсайт (напр. rclone/scp по тому же таймеру) — иначе отказ диска унесёт и БД, и бэкапы.
fn backup_cmd(out: Option<String>, keep: usize) -> Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let s = Settings::from_env();
    let dir = out.unwrap_or_else(|| {
        let p = std::path::Path::new(&s.db_path).parent()
            .map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        format!("{p}/backups")
    });
    std::fs::create_dir_all(&dir)?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = format!("{dir}/subscriptions.db.bak-{ts}");
    let conn = registry::open(&s.db_path)?;
    registry::backup_to(&conn, &path)?;
    let sz = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    println!("✓ бэкап: {path} ({} КиБ)", sz / 1024);
    // ротация: оставляем `keep` самых свежих snapshot'ов, старые удаляем
    let mut snaps: Vec<_> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_name().to_string_lossy().starts_with("subscriptions.db.bak-"))
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
    let conn = registry::open(&s.db_path)?;
    match op {
        AccountOp::Create { handle, mult_bp, id } => {
            let id = match id { Some(i) => i, None => gen_account_id()? };
            let mult = mult_bp.unwrap_or(s.mult_bp);
            registry::account_create(&conn, &id, handle.as_deref(), mult)?;
            println!("✓ аккаунт создан: {id}  ·  наценка ×{}  ·  handle={}",
                mult as f64 / 10000.0, handle.as_deref().unwrap_or("—"));
            println!("  выпустить ключ:  claude-api key issue --account {id}");
        }
        AccountOp::Topup { id, usd, r#ref } => {
            match registry::account_topup(&conn, &id, usd_to_nano(usd), r#ref.as_deref())? {
                Some(bal) => println!("✓ баланс аккаунта {id}: {}", usd_str(bal)),
                None => println!("аккаунт не найден: {id}"),
            }
        }
        AccountOp::Balance { id } => match registry::account_get(&conn, &id)? {
            Some(a) => println!("{}\tбаланс={}\tпотрачено={}\tрезерв={}\tнаценка=×{}\tstatus={}\thandle={}",
                a.id, usd_str(a.balance_nano), usd_str(a.spent_nano), usd_str(a.reserved_nano),
                a.mult_bp as f64 / 10000.0, a.status, a.handle.as_deref().unwrap_or("—")),
            None => println!("аккаунт не найден: {id}"),
        },
        AccountOp::List => {
            let rows = registry::account_list(&conn)?;
            if rows.is_empty() { println!("(аккаунтов нет) · БД: {}", s.db_path); return Ok(()); }
            for a in rows {
                println!("{}\tбаланс={}\tпотрачено={}\tнаценка=×{}\tstatus={}\thandle={}",
                    a.id, usd_str(a.balance_nano), usd_str(a.spent_nano),
                    a.mult_bp as f64 / 10000.0, a.status, a.handle.as_deref().unwrap_or("—"));
            }
        }
        AccountOp::Disable { id } => println!("обновлено: {}", registry::account_set_status(&conn, &id, "disabled")?),
        AccountOp::Enable { id } => println!("обновлено: {}", registry::account_set_status(&conn, &id, "active")?),
        AccountOp::Rm { id, yes } => {
            if !yes { println!("нужен --yes: удалит аккаунт {id} с ключами и историей"); }
            else { println!("удалено аккаунтов: {}", registry::account_remove(&conn, &id)?); }
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
        || !scheme.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
    {
        return "<proxy-redacted>".into();
    }

    let authority = rest.split(|c| matches!(c, '/' | '?' | '#')).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let port = if let Some(bracketed) = host_port.strip_prefix('[') {
        bracketed.split_once("]:")
            .and_then(|(_, p)| p.parse::<u16>().ok())
    } else {
        host_port.rsplit_once(':')
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

    let mut file = std::fs::File::open(path)
        .with_context(|| format!("открыть файл секрета {kind}"))?;
    let metadata = file.metadata()
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
    let value = value.trim_end_matches(|c| c == '\r' || c == '\n').to_string();
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
    let conn = registry::open(&s.db_path)?;
    match op {
        KeyOp::Issue { account, label, key_file } => {
            // проверяем, что аккаунт есть (иначе ключ никуда не резолвится)
            if registry::account_get(&conn, &account)?.is_none() {
                println!("аккаунт не найден: {account} (создай: claude-api account create)"); return Ok(());
            }
            let generated = key_file.is_none();
            let key = match key_file {
                Some(path) => read_secret_file(&path, "customer API key")?,
                None => gen_key()?,
            };
            registry::key_issue(&conn, &key, &account, label.as_deref())?;
            if generated {
                println!("✓ ключ выпущен: {key}");
            } else {
                println!("✓ ключ выпущен: {}", mask_key(&key));
            }
            println!("  аккаунт: {account}  ·  метка: {}  (баланс общий с аккаунтом)",
                label.as_deref().unwrap_or("—"));
        }
        KeyOp::Balance { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            match registry::key_get(&conn, &key)? {
                Some(r) => {
                    let acct = r.account_id.clone().unwrap_or_default();
                    let abal = registry::account_get(&conn, &acct)?
                        .map(|a| usd_str(a.balance_nano)).unwrap_or_else(|| "?".into());
                    println!("{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tбаланс_аккаунта={}\tstatus={}",
                        mask_key(&r.key), acct, r.label.as_deref().unwrap_or("—"),
                        usd_str(r.spent_nano), abal, r.status);
                }
                None => println!("ключ не найден: {}", mask_key(&key)),
            }
        }
        KeyOp::List => {
            let rows = registry::key_list(&conn)?;
            if rows.is_empty() { println!("(ключей нет) · БД: {}", s.db_path); return Ok(()); }
            for r in rows {
                println!("{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tstatus={}",
                    mask_key(&r.key), r.account_id.as_deref().unwrap_or("—"),
                    r.label.as_deref().unwrap_or("—"), usd_str(r.spent_nano), r.status);
            }
        }
        KeyOp::Disable { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("обновлено: {}", registry::key_set_status(&conn, &key, "disabled")?);
        }
        KeyOp::Enable { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("обновлено: {}", registry::key_set_status(&conn, &key, "active")?);
        }
        KeyOp::Rm { key_file } => {
            let key = read_secret_file(&key_file, "customer API key")?;
            println!("удалено ключей: {}", registry::key_remove(&conn, &key)?);
        }
        KeyOp::Clear { yes } => {
            if !yes { println!("нужен --yes: удалит ВСЕ ключи (балансы клиентов пропадут)"); }
            else { println!("удалено ключей: {}", registry::key_clear(&conn)?); }
        }
    }
    Ok(())
}

/// Определить тариф подписки (профиль Anthropic через её прокси) и записать в реестр.
/// Возвращает человекочитаемую строку итога.
async fn detect_and_store(s: &Settings, email: &str) -> String {
    let conn = match registry::open(&s.db_path) { Ok(c) => c, Err(e) => return format!("db: {e}") };
    let (tok, proxy) = match registry::get_creds(&conn, email) {
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
    match detect_plan(&client, &s.proxy, &tok, &ua).await {
        PlanDetect::Plan(p) => {
            let _ = registry::set_plan(&conn, email, &p);
            format!("тариф: {p}")
        }
        PlanDetect::NoScope =>
            "тариф: noscope (у токена нет scope user:profile — задай вручную: sub set-plan)".into(),
        PlanDetect::Err(e) => format!("тариф: не определён ({e})"),
    }
}

async fn sub_cmd(op: SubOp) -> Result<()> {
    let s = Settings::from_env();
    let conn = registry::open(&s.db_path)?;
    match op {
        SubOp::AddFile { email, token_file, proxy_file, fleet } => {
            let token = read_secret_file(&token_file, "subscription token")?;
            let proxy = match proxy_file {
                Some(path) => read_secret_file(&path, "proxy URL")?,
                None => String::new(),
            };
            registry::add(&conn, &email, &token, &proxy, &fleet)?;
            // Тариф НЕ детектим синтетическим запросом к Anthropic: у OAuth-токенов нет scope
            // user:profile → всё равно NoScope, а лишний запрос = фингерпринт. Ёмкость калибруется
            // из боевого трафика; при необходимости тариф задаётся вручную (`sub set-plan`).
            println!("✓ добавлена {email} (fleet={fleet}, proxy={})", mask_proxy(&proxy));
        }
        SubOp::List => {
            let rows = registry::list(&conn)?;
            if rows.is_empty() { println!("(подписок нет) · БД: {}", s.db_path); return Ok(()); }
            for r in rows {
                println!("{}\tstatus={}\tfleet={}\tplan={}\ttoken={}\tproxy={}",
                    r.email, r.status, r.fleet,
                    if r.plan.is_empty() { "—" } else { &r.plan },
                    if r.has_token { "есть" } else { "НЕТ" },
                    mask_proxy(&r.proxy));
            }
        }
        SubOp::Rm { email } => { println!("удалено строк: {}", registry::remove(&conn, &email)?); }
        SubOp::Clear { yes, fleet } => {
            if !yes {
                let n = registry::list(&conn)?.len();
                println!("⚠️  удалит ВСЕ подписки{} — {n} шт. Повтори с --yes для подтверждения.",
                    fleet.as_deref().map(|f| format!(" флота {f}")).unwrap_or_default());
            } else {
                let n = registry::clear(&conn, fleet.as_deref())?;
                println!("✓ удалено подписок: {n}{}", fleet.as_deref().map(|f| format!(" (флот {f})")).unwrap_or_default());
            }
        }
        SubOp::Status { email, status } => { println!("обновлено: {}", registry::set_status(&conn, &email, &status)?); }
        SubOp::Proxy { email, proxy_file } => {
            let proxy = read_secret_file(&proxy_file, "proxy URL")?;
            println!("обновлено: {}", registry::set_proxy(&conn, &email, &proxy)?);
        }
        SubOp::Fleet { email, fleet } => { println!("обновлено: {}", registry::set_fleet(&conn, &email, &fleet)?); }
        SubOp::SetPlan { email, plan } => { println!("обновлено: {} (plan={plan})", registry::set_plan(&conn, &email, &plan)?); }
        SubOp::DetectPlan { email } => {
            let targets: Vec<String> = match email {
                Some(e) => vec![e],
                None => registry::list(&conn)?.into_iter()
                    .filter(|r| r.plan.is_empty()).map(|r| r.email).collect(),
            };
            if targets.is_empty() { println!("нечего определять (у всех тариф уже задан)"); }
            for e in targets { println!("{e} → {}", detect_and_store(&s, &e).await); }
        }
    }
    Ok(())
}

async fn serve() -> Result<()> {
    let s = Settings::from_env();
    // Lock before reconciliation, pool loading, binding, or accepting work. The descriptor is held
    // through final billing/pool flush and makes overlapping instances fail closed.
    let instance_lock = acquire_instance_lock(&s.db_path)?;
    let conn = registry::open(&s.db_path)?;
    let subs = registry::load_active(&conn, s.fleet.as_deref())?;
    drop(conn);
    let n = subs.len();

    // Биллинг — async DB-акторы (синхронный SQLite на выделенных потоках, не на воркерах рантайма):
    // 1 writer + N reader-потоков (WAL параллелит чтения). N ограничен диапазоном 4..=16.
    let billing = if s.billing {
        let readers = std::thread::available_parallelism()
            .map(|n| n.get().clamp(4, 16))
            .unwrap_or(4);
        // TTL-кэш key_auth (мс): срезает read/запрос под нагрузкой; reserve перечитывает баланс
        // атомарно, а кэш чистится при смене статуса ключа/аккаунта.
        Some(Arc::new(forward::AsyncBilling::start_with(
            s.db_path.clone(), readers, KEY_AUTH_TTL_MS,
        )?))
    } else {
        None
    };

    // Poke поллера: и расписание (reload/poll_loop), и probe-по-требованию из forward (401/403 →
    // ранний clean-probe). Создаём ДО AppState, чтобы отдать хэндл в него. Без поллера — probe не нужен.
    let poke = std::sync::Arc::new(tokio::sync::Notify::new());
    let app = AppState {
        cfg: Arc::new(s.proxy.clone()),
        db_path: Arc::new(s.db_path.clone()),
        pool: Arc::new(Pool::new(subs,
            pool::Reserve::new(s.reserve5h, s.reserve7d, s.reserve_jitter),
            s.cap5h_usd, s.cap7d_usd)),
        clients: Arc::new(Clients::new(&s.proxy)),
        billing,
        breaker: Arc::new(forward::Breaker::new()),
        metrics: Arc::new(forward::Metrics::new()),
        key_limiter: Arc::new(forward::KeyLimiter::new()),
        // Глобальный потолок одновременной обработки (анти-DoS). Деф 1024 — с запасом под легит-нагрузку
        // (нагрузочный тест давал ~1000 req/с), но отсекает флуд от бесконтрольного роста.
        concurrency: Arc::new(tokio::sync::Semaphore::new(MAX_CONCURRENT)),
        probe_poke: if s.proxy.poll { Some(poke.clone()) } else { None },
    };

    // Восстановить durable-состояние пула (cooling/калибровка) — бан на дни переживает деплой;
    // и реконсилить осиротевшие при краше резервы баланса обратно клиентам.
    if let Ok(conn) = registry::open(&s.db_path) {
        if let Ok(rows) = registry::load_pool_state(&conn) {
            let n = rows.len();
            app.pool.import_state(rows);
            if n > 0 { eprintln!("восстановлено состояние пула: {n} подписок"); }
        }
        // Эксклюзивный advisory lock уже удерживается: живой второй writer не может дренировать
        // стримы параллельно, поэтому оставшиеся резервы действительно являются crash-остатками.
        if s.billing {
            match registry::reconcile_reservations(&conn) {
                Ok(n) if n > 0 => eprintln!("реконсиляция резервов: возвращено {n} ключам (краш-остатки)"),
                Err(e) => eprintln!("⚠ reconcile резервов не удался: {e}"),
                _ => {}
            }
        }
    }

    // Write-through персист: pool сигналит cooling → poke → persist_loop пишет (плюс safety-flush).
    let persist_poke = std::sync::Arc::new(tokio::sync::Notify::new());
    {
        let pk = persist_poke.clone();
        app.pool.set_on_change(std::sync::Arc::new(move || pk.notify_one()));
    }
    tokio::spawn(poller::persist_loop(app.clone(), s.db_path.clone(), persist_poke));

    tokio::spawn(poller::reload_loop(app.clone(), s.db_path.clone(), s.fleet.clone(), poke.clone()));
    // Фоновая обрезка ledger под масштаб; bounded default до переноса настройки в Settings.
    if s.billing {
        tokio::spawn(poller::ledger_prune_loop(
            s.db_path.clone(), LEDGER_RETENTION_DAYS,
        ));
    }
    if s.proxy.poll {
        tokio::spawn(poller::poll_loop(app.clone(), poke.clone()));
        eprintln!("поллер лимитов: событийный (liveness-only)");
    }
    // Коллектор истории метрик: снапшоты агрегата (спрос/предложение/headroom) в отдельную metrics.db.
    // Фундамент под capacity-planning и предсказательную модель; bounded retention 90д.
    {
        let mdir = std::path::Path::new(&s.db_path).parent()
            .map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        let metrics_db = format!("{mdir}/metrics.db");
        tokio::spawn(poller::metrics_loop(
            app.clone(), metrics_db, METRICS_RETENTION_DAYS,
        ));
        eprintln!(
            "коллектор истории: metrics.db (снапшот/60с, retention {METRICS_RETENTION_DAYS}д)"
        );
    }
    if s.proxy.api_keys.is_empty() {
        if s.proxy.trust_loopback {
            eprintln!("⚠️  CLAUDE_API_KEYS не заданы — админ ТОЛЬКО с loopback (bind {})", s.bind);
        } else {
            eprintln!("🛑 CLAUDE_API_KEYS не заданы, а bind {} НЕ loopback — сервер ОТКЛОНЯЕТ все \
                       запросы (за реверс-прокси peer виден как 127.0.0.1). Задай CLAUDE_API_KEYS.", s.bind);
        }
    }

    let listener = tokio::net::TcpListener::bind(&s.bind).await?;
    eprintln!("claude-api слушает http://{}  (подписок: {n}, апстрим {}, реестр {})",
        s.bind, s.proxy.upstream, s.db_path);
    // Graceful shutdown: на SIGTERM (деплой) / SIGINT axum ДОЖДЁТСЯ завершения in-flight стримов
    // (их HoldGuard/tee-метеринг корректно закроют резервы — не теряем деньги на деплое), затем —
    // финальный флаш состояния пула (свежая калибровка/cooling переживут рестарт без потери до 120с).
    let flush_app = app.clone();
    let flush_db = s.db_path.clone();
    axum::serve(listener, http::router(app).into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    eprintln!("graceful shutdown: дренаж стримов завершён, дренирую очередь биллинга + флаш пула");
    // Стримы дотекли → их TeeMeter/HoldGuard поставили последние settle в очередь DB-актора. Ждём,
    // пока актор их применит (flush = FIFO-барьер), иначе выход процесса потерял бы эти списания (выручку).
    if let Some(b) = &flush_app.billing {
        // Never convert a slow money-writer into lost revenue by exiting on a local timeout.
        // AUDIT-TODO(C12): make AsyncBilling::flush return Result so a closed writer is distinguishable
        // from an acknowledged FIFO barrier, then fail shutdown loudly on the former.
        b.flush().await;
    }
    match registry::open(&flush_db) {
        Ok(conn) => if let Err(e) = registry::save_pool_state(&conn, &flush_app.pool.export_state()) {
            eprintln!("⚠ финальный флаш не удался: {e}");
        },
        Err(e) => eprintln!("⚠ финальный флаш: открыть БД не удалось: {e}"),
    }
    drop(instance_lock);
    Ok(())
}

/// Ждать сигнала завершения: SIGINT (Ctrl-C) или SIGTERM (деплой/systemd stop).
async fn shutdown_signal() {
    let ctrl_c = async { let _ = tokio::signal::ctrl_c().await; };
    #[cfg(unix)]
    let term = async {
        match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            Ok(mut s) => { s.recv().await; }
            Err(_) => std::future::pending::<()>().await,
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
}
