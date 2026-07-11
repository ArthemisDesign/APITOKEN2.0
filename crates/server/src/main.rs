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

use anyhow::Result;
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
    }
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
    match detect_plan(&client, &s.proxy, &tok).await {
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

    let app = AppState {
        cfg: Arc::new(s.proxy.clone()),
        pool: Arc::new(Pool::new(subs, s.proxy.util_cap)),
        clients: Arc::new(Clients::new(&s.proxy)),
    };

    tokio::spawn(poller::reload_loop(app.clone(), s.db_path.clone(), s.fleet.clone()));
    if s.proxy.poll {
        tokio::spawn(poller::poll_loop(app.clone()));
        eprintln!("поллер лимитов: включён");
    }
    if s.proxy.api_keys.is_empty() {
        eprintln!("⚠️  CLAUDE_API_KEYS не заданы — сервер принимает ТОЛЬКО с 127.0.0.1");
    }

    let listener = tokio::net::TcpListener::bind(&s.bind).await?;
    eprintln!("claude-api слушает http://{}  (подписок: {n}, апстрим {}, реестр {})",
        s.bind, s.proxy.upstream, s.db_path);
    axum::serve(listener, http::router(app).into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}
