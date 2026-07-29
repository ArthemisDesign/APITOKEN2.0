//! Google OAuth producer for paid Gemini Code Assist subscriptions.
//!
//! The browser callback is state+PKCE protected. Only the resulting encrypted credential envelope
//! is published; account email, Google subject, refresh token, OAuth client secret and authenticated
//! proxy never enter the roster, bot database, Telegram messages, filenames or logs.

use crate::db::{GeminiOAuthSession, Store};
use crate::tg::Bot;
use crate::Config as BotConfig;
use anyhow::{bail, Context};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use gemini_credential::{
    decode_envelope, encode_envelope, CredentialKeyring, GeminiCredential, SealedCredential,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::SocketAddr;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const CODE_ASSIST_URL: &str = "https://cloudcode-pa.googleapis.com";
const OAUTH_SESSION_SECS: i64 = 600;
const MAX_ONBOARD_POLLS: usize = 24;

const SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";

#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PendingOAuthSecret {
    verifier: String,
    proxy: String,
    proxy_order_id: i64,
    // The seller's own Google OAuth client, sealed so the authorize URL's client and the callback's
    // token exchange stay consistent across the begin→callback boundary. Falls back to the operator
    // client when the seller did not supply one.
    client_id: String,
    client_secret: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IproyalLease {
    pub profile_id: String,
    pub order_id: i64,
    pub issued_at: i64,
}

#[derive(Clone)]
pub struct Config {
    pub client_id: String,
    client_secret: String,
    pub redirect_uri: String,
    pub bind: SocketAddr,
    root: PathBuf,
    keyring: CredentialKeyring,
    active_key_id: String,
    publish_lock: Arc<tokio::sync::Mutex<()>>,
    callback_limit: Arc<tokio::sync::Semaphore>,
}

impl std::fmt::Debug for Config {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GeminiOAuthConfig")
            .field("redirect_uri", &self.redirect_uri)
            .field("bind", &self.bind)
            .field("root", &self.root)
            .field("secrets", &"REDACTED")
            .finish()
    }
}

impl Config {
    pub fn new(
        client_id: String,
        client_secret: String,
        redirect_uri: String,
        bind: SocketAddr,
        root: String,
        keyring: CredentialKeyring,
        active_key_id: String,
    ) -> anyhow::Result<Self> {
        // The operator OAuth client is an optional fallback now that sellers submit their own; empty
        // means "no fallback" and a seller must supply a client. When present, it is bounds-checked.
        if !client_id.is_empty() || !client_secret.is_empty() {
            bounded(&client_id, 8, 1_024, "Gemini OAuth client id")?;
            bounded(&client_secret, 1, 4_096, "Gemini OAuth client secret")?;
        }
        if !keyring.contains(&active_key_id) {
            bail!("AUTH_BOT_GEMINI_CREDENTIAL_ACTIVE_KID is absent from the keyring");
        }
        let redirect = reqwest::Url::parse(&redirect_uri)
            .context("AUTH_BOT_GEMINI_REDIRECT_URI must be an absolute URL")?;
        let secure = redirect.scheme() == "https";
        let loopback = redirect
            .host_str()
            .and_then(|host| host.parse::<std::net::IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if !(secure || redirect.scheme() == "http" && loopback) {
            bail!("Gemini OAuth redirect must use HTTPS or literal HTTP loopback");
        }
        if redirect.query().is_some() || redirect.fragment().is_some() {
            bail!("Gemini OAuth redirect must not contain query or fragment");
        }
        if !redirect.username().is_empty()
            || redirect.password().is_some()
            || redirect.path() != "/oauth/callback"
        {
            bail!("Gemini OAuth redirect must be a credential-free /oauth/callback URL");
        }
        if !bind.ip().is_loopback() || bind.port() == 0 {
            bail!("Gemini OAuth callback must bind a non-zero loopback port");
        }
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            bail!("AUTH_BOT_GEMINI_DIR must be absolute");
        }
        Ok(Self {
            client_id,
            client_secret,
            redirect_uri: redirect.to_string(),
            bind,
            root,
            keyring,
            active_key_id,
            publish_lock: Arc::new(tokio::sync::Mutex::new(())),
            callback_limit: Arc::new(tokio::sync::Semaphore::new(32)),
        })
    }

    /// Re-seal existing profiles under the active key during authbot startup. Operators can add a
    /// new key, restart authbot, verify the roster, and then retire the old key without asking
    /// every account owner to authorize again.
    pub fn rewrap_existing(&self) -> anyhow::Result<()> {
        let roster_path = self.root.join("profiles.json");
        let credentials_dir = self.root.join("credentials");
        // Create the private layout even for an empty pool. The Gemini systemd unit requires this
        // path to exist before it can install a fail-closed read-only bind mount.
        private_dir(&self.root)?;
        private_dir(&credentials_dir)?;
        if !roster_path.exists() {
            return Ok(());
        }
        let roster: ProfilesFile = serde_json::from_slice(&read_private(&roster_path)?)
            .context("parse encrypted Gemini roster for key rotation")?;
        let mut ids = HashSet::new();
        let mut subjects = HashSet::new();
        let mut proxies = HashSet::new();
        let mut proxy_orders = HashSet::new();
        for profile in roster.profiles {
            gemini_credential::validate_profile_id(&profile.id)?;
            if !ids.insert(profile.id.clone()) {
                bail!("duplicate Gemini profile id during key rotation");
            }
            let expected = credentials_dir.join(format!("{}.json", profile.id));
            if Path::new(&profile.credential_file) != expected {
                bail!("Gemini credential path is outside the sealed roster layout");
            }
            let envelope = decode_envelope(&read_private(&expected)?)?;
            let credential = self.keyring.open(&profile.id, &envelope)?;
            if !subjects.insert(credential.subject.clone()) {
                bail!("duplicate Gemini subscription during key rotation");
            }
            let proxy = normalize_proxy_url(&credential.proxy)
                .map_err(|_| anyhow::anyhow!("invalid Gemini proxy during key rotation"))?;
            if !proxies.insert(proxy) {
                bail!("duplicate Gemini proxy during key rotation");
            }
            if credential.proxy_order_id > 0 && !proxy_orders.insert(credential.proxy_order_id) {
                bail!("duplicate Gemini IPRoyal order during key rotation");
            }
            if envelope.key_id != self.active_key_id {
                let rotated = self
                    .keyring
                    .seal(&self.active_key_id, &profile.id, &credential)?;
                atomic_private_replace(&expected, &encode_envelope(&rotated)?)?;
            }
        }
        Ok(())
    }

    /// Expose only opaque lifecycle data needed to retain the exact IPRoyal allocation. Google
    /// subject/email/project and proxy credentials never leave this module.
    pub fn iproyal_leases(&self) -> anyhow::Result<Vec<IproyalLease>> {
        let roster_path = self.root.join("profiles.json");
        if !roster_path.exists() {
            return Ok(Vec::new());
        }
        let credentials_dir = self.root.join("credentials");
        let roster: ProfilesFile = serde_json::from_slice(&read_private(&roster_path)?)
            .context("parse Gemini roster for IPRoyal lifecycle")?;
        let mut ids = HashSet::new();
        let mut order_ids = HashSet::new();
        let mut leases = Vec::new();
        for profile in roster.profiles {
            gemini_credential::validate_profile_id(&profile.id)?;
            if !ids.insert(profile.id.clone()) {
                bail!("duplicate Gemini profile id in IPRoyal lifecycle");
            }
            let expected = credentials_dir.join(format!("{}.json", profile.id));
            if Path::new(&profile.credential_file) != expected {
                bail!("Gemini credential path is outside the lifecycle roster layout");
            }
            let envelope = decode_envelope(&read_private(&expected)?)?;
            let credential = self.keyring.open(&profile.id, &envelope)?;
            if credential.proxy_order_id > 0 {
                if !order_ids.insert(credential.proxy_order_id) {
                    bail!("duplicate Gemini IPRoyal order id");
                }
                leases.push(IproyalLease {
                    profile_id: profile.id,
                    order_id: credential.proxy_order_id,
                    issued_at: credential.issued_at,
                });
            }
        }
        Ok(leases)
    }
}

fn bounded(value: &str, min: usize, max: usize, description: &str) -> anyhow::Result<()> {
    if !(min..=max).contains(&value.len()) || value.chars().any(char::is_control) {
        bail!("invalid {description}");
    }
    Ok(())
}

#[derive(Debug)]
pub enum StartError {
    Random,
    Proxy,
    State,
    Url,
    Client,
}

impl StartError {
    pub fn public_message(&self) -> &'static str {
        match self {
            Self::Proxy => "Прокси не прошёл безопасную проверку. Пришли корректный HTTP(S) URL без path/query/fragment.",
            Self::Random | Self::State => {
                "Не удалось создать защищённую OAuth-сессию. Попробуй ещё раз."
            }
            Self::Url => "Gemini OAuth временно неверно настроен. Администратор уже уведомлён.",
            Self::Client => "OAuth client id/secret не прошли проверку. Пришли значения из своего Google Cloud OAuth-клиента (client id — длинная строка, secret — из раздела Credentials).",
        }
    }
}

/// Create a restart-safe, one-use PKCE transaction. The returned link contains no token.
pub fn begin(
    store: &Store,
    config: &Config,
    chat_id: i64,
    proxy: &str,
    proxy_order_id: i64,
    client_id: Option<String>,
    client_secret: Option<String>,
) -> Result<String, StartError> {
    if proxy_order_id < 0 {
        return Err(StartError::Proxy);
    }
    // The seller's own OAuth client distributes the fleet across many Google clients so the whole
    // pool cannot be revoked at once. When absent, fall back to the operator client.
    let client_id = client_id.unwrap_or_else(|| config.client_id.clone());
    let client_secret = client_secret.unwrap_or_else(|| config.client_secret.clone());
    if !(8..=1_024).contains(&client_id.len())
        || !(1..=4_096).contains(&client_secret.len())
        || client_id.bytes().any(|byte| byte.is_ascii_control())
        || client_secret.bytes().any(|byte| byte.is_ascii_control())
    {
        return Err(StartError::Client);
    }
    let proxy = normalize_proxy_url(proxy)?;
    let mut state_bytes = [0u8; 32];
    let mut verifier_bytes = [0u8; 48];
    getrandom::fill(&mut state_bytes).map_err(|_| StartError::Random)?;
    getrandom::fill(&mut verifier_bytes).map_err(|_| StartError::Random)?;
    let state = URL_SAFE_NO_PAD.encode(state_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let pending = PendingOAuthSecret {
        verifier,
        proxy,
        proxy_order_id,
        client_id: client_id.clone(),
        client_secret,
    };
    let payload = Zeroizing::new(serde_json::to_string(&pending).map_err(|_| StartError::State)?);
    let sealed_payload = config
        .keyring
        .seal_secret(&config.active_key_id, &state, payload.as_str())
        .and_then(|envelope| {
            serde_json::to_string(&envelope).context("encode pending Gemini OAuth payload")
        })
        .map_err(|_| StartError::State)?;
    store
        .start_gemini_oauth(chat_id, &state, &sealed_payload, now() + OAUTH_SESSION_SECS)
        .map_err(|_| StartError::State)?;
    eprintln!(
        "[gemini-oauth] chat={} proxy_order={} started OAuth session (awaiting Google consent callback)",
        chat_id, proxy_order_id
    );
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).map_err(|_| StartError::Url)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &client_id)
        .append_pair("redirect_uri", &config.redirect_uri)
        .append_pair("scope", SCOPES)
        .append_pair("access_type", "offline")
        .append_pair("prompt", "consent")
        .append_pair("include_granted_scopes", "true")
        .append_pair("state", &state)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", &challenge);
    Ok(url.into())
}

fn normalize_proxy_url(proxy: &str) -> Result<String, StartError> {
    if proxy.len() < 8 || proxy.len() > 4_096 || proxy.chars().any(char::is_control) {
        return Err(StartError::Proxy);
    }
    let url = reqwest::Url::parse(proxy).map_err(|_| StartError::Proxy)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(StartError::Proxy);
    }
    reqwest::Proxy::all(url.as_str()).map_err(|_| StartError::Proxy)?;
    Ok(url.to_string())
}

#[derive(Clone)]
struct CallbackState {
    bot: Bot,
    store: Arc<Store>,
    config: Arc<BotConfig>,
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
}

pub async fn serve(bot: Bot, store: Arc<Store>, config: Arc<BotConfig>) -> anyhow::Result<()> {
    let oauth = config
        .gemini_oauth
        .as_ref()
        .context("Gemini OAuth callback started without configuration")?;
    let listener = tokio::net::TcpListener::bind(oauth.bind)
        .await
        .context("bind Gemini OAuth callback")?;
    let app = Router::new()
        .route("/oauth/callback", get(callback))
        .with_state(CallbackState { bot, store, config });
    axum::serve(listener, app)
        .await
        .context("serve Gemini OAuth callback")
}

async fn callback(
    State(state): State<CallbackState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Response {
    let Some(oauth) = state.config.gemini_oauth.as_ref() else {
        return callback_html(StatusCode::SERVICE_UNAVAILABLE, false);
    };
    let Ok(_callback_permit) = oauth.callback_limit.clone().try_acquire_owned() else {
        return callback_html(StatusCode::SERVICE_UNAVAILABLE, false);
    };
    let callback_state = query.state.as_deref().or_else(|| {
        headers
            .get("x-gemini-oauth-state")
            .and_then(|value| value.to_str().ok())
    });
    let Some(callback_state) = callback_state.filter(|value| valid_oauth_value(value, 128)) else {
        return callback_html(StatusCode::BAD_REQUEST, false);
    };
    let session = match state.store.claim_gemini_oauth(callback_state) {
        Ok(Some(session)) => session,
        Ok(None) | Err(_) => return callback_html(StatusCode::BAD_REQUEST, false),
    };
    let callback_error = query
        .error
        .as_deref()
        .or_else(|| {
            headers
                .get("x-gemini-oauth-error")
                .and_then(|value| value.to_str().ok())
        })
        .filter(|value| !value.is_empty());
    if callback_error.is_some() {
        fail_callback(&state, &session, Failure::Authorization).await;
        return callback_html(StatusCode::BAD_REQUEST, false);
    }
    let code = query.code.as_deref().or_else(|| {
        headers
            .get("x-gemini-oauth-code")
            .and_then(|value| value.to_str().ok())
    });
    let Some(code) = code.filter(|value| valid_oauth_value(value, 4_096)) else {
        fail_callback(&state, &session, Failure::Authorization).await;
        return callback_html(StatusCode::BAD_REQUEST, false);
    };
    let payload_envelope: SealedCredential = match serde_json::from_str(&session.sealed_payload) {
        Ok(envelope) => envelope,
        Err(_) => {
            fail_callback(&state, &session, Failure::Storage).await;
            return callback_html(StatusCode::BAD_GATEWAY, false);
        }
    };
    let decrypted_payload = match oauth.keyring.open_secret(&session.state, &payload_envelope) {
        Ok(payload) => payload,
        Err(_) => {
            fail_callback(&state, &session, Failure::Storage).await;
            return callback_html(StatusCode::BAD_GATEWAY, false);
        }
    };
    let pending: PendingOAuthSecret =
        match serde_json::from_str::<PendingOAuthSecret>(decrypted_payload.as_str()) {
            Ok(pending) if valid_oauth_value(&pending.verifier, 256) => pending,
            _ => {
                fail_callback(&state, &session, Failure::Storage).await;
                return callback_html(StatusCode::BAD_GATEWAY, false);
            }
        };
    match complete(
        oauth,
        &session,
        code,
        pending.verifier.as_str(),
        pending.proxy.as_str(),
        pending.proxy_order_id,
        pending.client_id.as_str(),
        pending.client_secret.as_str(),
    )
    .await
    {
        Ok(profile) => {
            let _ = state.store.finish_gemini_oauth(&session.state);
            let _ = state.store.set_want(session.chat_id, "");
            let _ = state.store.set_hproxy(session.chat_id, "");
            let _ = state
                .bot
                .send(
                    session.chat_id,
                    &format!(
                        "✅ <b>Gemini-подписка подключена.</b> План: <b>{}</b>. Профиль <code>{}</code> опубликован в отдельном Gemini-пуле.",
                        plan_label(&profile.plan),
                        profile.id
                    ),
                )
                .await;
            for admin in &state.config.admins_id {
                let _ = state
                    .bot
                    .send(
                        *admin,
                        &format!(
                            "✅ <b>Gemini OAuth получен</b>: <code>{}</code>, план <b>{}</b>, отдельный прокси: {}. Identity и токены не публиковались.",
                            profile.id,
                            plan_label(&profile.plan),
                            if profile.has_proxy { "да" } else { "нет" }
                        ),
                    )
                    .await;
            }
            callback_html(StatusCode::OK, true)
        }
        Err(failure) => {
            fail_callback(&state, &session, failure).await;
            callback_html(StatusCode::BAD_GATEWAY, false)
        }
    }
}

fn callback_html(status: StatusCode, success: bool) -> Response {
    let body = if success {
        "<!doctype html><meta charset=utf-8><title>Gemini connected</title><h1>Gemini подключён</h1><p>Можно закрыть эту вкладку и вернуться в Telegram.</p>"
    } else {
        "<!doctype html><meta charset=utf-8><title>Gemini authorization failed</title><h1>Авторизация не завершена</h1><p>Вернитесь в Telegram и начните подключение заново.</p>"
    };
    let mut response = (status, Html(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store, max-age=0"),
    );
    headers.insert(
        "content-security-policy",
        axum::http::HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    headers.insert(
        "cross-origin-opener-policy",
        axum::http::HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "cross-origin-resource-policy",
        axum::http::HeaderValue::from_static("same-origin"),
    );
    headers.insert(
        "permissions-policy",
        axum::http::HeaderValue::from_static(
            "camera=(), microphone=(), geolocation=(), payment=(), usb=()",
        ),
    );
    headers.insert(
        "referrer-policy",
        axum::http::HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        "x-frame-options",
        axum::http::HeaderValue::from_static("DENY"),
    );
    response
}

async fn fail_callback(state: &CallbackState, session: &GeminiOAuthSession, failure: Failure) {
    let _ = state.store.fail_gemini_oauth(&session.state);
    let _ = state.store.set_want(session.chat_id, "gm_proxy");
    let _ = state
        .bot
        .send(session.chat_id, failure.public_message())
        .await;
    if failure.operator_action_required() {
        for admin in &state.config.admins_id {
            let _ = state.bot.send(*admin, failure.operator_message()).await;
        }
    }
    eprintln!(
        "[gemini-oauth] chat={} callback failed: {}{}",
        session.chat_id,
        failure.code(),
        if failure.operator_action_required() { " (operator notified)" } else { "" }
    );
}

fn valid_oauth_value(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|byte| byte.is_ascii_graphic())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Authorization,
    Proxy,
    Temporary,
    UnsupportedPlan,
    Duplicate,
    DuplicateProxy,
    Storage,
}

impl Failure {
    fn code(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::Proxy => "proxy",
            Self::Temporary => "temporary_upstream",
            Self::UnsupportedPlan => "unsupported_plan",
            Self::Duplicate => "duplicate_account",
            Self::DuplicateProxy => "duplicate_proxy",
            Self::Storage => "storage",
        }
    }

    fn public_message(self) -> &'static str {
        match self {
            Self::Authorization => "❌ Google отклонил или завершил OAuth-сессию. Пришли прокси ещё раз, чтобы получить новую ссылку.",
            Self::Proxy => "❌ Не удалось использовать закреплённый прокси. Проверь его и пришли заново.",
            Self::Temporary => "⚠️ Google временно не завершил проверку. Пришли прокси ещё раз и повтори OAuth.",
            Self::UnsupportedPlan => "❌ Аккаунт авторизован, но совместимая подписка не обнаружена. Поддерживаются Google AI Pro/Ultra, Code Assist Standard/Enterprise и Workspace AI Ultra; Google AI Plus и другие Workspace AI планы не дают этот Code Assist tier.",
            Self::Duplicate => "❌ Эта Google-подписка уже присутствует в пуле.",
            Self::DuplicateProxy => "❌ Этот прокси уже закреплён за другим Gemini-профилем. Для подписки нужен отдельный прокси.",
            Self::Storage => "⚠️ Подписка проверена, но защищённая публикация не завершилась. Токен не попал в roster; администратор уведомлён.",
        }
    }

    fn operator_action_required(self) -> bool {
        matches!(self, Self::Storage)
    }

    fn operator_message(self) -> &'static str {
        "⚠️ Gemini OAuth publication failed closed. Проверь права AUTH_BOT_GEMINI_DIR, profiles.json и совпадение credential keyring; секреты не логировались."
    }
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct UserInfo {
    id: String,
    email: String,
    #[serde(default)]
    verified_email: bool,
}

#[derive(Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Tier {
    id: Option<String>,
    name: Option<String>,
    #[serde(default)]
    is_default: bool,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LoadCodeAssistResponse {
    current_tier: Option<Tier>,
    paid_tier: Option<Tier>,
    #[serde(default)]
    allowed_tiers: Vec<Tier>,
    cloudaicompanion_project: Option<Value>,
}

#[derive(Deserialize)]
struct OperationResponse {
    name: Option<String>,
    #[serde(default)]
    done: bool,
}

struct ResolvedAccount {
    project_id: String,
    tier_id: String,
    tier_name: String,
    plan: String,
}

struct PublishedProfile {
    id: String,
    plan: String,
    has_proxy: bool,
}

async fn complete(
    config: &Config,
    session: &GeminiOAuthSession,
    code: &str,
    verifier: &str,
    proxy: &str,
    proxy_order_id: i64,
    client_id: &str,
    client_secret: &str,
) -> Result<PublishedProfile, Failure> {
    eprintln!(
        "[gemini-oauth] chat={} proxy_order={} finalizing: exchanging Google authorization code",
        session.chat_id, proxy_order_id
    );
    if session.expires_ts < now() {
        eprintln!(
            "[gemini-oauth] chat={} aborted: OAuth session expired before the callback arrived",
            session.chat_id
        );
        return Err(Failure::Authorization);
    }
    let client = match http_client(proxy) {
        Ok(client) => client,
        Err(failure) => {
            eprintln!(
                "[gemini-oauth] chat={} proxy HTTP client build failed: {}",
                session.chat_id,
                failure.code()
            );
            return Err(failure);
        }
    };
    let response = client
        .post(TOKEN_URL)
        .form(&[
            ("grant_type", "authorization_code"),
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("code_verifier", verifier),
            ("redirect_uri", config.redirect_uri.as_str()),
        ])
        .send()
        .await
        .map_err(|error| {
            eprintln!(
                "[gemini-oauth] chat={} token exchange transport error (proxy/network): {}",
                session.chat_id,
                error.is_timeout()
            );
            Failure::Temporary
        })?;
    if !response.status().is_success() {
        eprintln!(
            "[gemini-oauth] chat={} Google rejected the token exchange: HTTP {}",
            session.chat_id,
            response.status().as_u16()
        );
        return Err(if response.status().is_server_error() {
            Failure::Temporary
        } else {
            Failure::Authorization
        });
    }
    let mut token: TokenResponse = response.json().await.map_err(|_| Failure::Temporary)?;
    if !valid_oauth_value(&token.access_token, 16_384)
        || token
            .refresh_token
            .as_deref()
            .is_none_or(|value| !valid_oauth_value(value, 16_384))
        || !(60..=86_400).contains(&token.expires_in)
    {
        return Err(Failure::Authorization);
    }
    let refresh_token = token.refresh_token.take().ok_or(Failure::Authorization)?;
    let user_info_response = client
        .get(USERINFO_URL)
        .bearer_auth(&token.access_token)
        .send()
        .await
        .map_err(|_| Failure::Temporary)?;
    if !user_info_response.status().is_success() {
        eprintln!(
            "[gemini-oauth] chat={} Google userinfo call failed: HTTP {}",
            session.chat_id,
            user_info_response.status().as_u16()
        );
        return Err(Failure::Authorization);
    }
    let mut user: UserInfo = user_info_response
        .json()
        .await
        .map_err(|_| Failure::Temporary)?;
    if !user.verified_email || !valid_identity(&user.id, 512) || !valid_identity(&user.email, 512) {
        eprintln!(
            "[gemini-oauth] chat={} rejected: Google account is unverified or malformed",
            session.chat_id
        );
        return Err(Failure::Authorization);
    }
    let resolved = match resolve_account(&client, &token.access_token).await {
        Ok(resolved) => resolved,
        Err(failure) => {
            eprintln!(
                "[gemini-oauth] chat={} Code Assist tier/project resolution failed: {}",
                session.chat_id,
                failure.code()
            );
            return Err(failure);
        }
    };
    if !supported_paid_plan(&resolved.plan) {
        eprintln!(
            "[gemini-oauth] chat={} rejected: unsupported Google plan {}",
            session.chat_id,
            plan_label(&resolved.plan)
        );
        return Err(Failure::UnsupportedPlan);
    }
    eprintln!(
        "[gemini-oauth] chat={} Google account verified, plan={}, sealing credential",
        session.chat_id,
        plan_label(&resolved.plan)
    );
    let credential = GeminiCredential {
        version: 1,
        access_token: std::mem::take(&mut token.access_token),
        refresh_token,
        expires_at: now().saturating_add(token.expires_in).saturating_sub(60),
        oauth_client_id: client_id.to_string(),
        oauth_client_secret: client_secret.to_string(),
        token_uri: TOKEN_URL.to_string(),
        subject: std::mem::take(&mut user.id),
        email: std::mem::take(&mut user.email),
        project_id: resolved.project_id,
        tier_id: resolved.tier_id,
        tier_name: resolved.tier_name,
        plan: resolved.plan,
        proxy: proxy.to_string(),
        proxy_order_id,
        issued_at: now(),
    };
    let _guard = config.publish_lock.lock().await;
    let root = config.root.clone();
    let ring = config.keyring.clone();
    let active = config.active_key_id.clone();
    let published = tokio::task::spawn_blocking(move || publish(&root, &ring, &active, credential))
        .await
        .map_err(|_| Failure::Storage)?;
    match &published {
        Ok(profile) => eprintln!(
            "[gemini-oauth] chat={} sealed and published profile {} (plan {}) into the Gemini roster",
            session.chat_id,
            profile.id,
            plan_label(&profile.plan)
        ),
        Err(failure) => eprintln!(
            "[gemini-oauth] chat={} sealing/publishing the profile failed: {}",
            session.chat_id,
            failure.code()
        ),
    }
    published
}

fn http_client(proxy: &str) -> Result<reqwest::Client, Failure> {
    let mut builder = reqwest::Client::builder()
        // Ignore ambient proxy variables: OAuth exchange, userinfo and Code Assist eligibility
        // must use exactly the encrypted per-account proxy or fail, never silently change egress.
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(90))
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(60))
        .user_agent(gemini_credential::GEMINI_PROVIDER_USER_AGENT);
    if !proxy.trim().is_empty() {
        builder = builder.proxy(reqwest::Proxy::all(proxy).map_err(|_| Failure::Proxy)?);
    }
    builder.build().map_err(|_| Failure::Proxy)
}

fn valid_identity(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

async fn resolve_account(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<ResolvedAccount, Failure> {
    let mut loaded = load_code_assist(client, access_token).await?;
    if project_from_value(loaded.cloudaicompanion_project.as_ref()).is_none()
        && loaded.current_tier.is_none()
    {
        let tier = loaded
            .allowed_tiers
            .iter()
            .find(|tier| tier.is_default)
            .or_else(|| loaded.allowed_tiers.first())
            .cloned()
            .ok_or(Failure::UnsupportedPlan)?;
        let tier_id = tier.id.as_deref().ok_or(Failure::UnsupportedPlan)?;
        let operation = post_json::<OperationResponse>(
            client,
            access_token,
            &format!("{CODE_ASSIST_URL}/v1internal:onboardUser"),
            &json!({
                "tierId": tier_id,
                "metadata": client_metadata(None),
            }),
        )
        .await?;
        if !operation.done {
            let name = operation.name.ok_or(Failure::Temporary)?;
            if !valid_operation_name(&name) {
                return Err(Failure::Temporary);
            }
            let mut done = false;
            for _ in 0..MAX_ONBOARD_POLLS {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let url = format!("{CODE_ASSIST_URL}/v1internal/{name}");
                let response = client
                    .get(url)
                    .bearer_auth(access_token)
                    .send()
                    .await
                    .map_err(|_| Failure::Temporary)?;
                if !response.status().is_success() {
                    return Err(Failure::Temporary);
                }
                let operation: OperationResponse =
                    response.json().await.map_err(|_| Failure::Temporary)?;
                if operation.done {
                    done = true;
                    break;
                }
            }
            if !done {
                return Err(Failure::Temporary);
            }
        }
        loaded = load_code_assist(client, access_token).await?;
    }
    let project_id = project_from_value(loaded.cloudaicompanion_project.as_ref())
        .ok_or(Failure::UnsupportedPlan)?;
    let paid = loaded.paid_tier.as_ref();
    let tier = paid
        .or(loaded.current_tier.as_ref())
        .ok_or(Failure::UnsupportedPlan)?;
    let tier_id = tier.id.clone().unwrap_or_default();
    let tier_name = tier.name.clone().unwrap_or_default();
    let plan = classify_plan(&tier_id, &tier_name, paid.is_some());
    Ok(ResolvedAccount {
        project_id,
        tier_id,
        tier_name,
        plan,
    })
}

async fn load_code_assist(
    client: &reqwest::Client,
    access_token: &str,
) -> Result<LoadCodeAssistResponse, Failure> {
    post_json(
        client,
        access_token,
        &format!("{CODE_ASSIST_URL}/v1internal:loadCodeAssist"),
        &json!({
            "metadata": client_metadata(None),
            "mode": "FULL_ELIGIBILITY_CHECK"
        }),
    )
    .await
}

async fn post_json<T: for<'de> Deserialize<'de>>(
    client: &reqwest::Client,
    access_token: &str,
    url: &str,
    body: &Value,
) -> Result<T, Failure> {
    let response = client
        .post(url)
        .bearer_auth(access_token)
        .header("client-metadata", client_metadata_header())
        .json(body)
        .send()
        .await
        .map_err(|_| Failure::Temporary)?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 | 403 => Failure::Authorization,
            _ => Failure::Temporary,
        });
    }
    response.json().await.map_err(|_| Failure::Temporary)
}

fn client_metadata(project: Option<&str>) -> Value {
    let mut value = json!({
        "ideType": "IDE_UNSPECIFIED",
        "platform": "PLATFORM_UNSPECIFIED",
        "pluginType": "GEMINI"
    });
    if let Some(project) = project {
        value["duetProject"] = Value::String(project.to_string());
    }
    value
}

fn client_metadata_header() -> &'static str {
    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
}

fn valid_operation_name(name: &str) -> bool {
    name.starts_with("operations/")
        && name.len() <= 512
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'-' | b'_' | b'.'))
}

fn project_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let project = value
        .as_str()
        .or_else(|| value.get("id").and_then(Value::as_str))?
        .trim();
    valid_identity(project, 512).then(|| project.to_string())
}

fn classify_plan(tier_id: &str, tier_name: &str, explicitly_paid: bool) -> String {
    let value = format!("{tier_id} {tier_name}").to_ascii_lowercase();
    if value.contains("workspace") && value.contains("ultra") {
        "workspace_ai_ultra".into()
    } else if value.contains("workspace") && (value.contains("standard") || value.contains("plus"))
    {
        "workspace_ai_unsupported".into()
    } else if value.contains("expanded") {
        "ai_expanded_unsupported".into()
    } else if value.contains("enterprise") {
        "code_assist_enterprise".into()
    } else if value.contains("standard") {
        "code_assist_standard".into()
    } else if value.contains("ultra") {
        "google_ai_ultra".into()
    } else if value.contains("pro") {
        "google_ai_pro".into()
    } else if value.contains("plus") || value.contains("premium") {
        "google_ai_plus_unsupported".into()
    } else if explicitly_paid {
        // A newly introduced paid tier is not proof that the private Code Assist transport and
        // quotas are compatible. Future tiers require an explicit review before publication.
        "unknown_paid_unsupported".into()
    } else {
        "individual_free".into()
    }
}

fn supported_paid_plan(plan: &str) -> bool {
    matches!(
        plan,
        "google_ai_pro"
            | "google_ai_ultra"
            | "code_assist_standard"
            | "code_assist_enterprise"
            | "workspace_ai_ultra"
    )
}

fn plan_label(plan: &str) -> &'static str {
    match plan {
        "google_ai_pro" => "Google AI Pro",
        "google_ai_ultra" => "Google AI Ultra",
        "code_assist_standard" => "Code Assist Standard",
        "code_assist_enterprise" => "Code Assist Enterprise",
        "workspace_ai_ultra" => "Workspace AI Ultra",
        _ => "unsupported",
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ProfileSpec {
    id: String,
    credential_file: String,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ProfilesFile {
    profiles: Vec<ProfileSpec>,
}

fn publish(
    root: &Path,
    keyring: &CredentialKeyring,
    active_key_id: &str,
    credential: GeminiCredential,
) -> Result<PublishedProfile, Failure> {
    let credentials_dir = root.join("credentials");
    private_dir(root).map_err(|_| Failure::Storage)?;
    private_dir(&credentials_dir).map_err(|_| Failure::Storage)?;
    let roster_path = root.join("profiles.json");
    let mut roster = if roster_path.exists() {
        let bytes = read_private(&roster_path).map_err(|_| Failure::Storage)?;
        serde_json::from_slice::<ProfilesFile>(&bytes).map_err(|_| Failure::Storage)?
    } else {
        ProfilesFile::default()
    };
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut proxies = HashSet::new();
    let mut proxy_orders = HashSet::new();
    for profile in &roster.profiles {
        gemini_credential::validate_profile_id(&profile.id).map_err(|_| Failure::Storage)?;
        if !ids.insert(profile.id.clone()) {
            return Err(Failure::Storage);
        }
        let path = Path::new(&profile.credential_file);
        let expected_path = credentials_dir.join(format!("{}.json", profile.id));
        if !path.is_absolute() || path != expected_path {
            return Err(Failure::Storage);
        }
        let envelope = decode_envelope(&read_private(path).map_err(|_| Failure::Storage)?)
            .map_err(|_| Failure::Storage)?;
        let existing = keyring
            .open(&profile.id, &envelope)
            .map_err(|_| Failure::Storage)?;
        if !subjects.insert(existing.subject.clone()) {
            return Err(Failure::Storage);
        }
        let existing_proxy = normalize_proxy_url(&existing.proxy).map_err(|_| Failure::Storage)?;
        if !proxies.insert(existing_proxy) {
            return Err(Failure::Storage);
        }
        if existing.proxy_order_id > 0 && !proxy_orders.insert(existing.proxy_order_id) {
            return Err(Failure::Storage);
        }
    }
    if subjects.contains(&credential.subject) {
        return Err(Failure::Duplicate);
    }
    let candidate_proxy = normalize_proxy_url(&credential.proxy).map_err(|_| Failure::Storage)?;
    if proxies.contains(&candidate_proxy) {
        return Err(Failure::DuplicateProxy);
    }
    if credential.proxy_order_id > 0 && proxy_orders.contains(&credential.proxy_order_id) {
        return Err(Failure::DuplicateProxy);
    }
    let profile_id = (1u32..=999_999)
        .map(|index| format!("gemini_oauth_{index:06}"))
        .find(|candidate| !ids.contains(candidate))
        .ok_or(Failure::Storage)?;
    let credential_path = credentials_dir.join(format!("{profile_id}.json"));
    let envelope: SealedCredential = keyring
        .seal(active_key_id, &profile_id, &credential)
        .map_err(|_| Failure::Storage)?;
    let encoded = encode_envelope(&envelope).map_err(|_| Failure::Storage)?;
    write_new_private(&credential_path, &encoded).map_err(|_| Failure::Storage)?;
    fs::File::open(&credentials_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Failure::Storage)?;
    roster.profiles.push(ProfileSpec {
        id: profile_id.clone(),
        credential_file: credential_path.to_string_lossy().into_owned(),
    });
    let roster_bytes = serde_json::to_vec_pretty(&roster).map_err(|_| Failure::Storage)?;
    if atomic_private_replace(&roster_path, &roster_bytes).is_err() {
        let _ = fs::remove_file(&credential_path);
        let _ = fs::File::open(&credentials_dir).and_then(|directory| directory.sync_all());
        return Err(Failure::Storage);
    }
    Ok(PublishedProfile {
        id: profile_id,
        plan: credential.plan.clone(),
        has_proxy: !credential.proxy.is_empty(),
    })
}

fn private_dir(path: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(path).with_context(|| "create Gemini credential directory")?;
    let metadata = fs::symlink_metadata(path).context("stat Gemini credential directory")?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("Gemini credential directory must be a real directory");
    }
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn read_private(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("stat private Gemini file")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.permissions().mode() & 0o077 != 0
    {
        bail!("Gemini file must be a private regular non-symlink file");
    }
    fs::read(path).context("read private Gemini file")
}

fn write_new_private(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .context("create private Gemini file")?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn atomic_private_replace(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().context("Gemini roster has no parent")?;
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| anyhow::anyhow!("CSPRNG unavailable"))?;
    let staging = parent.join(format!(
        ".profiles.{}.pending",
        URL_SAFE_NO_PAD.encode(random)
    ));
    write_new_private(&staging, bytes)?;
    if let Err(error) = fs::rename(&staging, path) {
        let _ = fs::remove_file(&staging);
        return Err(error).context("publish Gemini roster");
    }
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn credential(subject: &str) -> GeminiCredential {
        GeminiCredential {
            version: 1,
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at: 1_000,
            oauth_client_id: "client.apps.googleusercontent.com".into(),
            oauth_client_secret: "client-secret".into(),
            token_uri: TOKEN_URL.into(),
            subject: subject.into(),
            email: "owner@example.com".into(),
            project_id: "managed-project".into(),
            tier_id: "paid-tier".into(),
            tier_name: "Google AI Pro".into(),
            plan: "google_ai_pro".into(),
            proxy: "http://user:pass@127.0.0.1:8080".into(),
            proxy_order_id: 42,
            issued_at: 100,
        }
    }

    fn fixture() -> (PathBuf, CredentialKeyring) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let root = std::env::temp_dir().join(format!(
            "gemini-oauth-publish-{}-{}",
            std::process::id(),
            URL_SAFE_NO_PAD.encode(random)
        ));
        let ring = CredentialKeyring::parse(&format!("current:{}", "55".repeat(32))).unwrap();
        (root, ring)
    }

    #[test]
    fn operator_client_is_optional_and_seller_client_is_required_without_it() {
        let (root, ring) = fixture();
        let store = Store::open(
            root.join("state")
                .join("authbot.db")
                .to_str()
                .unwrap(),
        )
        .unwrap();
        // Config initializes with no operator client (empty = no fallback).
        let config = Config::new(
            String::new(),
            String::new(),
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let proxy = "http://user:pass@127.0.0.1:8080";
        // A seller-supplied client works.
        assert!(begin(
            &store,
            &config,
            1,
            proxy,
            0,
            Some("seller-1.apps.googleusercontent.com".into()),
            Some("seller-secret".into()),
        )
        .is_ok());
        // No seller client and no operator fallback → rejected before any session is created.
        assert!(matches!(
            begin(&store, &config, 2, proxy, 0, None, None),
            Err(StartError::Client)
        ));
    }

    #[test]
    fn oauth_start_persists_only_a_state_bound_encrypted_payload() {
        let (root, ring) = fixture();
        let state_dir = root.join("state");
        let database = state_dir.join("authbot.db");
        let store = Store::open(database.to_str().unwrap()).unwrap();
        let config = Config::new(
            "client.apps.googleusercontent.com".into(),
            "client-secret-value".into(),
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let proxy = "http://user:pass@127.0.0.1:8080";
        let authorization = begin(
            &store,
            &config,
            42,
            proxy,
            777,
            Some("seller-1234.apps.googleusercontent.com".into()),
            Some("seller-secret-value".into()),
        )
        .unwrap();
        assert!(!authorization.contains("user:pass"));
        assert!(!authorization.contains("client-secret-value"));
        assert!(!authorization.contains("seller-secret-value"));
        let url = reqwest::Url::parse(&authorization).unwrap();
        // The authorize URL must carry the seller's client id, not the operator fallback.
        assert!(url.query_pairs().any(|(name, value)| {
            name == "client_id" && value == "seller-1234.apps.googleusercontent.com"
        }));
        let state = url
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        assert!(url
            .query_pairs()
            .any(|(name, value)| { name == "code_challenge_method" && value == "S256" }));
        let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
        assert!(!session.sealed_payload.contains("user:pass"));
        assert!(!session.sealed_payload.contains("seller-secret-value"));
        let envelope: SealedCredential = serde_json::from_str(&session.sealed_payload).unwrap();
        let decrypted = config
            .keyring
            .open_secret(&session.state, &envelope)
            .unwrap();
        let pending: PendingOAuthSecret = serde_json::from_str(decrypted.as_str()).unwrap();
        assert_eq!(pending.proxy, format!("{proxy}/"));
        assert_eq!(pending.proxy_order_id, 777);
        assert_eq!(pending.client_id, "seller-1234.apps.googleusercontent.com");
        assert_eq!(pending.client_secret, "seller-secret-value");
        assert!(valid_oauth_value(&pending.verifier, 256));
        assert!(!session.sealed_payload.contains(&pending.verifier));
        for path in [
            database.clone(),
            PathBuf::from(format!("{}-wal", database.display())),
        ] {
            if let Ok(bytes) = fs::read(path) {
                assert!(!bytes
                    .windows(proxy.len())
                    .any(|window| window == proxy.as_bytes()));
                assert!(!bytes
                    .windows(pending.verifier.len())
                    .any(|window| window == pending.verifier.as_bytes()));
            }
        }
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn callback_page_is_non_cacheable_and_cannot_load_or_refer() {
        let response = callback_html(StatusCode::OK, true);
        assert_eq!(response.headers()["cache-control"], "no-store, max-age=0");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert_eq!(response.headers()["x-frame-options"], "DENY");
        assert_eq!(
            response.headers()["cross-origin-opener-policy"],
            "same-origin"
        );
        assert_eq!(
            response.headers()["cross-origin-resource-policy"],
            "same-origin"
        );
        assert!(response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'none'"));
    }

    #[test]
    fn pending_proxy_is_restricted_to_a_credential_safe_http_origin() {
        assert!(normalize_proxy_url("http://user:pass@127.0.0.1:8080").is_ok());
        assert!(normalize_proxy_url("https://proxy.example:8443").is_ok());
        for invalid in [
            "socks5://user:pass@127.0.0.1:1080",
            "http://proxy.example/path",
            "http://proxy.example?token=secret",
            "http://proxy.example/#fragment",
            "http://proxy.example/\nheader",
        ] {
            assert!(
                normalize_proxy_url(invalid).is_err(),
                "accepted {invalid:?}"
            );
        }
    }

    #[test]
    fn plan_detection_distinguishes_supported_subscriptions() {
        assert_eq!(classify_plan("", "Google AI Pro", true), "google_ai_pro");
        assert_eq!(
            classify_plan("", "Google AI Ultra", true),
            "google_ai_ultra"
        );
        assert_eq!(
            classify_plan("standard-tier", "Code Assist Standard", false),
            "code_assist_standard"
        );
        assert_eq!(
            classify_plan("free-tier", "Individual", false),
            "individual_free"
        );
        assert_eq!(
            classify_plan("", "Google AI Plus", true),
            "google_ai_plus_unsupported"
        );
        assert_eq!(
            classify_plan("future-paid", "Future Paid", true),
            "unknown_paid_unsupported"
        );
        assert!(!supported_paid_plan("google_ai_plus_unsupported"));
        assert!(!supported_paid_plan("unknown_paid_unsupported"));
    }

    #[test]
    fn publication_encrypts_identity_proxy_and_tokens_and_rejects_duplicates() {
        let (root, ring) = fixture();
        let first = publish(&root, &ring, "current", credential("subject-1")).unwrap();
        let roster = fs::read_to_string(root.join("profiles.json")).unwrap();
        assert!(!roster.contains("owner@example.com"));
        assert!(!roster.contains("user:pass"));
        assert!(!roster.contains("refresh-token"));
        let sealed =
            fs::read_to_string(root.join("credentials").join(format!("{}.json", first.id)))
                .unwrap();
        assert!(!sealed.contains("owner@example.com"));
        assert!(!sealed.contains("refresh-token"));
        assert!(matches!(
            publish(&root, &ring, "current", credential("subject-1")),
            Err(Failure::Duplicate)
        ));
        assert!(matches!(
            publish(&root, &ring, "current", credential("subject-2")),
            Err(Failure::DuplicateProxy)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_rewrap_moves_existing_envelopes_to_the_active_key() {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let root = std::env::temp_dir().join(format!(
            "gemini-oauth-rewrap-{}-{}",
            std::process::id(),
            URL_SAFE_NO_PAD.encode(random)
        ));
        let ring = CredentialKeyring::parse(&format!(
            "current:{},old:{}",
            "77".repeat(32),
            "88".repeat(32)
        ))
        .unwrap();
        let profile = publish(&root, &ring, "old", credential("rotate-subject")).unwrap();
        let config = Config::new(
            "client.apps.googleusercontent.com".into(),
            "client-secret".into(),
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        config.rewrap_existing().unwrap();
        let path = root
            .join("credentials")
            .join(format!("{}.json", profile.id));
        let envelope = decode_envelope(&fs::read(path).unwrap()).unwrap();
        assert_eq!(envelope.key_id, "current");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn startup_prepares_a_private_empty_layout_for_the_runtime_mount() {
        let (root, ring) = fixture();
        let config = Config::new(
            "client.apps.googleusercontent.com".into(),
            "client-secret".into(),
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        config.rewrap_existing().unwrap();
        for directory in [&root, &root.join("credentials")] {
            let metadata = fs::symlink_metadata(directory).unwrap();
            assert!(metadata.is_dir());
            assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
        }
        assert!(!root.join("profiles.json").exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn iproyal_lifecycle_exports_only_opaque_order_metadata() {
        let (root, ring) = fixture();
        let published = publish(&root, &ring, "current", credential("private-subject")).unwrap();
        let config = Config::new(
            "client.apps.googleusercontent.com".into(),
            "client-secret".into(),
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let leases = config.iproyal_leases().unwrap();
        assert_eq!(
            leases,
            vec![IproyalLease {
                profile_id: published.id,
                order_id: 42,
                issued_at: 100,
            }]
        );
        let debug = format!("{leases:?}");
        for private in [
            "private-subject",
            "owner@example.com",
            "user:pass",
            "managed-project",
        ] {
            assert!(!debug.contains(private));
        }
        let _ = fs::remove_dir_all(root);
    }
}
