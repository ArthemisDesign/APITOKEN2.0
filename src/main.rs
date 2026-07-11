//! claude-api — пул Claude-подписок как ПРОЗРАЧНЫЙ /v1 API.
//!
//! Для клиента неотличим от api.anthropic.com: наводишь Anthropic SDK на этот сервер
//! (base_url + наш api-key) — а под капотом запрос идёт на квоте подписки из пула, с
//! ротацией по лимитам. Реестр подписок (пункт 1) — SQLite, управляется подкомандой `sub`.
//!
//!   claude-api serve                 # поднять сервер (см. переменные окружения в config.rs)
//!   claude-api sub add <email> --token <tok> [--proxy ip:port] [--fleet prod]
//!   claude-api sub add-file <email> --token-file <path> [--proxy ...] [--fleet ...]
//!   claude-api sub list | rm <email> | status <email> <s> | proxy <email> <p> | fleet <email> <f>

mod config;
mod db;
mod pool;
mod poller;
mod proxy;
mod server;
mod state;
mod upstream;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use pool::Pool;
use state::AppState;
use std::net::SocketAddr;
use std::sync::Arc;
use upstream::Clients;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Serve) {
        Cmd::Serve => serve().await,
        Cmd::Sub { op } => sub_cmd(op),
    }
}

fn sub_cmd(op: SubOp) -> Result<()> {
    let cfg = Config::from_env();
    let conn = db::open(&cfg.db_path)?;
    match op {
        SubOp::Add { email, token, proxy, fleet } => {
            db::add(&conn, &email, &token, &proxy, &fleet)?;
            println!("✓ добавлена {email} (fleet={fleet}, proxy={})", if proxy.is_empty() { "—" } else { &proxy });
        }
        SubOp::AddFile { email, token_file, proxy, fleet } => {
            db::add_file(&conn, &email, &token_file, &proxy, &fleet)?;
            println!("✓ добавлена {email} (token_file={token_file}, fleet={fleet})");
        }
        SubOp::List => {
            let rows = db::list(&conn)?;
            if rows.is_empty() { println!("(подписок нет) · БД: {}", cfg.db_path); return Ok(()); }
            for (email, status, fleet, has_tok, proxy) in rows {
                println!("{email}\tstatus={status}\tfleet={fleet}\ttoken={}\tproxy={}",
                    if has_tok { "есть" } else { "НЕТ" },
                    if proxy.is_empty() { "—" } else { &proxy });
            }
        }
        SubOp::Rm { email } => { println!("удалено строк: {}", db::remove(&conn, &email)?); }
        SubOp::Status { email, status } => { println!("обновлено: {}", db::set_status(&conn, &email, &status)?); }
        SubOp::Proxy { email, proxy } => { println!("обновлено: {}", db::set_proxy(&conn, &email, &proxy)?); }
        SubOp::Fleet { email, fleet } => { println!("обновлено: {}", db::set_fleet(&conn, &email, &fleet)?); }
    }
    Ok(())
}

async fn serve() -> Result<()> {
    let cfg = Arc::new(Config::from_env());
    let conn = db::open(&cfg.db_path)?;
    let subs = db::load_active(&conn, cfg.fleet.as_deref())?;
    drop(conn);
    let n = subs.len();
    let app = AppState {
        cfg: cfg.clone(),
        pool: Arc::new(Pool::new(subs, cfg.util_cap)),
        clients: Arc::new(Clients::new(&cfg)),
    };

    tokio::spawn(poller::reload_loop(app.clone()));
    if cfg.poll {
        tokio::spawn(poller::poll_loop(app.clone()));
        eprintln!("поллер лимитов: включён");
    }
    if cfg.api_keys.is_empty() {
        eprintln!("⚠️  CLAUDE_API_KEYS не заданы — сервер принимает ТОЛЬКО с 127.0.0.1");
    }

    let listener = tokio::net::TcpListener::bind(&cfg.bind).await?;
    eprintln!("claude-api слушает http://{}  (подписок: {n}, апстрим {}, реестр {})",
        cfg.bind, cfg.upstream, cfg.db_path);
    axum::serve(listener, server::router(app).into_make_service_with_connect_info::<SocketAddr>())
        .await?;
    Ok(())
}
