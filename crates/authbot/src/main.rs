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
mod gemini_transport;
mod glm_key;
mod glm_roster;
mod iproyal;
mod kimi_oauth;
mod kimi_roster;
mod proxy_admin;
mod setup_token;
mod tg;
mod tripo3d_key;
mod tripo3d_roster;

use anyhow::{anyhow, Context, Result};
use db::Store;
use std::collections::HashSet;
use std::env;
use std::fs::OpenOptions;
use std::io::Read as _;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::sync::Arc;
use tg::Bot;
use zeroize::Zeroizing;

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
    pub codex_homes_dir: String,   // staging-каталог device-флоу (скрытые каталоги логина; не пул)
    pub codex_roster: Option<codex_login::RosterConfig>, // encrypted roster движка (seal-публикация)
    pub gemini_dir: String, // каталог encrypted credentials + roster отдельного Gemini provider
    pub gemini_oauth: Option<gemini_oauth::Config>,
    pub kimi_roster: Option<kimi_roster::RosterConfig>,
    pub glm_roster: Option<glm_roster::RosterConfig>,
    pub tripo3d_roster: Option<tripo3d_roster::RosterConfig>,
}

fn env_opt(k: &str) -> Option<String> {
    env::var(k)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn harden_daemon_process() -> Result<()> {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: PR_SET_DUMPABLE changes only this process attribute. Disabling dumpability blocks
        // same-UID ptrace/process_vm_readv and sensitive /proc reads before any daemon secret loads.
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(std::io::Error::last_os_error())
                .context("disable authbot process dumpability");
        }
    }
    Ok(())
}

fn read_proxy_admin_key(path: &Path) -> Result<proxy_admin::ProxyAdminKey> {
    if !path.is_absolute() {
        return Err(anyhow!(
            "AUTH_BOT_PROXY_ADMIN_KEY_FILE должен быть абсолютным путём"
        ));
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .context("open AUTH_BOT_PROXY_ADMIN_KEY_FILE")?;
    if !file
        .metadata()
        .context("stat AUTH_BOT_PROXY_ADMIN_KEY_FILE")?
        .is_file()
    {
        return Err(anyhow!(
            "AUTH_BOT_PROXY_ADMIN_KEY_FILE должен быть обычным файлом"
        ));
    }
    let mut bytes = Zeroizing::new(Vec::with_capacity(66));
    file.take(66)
        .read_to_end(&mut bytes)
        .context("read AUTH_BOT_PROXY_ADMIN_KEY_FILE")?;
    if bytes.len() > 65 {
        return Err(anyhow!(
            "AUTH_BOT_PROXY_ADMIN_KEY_FILE превышает допустимый размер"
        ));
    }
    proxy_admin::ProxyAdminKey::parse(bytes)
}

/// Конфигурация encrypted roster движка для ChatGPT-аккаунтов. Intake гейтится только на AEAD
/// keyring — как и у Gemini: без ключей модуль просто не публикует профили.
fn codex_roster_config() -> Result<Option<codex_login::RosterConfig>> {
    let keys = env_opt("AUTH_BOT_CODEX_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_CODEX_CREDENTIAL_ACTIVE_KID");
    if keys.is_none() && active.is_none() {
        return Ok(None);
    }
    let keyring = codex_credential::CredentialKeyring::parse(
        &keys.ok_or_else(|| anyhow!("AUTH_BOT_CODEX_CREDENTIAL_KEYS не задан"))?,
    )?;
    let active = active.ok_or_else(|| anyhow!("AUTH_BOT_CODEX_CREDENTIAL_ACTIVE_KID не задан"))?;
    let dir = std::path::PathBuf::from(
        env_opt("AUTH_BOT_CODEX_ROSTER_DIR").unwrap_or_else(|| "/srv/claude-api/data/codex".into()),
    );
    if dir.is_relative() {
        return Err(anyhow!(
            "AUTH_BOT_CODEX_ROSTER_DIR должен быть абсолютным путём"
        ));
    }
    Ok(Some(codex_login::RosterConfig {
        dir,
        keyring,
        active_key_id: active,
    }))
}

/// Encrypted roster of the KIMI plane. Gated on the AEAD keyring exactly like Codex and Gemini:
/// with no keys the branch publishes nothing instead of starting half-configured.
fn kimi_roster_config() -> Result<Option<kimi_roster::RosterConfig>> {
    let keys = env_opt("AUTH_BOT_KIMI_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_KIMI_CREDENTIAL_ACTIVE_KID");
    if keys.is_none() && active.is_none() {
        return Ok(None);
    }
    let keyring = kimi_credential::CredentialKeyring::parse(
        &keys.ok_or_else(|| anyhow!("AUTH_BOT_KIMI_CREDENTIAL_KEYS не задан"))?,
    )?;
    let active = active.ok_or_else(|| anyhow!("AUTH_BOT_KIMI_CREDENTIAL_ACTIVE_KID не задан"))?;
    let dir = std::path::PathBuf::from(
        env_opt("AUTH_BOT_KIMI_DIR").unwrap_or_else(|| "/srv/claude-api/data/kimi".into()),
    );
    if dir.is_relative() {
        return Err(anyhow!("AUTH_BOT_KIMI_DIR должен быть абсолютным путём"));
    }
    Ok(Some(kimi_roster::RosterConfig {
        dir,
        keyring,
        active_key_id: active,
    }))
}

/// Encrypted roster of the GLM plane. Gated on the AEAD keyring exactly like KIMI: with no
/// keys the branch publishes nothing instead of starting half-configured.
fn glm_roster_config() -> Result<Option<glm_roster::RosterConfig>> {
    let keys = env_opt("AUTH_BOT_GLM_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_GLM_CREDENTIAL_ACTIVE_KID");
    if keys.is_none() && active.is_none() {
        return Ok(None);
    }
    let keyring = glm_credential::CredentialKeyring::parse(
        &keys.ok_or_else(|| anyhow!("AUTH_BOT_GLM_CREDENTIAL_KEYS не задан"))?,
    )?;
    let active = active.ok_or_else(|| anyhow!("AUTH_BOT_GLM_CREDENTIAL_ACTIVE_KID не задан"))?;
    let dir = std::path::PathBuf::from(
        env_opt("AUTH_BOT_GLM_DIR").unwrap_or_else(|| "/srv/claude-api/data/glm".into()),
    );
    if dir.is_relative() {
        return Err(anyhow!("AUTH_BOT_GLM_DIR должен быть абсолютным путём"));
    }
    Ok(Some(glm_roster::RosterConfig {
        dir,
        keyring,
        active_key_id: active,
    }))
}

/// Encrypted roster of the Tripo3D plane. Gated on the AEAD keyring exactly like GLM: with
/// no keys the branch publishes nothing instead of starting half-configured.
fn tripo3d_roster_config() -> Result<Option<tripo3d_roster::RosterConfig>> {
    let keys = env_opt("AUTH_BOT_TRIPO3D_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_TRIPO3D_CREDENTIAL_ACTIVE_KID");
    if keys.is_none() && active.is_none() {
        return Ok(None);
    }
    let keyring = tripo3d_credential::CredentialKeyring::parse(
        &keys.ok_or_else(|| anyhow!("AUTH_BOT_TRIPO3D_CREDENTIAL_KEYS не задан"))?,
    )?;
    let active =
        active.ok_or_else(|| anyhow!("AUTH_BOT_TRIPO3D_CREDENTIAL_ACTIVE_KID не задан"))?;
    let dir = std::path::PathBuf::from(
        env_opt("AUTH_BOT_TRIPO3D_DIR").unwrap_or_else(|| "/srv/claude-api/data/tripo3d".into()),
    );
    if dir.is_relative() {
        return Err(anyhow!("AUTH_BOT_TRIPO3D_DIR должен быть абсолютным путём"));
    }
    Ok(Some(tripo3d_roster::RosterConfig {
        dir,
        keyring,
        active_key_id: active,
    }))
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
    let keys = env_opt("AUTH_BOT_GEMINI_CREDENTIAL_KEYS");
    let active = env_opt("AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID");
    if keys.is_none() && active.is_none() {
        return Ok(None);
    }
    // Intake is gated only on the AEAD keyring. Authorization uses the public installed-app OAuth
    // identity embedded by Antigravity, so no operator/seller OAuth client is configured.
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
    let config =
        gemini_oauth::Config::new(redirect, bind, gemini_dir.to_string(), keyring, active)?;
    config
        .rewrap_existing()
        .context("Gemini credential key rotation failed closed")?;
    Ok(Some(config))
}

fn run_gemini_proxy_operator(command: &str, profile_id: &str) -> Result<()> {
    let gemini_dir =
        env_opt("AUTH_BOT_GEMINI_DIR").unwrap_or_else(|| "/srv/claude-api/data/gemini".into());
    let config = gemini_oauth_config(&gemini_dir)?
        .ok_or_else(|| anyhow!("Gemini credential keyring is not configured"))?;
    match command {
        "gemini-proxy-stage" => {
            let mut raw = Zeroizing::new(String::new());
            std::io::stdin()
                .lock()
                .take(4_097)
                .read_to_string(&mut raw)
                .context("read Gemini replacement proxy from stdin")?;
            if raw.len() > 4_096 {
                return Err(anyhow!("Gemini replacement proxy input is too long"));
            }
            let proxy = bot::proxy_url(&raw);
            if proxy.is_empty() {
                return Err(anyhow!("Gemini replacement proxy input is invalid"));
            }
            config.stage_proxy_replacement(profile_id, &proxy)?;
            println!("Gemini proxy replacement staged for {profile_id}");
        }
        "gemini-proxy-rollback" => {
            config.rollback_proxy_replacement(profile_id)?;
            println!("Gemini proxy replacement rolled back for {profile_id}");
        }
        "gemini-proxy-commit" => {
            config.commit_proxy_replacement(profile_id)?;
            println!("Gemini proxy replacement committed for {profile_id}");
        }
        _ => return Err(anyhow!("unsupported Auth Bot operator command")),
    }
    Ok(())
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

/// Автоматическое окно допуска Gemini: раз в минуту забираем аккаунты, у которых наступил срок
/// следующей пробы. Само расписание (каждые 5 минут в течение 24 часов) живёт в SQLite, поэтому
/// перезапуск бота его не сбрасывает и не устраивает залп проб на старте.
async fn gemini_verification_sweep_loop(bot: tg::Bot, store: Arc<db::Store>, cfg: Arc<Config>) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        gemini_oauth::sweep_recorded_verifications(&bot, &store, &cfg).await;
    }
}

/// Authbot is a producer for the live engine authority. SQLite is only its private workflow state.
pub fn authority_cfg(cfg: &Config) -> registry::authority::AuthorityConfig {
    registry::authority::AuthorityConfig::Postgres {
        url: cfg.database_url.clone(),
    }
}

async fn preflight_authority(authority: registry::authority::AuthorityConfig) -> Result<()> {
    tokio::task::spawn_blocking(move || -> Result<()> {
        let mut authority = authority
            .connect_with_application_name("claude-authbot")
            .context("authbot не подключился к PostgreSQL authority движка")?;
        authority
            .subs_admin()
            .context("authbot не прочитал PostgreSQL registry движка")?;
        Ok(())
    })
    .await
    .context("authbot PostgreSQL preflight task failed")?
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    if let Some(command) = args.next() {
        let profile_id = args
            .next()
            .ok_or_else(|| anyhow!("Auth Bot operator command requires one opaque profile id"))?;
        if args.next().is_some() {
            return Err(anyhow!(
                "Auth Bot operator command accepts exactly one opaque profile id"
            ));
        }
        return run_gemini_proxy_operator(&command, &profile_id);
    }
    harden_daemon_process()?;
    let token = env_opt("AUTH_BOT_TOKEN").ok_or_else(|| anyhow!("AUTH_BOT_TOKEN не задан"))?;
    let proxy_admin_key_file = env_opt("AUTH_BOT_PROXY_ADMIN_KEY_FILE")
        .ok_or_else(|| anyhow!("AUTH_BOT_PROXY_ADMIN_KEY_FILE обязателен для proxy admin API"))?;
    let proxy_admin_key = read_proxy_admin_key(Path::new(&proxy_admin_key_file))?;
    let runtime_control_key = env_opt("CLAUDE_API_CONTROL_KEY")
        .ok_or_else(|| anyhow!("CLAUDE_API_CONTROL_KEY обязателен для runtime status API"))?;
    let proxy_admin_bind_raw = env_opt("AUTH_BOT_PROXY_ADMIN_BIND");
    let proxy_admin_bind = proxy_admin::parse_bind(proxy_admin_bind_raw.as_deref())?;
    let codex_runtime_url_raw = env_opt("AUTH_BOT_PROXY_ADMIN_CODEX_RUNTIME_URL");
    let codex_runtime_url = proxy_admin::parse_runtime_url(
        codex_runtime_url_raw.as_deref(),
        proxy_admin::DEFAULT_CODEX_RUNTIME_URL,
        "/codex-subs",
    )?;
    let gemini_runtime_url_raw = env_opt("AUTH_BOT_PROXY_ADMIN_GEMINI_RUNTIME_URL");
    let gemini_runtime_url = proxy_admin::parse_runtime_url(
        gemini_runtime_url_raw.as_deref(),
        proxy_admin::DEFAULT_GEMINI_RUNTIME_URL,
        "/gemini-subs",
    )?;
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
            .unwrap_or_else(|| "/srv/claude-api/data/codex-staging".into()),
        codex_roster: codex_roster_config()?,
        gemini_dir,
        gemini_oauth,
        kimi_roster: kimi_roster_config()?,
        glm_roster: glm_roster_config()?,
        tripo3d_roster: tripo3d_roster_config()?,
    });
    let store = Arc::new(Store::open(&state_db())?);
    let recovered = store.recover_interrupted_handoffs()?;
    let recovered_codex = store.recover_interrupted_codex_handoffs()?;
    let recovered_glm = store.recover_interrupted_glm_handoffs()?;
    let recovered_tripo3d = store.recover_interrupted_tripo3d_handoffs()?;
    let recovered_gemini = store.recover_legacy_gemini_handoffs()?;
    let recovered_seller_jobs = store.recover_seller_jobs()?;
    preflight_authority(authority_cfg(&cfg)).await?;
    let bot = Bot::new(&token);
    let _ = bot.delete_webhook().await;

    let uname = bot.get_me().await.unwrap_or_else(|_| "?".into());
    let recovered_gemini_callbacks =
        bot::recover_interrupted_gemini_oauth(&bot, &store, &cfg).await;
    bot::resume_batches(&bot, &store, &cfg).await;
    let (users, offers) = store.counts();
    let admin_state = if cfg.admins_id.is_empty() && cfg.admins_name.is_empty() {
        "EMPTY"
    } else {
        "set"
    };
    elog::info("authbot", format!("authbot (Rust) запущен: @{uname} admin={admin_state} users={users} offers={offers} db={}", state_db()));
    if recovered > 0 {
        elog::info("authbot", format!("authbot: восстановлено прерванных Claude handoff: {recovered}"));
    }
    if recovered_codex > 0 {
        elog::info("authbot", format!("authbot: восстановлено прерванных ChatGPT handoff: {recovered_codex}"));
    }
    if recovered_glm > 0 {
        elog::info("authbot", format!("authbot: восстановлено прерванных GLM handoff: {recovered_glm}"));
    }
    if recovered_tripo3d > 0 {
        elog::info("authbot", format!("authbot: восстановлено прерванных Tripo3D handoff: {recovered_tripo3d}"));
    }
    if recovered_gemini > 0 {
        elog::info("authbot", format!("authbot: восстановлено устаревших Gemini handoff: {recovered_gemini}"));
    }
    if recovered_gemini_callbacks > 0 {
        elog::info("authbot", format!("authbot: перезапущено прерванных Gemini OAuth callback: {recovered_gemini_callbacks}"));
    }
    if recovered_seller_jobs > 0 {
        elog::info("authbot", format!("authbot: восстановлено активных seller jobs: {recovered_seller_jobs}"));
    }
    if admin_state == "EMPTY" {
        elog::warn("authbot", "AUTH_BOT_ADMIN пуст — админ не задан");
    }

    let proxy_admin_iproyal = if cfg.iproyal_key.is_empty() {
        None
    } else {
        Some(Arc::new(iproyal::Iproyal::new(&cfg.iproyal_key)))
    };
    let proxy_admin = proxy_admin::Service::new(
        proxy_admin_bind,
        proxy_admin_key,
        runtime_control_key,
        store.clone(),
        proxy_admin_iproyal,
        authority_cfg(&cfg),
        cfg.fleet.clone(),
        cfg.codex_roster.clone(),
        cfg.gemini_oauth.clone(),
        codex_runtime_url,
        gemini_runtime_url,
    )?;
    let mut proxy_admin_runtime = tokio::spawn(proxy_admin.run());
    elog::info("authbot", "proxy-admin: loopback listener and lifecycle actor enabled");

    let mut gemini_callback = if cfg.gemini_oauth.is_some() {
        let (oauth_bot, oauth_store, oauth_cfg) = (bot.clone(), store.clone(), cfg.clone());
        let task =
            tokio::spawn(
                async move { gemini_oauth::serve(oauth_bot, oauth_store, oauth_cfg).await },
            );
        elog::info("authbot", "Gemini OAuth callback enabled");
        let (sweep_bot, sweep_store, sweep_cfg) = (bot.clone(), store.clone(), cfg.clone());
        tokio::spawn(gemini_verification_sweep_loop(
            sweep_bot,
            sweep_store,
            sweep_cfg,
        ));
        elog::info("authbot", "Gemini acceptance sweep enabled: recorded accounts are retried every 5 min for 24 h");
        Some(task)
    } else {
        elog::info("authbot", "Gemini OAuth intake disabled: credentials are not configured");
        None
    };

    let mut offset: Option<i64> = None;
    loop {
        let updates = match gemini_callback.as_mut() {
            Some(callback) => {
                tokio::select! {
                    result = &mut proxy_admin_runtime => {
                        let _ = result;
                        return Err(anyhow!("Proxy admin runtime stopped"));
                    }
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
            None => {
                tokio::select! {
                    result = &mut proxy_admin_runtime => {
                        let _ = result;
                        return Err(anyhow!("Proxy admin runtime stopped"));
                    }
                    updates = bot.get_updates(offset, 50) => updates,
                }
            }
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
                elog::error("authbot", format!("getUpdates err: {e}"));
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    struct TempDir(PathBuf);

    impl TempDir {
        fn new() -> Self {
            let path = env::temp_dir().join(format!(
                "authbot-proxy-admin-key-{}-{}",
                std::process::id(),
                TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn daemon_hardening_disables_process_dumpability() {
        // SAFETY: PR_GET_DUMPABLE reads only this process attribute.
        let before = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
        assert!(before >= 0);
        harden_daemon_process().unwrap();
        // SAFETY: PR_GET_DUMPABLE reads only this process attribute.
        assert_eq!(unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) }, 0);
        if before != 0 {
            // SAFETY: Restore the test process attribute so parallel tests keep their original state.
            assert_eq!(
                unsafe { libc::prctl(libc::PR_SET_DUMPABLE, before, 0, 0, 0) },
                0
            );
        }
    }

    #[test]
    fn proxy_admin_key_parser_accepts_only_canonical_shapes() {
        assert!(proxy_admin::ProxyAdminKey::parse(Zeroizing::new(b"a".repeat(64))).is_ok());
        assert!(proxy_admin::ProxyAdminKey::parse(Zeroizing::new(
            [b"0123456789abcdef".repeat(4), vec![b'\n']].concat()
        ))
        .is_ok());

        for malformed in [
            b"a".repeat(63),
            b"a".repeat(65),
            b"A".repeat(64),
            [b"a".repeat(63), vec![b'g']].concat(),
            [b"a".repeat(64), vec![b'\r']].concat(),
            [b"a".repeat(64), vec![b' ']].concat(),
            [b"a".repeat(64), vec![b'\n', b'\n']].concat(),
        ] {
            assert!(proxy_admin::ProxyAdminKey::parse(Zeroizing::new(malformed)).is_err());
        }
    }

    #[test]
    fn proxy_admin_key_reader_accepts_lf_and_no_lf() {
        let temp = TempDir::new();
        for (name, bytes) in [
            ("no-lf", b"a".repeat(64)),
            ("lf", [b"0123456789abcdef".repeat(4), vec![b'\n']].concat()),
        ] {
            let path = temp.path().join(name);
            fs::write(&path, bytes).unwrap();
            assert!(read_proxy_admin_key(&path).is_ok());
        }
    }

    #[test]
    fn proxy_admin_key_reader_rejects_bounds_and_invalid_paths() {
        let temp = TempDir::new();
        for (name, bytes) in [
            ("short", b"a".repeat(63)),
            ("canonical-bound-invalid", b"a".repeat(65)),
            ("overflow", b"a".repeat(66)),
            ("malformed", [b"a".repeat(64), vec![b' ']].concat()),
        ] {
            let path = temp.path().join(name);
            fs::write(&path, bytes).unwrap();
            assert!(read_proxy_admin_key(&path).is_err(), "{name}");
        }

        let target = temp.path().join("target");
        fs::write(&target, b"a".repeat(64)).unwrap();
        let link = temp.path().join("link");
        symlink(&target, &link).unwrap();
        assert!(read_proxy_admin_key(&link).is_err());
        assert!(read_proxy_admin_key(temp.path()).is_err());
        assert!(read_proxy_admin_key(&temp.path().join("missing")).is_err());
        assert!(read_proxy_admin_key(Path::new("relative-key")).is_err());
    }

    #[tokio::test]
    async fn postgres_preflight_does_not_nest_a_runtime() {
        let authority = registry::authority::AuthorityConfig::Postgres {
            url: "postgresql://nobody:nothing@127.0.0.1:1/missing?connect_timeout=1".into(),
        };
        assert!(preflight_authority(authority).await.is_err());
    }
}
