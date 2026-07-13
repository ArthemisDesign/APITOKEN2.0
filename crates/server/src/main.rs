//! claude-api — пул Claude-подписок как ПРОЗРАЧНЫЙ /v1 API.
//!
//! Крейт `server` — КОМПОЗИЦИЯ: читает окружение, поднимает пул из реестра, стартует
//! фоновые циклы и HTTP-роутер. Логика по слоям: registry ← pool ← forward ← server.
//!
//!   claude-api serve                 # поднять сервер (переменные окружения см. config.rs)
//!   claude-api sub add <email> --token <tok> [--proxy ...] [--fleet prod]
//!   claude-api sub add-file <email> --token-file <path> [--proxy ...] [--fleet ...]
//!   claude-api sub list | rm <email> | status <email> <s> | proxy <email> <p> | fleet <email> <f>

mod config;
mod http;
mod poller;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use config::Settings;
use forward::{detect_plan, AppState, Clients, PlanDetect};
use pool::Pool;
use std::net::SocketAddr;
use std::sync::Arc;

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
    /// Выпустить ключ ПОД аккаунт (баланс общий с аккаунтом). Без --key генерится и печатается ОДИН раз.
    Issue {
        #[arg(long)] account: String,
        /// Имя ключа (проект/член команды) — для атрибуции расхода.
        #[arg(long)] label: Option<String>,
        #[arg(long)] key: Option<String>,
    },
    /// Показать ключ: аккаунт, метка, расход по ключу, баланс аккаунта.
    Balance { key: String },
    /// Список ключей (ключ маскируется).
    List,
    /// Заблокировать ключ.
    Disable { key: String },
    /// Разблокировать ключ.
    Enable { key: String },
    /// Удалить ключ НАВСЕГДА (баланс аккаунта не трогается).
    Rm { key: String },
    /// Удалить ВСЕ ключи (нужен --yes). Балансы аккаунтов НЕ трогаются.
    Clear {
        #[arg(long)] yes: bool,
    },
}

#[derive(Subcommand)]
enum SubOp {
    /// Добавить подписку с inline-токеном
    Add {
        email: String,
        #[arg(long)] token: String,
        #[arg(long, default_value = "")] proxy: String,
        #[arg(long, default_value = "prod")] fleet: String,
    },
    /// Добавить подписку, читая токен из файла
    AddFile {
        email: String,
        #[arg(long)] token_file: String,
        #[arg(long, default_value = "")] proxy: String,
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
    /// Сменить прокси
    Proxy { email: String, proxy: String },
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
fn gen_account_id() -> Result<String> {
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
    if k.len() <= 12 {
        return format!("{}…", &k[..k.len().min(4)]);
    }
    format!("{}…{}", &k[..8], &k[k.len() - 4..])
}

/// Сгенерировать ключ из /dev/urandom (ровно 24 байта → hex): sk-pool-<48hex>.
/// ВАЖНО: читаем фиксированный буфер (`read_exact`), а не весь файл — /dev/urandom бесконечен.
fn gen_key() -> Result<String> {
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
        KeyOp::Issue { account, label, key } => {
            // проверяем, что аккаунт есть (иначе ключ никуда не резолвится)
            if registry::account_get(&conn, &account)?.is_none() {
                println!("аккаунт не найден: {account} (создай: claude-api account create)"); return Ok(());
            }
            let key = match key { Some(k) => k, None => gen_key()? };
            registry::key_issue(&conn, &key, &account, label.as_deref())?;
            println!("✓ ключ выпущен: {key}");
            println!("  аккаунт: {account}  ·  метка: {}  (баланс общий с аккаунтом)",
                label.as_deref().unwrap_or("—"));
        }
        KeyOp::Balance { key } => match registry::key_get(&conn, &key)? {
            Some(r) => {
                let acct = r.account_id.clone().unwrap_or_default();
                let abal = registry::account_get(&conn, &acct)?
                    .map(|a| usd_str(a.balance_nano)).unwrap_or_else(|| "?".into());
                println!("{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tбаланс_аккаунта={}\tstatus={}",
                    mask_key(&r.key), acct, r.label.as_deref().unwrap_or("—"),
                    usd_str(r.spent_nano), abal, r.status);
            }
            None => println!("ключ не найден: {}", mask_key(&key)),
        },
        KeyOp::List => {
            let rows = registry::key_list(&conn)?;
            if rows.is_empty() { println!("(ключей нет) · БД: {}", s.db_path); return Ok(()); }
            for r in rows {
                println!("{}\tаккаунт={}\tметка={}\tрасход_ключа={}\tstatus={}",
                    mask_key(&r.key), r.account_id.as_deref().unwrap_or("—"),
                    r.label.as_deref().unwrap_or("—"), usd_str(r.spent_nano), r.status);
            }
        }
        KeyOp::Disable { key } => println!("обновлено: {}", registry::key_set_status(&conn, &key, "disabled")?),
        KeyOp::Enable { key } => println!("обновлено: {}", registry::key_set_status(&conn, &key, "active")?),
        KeyOp::Rm { key } => println!("удалено ключей: {}", registry::key_remove(&conn, &key)?),
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
    let client = match clients.get(&proxy) { Ok(c) => c, Err(e) => return format!("proxy: {e}") };
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
        SubOp::Add { email, token, proxy, fleet } => {
            registry::add(&conn, &email, &token, &proxy, &fleet)?;
            let plan = detect_and_store(&s, &email).await;   // авто-детект тарифа
            println!("✓ добавлена {email} (fleet={fleet}, proxy={}) · {plan}",
                if proxy.is_empty() { "—" } else { &proxy });
        }
        SubOp::AddFile { email, token_file, proxy, fleet } => {
            registry::add_file(&conn, &email, &token_file, &proxy, &fleet)?;
            let plan = detect_and_store(&s, &email).await;   // авто-детект тарифа
            println!("✓ добавлена {email} (token_file={token_file}, fleet={fleet}) · {plan}");
        }
        SubOp::List => {
            let rows = registry::list(&conn)?;
            if rows.is_empty() { println!("(подписок нет) · БД: {}", s.db_path); return Ok(()); }
            for r in rows {
                println!("{}\tstatus={}\tfleet={}\tplan={}\ttoken={}\tproxy={}",
                    r.email, r.status, r.fleet,
                    if r.plan.is_empty() { "—" } else { &r.plan },
                    if r.has_token { "есть" } else { "НЕТ" },
                    if r.proxy.is_empty() { "—" } else { &r.proxy });
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
        SubOp::Proxy { email, proxy } => { println!("обновлено: {}", registry::set_proxy(&conn, &email, &proxy)?); }
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
    let conn = registry::open(&s.db_path)?;
    let subs = registry::load_active(&conn, s.fleet.as_deref())?;
    drop(conn);
    let n = subs.len();

    // Биллинг — async DB-акторы (синхронный SQLite на выделенных потоках, не на воркерах рантайма):
    // 1 writer + N reader-потоков (WAL параллелит чтения). N из env, деф 4 — с запасом под 1000 юзеров.
    let billing = if s.billing {
        let readers = std::env::var("CLAUDE_API_DB_READERS").ok()
            .and_then(|v| v.parse().ok()).unwrap_or(4);
        Some(Arc::new(forward::AsyncBilling::start(s.db_path.clone(), readers)?))
    } else {
        None
    };

    let app = AppState {
        cfg: Arc::new(s.proxy.clone()),
        pool: Arc::new(Pool::new(subs,
            pool::Reserve::new(s.reserve5h, s.reserve7d, s.reserve_jitter),
            s.cap5h_usd, s.cap7d_usd)),
        clients: Arc::new(Clients::new(&s.proxy)),
        billing,
        breaker: Arc::new(forward::Breaker::new()),
        metrics: Arc::new(forward::Metrics::new()),
        key_limiter: Arc::new(forward::KeyLimiter::new()),
    };

    // Восстановить durable-состояние пула (cooling/калибровка) — бан на дни переживает деплой;
    // и реконсилить осиротевшие при краше резервы баланса обратно клиентам.
    if let Ok(conn) = registry::open(&s.db_path) {
        if let Ok(rows) = registry::load_pool_state(&conn) {
            let n = rows.len();
            app.pool.import_state(rows);
            if n > 0 { eprintln!("восстановлено состояние пула: {n} подписок"); }
        }
        // Реконсиляция осиротевших резервов — ТОЛЬКО если НЕ обнаружен другой живой инстанс. Иначе
        // при перекрытии деплоя (старый ещё дренирует стримы с живыми резервами) вернули бы ЧУЖИЕ
        // холды → двойное зачисление на settle старого инстанса. Детект по PID-lockfile + /proc.
        if s.billing {
            let lockpath = format!("{}.lock", s.db_path);
            let me = std::process::id();
            // «Живой другой инстанс» = PID из лока занят другим процессом, который РЕАЛЬНО делает `serve`
            // (проверяем /proc/pid/cmdline на аргумент "serve", а не просто comm=claude-api). Иначе
            // переиспользование PID нашей же CLI-командой (backup по таймеру, authbot sub add, topup)
            // дало бы ложный «alive» → reconcile пропущен → осиротевшие резервы застряли бы до чистого
            // рестарта. Исключаем собственный PID (reuse нашего же PID).
            let other_alive = std::fs::read_to_string(&lockpath).ok()
                .and_then(|p| p.trim().parse::<u32>().ok())
                .filter(|&pid| pid != me)
                .map(|pid| std::fs::read(format!("/proc/{pid}/cmdline"))
                    .map(|c| c.split(|&b| b == 0).any(|arg| arg == b"serve"))
                    .unwrap_or(false))
                .unwrap_or(false);
            let _ = std::fs::write(&lockpath, me.to_string());
            if other_alive {
                eprintln!("⚠ обнаружен другой живой инстанс — ПРОПУСКАЮ reconcile резервов (защита денег)");
            } else {
                match registry::reconcile_reservations(&conn) {
                    Ok(n) if n > 0 => eprintln!("реконсиляция резервов: возвращено {n} ключам (краш-остатки)"),
                    Err(e) => eprintln!("⚠ reconcile резервов не удался: {e}"),
                    _ => {}
                }
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

    let poke = std::sync::Arc::new(tokio::sync::Notify::new());
    tokio::spawn(poller::reload_loop(app.clone(), s.db_path.clone(), s.fleet.clone(), poke.clone()));
    // Фоновая обрезка ledger под масштаб (retention из env, деф 30д; 0 = выключено).
    if s.billing {
        let days = std::env::var("CLAUDE_API_LEDGER_DAYS").ok().and_then(|v| v.parse().ok()).unwrap_or(30);
        tokio::spawn(poller::ledger_prune_loop(s.db_path.clone(), days));
    }
    if s.proxy.poll {
        tokio::spawn(poller::poll_loop(app.clone(), poke.clone()));
        eprintln!("поллер лимитов: событийный (liveness-only)");
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
        tokio::time::timeout(std::time::Duration::from_secs(10), b.flush()).await.ok();
    }
    match registry::open(&flush_db) {
        Ok(conn) => if let Err(e) = registry::save_pool_state(&conn, &flush_app.pool.export_state()) {
            eprintln!("⚠ финальный флаш не удался: {e}");
        },
        Err(e) => eprintln!("⚠ финальный флаш: открыть БД не удалось: {e}"),
    }
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
