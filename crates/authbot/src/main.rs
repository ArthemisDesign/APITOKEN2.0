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
mod gemini_oauth;
mod iproyal;
mod setup_token;
mod tg;

use anyhow::{anyhow, Context, Result};
use db::Store;
use std::collections::HashSet;
use std::env;
use std::sync::Arc;
use tg::Bot;

pub struct Config {
    pub admins_id: HashSet<i64>,
    pub admins_name: HashSet<String>,
    pub claude_bin: String,        // путь к claude CLI (для setup-token)
    pub claude_config_dir: String, // writable root for per-account Claude state
    pub database_url: String,      // required PostgreSQL authority shared with the engine
    pub fleet: String,             // флот, в который писать купленные подписки
    pub bsc_python: String,        // venv-python с web3 (для bsc_pay CLI)
    pub bsc_script: String,        // путь к bsc_pay.py
    pub iproyal_key: String,       // ключ IPRoyal reseller API (авто-выпуск прокси); пусто = выкл
    pub codex_bin: String,         // пиннованный codex CLI (device-флоу покупки ChatGPT-подписки)
    pub codex_homes_dir: String,   // каталог, который сканирует движок: подкаталог = аккаунт в пуле
    pub gemini_dir: String, // каталог encrypted credentials + roster отдельного Gemini provider
    pub gemini_oauth: Option<gemini_oauth::Config>,
    // Transient per-chat wizard draft for the step-by-step Gemini onboarding: (client_id,
    // client_secret). RAM only — the client secret is never written to the bot DB or logs; the
    // entry is removed as soon as the OAuth session is sealed (or the flow ends).
    pub gemini_client_drafts: Arc<std::sync::Mutex<std::collections::HashMap<i64, (String, String)>>>,
}

fn env_opt(k: &str) -> Option<String> {
    env::var(k)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_admins(raw: &str) -> (HashSet<i64>, HashSet<String>) {
    let (mut ids, mut names) = (HashSet::new(), HashSet::new());
    for tok in raw.split([',', ';']) {
        let t = tok.trim().trim_start_matches('@');
        if t.is_empty() {
            continue;
        }
        if t.trim_start_matches('-')
            .chars()
            .all(|c| c.is_ascii_digit())
        {
            if let Ok(id) = t.parse::<i64>() {
                ids.insert(id);
            }
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

fn gemini_oauth_config(gemini_dir: &str) -> Result<Option<gemini_oauth::Config>> {
    let client_id = env_opt("AUTH_BOT_GEMINI_CLIENT_ID");
    let client_secret = env_opt("AUTH_BOT_GEMINI_CLIENT_SECRET");
    let keys = env_opt("AUTH_BOT_GEMINI_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID");
    if client_id.is_none() && client_secret.is_none() && keys.is_none() && active.is_none() {
        return Ok(None);
    }
    // Intake is gated on the AEAD keyring (required to seal credentials). The operator OAuth client
    // is now an optional fallback — sellers submit their own — so it defaults to empty when unset.
    let keyring = gemini_credential::CredentialKeyring::parse(
        &keys.ok_or_else(|| anyhow!("AUTH_BOT_GEMINI_CREDENTIAL_KEYS не задан"))?,
    )?;
    let active = active.ok_or_else(|| anyhow!("AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID не задан"))?;
    let bind = env_opt("AUTH_BOT_GEMINI_OAUTH_BIND")
        .unwrap_or_else(|| "127.0.0.1:8796".into())
        .parse()
        .map_err(|_| anyhow!("AUTH_BOT_GEMINI_OAUTH_BIND должен быть ip:port"))?;
    let redirect = env_opt("AUTH_BOT_GEMINI_REDIRECT_URI")
        .unwrap_or_else(|| "https://gemini.api.apitoken.sale/oauth/callback".into());
    let config = gemini_oauth::Config::new(
        client_id.unwrap_or_default(),
        client_secret.unwrap_or_default(),
        redirect,
        bind,
        gemini_dir.to_string(),
        keyring,
        active,
    )?;
    config
        .rewrap_existing()
        .map_err(|_| anyhow!("Gemini credential key rotation failed closed"))?;
    Ok(Some(config))
}

async fn dispatch(bot: Bot, store: Arc<Store>, cfg: Arc<Config>, upd: tg::Update) {
    if let Some(m) = upd.message.or(upd.edited_message) {
        let text = match m.text {
            Some(t) => t,
            None => {
                // не-текст: подсказка (кроме админов — им молчим)
                let uid = m.from.as_ref().map(|u| u.id).unwrap_or(0);
                let uname = m
                    .from
                    .as_ref()
                    .and_then(|u| u.username.clone())
                    .unwrap_or_default();
                if !bot::is_admin(&cfg, &store, uid, &uname) {
                    let _ = bot
                        .send(
                            m.chat.id,
                            "Принимаю только текст — пришли ровно то, что просит бот.",
                        )
                        .await;
                }
                return;
            }
        };
        let uid = m.from.as_ref().map(|u| u.id).unwrap_or(m.chat.id);
        let uname = m
            .from
            .as_ref()
            .and_then(|u| u.username.clone())
            .unwrap_or_default();
        bot::on_message(
            &bot,
            &store,
            &cfg,
            m.chat.id,
            uid,
            &uname,
            m.message_id,
            &text,
        )
        .await;
    } else if let Some(cb) = upd.callback_query {
        bot::on_callback(&bot, &store, &cfg, cb).await;
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// IP (host) из строки прокси: `scheme://user:pass@ip:port` → `ip`.
fn proxy_ip(p: &str) -> String {
    if p.is_empty() {
        return String::new();
    }
    let after_scheme = p.rsplit("://").next().unwrap_or(p);
    let host_port = after_scheme.rsplit('@').next().unwrap_or(after_scheme);
    host_port.split(':').next().unwrap_or("").to_string()
}

/// "YYYY-MM-DD HH:MM:SS" → unix (UTC). 0 при ошибке. Без chrono (civil-from-date, Хиннант).
fn iso_to_unix(s: &str) -> i64 {
    let n: Vec<i64> = s
        .trim()
        .split(|c| c == '-' || c == ' ' || c == ':' || c == 'T')
        .filter_map(|p| p.parse().ok())
        .collect();
    if n.len() < 3 {
        return 0;
    }
    let (mut y, mo, d) = (n[0], n[1], n[2]);
    let (h, mi, se) = (
        n.get(3).copied().unwrap_or(0),
        n.get(4).copied().unwrap_or(0),
        n.get(5).copied().unwrap_or(0),
    );
    if mo <= 2 {
        y -= 1;
    }
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + h * 3600 + mi * 60 + se
}

/// Stable-egress контроль прокси: маппит IP прокси подписки → срок из IPRoyal (список заказов),
/// пишет `proxy_expire` в реестр (сырой SQL — колонку создаёт движок в общей БД), алертит админам о
/// истекающих (<3д) / осиротевших (нет в IPRoyal). Provider traffic эта проверка не создаёт.
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

/// Authbot is a producer for the live engine authority. SQLite is only its private workflow state.
pub fn authority_cfg(cfg: &Config) -> registry::authority::AuthorityConfig {
    registry::authority::AuthorityConfig::Postgres {
        url: cfg.database_url.clone(),
    }
}

async fn preflight_authority(authority: registry::authority::AuthorityConfig) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut authority = authority
            .connect()
            .context("authbot не подключился к PostgreSQL authority движка")?;
        authority
            .subs_admin()
            .context("authbot не прочитал PostgreSQL registry движка")?;
        Ok(())
    })
    .await
    .context("authbot PostgreSQL preflight task failed")?
}

/// Записать proxy_expire/checked в PostgreSQL authority движка.
async fn write_proxy_expire(
    auth: registry::authority::AuthorityConfig,
    email: String,
    expire: String,
    now: i64,
    ok: bool,
) {
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(mut a) = auth.connect() {
            let _ = a.set_proxy_meta(&email, &expire, now, ok);
        }
    })
    .await;
}

async fn proxy_check_once(bot: &Bot, cfg: &Config, ipr: &iproyal::Iproyal) {
    let orders = match ipr.list_isp_orders().await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("proxy-lifecycle: IPRoyal {e}");
            return;
        }
    };
    let by_ip: std::collections::HashMap<String, (String, i64)> = orders
        .iter()
        .cloned()
        .map(|(ip, exp, oid)| (ip, (exp, oid)))
        .collect();
    let by_order: std::collections::HashMap<i64, String> = orders
        .into_iter()
        .filter(|(_, _, order_id)| *order_id > 0)
        .map(|(_, expire, order_id)| (order_id, expire))
        .collect();
    let now = unix_now();
    // подписки (email, proxy_host, added_ts) — из authority движка (Postgres/SQLite) на blocking-потоке.
    // subs_admin отдаёт proxy_host без user:pass — proxy_ip это переваривает.
    let ac = authority_cfg(cfg);
    let subs: Vec<(String, String, i64)> = tokio::task::spawn_blocking(move || {
        ac.connect()
            .ok()
            .and_then(|mut a| a.subs_admin().ok())
            .map(|rows| {
                rows.into_iter()
                    .map(|s| (s.email, s.proxy_host, s.added_ts))
                    .collect()
            })
            .unwrap_or_default()
    })
    .await
    .unwrap_or_default();

    let mut alerts: Vec<(String, String)> = Vec::new();
    for (email, proxy, added_ts) in subs {
        let ip = proxy_ip(&proxy);
        if ip.is_empty() {
            continue;
        }
        let sub_days_left = if added_ts > 0 {
            (added_ts + SUB_LIFETIME_DAYS * 86400 - now) as f64 / 86400.0
        } else {
            999.0
        };
        match by_ip.get(&ip) {
            Some((exp, oid)) => {
                let t = iso_to_unix(exp);
                let prox_days = if t > 0 {
                    (t - now) as f64 / 86400.0
                } else {
                    999.0
                };
                // Прокси истекает <3д, а подписка жить ещё >5д → ПРОДЛИТЬ тот же IP (extend), а не менять.
                if t > 0 && prox_days < 3.0 && sub_days_left > 5.0 && *oid > 0 {
                    match ipr.extend_order(*oid).await {
                        Ok(ne) => {
                            let show = ne.get(..10).unwrap_or(&ne).to_string();
                            alerts.push((
                                email.clone(),
                                format!("✅ авто-продлён (тот же IP) до {show}"),
                            ));
                            write_proxy_expire(authority_cfg(cfg), email.clone(), ne, now, true)
                                .await;
                        }
                        Err(e) => {
                            alerts.push((
                                email.clone(),
                                format!("❌ продление не удалось (баланс IPRoyal?): {e}"),
                            ));
                            write_proxy_expire(
                                authority_cfg(cfg),
                                email.clone(),
                                exp.clone(),
                                now,
                                true,
                            )
                            .await;
                        }
                    }
                } else {
                    if t > 0 && prox_days < 3.0 && sub_days_left <= 5.0 {
                        let show = exp.get(..10).unwrap_or(exp).to_string();
                        alerts.push((
                            email.clone(),
                            format!(
                                "истекает {show} — подписка тоже на исходе, продление пропущено"
                            ),
                        ));
                    }
                    write_proxy_expire(authority_cfg(cfg), email.clone(), exp.clone(), now, true)
                        .await;
                }
            }
            None => {
                alerts.push((
                    email.clone(),
                    format!("прокси {ip} НЕ найден в IPRoyal (осиротел?)"),
                ));
                write_proxy_expire(authority_cfg(cfg), email.clone(), String::new(), now, false)
                    .await;
            }
        }
    }
    if let Some(oauth) = cfg.gemini_oauth.as_ref() {
        match oauth.iproyal_leases() {
            Ok(leases) => {
                for lease in leases {
                    let label = format!("Gemini {}", lease.profile_id);
                    let sub_days_left = if lease.issued_at > 0 {
                        (lease.issued_at + SUB_LIFETIME_DAYS * 86400 - now) as f64 / 86400.0
                    } else {
                        999.0
                    };
                    match by_order.get(&lease.order_id) {
                        Some(expire) => {
                            let expiry_ts = iso_to_unix(expire);
                            let proxy_days_left = if expiry_ts > 0 {
                                (expiry_ts - now) as f64 / 86400.0
                            } else {
                                999.0
                            };
                            if expiry_ts > 0 && proxy_days_left < 3.0 && sub_days_left > 5.0 {
                                match ipr.extend_order(lease.order_id).await {
                                    Ok(new_expiry) => {
                                        let show = new_expiry
                                            .get(..10)
                                            .unwrap_or(&new_expiry)
                                            .to_string();
                                        alerts.push((
                                            label,
                                            format!(
                                                "✅ IPRoyal allocation продлён без смены egress до {show}"
                                            ),
                                        ));
                                    }
                                    Err(_) => alerts.push((
                                        label,
                                        "❌ IPRoyal allocation не продлён; provider response redacted"
                                            .into(),
                                    )),
                                }
                            } else if expiry_ts > 0 && proxy_days_left < 3.0 && sub_days_left <= 5.0
                            {
                                let show = expire.get(..10).unwrap_or(expire);
                                alerts.push((
                                    label,
                                    format!(
                                        "IPRoyal allocation истекает {show}; подписка тоже на исходе"
                                    ),
                                ));
                            }
                        }
                        None => alerts.push((
                            label,
                            "IPRoyal order отсутствует; egress автоматически не заменялся".into(),
                        )),
                    }
                }
            }
            Err(_) => eprintln!("proxy-lifecycle: encrypted Gemini lease scan skipped"),
        }
    }
    if !alerts.is_empty() {
        let mut msg = String::from("🔌 <b>Контроль прокси</b> (стабильный egress, тот же IP):\n");
        for (email, why) in &alerts {
            msg.push_str(&format!("• <code>{email}</code> — {why}\n"));
        }
        for id in &cfg.admins_id {
            let _ = bot.send(*id, &msg).await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = env_opt("AUTH_BOT_TOKEN").ok_or_else(|| anyhow!("AUTH_BOT_TOKEN не задан"))?;
    let (admins_id, admins_name) = parse_admins(&env_opt("AUTH_BOT_ADMIN").unwrap_or_default());
    let home = env::var("HOME").unwrap_or_default();
    let database_url = env_opt("CLAUDE_API_DATABASE_URL")
        .or_else(|| env_opt("AUTH_BOT_DATABASE_URL"))
        .ok_or_else(|| anyhow!("CLAUDE_API_DATABASE_URL обязателен; SQLite registry отключён"))?;
    let gemini_dir =
        env_opt("AUTH_BOT_GEMINI_DIR").unwrap_or_else(|| "/srv/claude-api/data/gemini".into());
    let gemini_oauth = gemini_oauth_config(&gemini_dir)?;
    let cfg = Arc::new(Config {
        admins_id,
        admins_name,
        claude_bin: env_opt("AUTH_BOT_CLAUDE_BIN")
            .or_else(|| env_opt("CLAUDE_BIN"))
            .unwrap_or_else(|| format!("{home}/.local/bin/claude")),
        claude_config_dir: env_opt("AUTH_BOT_CLAUDE_CONFIG_DIR")
            .unwrap_or_else(|| "/srv/claude-api/data/authbot".into()),
        database_url,
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
        gemini_dir,
        gemini_oauth,
        gemini_client_drafts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
    });
    let store = Arc::new(Store::open(&state_db())?);
    let recovered = store.recover_interrupted_handoffs()?;
    preflight_authority(authority_cfg(&cfg)).await?;
    let bot = Bot::new(&token);
    let _ = bot.delete_webhook().await;

    let uname = bot.get_me().await.unwrap_or_else(|_| "?".into());
    let (users, offers) = store.counts();
    let admin_state = if cfg.admins_id.is_empty() && cfg.admins_name.is_empty() {
        "EMPTY"
    } else {
        "set"
    };
    eprintln!(
        "authbot (Rust) запущен: @{uname} admin={admin_state} users={users} offers={offers} db={}",
        state_db()
    );
    if recovered > 0 {
        eprintln!("authbot: восстановлено прерванных Claude handoff: {recovered}");
    }
    if admin_state == "EMPTY" {
        eprintln!("⚠️ AUTH_BOT_ADMIN пуст — админ не задан");
    }

    // Stable-egress контроль прокси: срок из IPRoyal → реестр (панель показывает «прокси до»),
    // алерты об истекающих/осиротевших. Не создаёт provider traffic. Раз в 30 мин.
    tokio::spawn(proxy_lifecycle_loop(bot.clone(), cfg.clone()));
    eprintln!("proxy-lifecycle: контроль прокси запущен (IPRoyal, 30 мин)");

    let mut gemini_callback = if cfg.gemini_oauth.is_some() {
        let (oauth_bot, oauth_store, oauth_cfg) = (bot.clone(), store.clone(), cfg.clone());
        let task =
            tokio::spawn(
                async move { gemini_oauth::serve(oauth_bot, oauth_store, oauth_cfg).await },
            );
        eprintln!("Gemini OAuth callback enabled");
        Some(task)
    } else {
        eprintln!("Gemini OAuth intake disabled: credentials are not configured");
        None
    };

    let mut offset: Option<i64> = None;
    loop {
        let updates = match gemini_callback.as_mut() {
            Some(callback) => {
                tokio::select! {
                    result = callback => {
                        let _ = result;
                        // A configured callback is part of intake readiness. Exit with one generic
                        // error so systemd restarts the complete bot instead of silently accepting
                        // Gemini offers that can no longer finish.
                        return Err(anyhow!("Gemini OAuth callback stopped"));
                    }
                    updates = bot.get_updates(offset, 50) => updates,
                }
            }
            None => bot.get_updates(offset, 50).await,
        };
        match updates {
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

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[tokio::test]
    async fn postgres_preflight_does_not_nest_a_runtime() {
        let authority = registry::authority::AuthorityConfig::Postgres {
            url: "postgresql://nobody:nothing@127.0.0.1:1/missing?connect_timeout=1".into(),
        };
        assert!(preflight_authority(authority).await.is_err());
    }
}
