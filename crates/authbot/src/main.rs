//! authbot — Telegram-бот покупки подписок (Rust, async). Пополняет пул ЭТОГО проекта.
//!
//! Ключевое отличие от старого Python-бота: (1) каждый апдейт обрабатывается в отдельной
//! tokio-задаче — бот не «залипает» на медленной операции (без лагов); (2) всё состояние
//! (пользователи, офферы, машина создания оффера) — в SQLite, переживает рестарт.
//!
//! Фаза 1: ядро + офферы + продавцы. Выплаты (alloy) и выпуск setup-token (PTY) — Фазы 2–3.

mod bot;
mod codex_login;
mod db;
mod iproyal;
mod setup_token;
mod tg;

use anyhow::{anyhow, Result};
use db::Store;
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use tg::Bot;

pub struct Config {
    pub admins_id: HashSet<i64>,
    pub admins_name: HashSet<String>,
    pub claude_bin: String, // путь к claude CLI (для setup-token)
    pub sub_db: String,     // subscriptions.db (SQLite) — fallback, если Postgres-URL не задан
    pub database_url: String,   // CLAUDE_API_DATABASE_URL движка: реестр движка (Postgres) напрямую
    pub fleet: String,      // флот, в который писать купленные подписки
    pub bsc_python: String, // venv-python с web3 (для bsc_pay CLI)
    pub bsc_script: String, // путь к bsc_pay.py
    pub iproyal_key: String, // ключ IPRoyal reseller API (авто-выпуск прокси); пусто = выкл
    pub codex_bin: String,   // пиннованный codex CLI (device-флоу покупки ChatGPT-подписки)
    pub codex_homes_dir: String, // каталог, который сканирует движок: подкаталог = аккаунт в пуле
}

fn env_opt(k: &str) -> Option<String> {
    env::var(k).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn parse_admins(raw: &str) -> (HashSet<i64>, HashSet<String>) {
    let (mut ids, mut names) = (HashSet::new(), HashSet::new());
    for tok in raw.split([',', ';']) {
        let t = tok.trim().trim_start_matches('@');
        if t.is_empty() { continue; }
        if t.trim_start_matches('-').chars().all(|c| c.is_ascii_digit()) {
            if let Ok(id) = t.parse::<i64>() { ids.insert(id); }
        } else {
            names.insert(t.to_lowercase());
        }
    }
    (ids, names)
}

fn state_db() -> String {
    let dir = env_opt("AUTH_BOT_STATE_DIR").unwrap_or_else(|| {
        let home = env::var("HOME").unwrap_or_default();
        format!("{home}/.config/claude-api-authbot/state")
    });
    format!("{dir}/authbot.db")
}

async fn dispatch(bot: Bot, store: Arc<Store>, cfg: Arc<Config>, upd: tg::Update) {
    if let Some(m) = upd.message.or(upd.edited_message) {
        let text = match m.text { Some(t) => t, None => {
            // не-текст: подсказка (кроме админов — им молчим)
            let uid = m.from.as_ref().map(|u| u.id).unwrap_or(0);
            let uname = m.from.as_ref().and_then(|u| u.username.clone()).unwrap_or_default();
            if !bot::is_admin(&cfg, &store, uid, &uname) {
                let _ = bot.send(m.chat.id, "Принимаю только текст — пришли ровно то, что просит бот.").await;
            }
            return;
        }};
        let uid = m.from.as_ref().map(|u| u.id).unwrap_or(m.chat.id);
        let uname = m.from.as_ref().and_then(|u| u.username.clone()).unwrap_or_default();
        bot::on_message(&bot, &store, &cfg, m.chat.id, uid, &uname, &text).await;
    } else if let Some(cb) = upd.callback_query {
        bot::on_callback(&bot, &store, &cfg, cb).await;
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// IP (host) из строки прокси: `scheme://user:pass@ip:port` → `ip`.
fn proxy_ip(p: &str) -> String {
    if p.is_empty() { return String::new(); }
    let after_scheme = p.rsplit("://").next().unwrap_or(p);
    let host_port = after_scheme.rsplit('@').next().unwrap_or(after_scheme);
    host_port.split(':').next().unwrap_or("").to_string()
}

/// "YYYY-MM-DD HH:MM:SS" → unix (UTC). 0 при ошибке. Без chrono (civil-from-date, Хиннант).
fn iso_to_unix(s: &str) -> i64 {
    let n: Vec<i64> = s.trim().split(|c| c == '-' || c == ' ' || c == ':' || c == 'T')
        .filter_map(|p| p.parse().ok()).collect();
    if n.len() < 3 { return 0; }
    let (mut y, mo, d) = (n[0], n[1], n[2]);
    let (h, mi, se) = (n.get(3).copied().unwrap_or(0), n.get(4).copied().unwrap_or(0), n.get(5).copied().unwrap_or(0));
    if mo <= 2 { y -= 1; }
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + h * 3600 + mi * 60 + se
}

/// Fingerprint-free контроль прокси: маппит IP прокси подписки → срок из IPRoyal (список заказов),
/// пишет `proxy_expire` в реестр (сырой SQL — колонку создаёт движок в общей БД), алертит админам о
/// истекающих (<3д) / осиротевших (нет в IPRoyal). Здоровье прокси — из движка (органика), тут НЕ трогаем.
async fn proxy_lifecycle_loop(bot: Bot, cfg: Arc<Config>) {
    if cfg.iproyal_key.is_empty() {
        eprintln!("proxy-lifecycle: AUTH_BOT_IPROYAL_KEY пуст — контроль прокси выключен");
        return;
    }
    let ipr = iproyal::Iproyal::new(&cfg.iproyal_key);
    loop {
        proxy_check_once(&bot, &cfg, &ipr).await;
        tokio::time::sleep(std::time::Duration::from_secs(1800)).await; // 30 мин (IPRoyal API, не Anthropic)
    }
}

const SUB_LIFETIME_DAYS: i64 = 30; // срок жизни подписки = added_ts + 30д (совпадает с движком)

/// Authority реестра движка: Postgres (если задан CLAUDE_API_DATABASE_URL), иначе SQLite-fallback.
pub fn authority_cfg(cfg: &Config) -> registry::authority::AuthorityConfig {
    registry::authority::AuthorityConfig::new(
        cfg.sub_db.clone(),
        (!cfg.database_url.is_empty()).then(|| cfg.database_url.clone()),
    )
}

/// Записать proxy_expire/checked в реестр через authority (Postgres/SQLite — единый путь).
async fn write_proxy_expire(auth: registry::authority::AuthorityConfig,
                            email: String, expire: String, now: i64, ok: bool) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut a) = auth.connect() {
            let _ = a.set_proxy_meta(&email, &expire, now, ok);
        }
    }).await;
}

async fn proxy_check_once(bot: &Bot, cfg: &Config, ipr: &iproyal::Iproyal) {
    let orders = match ipr.list_isp_orders().await {
        Ok(o) => o, Err(e) => { eprintln!("proxy-lifecycle: IPRoyal {e}"); return; }
    };
    let by_ip: std::collections::HashMap<String, (String, i64)> =
        orders.into_iter().map(|(ip, exp, oid)| (ip, (exp, oid))).collect();
    let now = unix_now();
    // подписки (email, proxy_host, added_ts) — из authority движка (Postgres/SQLite) на blocking-потоке.
    // subs_admin отдаёт proxy_host без user:pass — proxy_ip это переваривает.
    let ac = authority_cfg(cfg);
    let subs: Vec<(String, String, i64)> = tokio::task::spawn_blocking(move || {
        ac.connect().ok()
            .and_then(|mut a| a.subs_admin().ok())
            .map(|rows| rows.into_iter().map(|s| (s.email, s.proxy_host, s.added_ts)).collect())
            .unwrap_or_default()
    }).await.unwrap_or_default();

    let mut alerts: Vec<(String, String)> = Vec::new();
    for (email, proxy, added_ts) in subs {
        let ip = proxy_ip(&proxy);
        if ip.is_empty() { continue; }
        let sub_days_left = if added_ts > 0 { (added_ts + SUB_LIFETIME_DAYS * 86400 - now) as f64 / 86400.0 } else { 999.0 };
        match by_ip.get(&ip) {
            Some((exp, oid)) => {
                let t = iso_to_unix(exp);
                let prox_days = if t > 0 { (t - now) as f64 / 86400.0 } else { 999.0 };
                // Прокси истекает <3д, а подписка жить ещё >5д → ПРОДЛИТЬ тот же IP (extend), а не менять.
                if t > 0 && prox_days < 3.0 && sub_days_left > 5.0 && *oid > 0 {
                    match ipr.extend_order(*oid).await {
                        Ok(ne) => {
                            let show = ne.get(..10).unwrap_or(&ne).to_string();
                            alerts.push((email.clone(), format!("✅ авто-продлён (тот же IP) до {show}")));
                            write_proxy_expire(authority_cfg(cfg), email.clone(), ne, now, true).await;
                        }
                        Err(e) => {
                            alerts.push((email.clone(), format!("❌ продление не удалось (баланс IPRoyal?): {e}")));
                            write_proxy_expire(authority_cfg(cfg), email.clone(), exp.clone(), now, true).await;
                        }
                    }
                } else {
                    if t > 0 && prox_days < 3.0 && sub_days_left <= 5.0 {
                        let show = exp.get(..10).unwrap_or(exp).to_string();
                        alerts.push((email.clone(), format!("истекает {show} — подписка тоже на исходе, продление пропущено")));
                    }
                    write_proxy_expire(authority_cfg(cfg), email.clone(), exp.clone(), now, true).await;
                }
            }
            None => {
                alerts.push((email.clone(), format!("прокси {ip} НЕ найден в IPRoyal (осиротел?)")));
                write_proxy_expire(authority_cfg(cfg), email.clone(), String::new(), now, false).await;
            }
        }
    }
    if !alerts.is_empty() {
        let mut msg = String::from("🔌 <b>Контроль прокси</b> (fingerprint-free, тот же IP):\n");
        for (email, why) in &alerts { msg.push_str(&format!("• <code>{email}</code> — {why}\n")); }
        for id in &cfg.admins_id { let _ = bot.send(*id, &msg).await; }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = env_opt("AUTH_BOT_TOKEN").ok_or_else(|| anyhow!("AUTH_BOT_TOKEN не задан"))?;
    let (admins_id, admins_name) = parse_admins(&env_opt("AUTH_BOT_ADMIN").unwrap_or_default());
    let home = env::var("HOME").unwrap_or_default();
    let sub_dir = env_opt("AUTH_BOT_SUB_CFG_DIR")
        .or_else(|| env_opt("SUB_CFG_DIR"))
        .unwrap_or_else(|| "/srv/claude-api/data".into());
    let cfg = Arc::new(Config {
        admins_id, admins_name,
        claude_bin: env_opt("CLAUDE_BIN").unwrap_or_else(|| format!("{home}/.local/bin/claude")),
        sub_db: format!("{sub_dir}/subscriptions.db"),
        database_url: env_opt("CLAUDE_API_DATABASE_URL")
            .or_else(|| env_opt("AUTH_BOT_DATABASE_URL"))
            .unwrap_or_default(),
        fleet: env_opt("AUTH_BOT_FLEET").unwrap_or_else(|| "prod".into()),
        bsc_python: env_opt("AUTH_BOT_BSC_PYTHON")
            .unwrap_or_else(|| "/srv/claude-api/tools/authbot/venv/bin/python".into()),
        bsc_script: env_opt("AUTH_BOT_BSC_SCRIPT")
            .unwrap_or_else(|| "/srv/claude-api/tools/authbot/bsc_pay.py".into()),
        iproyal_key: env_opt("AUTH_BOT_IPROYAL_KEY").unwrap_or_default(),
        codex_bin: env_opt("AUTH_BOT_CODEX_BIN")
            .unwrap_or_else(|| "/srv/claude-api/data/codex/bin/codex".into()),
        codex_homes_dir: env_opt("AUTH_BOT_CODEX_HOMES_DIR")
            .unwrap_or_else(|| "/srv/claude-api/data/codex-homes".into()),
    });
    let store = Arc::new(Store::open(&state_db())?);
    let bot = Bot::new(&token);
    let _ = bot.delete_webhook().await;

    let uname = bot.get_me().await.unwrap_or_else(|_| "?".into());
    let (users, offers) = store.counts();
    let admin_state = if cfg.admins_id.is_empty() && cfg.admins_name.is_empty() { "EMPTY" } else { "set" };
    eprintln!("authbot (Rust) запущен: @{uname} admin={admin_state} users={users} offers={offers} db={}", state_db());
    if admin_state == "EMPTY" {
        eprintln!("⚠️ AUTH_BOT_ADMIN пуст — админ не задан");
    }

    // Fingerprint-free контроль прокси: срок из IPRoyal → реестр (панель показывает «прокси до»),
    // алерты об истекающих/осиротевших. Не трогает Anthropic. Раз в 30 мин.
    tokio::spawn(proxy_lifecycle_loop(bot.clone(), cfg.clone()));
    eprintln!("proxy-lifecycle: контроль прокси запущен (IPRoyal, 30 мин)");

    let mut offset: Option<i64> = None;
    loop {
        match bot.get_updates(offset, 50).await {
            Ok(updates) => {
                for upd in updates {
                    offset = Some(upd.update_id + 1);
                    // конкурентно: медленная операция одного апдейта не блокирует остальные
                    let (b, s, c) = (bot.clone(), store.clone(), cfg.clone());
                    tokio::spawn(async move { dispatch(b, s, c, upd).await });
                }
            }
            Err(e) => {
                eprintln!("getUpdates err: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}
