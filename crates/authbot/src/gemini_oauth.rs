//! Two-stage Google OAuth producer for paid Antigravity-backed Gemini subscriptions.
//!
//! The legacy Gemini CLI identity first initializes Code Assist; a fresh Antigravity consent for the
//! exact same Google subject and egress then has to pass a real generation probe. Browser
//! authorization and both HTTPS code-entry forms are state+PKCE protected. Only the final encrypted
//! Antigravity credential is published; account email, Google subject, refresh tokens, authenticated
//! proxy and PKCE material never enter the roster, Telegram messages, filenames or logs.

use crate::db::{GeminiOAuthSession, SellerJobRef, Store};
use crate::gemini_transport::{Client as GeminiHttpClient, Method as GeminiHttpMethod};
use crate::tg::Bot;
use crate::Config as BotConfig;
use anyhow::{bail, Context};
use axum::extract::{DefaultBodyLimit, Form, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine as _;
use gemini_credential::{
    decode_envelope, encode_envelope, CredentialKeyring, GeminiCredential, OAuthKind,
    SealedCredential,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::SocketAddr;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = gemini_credential::GEMINI_OFFICIAL_TOKEN_URI;
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const CODE_ASSIST_PROD_URL: &str = "https://cloudcode-pa.googleapis.com";
const CODE_ASSIST_DAILY_URL: &str = "https://daily-cloudcode-pa.googleapis.com";
const CODE_ASSIST_SANDBOX_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
const LEGACY_CLIENT_ID: &str = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID;
const LEGACY_CLIENT_SECRET: &str = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET;
const LEGACY_REDIRECT_URI: &str = "https://codeassist.google.com/authcode";
// Public installed-application OAuth identity embedded by Antigravity. Google installed-app
// secrets are non-confidential application metadata; sealing the exact pair and redirect with the
// PKCE transaction prevents a callback from changing consumer identity mid-flight.
const ANTIGRAVITY_CLIENT_ID: &str = gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_ID;
const ANTIGRAVITY_CLIENT_SECRET: &str = gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_SECRET;
const ANTIGRAVITY_REDIRECT_URI: &str = gemini_credential::ANTIGRAVITY_REDIRECT_URI;
const OAUTH_SESSION_SECS: i64 = 1200;
const MAX_ONBOARD_POLLS: usize = 24;
const GENERATION_PROBE_MODEL: &str = "gemini-2.5-flash-lite";
const TRANSPORT_RECOVERY_DELAYS: [Duration; 4] = [
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(20),
];

const LEGACY_SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile";
const ANTIGRAVITY_SCOPES: &str = "https://www.googleapis.com/auth/cloud-platform https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/userinfo.profile https://www.googleapis.com/auth/cclog https://www.googleapis.com/auth/experimentsandconfigs";

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum OAuthPhase {
    /// Compatibility for Antigravity sessions created before the two-stage rollout.
    #[default]
    DirectAntigravity,
    LegacyBootstrap,
    AntigravityFinal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PostIdentityAction {
    StartAntigravityConsent,
    ResolveAntigravitySubscription,
}

fn post_identity_action(phase: OAuthPhase) -> PostIdentityAction {
    match phase {
        OAuthPhase::LegacyBootstrap => PostIdentityAction::StartAntigravityConsent,
        OAuthPhase::AntigravityFinal | OAuthPhase::DirectAntigravity => {
            PostIdentityAction::ResolveAntigravitySubscription
        }
    }
}

#[derive(Deserialize, Serialize, Zeroize, ZeroizeOnDrop)]
#[serde(deny_unknown_fields)]
struct PendingOAuthSecret {
    verifier: String,
    proxy: String,
    proxy_order_id: i64,
    // Keep client material and the redirect in the state-bound envelope so an in-flight session
    // always exchanges its code with the same OAuth identity and redirect that initiated it. The
    // default preserves callbacks created by the former hosted Web-client flow during deployment.
    client_id: String,
    client_secret: String,
    #[serde(default)]
    redirect_uri: String,
    #[serde(default)]
    #[zeroize(skip)]
    phase: OAuthPhase,
    /// Google subject attested by the legacy phase. Present only in the final Antigravity phase and
    /// sealed under that phase's state-bound AEAD envelope.
    #[serde(default)]
    bootstrap_subject: String,
}

struct PreparedOAuth {
    state: String,
    sealed_payload: String,
    authorize_url: String,
    submit_url: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IproyalLease {
    pub profile_id: String,
    pub order_id: i64,
    pub issued_at: i64,
}

#[derive(Clone)]
pub struct Config {
    // Public HTTPS form where the seller submits the localhost callback URL produced by Google.
    // Despite the legacy env name, this is not sent to Google as the OAuth redirect URI.
    pub redirect_uri: String,
    pub bind: SocketAddr,
    root: PathBuf,
    keyring: CredentialKeyring,
    active_key_id: String,
    publish_lock: Arc<tokio::sync::Mutex<()>>,
    callback_limit: Arc<tokio::sync::Semaphore>,
    inflight: Arc<Mutex<HashMap<i64, InflightCompletion>>>,
}

struct InflightCompletion {
    state: String,
    abort: tokio::task::AbortHandle,
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
        redirect_uri: String,
        bind: SocketAddr,
        root: String,
        keyring: CredentialKeyring,
        active_key_id: String,
    ) -> anyhow::Result<Self> {
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
            redirect_uri: redirect.to_string(),
            bind,
            root,
            keyring,
            active_key_id,
            publish_lock: Arc::new(tokio::sync::Mutex::new(())),
            // A batch is sequential, and serializing completion across sellers prevents authbot
            // from creating its own CONNECT burst against residential proxy gateways.
            callback_limit: Arc::new(tokio::sync::Semaphore::new(1)),
            inflight: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) async fn terminal_guard(&self) -> tokio::sync::OwnedMutexGuard<()> {
        self.publish_lock.clone().lock_owned().await
    }

    fn register_inflight(&self, chat_id: i64, state: String, abort: tokio::task::AbortHandle) {
        let replaced = self
            .inflight
            .lock()
            .unwrap()
            .insert(chat_id, InflightCompletion { state, abort });
        if let Some(replaced) = replaced {
            replaced.abort.abort();
        }
    }

    fn clear_inflight(&self, chat_id: i64, state: &str) {
        let mut inflight = self.inflight.lock().unwrap();
        if inflight
            .get(&chat_id)
            .is_some_and(|completion| completion.state == state)
        {
            inflight.remove(&chat_id);
        }
    }

    pub(crate) fn abort_inflight(&self, chat_id: i64) -> bool {
        let Some(completion) = self.inflight.lock().unwrap().remove(&chat_id) else {
            return false;
        };
        completion.abort.abort();
        true
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

    /// Stage a manual proxy replacement for one opaque profile while retaining an encrypted,
    /// private rollback envelope. Operators must stop Auth Bot around this command so it cannot
    /// race a concurrent OAuth publication; the Gemini runtime may stay online and picks up the
    /// atomic credential replacement on its health loop.
    pub fn stage_proxy_replacement(&self, profile_id: &str, proxy: &str) -> anyhow::Result<()> {
        gemini_credential::validate_profile_id(profile_id)?;
        let replacement_proxy = normalize_proxy_url(proxy)
            .map_err(|_| anyhow::anyhow!("invalid Gemini replacement proxy"))?;
        let roster_path = self.root.join("profiles.json");
        let credentials_dir = self.root.join("credentials");
        private_dir(&self.root)?;
        private_dir(&credentials_dir)?;
        let roster: ProfilesFile = serde_json::from_slice(&read_private(&roster_path)?)
            .context("parse Gemini roster for proxy replacement")?;
        let rollback_path = proxy_rollback_path(&credentials_dir, profile_id);
        match fs::symlink_metadata(&rollback_path) {
            Ok(_) => bail!("Gemini profile already has a pending proxy replacement"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("stat Gemini proxy rollback envelope"),
        }

        let mut ids = HashSet::new();
        let mut subjects = HashSet::new();
        let mut proxies = HashSet::new();
        let mut proxy_orders = HashSet::new();
        let mut target = None;
        for profile in roster.profiles {
            gemini_credential::validate_profile_id(&profile.id)?;
            if !ids.insert(profile.id.clone()) {
                bail!("duplicate Gemini profile id during proxy replacement");
            }
            let expected = credentials_dir.join(format!("{}.json", profile.id));
            if Path::new(&profile.credential_file) != expected {
                bail!("Gemini credential path is outside the proxy replacement roster");
            }
            let bytes = read_private(&expected)?;
            let envelope = decode_envelope(&bytes)?;
            let credential = self.keyring.open(&profile.id, &envelope)?;
            if !subjects.insert(credential.subject.clone()) {
                bail!("duplicate Gemini subscription during proxy replacement");
            }
            let current_proxy = normalize_proxy_url(&credential.proxy)
                .map_err(|_| anyhow::anyhow!("invalid Gemini proxy during replacement"))?;
            if !proxies.insert(current_proxy.clone()) {
                bail!("duplicate Gemini proxy during replacement");
            }
            if credential.proxy_order_id > 0 && !proxy_orders.insert(credential.proxy_order_id) {
                bail!("duplicate Gemini IPRoyal order during proxy replacement");
            }
            if profile.id == profile_id {
                target = Some((expected, bytes, credential, current_proxy));
            }
        }
        let Some((credential_path, previous_envelope, mut credential, current_proxy)) = target
        else {
            bail!("Gemini proxy replacement target is absent from the roster");
        };
        if current_proxy == replacement_proxy {
            bail!("Gemini profile already uses the requested proxy");
        }
        if proxies.contains(&replacement_proxy) {
            bail!("Gemini replacement proxy is already assigned to another profile");
        }

        credential.proxy = replacement_proxy;
        // A manually supplied replacement has no IPRoyal order that Auth Bot can safely extend.
        credential.proxy_order_id = 0;
        let replacement = self
            .keyring
            .seal(&self.active_key_id, profile_id, &credential)?;
        let replacement = encode_envelope(&replacement)?;

        write_new_private(&rollback_path, &previous_envelope)?;
        fs::File::open(&credentials_dir)?.sync_all()?;
        if let Err(error) = atomic_private_replace(&credential_path, &replacement) {
            let cleanup = fs::remove_file(&rollback_path)
                .and_then(|()| fs::File::open(&credentials_dir)?.sync_all());
            if cleanup.is_err() {
                return Err(error).context(
                    "stage Gemini proxy replacement failed; encrypted rollback cleanup also failed",
                );
            }
            return Err(error).context("stage Gemini proxy replacement");
        }
        Ok(())
    }

    /// Restore the exact encrypted credential retained by [`Self::stage_proxy_replacement`].
    pub fn rollback_proxy_replacement(&self, profile_id: &str) -> anyhow::Result<()> {
        let (credential_path, rollback_path) =
            self.proxy_replacement_paths(profile_id, "rollback")?;
        let previous_envelope = read_private(&rollback_path)?;
        let decoded = decode_envelope(&previous_envelope)?;
        self.keyring.open(profile_id, &decoded)?;
        atomic_private_replace(&credential_path, &previous_envelope)?;
        fs::remove_file(&rollback_path)?;
        fs::File::open(
            credential_path
                .parent()
                .context("Gemini credential path has no parent")?,
        )?
        .sync_all()?;
        Ok(())
    }

    /// Remove the encrypted rollback envelope after the replacement passed exact-profile live
    /// validation. The active credential is not rewritten.
    pub fn commit_proxy_replacement(&self, profile_id: &str) -> anyhow::Result<()> {
        let (credential_path, rollback_path) =
            self.proxy_replacement_paths(profile_id, "commit")?;
        let rollback = decode_envelope(&read_private(&rollback_path)?)?;
        self.keyring.open(profile_id, &rollback)?;
        fs::remove_file(&rollback_path)?;
        fs::File::open(
            credential_path
                .parent()
                .context("Gemini credential path has no parent")?,
        )?
        .sync_all()?;
        Ok(())
    }

    fn proxy_replacement_paths(
        &self,
        profile_id: &str,
        operation: &str,
    ) -> anyhow::Result<(PathBuf, PathBuf)> {
        gemini_credential::validate_profile_id(profile_id)?;
        let credentials_dir = self.root.join("credentials");
        let roster: ProfilesFile =
            serde_json::from_slice(&read_private(&self.root.join("profiles.json"))?)
                .with_context(|| format!("parse Gemini roster for proxy {operation}"))?;
        let credential_path = credentials_dir.join(format!("{profile_id}.json"));
        if !roster.profiles.iter().any(|profile| {
            profile.id == profile_id && Path::new(&profile.credential_file) == credential_path
        }) {
            bail!("Gemini proxy replacement target is absent from the roster");
        }
        read_private(&credential_path)?;
        Ok((
            credential_path,
            proxy_rollback_path(&credentials_dir, profile_id),
        ))
    }
}

fn proxy_rollback_path(credentials_dir: &Path, profile_id: &str) -> PathBuf {
    credentials_dir.join(format!(".{profile_id}.proxy-rollback.json"))
}

#[derive(Debug)]
pub enum StartError {
    Random,
    Proxy,
    State,
    Url,
}

impl StartError {
    pub fn public_message(&self) -> &'static str {
        match self {
            Self::Proxy => "Не удалось проверить прокси. Пришли его ещё раз в формате <code>ip:port:user:pass</code> или <code>http://user:pass@ip:port</code>.",
            Self::Random | Self::State => {
                "Не удалось подготовить ссылку авторизации. Нажми «Аккаунт готов — продолжить» ещё раз."
            }
            Self::Url => "Подключение Gemini временно недоступно. Администратор уже уведомлён.",
        }
    }
}

pub struct AuthorizationLinks {
    pub authorize_url: String,
    pub submit_url: String,
    pub job: Option<SellerJobRef>,
}

/// Create the first restart-safe, one-use PKCE transaction using the official legacy Gemini CLI
/// installed-app identity. Its only durable output is an encrypted proof for the second phase; the
/// legacy OAuth material is never published to the runtime roster.
pub fn begin(
    store: &Store,
    config: &Config,
    chat_id: i64,
    proxy: &str,
    proxy_order_id: i64,
) -> Result<AuthorizationLinks, StartError> {
    if proxy_order_id < 0 {
        return Err(StartError::Proxy);
    }
    let proxy = normalize_proxy_url(proxy)?;
    let prepared = prepare_oauth(
        config,
        OAuthPhase::LegacyBootstrap,
        &proxy,
        proxy_order_id,
        "",
    )?;
    let job = store
        .start_gemini_oauth(
            chat_id,
            &prepared.state,
            &prepared.sealed_payload,
            now() + OAUTH_SESSION_SECS,
            proxy_order_id,
        )
        .map_err(|_| StartError::State)?;
    eprintln!(
        "[gemini-oauth] chat={} proxy_order={} started legacy bootstrap phase",
        chat_id, proxy_order_id
    );
    Ok(AuthorizationLinks {
        authorize_url: prepared.authorize_url,
        submit_url: prepared.submit_url,
        job,
    })
}

fn prepare_oauth(
    config: &Config,
    phase: OAuthPhase,
    proxy: &str,
    proxy_order_id: i64,
    bootstrap_subject: &str,
) -> Result<PreparedOAuth, StartError> {
    let (client_id, client_secret, redirect_uri, scopes) = match phase {
        OAuthPhase::LegacyBootstrap => (
            LEGACY_CLIENT_ID,
            LEGACY_CLIENT_SECRET,
            LEGACY_REDIRECT_URI,
            LEGACY_SCOPES,
        ),
        OAuthPhase::AntigravityFinal | OAuthPhase::DirectAntigravity => (
            ANTIGRAVITY_CLIENT_ID,
            ANTIGRAVITY_CLIENT_SECRET,
            ANTIGRAVITY_REDIRECT_URI,
            ANTIGRAVITY_SCOPES,
        ),
    };
    if phase == OAuthPhase::AntigravityFinal && !valid_identity(bootstrap_subject, 512) {
        return Err(StartError::State);
    }
    let mut state_bytes = [0u8; 32];
    let mut verifier_bytes = [0u8; 48];
    getrandom::fill(&mut state_bytes).map_err(|_| StartError::Random)?;
    getrandom::fill(&mut verifier_bytes).map_err(|_| StartError::Random)?;
    let state = URL_SAFE_NO_PAD.encode(state_bytes);
    let verifier = URL_SAFE_NO_PAD.encode(verifier_bytes);
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let pending = PendingOAuthSecret {
        verifier,
        proxy: proxy.to_string(),
        proxy_order_id,
        client_id: client_id.to_string(),
        client_secret: client_secret.to_string(),
        redirect_uri: redirect_uri.to_string(),
        phase,
        bootstrap_subject: bootstrap_subject.to_string(),
    };
    let payload = Zeroizing::new(serde_json::to_string(&pending).map_err(|_| StartError::State)?);
    let sealed_payload = config
        .keyring
        .seal_secret(&config.active_key_id, &state, payload.as_str())
        .and_then(|envelope| {
            serde_json::to_string(&envelope).context("encode pending Gemini OAuth payload")
        })
        .map_err(|_| StartError::State)?;
    let mut url = reqwest::Url::parse(AUTHORIZE_URL).map_err(|_| StartError::Url)?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client_id)
        .append_pair("redirect_uri", redirect_uri)
        .append_pair("scope", scopes)
        .append_pair("access_type", "offline")
        // `select_account` обязателен рядом с `consent`. Один `consent` показывает согласие для
        // УЖЕ залогиненного аккаунта, без экрана выбора: продавец, который делает позиции batch
        // подряд в одном профиле антидетект-браузера, повторно подтверждает предыдущий аккаунт,
        // сам того не видя. Google при этом выдаёт новый refresh-токен и аннулирует прежний —
        // только что опубликованная подписка умирает, а наша проверка на дубликат срабатывает уже
        // после согласия и спасти токен не может.
        .append_pair("prompt", "select_account consent")
        .append_pair("state", &state)
        .append_pair("code_challenge_method", "S256")
        .append_pair("code_challenge", &challenge);
    let mut submit_url = reqwest::Url::parse(&config.redirect_uri).map_err(|_| StartError::Url)?;
    submit_url.query_pairs_mut().append_pair("state", &state);
    Ok(PreparedOAuth {
        state,
        sealed_payload,
        authorize_url: url.into(),
        submit_url: submit_url.into(),
    })
}

fn begin_antigravity_phase(
    store: &Store,
    config: &Config,
    previous: &GeminiOAuthSession,
    proxy: &str,
    proxy_order_id: i64,
    bootstrap_subject: &str,
) -> Result<AuthorizationLinks, StartError> {
    let prepared = prepare_oauth(
        config,
        OAuthPhase::AntigravityFinal,
        proxy,
        proxy_order_id,
        bootstrap_subject,
    )?;
    let job = store
        .advance_gemini_oauth(
            previous,
            &prepared.state,
            &prepared.sealed_payload,
            now() + OAUTH_SESSION_SECS,
            proxy_order_id,
        )
        .map_err(|_| StartError::State)?;
    eprintln!(
        "[gemini-oauth] chat={} proxy_order={} legacy bootstrap passed; started Antigravity final phase",
        previous.chat_id, proxy_order_id
    );
    Ok(AuthorizationLinks {
        authorize_url: prepared.authorize_url,
        submit_url: prepared.submit_url,
        job,
    })
}

/// Parse and canonicalize the seller's proxy without opening a speculative CONNECT. Residential
/// gateways can transiently throttle that probe while the same browser allocation is healthy; the
/// real OAuth transaction owns bounded, classified recovery instead.
pub(crate) fn normalize_proxy_url(proxy: &str) -> Result<String, StartError> {
    let normalized =
        gemini_credential::normalize_proxy_url(proxy).map_err(|_| StartError::Proxy)?;
    reqwest::Proxy::all(&normalized).map_err(|_| StartError::Proxy)?;
    Ok(normalized)
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

#[derive(Deserialize)]
struct CodeSubmission {
    code: String,
    state: String,
}

fn submitted_authorization_code(
    value: &str,
    expected_state: &str,
    allow_raw_code: bool,
) -> Option<String> {
    let value = value.trim();
    if allow_raw_code
        && valid_oauth_value(value, 4_096)
        && !value.contains("://")
        && !value
            .bytes()
            .any(|byte| matches!(byte, b'&' | b'=' | b'?' | b'#'))
    {
        return Some(value.to_string());
    }
    let url = reqwest::Url::parse(value).ok()?;
    if url.scheme() != "http"
        || url.host_str() != Some("localhost")
        || url.port_or_known_default() != Some(51_121)
        || url.path() != "/oauth-callback"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return None;
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (name, value) in url.query_pairs() {
        match name.as_ref() {
            "code" if code.is_none() => code = Some(value.into_owned()),
            "code" => return None,
            "state" if state.is_none() => state = Some(value.into_owned()),
            "state" => return None,
            "error" if error.is_none() => error = Some(value.into_owned()),
            "error" => return None,
            _ => {}
        }
    }
    if error.is_some() || state.as_deref() != Some(expected_state) {
        return None;
    }
    code.filter(|code| valid_oauth_value(code, 4_096))
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
        // New Antigravity sessions use GET to render a no-store form and POST to submit the
        // localhost callback URL. GET with code/error remains for short-lived compatibility with hosted
        // callbacks that were already in flight when this version deployed.
        .route("/oauth/callback", get(callback).post(submit_code))
        .layer(DefaultBodyLimit::max(16 * 1024))
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
    // Caddy always creates the three redacted transport headers before erasing the public query.
    // Missing query fields therefore arrive as present-but-empty headers; normalize those to None
    // so opening the code-entry form is not mistaken for an empty-code completion attempt.
    let callback_state = first_nonempty(
        query.state.as_deref(),
        headers
            .get("x-gemini-oauth-state")
            .and_then(|value| value.to_str().ok()),
    );
    let callback_error = first_nonempty(
        query.error.as_deref(),
        headers
            .get("x-gemini-oauth-error")
            .and_then(|value| value.to_str().ok()),
    );
    let code = first_nonempty(
        query.code.as_deref(),
        headers
            .get("x-gemini-oauth-code")
            .and_then(|value| value.to_str().ok()),
    );
    if code.is_none() && callback_error.is_none() {
        return match callback_state.filter(|value| valid_oauth_state(value)) {
            Some(callback_state) => {
                let phase = state
                    .config
                    .gemini_oauth
                    .as_ref()
                    .and_then(|oauth| pending_phase(&state.store, oauth, callback_state))
                    .unwrap_or_default();
                code_form(callback_state, phase)
            }
            None => status_page(StatusPage::InvalidLink),
        };
    }
    finish_oauth(&state, callback_state, code, callback_error).await
}

fn first_nonempty<'a>(primary: Option<&'a str>, fallback: Option<&'a str>) -> Option<&'a str> {
    primary
        .filter(|value| !value.is_empty())
        .or_else(|| fallback.filter(|value| !value.is_empty()))
}

async fn submit_code(
    State(state): State<CallbackState>,
    Form(submission): Form<CodeSubmission>,
) -> Response {
    let allow_raw_code = state
        .config
        .gemini_oauth
        .as_ref()
        .and_then(|oauth| pending_phase(&state.store, oauth, &submission.state))
        .is_some_and(|phase| {
            matches!(
                phase,
                OAuthPhase::LegacyBootstrap | OAuthPhase::DirectAntigravity
            )
        });
    let code = submitted_authorization_code(&submission.code, &submission.state, allow_raw_code);
    finish_oauth(
        &state,
        Some(submission.state.as_str()),
        code.as_deref(),
        None,
    )
    .await
}

async fn finish_oauth(
    state: &CallbackState,
    callback_state: Option<&str>,
    code: Option<&str>,
    callback_error: Option<&str>,
) -> Response {
    let Some(callback_state) = callback_state.filter(|value| valid_oauth_state(value)) else {
        return status_page(StatusPage::InvalidLink);
    };
    let Some(oauth) = state.config.gemini_oauth.as_ref() else {
        return status_page(StatusPage::ServiceUnavailable);
    };
    let session = match state.store.claim_gemini_oauth(callback_state) {
        Ok(Some(session)) => session,
        Ok(None) | Err(_) => return status_page(StatusPage::ExpiredLink),
    };
    if !oauth_session_handoff_is_current(&state.store, &session) {
        let _ = state.store.fail_gemini_oauth(&session.state);
        eprintln!(
            "[gemini-oauth] chat={} rejected stale seller generation before code exchange",
            session.chat_id
        );
        return status_page(StatusPage::ExpiredLink);
    }
    if callback_error.is_some() {
        spawn_callback_failure(
            state.clone(),
            oauth.clone(),
            session,
            Failure::Authorization,
        );
        return status_page(StatusPage::AuthorizationDenied);
    }
    let Some(code) = code.filter(|value| valid_oauth_value(value, 4_096)) else {
        spawn_callback_failure(
            state.clone(),
            oauth.clone(),
            session,
            Failure::Authorization,
        );
        return status_page(StatusPage::InvalidCallback);
    };
    let payload_envelope: SealedCredential = match serde_json::from_str(&session.sealed_payload) {
        Ok(envelope) => envelope,
        Err(_) => {
            spawn_callback_failure(state.clone(), oauth.clone(), session, Failure::Storage);
            return status_page(StatusPage::ServiceUnavailable);
        }
    };
    let decrypted_payload = match oauth.keyring.open_secret(&session.state, &payload_envelope) {
        Ok(payload) => payload,
        Err(_) => {
            spawn_callback_failure(state.clone(), oauth.clone(), session, Failure::Storage);
            return status_page(StatusPage::ServiceUnavailable);
        }
    };
    let pending: PendingOAuthSecret =
        match serde_json::from_str::<PendingOAuthSecret>(decrypted_payload.as_str()) {
            Ok(pending)
                if valid_oauth_value(&pending.verifier, 256) && valid_pending_phase(&pending) =>
            {
                pending
            }
            _ => {
                spawn_callback_failure(state.clone(), oauth.clone(), session, Failure::Storage);
                return status_page(StatusPage::ServiceUnavailable);
            }
        };
    let phase = pending.phase;
    spawn_oauth_completion(
        state.clone(),
        oauth.clone(),
        session,
        Zeroizing::new(code.to_string()),
        pending,
    );
    status_page(match phase {
        OAuthPhase::LegacyBootstrap => StatusPage::CheckingIdentity,
        OAuthPhase::AntigravityFinal | OAuthPhase::DirectAntigravity => {
            StatusPage::CheckingSubscription
        }
    })
}

fn spawn_oauth_completion(
    state: CallbackState,
    oauth: Config,
    session: GeminiOAuthSession,
    code: Zeroizing<String>,
    pending: PendingOAuthSecret,
) {
    spawn_supervised_oauth(
        state,
        oauth,
        session,
        move |state, oauth, session| async move {
            process_oauth_completion(&state, &oauth, &session, code, pending).await;
        },
    );
}

fn spawn_callback_failure(
    state: CallbackState,
    oauth: Config,
    session: GeminiOAuthSession,
    failure: Failure,
) {
    spawn_supervised_oauth(
        state,
        oauth,
        session,
        move |state, _oauth, session| async move {
            fail_callback(&state, &session, failure).await;
        },
    );
}

/// Register a claimed callback before the HTTP handler returns, then run all terminal work in a
/// supervised task whose lifetime is independent of the browser connection. The start barrier
/// closes the small race where a very fast task could finish before its abort handle was indexed.
fn spawn_supervised_oauth<F, Fut>(
    state: CallbackState,
    oauth: Config,
    session: GeminiOAuthSession,
    completion: F,
) where
    F: FnOnce(CallbackState, Config, GeminiOAuthSession) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let chat_id = session.chat_id;
    let oauth_state = session.state.clone();
    let recovery_state = state.clone();
    let supervisor_oauth = oauth.clone();
    let (start_tx, start_rx) = tokio::sync::oneshot::channel();
    let worker = tokio::spawn(async move {
        if start_rx.await.is_err() {
            return;
        }
        completion(state, oauth, session).await;
    });
    let abort = worker.abort_handle();
    supervisor_oauth.register_inflight(chat_id, oauth_state.clone(), abort);
    tokio::spawn(async move {
        let outcome = worker.await;
        supervisor_oauth.clear_inflight(chat_id, &oauth_state);
        if outcome.is_err_and(|error| error.is_panic()) {
            eprintln!(
                "[gemini-oauth] chat={} detached completion panicked; restarting the exact handoff generation",
                chat_id
            );
            crate::bot::restart_interrupted_gemini_oauth(
                &recovery_state.bot,
                &recovery_state.store,
                &recovery_state.config,
                chat_id,
            )
            .await;
        }
    });
    let _ = start_tx.send(());
}

async fn process_oauth_completion(
    state: &CallbackState,
    oauth: &Config,
    session: &GeminiOAuthSession,
    code: Zeroizing<String>,
    pending: PendingOAuthSecret,
) {
    let Ok(_callback_permit) = oauth.callback_limit.clone().acquire_owned().await else {
        fail_callback(state, session, Failure::Interrupted).await;
        return;
    };
    let exchange_redirect = if pending.redirect_uri.is_empty() {
        oauth.redirect_uri.as_str()
    } else {
        pending.redirect_uri.as_str()
    };
    match complete(
        &state.store,
        oauth,
        &session,
        code.as_str(),
        pending.verifier.as_str(),
        pending.proxy.as_str(),
        pending.proxy_order_id,
        pending.client_id.as_str(),
        pending.client_secret.as_str(),
        exchange_redirect,
        pending.phase,
        pending.bootstrap_subject.as_str(),
    )
    .await
    {
        Ok(Completion::LegacyBootstrap { subject }) => {
            match begin_antigravity_phase(
                &state.store,
                oauth,
                &session,
                pending.proxy.as_str(),
                pending.proxy_order_id,
                subject.as_str(),
            ) {
                Ok(links)
                    if (links.job.is_none() && session.job.is_none())
                        || crate::bot::seller_handoff_is_current(
                            &state.store,
                            session.chat_id,
                            links.job.as_ref(),
                            crate::bot::HandoffKind::Gemini,
                        ) =>
                {
                    let _ = state
                        .bot
                        .send_url_button(
                            session.chat_id,
                            "✅ <b>Gemini CLI инициализировал подписку.</b> Теперь, не меняя Google-аккаунт, антидетект-профиль и прокси, выдай финальный доступ Antigravity по официальной ссылке.",
                            "Авторизовать через Antigravity",
                            &links.authorize_url,
                        )
                        .await;
                    let _ = state
                        .bot
                        .send_url_button(
                            session.chat_id,
                            "После согласия Google перенаправит на <code>localhost:51121</code>. Скопируй весь URL из адресной строки и вставь его в защищённую форму. Подписка и выплата завершатся только после реальной тестовой генерации.",
                            "Завершить Antigravity-подключение",
                            &links.submit_url,
                        )
                        .await;
                }
                Ok(_) => {
                    eprintln!(
                        "[gemini-oauth] chat={} Antigravity phase became stale immediately after transition",
                        session.chat_id
                    );
                }
                Err(_) => {
                    fail_callback(state, &session, Failure::Storage).await;
                }
            }
        }
        Ok(Completion::Published(profile, _terminal_guard)) => {
            let _ = state.store.finish_gemini_oauth(&session.state);
            let current = crate::bot::seller_handoff_is_current(
                &state.store,
                session.chat_id,
                session.job.as_ref(),
                crate::bot::HandoffKind::Gemini,
            );
            if current {
                let seller_outcome = if profile.reauthorized {
                    "переподключена"
                } else if profile.migrated {
                    "переведена на Antigravity"
                } else {
                    "подключена"
                };
                let _ = state
                    .bot
                    .send(
                        session.chat_id,
                        &format!(
                            "✅ <b>Gemini-подписка {seller_outcome}.</b> План: <b>{}</b>. Профиль <code>{}</code> опубликован в отдельном Gemini-пуле.",
                            plan_label(&profile.plan),
                            profile.id
                        ),
                    )
                    .await;
                for admin in &state.config.admins_id {
                    let admin_outcome = if profile.reauthorized {
                        "переавторизован; прежний токен был аннулирован Google, конверт заменён атомарно"
                    } else if profile.migrated {
                        "переведён на Antigravity; профиль обновлён атомарно"
                    } else {
                        "получен; аккаунт добавлен в пул"
                    };
                    let _ = state
                        .bot
                        .send(
                            *admin,
                            &format!(
                                "✅ <b>Gemini-доступ {admin_outcome}</b>: аккаунт <code>{}</code>, план <b>{}</b>, отдельный прокси: {}.",
                                profile.id,
                                plan_label(&profile.plan),
                                if profile.has_proxy { "да" } else { "нет" }
                            ),
                        )
                        .await;
                }
            }
            crate::bot::complete_seller_job_after_handoff(
                &state.bot,
                &state.store,
                &state.config,
                session.chat_id,
                session.job.clone(),
                crate::bot::HandoffKind::Gemini,
            )
            .await;
        }
        Err(failure) => {
            fail_callback(state, &session, failure).await;
        }
    }
}

fn oauth_session_handoff_is_current(store: &Store, session: &GeminiOAuthSession) -> bool {
    session.job.is_none()
        || crate::bot::seller_handoff_is_current(
            store,
            session.chat_id,
            session.job.as_ref(),
            crate::bot::HandoffKind::Gemini,
        )
}

#[derive(Clone, Copy)]
enum StatusPage {
    InvalidLink,
    InvalidCallback,
    ExpiredLink,
    AuthorizationDenied,
    ServiceUnavailable,
    CheckingIdentity,
    CheckingSubscription,
}

const PAGE_STYLES: &str = r#"
:root{color-scheme:light;--ink:#11182d;--muted:#66708a;--paper:#f7f8fc;--card:#fff;--line:#dfe4f1;--blue:#3267e3;--violet:#7654d8;--mint:#15a37f;--amber:#d47c11;--red:#c94b57;--shadow:0 24px 70px rgba(38,48,91,.15)}*{box-sizing:border-box}html{min-height:100%;background:radial-gradient(circle at 12% 8%,rgba(79,126,235,.14),transparent 34rem),radial-gradient(circle at 88% 92%,rgba(125,84,216,.12),transparent 32rem),var(--paper)}body{min-height:100vh;margin:0;padding:clamp(20px,5vw,64px) 18px;display:grid;place-items:center;color:var(--ink);font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif}.shell{width:min(100%,620px)}.brand{display:flex;align-items:center;gap:10px;margin:0 0 18px 6px;font-size:13px;font-weight:720;letter-spacing:.08em;text-transform:uppercase;color:#4d5875}.brand-mark{width:24px;height:24px;display:grid;place-items:center;border-radius:8px;color:#fff;background:linear-gradient(135deg,var(--blue),var(--violet));font:800 13px ui-rounded,"SF Pro Rounded",sans-serif;box-shadow:0 7px 18px rgba(71,94,190,.28)}.card{position:relative;overflow:hidden;padding:clamp(26px,6vw,48px);border:1px solid rgba(207,214,232,.88);border-radius:28px;background:rgba(255,255,255,.94);box-shadow:var(--shadow)}.card:before{content:"";position:absolute;inset:0 0 auto;height:4px;background:linear-gradient(90deg,var(--blue),#5f78ef 43%,var(--violet))}.signal{position:relative;width:70px;height:70px;margin-bottom:26px;display:grid;place-items:center}.signal:before,.signal:after{content:"";position:absolute;border-radius:50%}.signal:before{inset:0;border:1px solid #ced7ed;background:linear-gradient(145deg,#fff,#eef2fb)}.signal:after{width:13px;height:13px;top:4px;right:8px;background:var(--tone,var(--blue));box-shadow:0 0 0 6px color-mix(in srgb,var(--tone,var(--blue)) 14%,transparent);animation:orbit 2.8s ease-in-out infinite}.signal-core{position:relative;width:40px;height:40px;border-radius:14px;display:grid;place-items:center;background:color-mix(in srgb,var(--tone,var(--blue)) 11%,white);color:var(--tone,var(--blue));font:800 18px ui-rounded,"SF Pro Rounded",sans-serif}.card[data-tone=good]{--tone:var(--mint)}.card[data-tone=wait]{--tone:var(--blue)}.card[data-tone=warn]{--tone:var(--amber)}.card[data-tone=bad]{--tone:var(--red)}.kicker{margin:0 0 9px;color:var(--tone,var(--blue));font:750 12px ui-monospace,"SFMono-Regular",monospace;letter-spacing:.12em;text-transform:uppercase}h1{max-width:14ch;margin:0;font:760 clamp(31px,7vw,48px)/1.02 ui-rounded,"SF Pro Rounded",-apple-system,sans-serif;letter-spacing:-.045em}p{font-size:17px;line-height:1.58}.lead{margin:20px 0 0;color:#46516c}.rail{margin:30px 0 0;display:grid;grid-template-columns:auto 1fr auto 1fr auto;align-items:center;gap:9px}.step{display:grid;place-items:center;min-width:38px;height:32px;padding:0 10px;border:1px solid var(--line);border-radius:999px;background:#f8f9fd;color:#79829a;font:720 11px ui-monospace,"SFMono-Regular",monospace}.step.done{border-color:#b9dfd4;background:#eef9f5;color:#08795d}.step.active{border-color:color-mix(in srgb,var(--tone,var(--blue)) 38%,white);background:color-mix(in srgb,var(--tone,var(--blue)) 9%,white);color:var(--tone,var(--blue))}.track{height:1px;background:var(--line)}.callout{margin-top:26px;padding:16px 18px;border:1px solid #e3e7f2;border-radius:16px;background:#f8f9fc;color:#505b75;font-size:14px;line-height:1.5}.privacy{margin:16px 8px 0;color:#7a8399;font-size:12px;line-height:1.5}.field{display:block;margin-top:28px;color:#303a54;font-size:14px;font-weight:700}.field input{width:100%;margin-top:9px;padding:15px 16px;border:1px solid #cfd6e7;border-radius:14px;background:#fbfcff;color:#11182d;font:500 16px/1.35 ui-monospace,"SFMono-Regular",monospace;outline:none;box-shadow:inset 0 1px 1px rgba(23,31,62,.04)}.field input:focus{border-color:var(--blue);box-shadow:0 0 0 4px rgba(50,103,227,.13)}button{width:100%;margin-top:14px;padding:15px 18px;border:0;border-radius:14px;background:linear-gradient(135deg,var(--blue),#6556db);color:#fff;font:760 16px -apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;cursor:pointer;box-shadow:0 12px 28px rgba(67,84,192,.25)}button:focus-visible{outline:3px solid rgba(50,103,227,.32);outline-offset:3px}button:active{transform:translateY(1px)}.form-note{margin:13px 2px 0;color:#737d94;font-size:13px;line-height:1.45}@keyframes orbit{0%,100%{transform:translate(0,0);opacity:1}50%{transform:translate(-7px,5px);opacity:.66}}@media(max-width:420px){body{padding:18px 12px}.card{border-radius:22px}.rail{gap:5px}.step{min-width:32px;padding:0 7px;font-size:10px}}@media(prefers-reduced-motion:reduce){.signal:after{animation:none}button{transition:none}}
"#;

fn status_page(page: StatusPage) -> Response {
    let (status, tone, mark, kicker, title, lead, callout, active_step) = match page {
        StatusPage::InvalidLink => (
            StatusCode::BAD_REQUEST,
            "bad",
            "!",
            "Ссылка не принята",
            "Откройте актуальную кнопку в Telegram",
            "Адрес неполный или изменён. Не редактируйте OAuth-ссылку и не используйте старую вкладку.",
            "Вернитесь в Telegram. Команда /cancel погасит старую попытку и сразу выдаст новую.",
            0,
        ),
        StatusPage::InvalidCallback => (
            StatusCode::BAD_REQUEST,
            "bad",
            "!",
            "Неверный callback",
            "Нужен весь localhost-адрес",
            "Скопируйте адрес целиком из строки браузера: от http://localhost:51121 до последнего символа.",
            "Короткий код, текст страницы и адрес без state не подходят. В Telegram отправлять callback не нужно.",
            1,
        ),
        StatusPage::ExpiredLink => (
            StatusCode::CONFLICT,
            "warn",
            "↻",
            "Попытка завершена",
            "Эта ссылка уже не активна",
            "Одноразовая попытка была использована, отменена или успела истечь.",
            "Отправьте /cancel в Telegram — бот безопасно остановит старое поколение и выдаст новые ссылки с тем же прокси.",
            1,
        ),
        StatusPage::AuthorizationDenied => (
            StatusCode::BAD_REQUEST,
            "warn",
            "×",
            "Google не завершил вход",
            "Доступ не был подтверждён",
            "Google отменил согласие или вернул OAuth-ошибку. Подписка в пул не добавлена.",
            "Вернитесь в Telegram и отправьте /cancel, чтобы полностью начать авторизацию заново.",
            1,
        ),
        StatusPage::ServiceUnavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "warn",
            "…",
            "Проверка остановлена",
            "Доступ не был опубликован",
            "Сервис не смог безопасно завершить эту одноразовую попытку.",
            "Прокси и сделка сохранены. Команда /cancel в Telegram создаст новое поколение без зависшего запроса.",
            2,
        ),
        StatusPage::CheckingIdentity => (
            StatusCode::ACCEPTED,
            "wait",
            "01",
            "Код принят",
            "Проверяем Google-аккаунт",
            "Проверка уже выполняется независимо от этой вкладки. Закрытие браузера её не прервёт.",
            "Результат придёт в Telegram. Если хотите остановить попытку, отправьте /cancel — старый запрос потеряет право что-либо опубликовать.",
            0,
        ),
        StatusPage::CheckingSubscription => (
            StatusCode::ACCEPTED,
            "wait",
            "02",
            "Callback принят",
            "Проверяем подписку и генерацию",
            "OAuth-код уже передан защищённому фоновому процессу. Вкладку можно закрыть.",
            "Тариф, проект и реальная генерация проверяются через тот же прокси. Итог придёт в Telegram; /cancel в любой момент начнёт всё заново.",
            2,
        ),
    };
    secure_html(
        status,
        page_shell(tone, mark, kicker, title, lead, callout, active_step, ""),
        false,
    )
}

fn code_form(state: &str, phase: OAuthPhase) -> Response {
    // `valid_oauth_state` limits this interpolation to URL-safe ASCII, so the hidden value cannot
    // break out of its quoted attribute. The authorization code is submitted only in the POST body
    // and therefore stays out of Telegram, browser history, referrers and ordinary access logs.
    let (kicker, title, instruction, label, placeholder, button, active_step) = if phase
        == OAuthPhase::LegacyBootstrap
    {
        (
            "Этап 1 из 2",
            "Подтвердите Gemini CLI",
            "После согласия Google откроет страницу Gemini CLI с одноразовым кодом. Скопируйте только этот код и вставьте ниже.",
            "Одноразовый код Gemini CLI",
            "4/…",
            "Завершить инициализацию",
            0,
        )
    } else {
        (
            "Этап 2 из 2",
            "Завершите Antigravity OAuth",
            "После согласия Google перенаправит браузер на localhost; страница может не открыться — это нормально. Скопируйте весь адрес из адресной строки и вставьте ниже.",
            "Полный localhost callback URL",
            "http://localhost:51121/oauth-callback?state=…&amp;code=…",
            "Подключить подписку",
            1,
        )
    };
    let form = format!(
        "<form method=post action=\"/oauth/callback\"><input type=hidden name=state value=\"{state}\"><label class=field>{label}<input name=code required autofocus autocomplete=off autocapitalize=off spellcheck=false maxlength=4096 placeholder=\"{placeholder}\"></label><button type=submit>{button}</button><p class=form-note>Данные отправляются напрямую Auth Bot по HTTPS и не попадают в Telegram или журналы доступа.</p></form>"
    );
    let body = page_shell(
        "wait",
        if phase == OAuthPhase::LegacyBootstrap {
            "01"
        } else {
            "02"
        },
        kicker,
        title,
        instruction,
        "Не меняйте Google-аккаунт, профиль браузера или прокси между двумя этапами.",
        active_step,
        &form,
    );
    secure_html(StatusCode::OK, body, true)
}

#[allow(clippy::too_many_arguments)]
fn page_shell(
    tone: &str,
    mark: &str,
    kicker: &str,
    title: &str,
    lead: &str,
    callout: &str,
    active_step: usize,
    content: &str,
) -> String {
    let step = |index: usize| {
        if index < active_step {
            "step done"
        } else if index == active_step {
            "step active"
        } else {
            "step"
        }
    };
    format!(
        "<!doctype html><html lang=ru><head><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1,viewport-fit=cover\"><meta name=theme-color content=#f7f8fc><title>{title} · Gemini</title><style>{PAGE_STYLES}</style></head><body><main class=shell><div class=brand><span class=brand-mark>G</span>Gemini access</div><section class=card data-tone={tone} aria-live=polite><div class=signal aria-hidden=true><span class=signal-core>{mark}</span></div><p class=kicker>{kicker}</p><h1>{title}</h1><p class=lead>{lead}</p><div class=rail aria-label=\"Этапы подключения\"><span class=\"{}\">CLI</span><span class=track></span><span class=\"{}\">OAuth</span><span class=track></span><span class=\"{}\">Тест</span></div><div class=callout>{callout}</div>{content}</section><p class=privacy>Одноразовые коды и токены не сохраняются в странице. Результат подключения подтверждается только сообщением бота.</p></main></body></html>",
        step(0),
        step(1),
        step(2),
    )
}

fn secure_html(status: StatusCode, body: String, allow_form: bool) -> Response {
    let mut response = (status, Html(body)).into_response();
    let headers = response.headers_mut();
    headers.insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store, max-age=0"),
    );
    let style_hash = STANDARD.encode(Sha256::digest(PAGE_STYLES.as_bytes()));
    let form_action = if allow_form {
        " form-action 'self';"
    } else {
        ""
    };
    let policy = format!(
        "default-src 'none'; style-src 'sha256-{style_hash}';{form_action} base-uri 'none'; frame-ancestors 'none'"
    );
    headers.insert(
        "content-security-policy",
        axum::http::HeaderValue::from_str(&policy).expect("static CSP is a valid header"),
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
    let Some(oauth) = state.config.gemini_oauth.as_ref() else {
        return;
    };
    let (pending_proxy, pending_proxy_order_id) = pending_proxy(oauth, session).unwrap_or_default();
    // `/cancel`, publication and failure all serialize here. Whichever wins first rotates or
    // consumes the exact seller generation; every later path is guarded by the now-stale token.
    let _terminal_guard = oauth.terminal_guard().await;
    let _ = state.store.fail_gemini_oauth(&session.state);
    let accepts_proxy_input = session.job.as_ref().is_some_and(|expected| {
        crate::bot::gemini_job_accepts_proxy_input(&state.store, expected, pending_proxy_order_id)
    });
    // Keep the exact egress after every callback outcome. Seller-owned jobs can still replace it by
    // sending a new proxy, while `повторить` now starts a fresh PKCE generation with the retained
    // one. A transient gateway failure therefore never turns into a request to re-enter secrets.
    let (retry_proxy, retry_proxy_order_id) = (pending_proxy, pending_proxy_order_id);
    let retry_applied = session.job.as_ref().is_some_and(|expected| {
        state
            .store
            .set_handoff_state_for_seller_job(
                session.chat_id,
                expected,
                "gm_gproxy",
                &retry_proxy,
                retry_proxy_order_id,
            )
            .unwrap_or(false)
    });
    if !retry_applied {
        for admin in &state.config.admins_id {
            let _ = state
                .bot
                .send(
                    *admin,
                    &format!(
                        "⚠️ Устаревший Gemini OAuth callback продавца {} завершился ошибкой ({}) и был проигнорирован: активная работа и её прокси не изменены.",
                        session.chat_id,
                        failure.code()
                    ),
                )
                .await;
        }
        eprintln!(
            "[gemini-oauth] chat={} stale callback failed: {} (seller state unchanged)",
            session.chat_id,
            failure.code()
        );
        return;
    }
    // The authorization code and its encrypted PKCE transaction cannot be reused after this point;
    // the proxy remains available for a new generation or can be explicitly replaced by its owner.
    let _ = state
        .bot
        .send(
            session.chat_id,
            if accepts_proxy_input {
                failure.public_message()
            } else {
                failure.fixed_proxy_message()
            },
        )
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
        if failure.operator_action_required() {
            " (operator notified)"
        } else {
            ""
        }
    );
}

fn pending_proxy(config: &Config, session: &GeminiOAuthSession) -> Option<(String, i64)> {
    let pending = open_pending_secret(config, session)?;
    if pending.proxy.is_empty() {
        None
    } else {
        Some((pending.proxy.clone(), pending.proxy_order_id))
    }
}

fn pending_phase(store: &Store, config: &Config, state: &str) -> Option<OAuthPhase> {
    let session = store
        .pending_gemini_session_by_state(state)
        .ok()
        .flatten()?;
    open_pending_secret(config, &session).map(|pending| pending.phase)
}

fn open_pending_secret(
    config: &Config,
    session: &GeminiOAuthSession,
) -> Option<PendingOAuthSecret> {
    let envelope: SealedCredential = serde_json::from_str(&session.sealed_payload).ok()?;
    let payload = config.keyring.open_secret(&session.state, &envelope).ok()?;
    serde_json::from_str::<PendingOAuthSecret>(payload.as_str()).ok()
}

/// Egress незавершённой транзакции продавца — только чтобы вернуть работу на шаг назад, не
/// спрашивая прокси заново.
///
/// `start_gemini_oauth` стирает `users.hproxy`, поэтому на шаге `gm_wait` единственная копия
/// egress живёт внутри запечатанной PKCE-транзакции. Читатель в `db` намеренно не видит уже
/// заклеймленную сессию, так что откат не может обогнать обмен одноразового кода. Секрет
/// возвращается вызывающему и никогда не попадает ни в журнал, ни в Telegram.
pub(crate) fn pending_egress(store: &Store, config: &Config, chat: i64) -> Option<(String, i64)> {
    let session = store.pending_gemini_session(chat).ok().flatten()?;
    pending_proxy(config, &session)
}

/// Egress for an explicit restart. Unlike the ordinary back button, `/cancel` is allowed to fence
/// a claimed callback, so it must be able to recover the proxy from both pending and processing
/// generations before the generation-rotating DB transition deletes the old envelope.
pub(crate) fn active_egress(store: &Store, config: &Config, chat: i64) -> Option<(String, i64)> {
    let session = store.active_gemini_session(chat).ok().flatten()?;
    pending_proxy(config, &session)
}

fn valid_oauth_value(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && value.bytes().all(|byte| byte.is_ascii_graphic())
}

fn valid_oauth_state(value: &str) -> bool {
    value.len() == 43
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn valid_pending_phase(pending: &PendingOAuthSecret) -> bool {
    if pending.proxy_order_id < 0 || normalize_proxy_url(&pending.proxy).is_err() {
        return false;
    }
    match pending.phase {
        OAuthPhase::LegacyBootstrap => {
            pending.client_id == LEGACY_CLIENT_ID
                && pending.client_secret == LEGACY_CLIENT_SECRET
                && pending.redirect_uri == LEGACY_REDIRECT_URI
                && pending.bootstrap_subject.is_empty()
        }
        OAuthPhase::AntigravityFinal => {
            pending.client_id == ANTIGRAVITY_CLIENT_ID
                && pending.client_secret == ANTIGRAVITY_CLIENT_SECRET
                && pending.redirect_uri == ANTIGRAVITY_REDIRECT_URI
                && valid_identity(&pending.bootstrap_subject, 512)
        }
        // Payloads created before this rollout had no phase field. Preserve their exact sealed
        // OAuth identity/redirect so a deployment cannot strand an already-open consent page.
        OAuthPhase::DirectAntigravity => {
            valid_oauth_value(&pending.client_id, 1_024)
                && valid_oauth_value(&pending.client_secret, 4_096)
                && (pending.redirect_uri.is_empty()
                    || reqwest::Url::parse(&pending.redirect_uri).is_ok())
                && pending.bootstrap_subject.is_empty()
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    Authorization,
    Interrupted,
    CodeAssistApiDisabled,
    TransportUnavailable,
    Temporary,
    UnsupportedPlan,
    AccountMismatch,
    GenerationUnavailable,
    /// Google admits the login and the paid tier but refuses generation until the Google account
    /// itself is verified (`PERMISSION_DENIED` / `VALIDATION_REQUIRED`). Retrying cannot change it.
    AccountValidationRequired,
    StaleHandoff,
    Duplicate,
    DuplicateProxy,
    MigrationProxyMismatch,
    Storage,
}

impl Failure {
    fn code(self) -> &'static str {
        match self {
            Self::Authorization => "authorization",
            Self::Interrupted => "interrupted",
            Self::CodeAssistApiDisabled => "code_assist_api_disabled",
            Self::TransportUnavailable => "transport_unavailable",
            Self::Temporary => "temporary_upstream",
            Self::UnsupportedPlan => "unsupported_plan",
            Self::AccountMismatch => "account_mismatch",
            Self::GenerationUnavailable => "generation_unavailable",
            Self::AccountValidationRequired => "account_validation_required",
            Self::StaleHandoff => "stale_handoff",
            Self::Duplicate => "duplicate_account",
            Self::DuplicateProxy => "duplicate_proxy",
            Self::MigrationProxyMismatch => "migration_proxy_mismatch",
            Self::Storage => "storage",
        }
    }

    fn public_message(self) -> &'static str {
        match self {
            Self::Authorization => "❌ Google не подтвердил вход или ссылка истекла. Прокси сохранён: отправь <code>повторить</code> для новой авторизации либо пришли новый прокси, чтобы заменить его.",
            Self::Interrupted => "🔁 Проверка была прервана до завершения. Прокси сохранён: отправь <code>/cancel</code>, и бот немедленно выдаст полностью новую безопасную попытку.",
            Self::CodeAssistApiDisabled => "❌ Google не разрешил подключить этот аккаунт через Antigravity OAuth. Включать API в своём Cloud-проекте не нужно. Прокси сохранён: отправь <code>повторить</code>; если ошибка вернётся, администратор проверит причину.",
            Self::TransportUnavailable => "⚠️ OAuth-транспорт не установил стабильный CONNECT/TLS-путь после автоматических попыток. Подписка не отклонена; прокси сохранён. Отправь <code>повторить</code>, а если ошибка повторяется — пришли новый прокси для замены.",
            Self::Temporary => "⚠️ Google временно не завершил служебную проверку. Прокси не был отклонён — менять его не нужно. Подожди немного и отправь <code>повторить</code>.",
            Self::UnsupportedPlan => "❌ На этом Google-аккаунте не найдена активная подписка из оффера. Проверь, что нужный тариф активирован именно на этом аккаунте; прокси сохранён, для новой авторизации отправь <code>повторить</code>.",
            Self::AccountMismatch => "❌ На втором этапе выбран другой Google-аккаунт. Оба согласия должны быть выданы одной подпиской в одном профиле браузера. Профиль не опубликован; отправь <code>повторить</code> и начни заново.",
            Self::GenerationUnavailable => "⚠️ Google подтвердил вход и активный тариф, но реальная тестовая генерация не была подтверждена. Профиль не опубликован и сделка не завершена; прокси сохранён. Подожди немного и отправь <code>повторить</code>.",
            Self::AccountValidationRequired => "❌ Google подтвердил вход и активный тариф Google AI, но требует подтвердить сам аккаунт: генерация отклонена с «Verify your account to continue». Повтор не поможет, пока проверка не пройдена. В том же профиле антидетект-браузера и с тем же прокси открой <code>gemini.google.com</code> или Antigravity, выполни один запрос и заверши проверку, которую покажет Google (обычно номер телефона или подтверждение возраста). Профиль не опубликован, сделка не завершена, прокси сохранён; после успешной проверки отправь <code>повторить</code>.",
            Self::StaleHandoff => "❌ Эта попытка подключения уже не относится к текущей сделке. Профиль не опубликован; продолжи актуальный шаг в боте.",
            Self::Duplicate => "❌ Эта Google-подписка уже присутствует в пуле.",
            Self::DuplicateProxy => "❌ Этот прокси уже закреплён за другим Gemini-профилем. Для подписки нужен отдельный прокси.",
            Self::MigrationProxyMismatch => "❌ Для этой Google-подписки уже закреплён другой прокси. Переподключать её нужно через тот же прокси — egress аккаунта менять нельзя.",
            Self::Storage => "⚠️ Подписка проверена, но добавить аккаунт не получилось. Администратор уведомлён; повторять действия пока не нужно.",
        }
    }

    fn fixed_proxy_message(self) -> &'static str {
        match self {
            Self::Authorization => "❌ Google не подтвердил вход или ссылка истекла. Подключение осталось закреплено за прокси этой позиции. Отправь <code>повторить</code>, чтобы начать авторизацию заново.",
            Self::Interrupted => "🔁 Проверка была прервана до завершения. Отправь <code>/cancel</code>, и бот немедленно выдаст полностью новую безопасную попытку с тем же закреплённым прокси.",
            Self::CodeAssistApiDisabled => "❌ Google не разрешил подключить этот аккаунт через Antigravity OAuth. Включать API в своём Cloud-проекте не нужно. Отправь <code>повторить</code>; если ошибка вернётся, администратор проверит причину.",
            Self::TransportUnavailable => "⚠️ OAuth-транспорт не установил стабильный CONNECT/TLS-путь после автоматических попыток. Подписка не отклонена, прокси позиции сохранён. Подожди немного и отправь <code>повторить</code>.",
            Self::Temporary => "⚠️ Google временно не завершил служебную проверку. Закреплённый прокси не был отклонён — менять его не нужно. Подожди немного и отправь <code>повторить</code>.",
            Self::UnsupportedPlan => "❌ На этом Google-аккаунте не найдена активная подписка из оффера. Проверь тариф на этом аккаунте и отправь <code>повторить</code>; будет использован закреплённый прокси.",
            Self::AccountMismatch => "❌ На втором этапе выбран другой Google-аккаунт. Оба согласия должны быть выданы одной подпиской в одном профиле браузера. Профиль не опубликован; отправь <code>повторить</code> и начни заново.",
            Self::GenerationUnavailable => "⚠️ Google подтвердил вход и активный тариф, но реальная тестовая генерация не была подтверждена. Профиль не опубликован и сделка не завершена. Подожди немного и отправь <code>повторить</code>; будет использован закреплённый прокси.",
            Self::AccountValidationRequired => "❌ Google подтвердил вход и активный тариф Google AI, но требует подтвердить сам аккаунт: генерация отклонена с «Verify your account to continue». Повтор не поможет, пока проверка не пройдена. В том же профиле браузера и с закреплённым прокси открой <code>gemini.google.com</code> или Antigravity, выполни один запрос и заверши проверку, которую покажет Google (обычно номер телефона или подтверждение возраста). Затем отправь <code>повторить</code> — будет использован закреплённый прокси.",
            Self::StaleHandoff => "❌ Эта попытка подключения уже не относится к текущей сделке. Профиль не опубликован; продолжи актуальный шаг в боте.",
            _ => self.public_message(),
        }
    }

    fn operator_action_required(self) -> bool {
        matches!(
            self,
            Self::CodeAssistApiDisabled
                | Self::TransportUnavailable
                | Self::Temporary
                | Self::GenerationUnavailable
                | Self::AccountValidationRequired
                | Self::Storage
        )
    }

    fn operator_message(self) -> &'static str {
        match self {
            Self::CodeAssistApiDisabled => "⚠️ Antigravity OAuth завершился, но Cloud Code gateway отклонил consumer identity. Проверь bounded diagnostic в journalctl; пользовательский Cloud API включать не нужно.",
            Self::TransportUnavailable => "⚠️ Gemini OAuth transport исчерпал bounded CONNECT/TLS recovery. Проверь только phase и secret-free transport class в journalctl; это ещё не отказ Google account/tier.",
            Self::Temporary => "⚠️ Gemini OAuth получил временный HTTP/malformed control-plane outcome после установленного транспорта. Проверь phase/status diagnostic; не приписывай ошибку прокси без transport-class evidence.",
            Self::GenerationUnavailable => "⚠️ Gemini OAuth и тариф подтверждены, но exact generateContent acceptance не прошёл. Профиль не опубликован и выплата не завершена; проверь phase/status diagnostic без повторной атрибуции прокси.",
            Self::AccountValidationRequired => "⚠️ Gemini OAuth и тариф подтверждены, но Google требует верификацию аккаунта продавца (PERMISSION_DENIED/VALIDATION_REQUIRED) и отклоняет генерацию на всех surface. Профиль не опубликован и выплата не завершена; это состояние Google-аккаунта, а не прокси и не тарифа.",
            _ => "⚠️ Gemini OAuth publication failed closed. Проверь права AUTH_BOT_GEMINI_DIR, profiles.json и совпадение credential keyring; секреты не логировались.",
        }
    }
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: i64,
    #[serde(default)]
    #[zeroize(skip)]
    scope: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TierEvidenceClass {
    Absent,
    KnownIdAndName,
    KnownIdNameDrift,
    KnownIdNameConflict,
    KnownNameOnly,
    Unknown,
}

impl TierEvidenceClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::KnownIdAndName => "known_id_name_match",
            Self::KnownIdNameDrift => "known_id_name_drift",
            Self::KnownIdNameConflict => "known_id_name_conflict",
            Self::KnownNameOnly => "known_name_only",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CodeAssistDiagnostic {
    project_present: bool,
    paid_tier: TierEvidenceClass,
    current_tier: TierEvidenceClass,
    allowed_tier_count: usize,
}

impl CodeAssistDiagnostic {
    fn from_response(response: &LoadCodeAssistResponse) -> Self {
        Self {
            project_present: project_from_value(response.cloudaicompanion_project.as_ref())
                .is_some(),
            paid_tier: classify_tier_evidence(response.paid_tier.as_ref()),
            current_tier: classify_tier_evidence(response.current_tier.as_ref()),
            allowed_tier_count: response.allowed_tiers.len(),
        }
    }

    fn sanitized(self) -> String {
        format!(
            "project={} paid={} current={} allowed_tiers={}",
            if self.project_present {
                "present"
            } else {
                "absent_or_malformed"
            },
            self.paid_tier.as_str(),
            self.current_tier.as_str(),
            self.allowed_tier_count,
        )
    }
}

#[derive(Deserialize)]
struct OperationResponse {
    #[serde(default)]
    done: bool,
    response: Option<Value>,
}

struct ResolvedAccount {
    project_id: String,
    tier_id: String,
    tier_name: String,
    plan: String,
    diagnostic: CodeAssistDiagnostic,
}

enum Completion {
    LegacyBootstrap { subject: Zeroizing<String> },
    Published(PublishedProfile, tokio::sync::OwnedMutexGuard<()>),
}

struct PublishedProfile {
    id: String,
    plan: String,
    has_proxy: bool,
    migrated: bool,
    /// Тот же subject переавторизован свежим согласием: конверт заменён, профиль сохранён.
    reauthorized: bool,
}

/// One serialized OAuth transaction with evidence-based CONNECT recovery. A new helper is spawned
/// after every transport error so a half-closed tunnel cannot poison the next attempt. Token
/// exchange is replayed only when the helper proves the failure happened before the target request;
/// after a token exists, the idempotent control-plane reads/onboarding calls may recover from the
/// wider transient set.
struct RecoveringClient<'a> {
    proxy: &'a str,
    chat_id: i64,
    inner: GeminiHttpClient,
}

impl<'a> RecoveringClient<'a> {
    async fn connect(proxy: &'a str, chat_id: i64) -> Result<Self, Failure> {
        let inner = GeminiHttpClient::connect(proxy).await.map_err(|error| {
            eprintln!(
                "[gemini-oauth] chat={} attested OAuth transport startup failed: {}",
                chat_id,
                crate::gemini_transport::diagnostic_kind(&error),
            );
            Failure::TransportUnavailable
        })?;
        Ok(Self {
            proxy,
            chat_id,
            inner,
        })
    }

    async fn reconnect(&mut self) -> Result<(), Failure> {
        self.inner = GeminiHttpClient::connect(self.proxy)
            .await
            .map_err(|error| {
                eprintln!(
                    "[gemini-oauth] chat={} OAuth transport restart failed: {}",
                    self.chat_id,
                    crate::gemini_transport::diagnostic_kind(&error),
                );
                Failure::TransportUnavailable
            })?;
        Ok(())
    }

    async fn token_request(
        &mut self,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<crate::gemini_transport::Response, Failure> {
        for attempt in 1..=TRANSPORT_RECOVERY_DELAYS.len() + 1 {
            match self
                .inner
                .request(GeminiHttpMethod::Post, TOKEN_URL, headers, body)
                .await
            {
                Ok(response) => {
                    if attempt > 1 {
                        eprintln!(
                            "[gemini-oauth] chat={} token transport recovered on attempt {}/{}",
                            self.chat_id,
                            attempt,
                            TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        );
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let diagnostic = crate::gemini_transport::diagnostic_kind(&error);
                    let retryable = crate::gemini_transport::failure_kind(&error)
                        .is_some_and(|kind| kind.safe_to_retry_before_target());
                    eprintln!(
                        "[gemini-oauth] chat={} token transport attempt {}/{} failed: {}",
                        self.chat_id,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,
                    );
                    if !retryable || attempt > TRANSPORT_RECOVERY_DELAYS.len() {
                        return Err(Failure::TransportUnavailable);
                    }
                    tokio::time::sleep(TRANSPORT_RECOVERY_DELAYS[attempt - 1]).await;
                    self.reconnect().await?;
                }
            }
        }
        unreachable!("bounded token transport recovery loop always returns")
    }

    async fn control_request(
        &mut self,
        method: GeminiHttpMethod,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        phase: &'static str,
    ) -> Result<crate::gemini_transport::Response, Failure> {
        for attempt in 1..=TRANSPORT_RECOVERY_DELAYS.len() + 1 {
            match self.inner.request(method, url, headers, body).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let diagnostic = crate::gemini_transport::diagnostic_kind(&error);
                    let retryable = crate::gemini_transport::failure_kind(&error)
                        .is_some_and(|kind| kind.retryable_control_plane());
                    eprintln!(
                        "[gemini-oauth] chat={} {} transport attempt {}/{} failed: {}",
                        self.chat_id,
                        phase,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,
                    );
                    if !retryable || attempt > TRANSPORT_RECOVERY_DELAYS.len() {
                        return Err(Failure::TransportUnavailable);
                    }
                    tokio::time::sleep(TRANSPORT_RECOVERY_DELAYS[attempt - 1]).await;
                    self.reconnect().await?;
                }
            }
        }
        unreachable!("bounded control transport recovery loop always returns")
    }

    /// A generation can consume quota and may have reached Google even when the response tunnel is
    /// lost. Send it exactly once; unlike control-plane reads it must never be replayed after an
    /// ambiguous transport outcome.
    async fn generation_request(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<crate::gemini_transport::Response, Failure> {
        self.inner
            .request(GeminiHttpMethod::Post, url, headers, body)
            .await
            .map_err(|error| {
                eprintln!(
                    "[gemini-oauth] chat={} generation acceptance transport failed: {}",
                    self.chat_id,
                    crate::gemini_transport::diagnostic_kind(&error),
                );
                Failure::GenerationUnavailable
            })
    }

    async fn userinfo_request(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
    ) -> Result<crate::gemini_transport::Response, Failure> {
        for attempt in 1..=TRANSPORT_RECOVERY_DELAYS.len() + 1 {
            match self.inner.fetch_userinfo(url, headers).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let diagnostic = crate::gemini_transport::diagnostic_kind(&error);
                    let retryable = crate::gemini_transport::failure_kind(&error)
                        .is_some_and(|kind| kind.retryable_control_plane());
                    eprintln!(
                        "[gemini-oauth] chat={} userinfo transport attempt {}/{} failed: {}",
                        self.chat_id,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,
                    );
                    if !retryable || attempt > TRANSPORT_RECOVERY_DELAYS.len() {
                        return Err(Failure::TransportUnavailable);
                    }
                    tokio::time::sleep(TRANSPORT_RECOVERY_DELAYS[attempt - 1]).await;
                    self.reconnect().await?;
                }
            }
        }
        unreachable!("bounded userinfo transport recovery loop always returns")
    }
}

async fn complete(
    store: &Store,
    config: &Config,
    session: &GeminiOAuthSession,
    code: &str,
    verifier: &str,
    proxy: &str,
    proxy_order_id: i64,
    client_id: &str,
    client_secret: &str,
    redirect_uri: &str,
    phase: OAuthPhase,
    bootstrap_subject: &str,
) -> Result<Completion, Failure> {
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
    let mut client = RecoveringClient::connect(proxy, session.chat_id).await?;
    let form = token_exchange_form(
        phase,
        client_id,
        verifier,
        code,
        redirect_uri,
        client_secret,
    )?;
    let token_user_agent = match phase {
        OAuthPhase::LegacyBootstrap => format!(
            "google-api-nodejs-client/{}",
            gemini_credential::GEMINI_GOOGLE_AUTH_LIBRARY_VERSION
        ),
        OAuthPhase::AntigravityFinal | OAuthPhase::DirectAntigravity => {
            "Go-http-client/2.0".to_string()
        }
    };
    let google_api_client = legacy_google_api_client();
    let mut token_headers = vec![
        (
            "content-type",
            "application/x-www-form-urlencoded;charset=UTF-8",
        ),
        ("user-agent", token_user_agent.as_str()),
    ];
    if post_identity_action(phase) == PostIdentityAction::StartAntigravityConsent {
        token_headers.push(("x-goog-api-client", google_api_client.as_str()));
    }
    let response = client
        .token_request(&token_headers, form.as_bytes())
        .await?;
    if !(200..300).contains(&response.status) {
        eprintln!(
            "[gemini-oauth] chat={} Google rejected the token exchange: HTTP {}",
            session.chat_id, response.status
        );
        return Err(if response.status >= 500 {
            Failure::Temporary
        } else {
            Failure::Authorization
        });
    }
    let mut token: TokenResponse =
        serde_json::from_slice(&response.body).map_err(|_| Failure::Temporary)?;
    if !valid_oauth_value(&token.access_token, 16_384)
        || token
            .refresh_token
            .as_deref()
            .is_none_or(|value| !valid_oauth_value(value, 16_384))
        || !(60..=86_400).contains(&token.expires_in)
    {
        return Err(Failure::Authorization);
    }
    eprintln!(
        "[gemini-oauth] chat={} Google granted scopes: {}",
        session.chat_id,
        token.scope.as_deref().unwrap_or("<none>")
    );
    let authorization = Zeroizing::new(format!("Bearer {}", token.access_token));
    let headers = official_userinfo_headers(authorization.as_str());
    let user_info_response = client.userinfo_request(USERINFO_URL, &headers).await?;
    if !(200..300).contains(&user_info_response.status) {
        eprintln!(
            "[gemini-oauth] chat={} Google userinfo call failed: HTTP {}",
            session.chat_id, user_info_response.status
        );
        return Err(Failure::Authorization);
    }
    let mut user: UserInfo =
        serde_json::from_slice(&user_info_response.body).map_err(|_| Failure::Temporary)?;
    if !user.verified_email || !valid_identity(&user.id, 512) || !valid_identity(&user.email, 512) {
        eprintln!(
            "[gemini-oauth] chat={} rejected: Google account is unverified or malformed",
            session.chat_id
        );
        return Err(Failure::Authorization);
    }
    // The Gemini CLI OAuth transaction is an identity bootstrap, not the authority for
    // Antigravity subscription admission. Google can return no project/tier from the legacy Code
    // Assist surface; that absence neither proves nor disproves Antigravity eligibility.
    // Stop after verified userinfo and duplicate/proxy preflight; the fresh Antigravity consent
    // below still fails closed on exact tier, project, matching subject and real generation.
    if post_identity_action(phase) == PostIdentityAction::StartAntigravityConsent {
        preflight_bootstrap_candidate(
            &config.root,
            &config.keyring,
            &user.id,
            proxy,
            proxy_order_id,
        )?;
        eprintln!(
            "[gemini-oauth] chat={} legacy identity bootstrap passed; final subscription admission remains pending",
            session.chat_id
        );
        return Ok(Completion::LegacyBootstrap {
            subject: Zeroizing::new(std::mem::take(&mut user.id)),
        });
    }
    let refresh_token = token.refresh_token.take().ok_or(Failure::Authorization)?;
    let resolved = resolve_antigravity_account(&mut client, &token.access_token).await;
    let resolved = match resolved {
        Ok(resolved) => resolved,
        Err(failure) => {
            eprintln!(
                "[gemini-oauth] chat={} {:?} tier/project resolution failed: {}",
                session.chat_id,
                phase,
                failure.code()
            );
            return Err(failure);
        }
    };
    if !supported_paid_plan(&resolved.plan) {
        log_unsupported_plan("unreviewed_reported_tier", resolved.diagnostic);
        eprintln!(
            "[gemini-oauth] chat={} rejected: unsupported Google plan {}",
            session.chat_id,
            plan_label(&resolved.plan)
        );
        return Err(Failure::UnsupportedPlan);
    }
    if validate_final_subject(phase, &user.id, bootstrap_subject).is_err() {
        eprintln!(
            "[gemini-oauth] chat={} rejected: Antigravity subject differs from legacy bootstrap",
            session.chat_id
        );
        return Err(Failure::AccountMismatch);
    }
    generation_probe(
        &mut client,
        &token.access_token,
        &resolved.project_id,
        session.chat_id,
    )
    .await?;
    eprintln!(
        "[gemini-oauth] chat={} Google account and exact generation verified, plan={}, sealing credential",
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
    let terminal_guard = config.terminal_guard().await;
    // Generation acceptance may take long enough for the seller to cancel, rewind or replace the
    // exact job generation. Re-check after waiting for the filesystem publication lock and as close
    // as possible to the credential write; SQLite and the roster cannot form one atomic transaction.
    if !oauth_session_handoff_is_current(store, session) {
        eprintln!(
            "[gemini-oauth] chat={} rejected stale seller generation immediately before publication",
            session.chat_id
        );
        return Err(Failure::StaleHandoff);
    }
    let root = config.root.clone();
    let ring = config.keyring.clone();
    let active = config.active_key_id.clone();
    let published = tokio::task::spawn_blocking(move || publish(&root, &ring, &active, credential))
        .await
        .map_err(|_| Failure::Storage)?;
    match &published {
        Ok(profile) if profile.reauthorized => eprintln!(
            "[gemini-oauth] chat={} reauthorized profile {} in place (plan {}); the previous refresh token was invalidated by Google",
            session.chat_id, profile.id, plan_label(&profile.plan)
        ),
        Ok(profile) if profile.migrated => eprintln!(
            "[gemini-oauth] chat={} atomically migrated profile {} to Antigravity (plan {})",
            session.chat_id, profile.id, plan_label(&profile.plan)
        ),
        Ok(profile) => eprintln!(
            "[gemini-oauth] chat={} sealed and published profile {} (plan {}) into the Gemini roster",
            session.chat_id, profile.id, plan_label(&profile.plan)
        ),
        Err(failure) => eprintln!(
            "[gemini-oauth] chat={} sealing/publishing the profile failed: {}",
            session.chat_id,
            failure.code()
        ),
    }
    published.map(|profile| Completion::Published(profile, terminal_guard))
}

fn validate_final_subject(
    phase: OAuthPhase,
    subject: &str,
    bootstrap_subject: &str,
) -> Result<(), Failure> {
    if phase == OAuthPhase::AntigravityFinal && subject != bootstrap_subject {
        Err(Failure::AccountMismatch)
    } else {
        Ok(())
    }
}

fn valid_identity(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

async fn resolve_antigravity_account(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
) -> Result<ResolvedAccount, Failure> {
    let mut loaded = load_code_assist(client, access_token).await?;
    if project_from_value(loaded.cloudaicompanion_project.as_ref()).is_none() {
        let tier = loaded
            .allowed_tiers
            .iter()
            .find(|tier| tier.is_default)
            .or(loaded.current_tier.as_ref())
            .or_else(|| loaded.allowed_tiers.first())
            .cloned();
        let Some(tier) = tier else {
            log_unsupported_plan(
                "antigravity_missing_onboarding_tier",
                CodeAssistDiagnostic::from_response(&loaded),
            );
            return Err(Failure::UnsupportedPlan);
        };
        let Some(tier_id) = tier.id.as_deref() else {
            log_unsupported_plan(
                "antigravity_missing_onboarding_tier_id",
                CodeAssistDiagnostic::from_response(&loaded),
            );
            return Err(Failure::UnsupportedPlan);
        };
        onboard_antigravity(client, access_token, tier_id).await?;
        loaded = load_code_assist(client, access_token).await?;
    }
    let diagnostic = CodeAssistDiagnostic::from_response(&loaded);
    log_tier_evidence_if_enabled(&loaded);
    let Some(project_id) = project_from_value(loaded.cloudaicompanion_project.as_ref()) else {
        log_unsupported_plan("antigravity_missing_project", diagnostic);
        return Err(Failure::UnsupportedPlan);
    };
    let (tier_id, tier_name, plan) = resolve_reported_tier(&loaded)?;
    Ok(ResolvedAccount {
        project_id,
        tier_id,
        tier_name,
        plan,
        diagnostic,
    })
}

/// The reviewed acceptance surfaces for the one paid probe generation, in order. Antigravity's own
/// language server reads the sandbox host, but a subscription can be admitted there and rejected —
/// or the reverse — independently of the production host the gateway actually serves from. A
/// pre-generation rejection therefore moves to the next surface; nothing that already produced a
/// generation is ever replayed.
const GENERATION_PROBE_SURFACES: [(&str, &str); 2] = [
    ("sandbox", CODE_ASSIST_SANDBOX_URL),
    ("production", CODE_ASSIST_PROD_URL),
];

async fn generation_probe(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
    project_id: &str,
    chat_id: i64,
) -> Result<(), Failure> {
    let mut last = Failure::GenerationUnavailable;
    for (surface, host) in GENERATION_PROBE_SURFACES {
        let session_id = fresh_uuid_v4().map_err(|_| Failure::GenerationUnavailable)?;
        let request_id = fresh_uuid_v4().map_err(|_| Failure::GenerationUnavailable)?;
        let body = generation_probe_body(project_id, &session_id, &request_id);
        let encoded =
            Zeroizing::new(serde_json::to_vec(&body).map_err(|_| Failure::GenerationUnavailable)?);
        let authorization = Zeroizing::new(format!("Bearer {access_token}"));
        let user_agent = antigravity_user_agent();
        let response = client
            .generation_request(
                &format!("{host}/v1internal:generateContent"),
                &[
                    ("authorization", authorization.as_str()),
                    ("content-type", "application/json"),
                    ("user-agent", user_agent.as_str()),
                    (
                        "x-goog-api-client",
                        "google-cloud-sdk vscode_cloudshelleditor/0.1",
                    ),
                    ("client-metadata", legacy_client_metadata_header()),
                ],
                &encoded,
            )
            .await?;
        match validate_generation_probe_response(response.status, &response.body) {
            Ok(()) => {
                eprintln!(
                    "[gemini-oauth] chat={chat_id} exact generation acceptance passed on {surface}"
                );
                return Ok(());
            }
            Err(failure) => {
                log_generation_failure(chat_id, surface, response.status, &response.body);
                // An account-level rejection is the same on every host, so stop asking. A 2xx that
                // fails acceptance already consumed a paid generation, and any other status is not
                // evidence that a different host would answer differently. Only an access rejection
                // made before the model ran may try the next reviewed surface.
                let classified = classify_generation_failure(&response.body);
                if let Some(classified) = classified {
                    return Err(classified);
                }
                if !matches!(response.status, 403 | 404) {
                    return Err(failure);
                }
                last = failure;
            }
        }
    }
    Err(last)
}

/// Google's private error bodies can name the project and account, so only the enum-shaped fields
/// are journalled by default. The free-form message is available behind the same opt-in operator
/// switch as raw tier evidence, bounded and stripped of control characters.
fn log_generation_failure(chat_id: i64, surface: &str, status: u16, body: &[u8]) {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let google_status = error
        .and_then(|error| error.get("status"))
        .and_then(Value::as_str);
    let reason = error
        .and_then(|error| error.get("details"))
        .and_then(Value::as_array)
        .and_then(|details| {
            details
                .iter()
                .find_map(|detail| detail.get("reason").and_then(Value::as_str))
        });
    eprintln!(
        "[gemini-oauth] chat={chat_id} generation acceptance failed on {surface}: HTTP {status} google_status={} reason={}",
        bounded_label(google_status),
        bounded_label(reason)
    );
    if std::env::var("AUTH_BOT_GEMINI_TIER_EVIDENCE").as_deref() == Ok("1") {
        let message = error
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str);
        eprintln!(
            "[gemini-oauth] chat={chat_id} generation acceptance detail: {}",
            bounded_label(message)
        );
    }
}

/// Classify the account-level states Google reports through a generation rejection. These are
/// properties of the Google account, not of the surface that answered, so the caller stops probing
/// instead of asking another host the same question — and the seller gets the real instruction
/// instead of "try again later", which can never change any of them.
fn classify_generation_failure(body: &[u8]) -> Option<Failure> {
    let detail = String::from_utf8_lossy(body);
    if matches!(
        classify_google_http_failure(403, &detail),
        Failure::CodeAssistApiDisabled
    ) {
        return Some(Failure::CodeAssistApiDisabled);
    }
    let parsed = serde_json::from_slice::<Value>(body).ok();
    let error = parsed.as_ref().and_then(|value| value.get("error"));
    let validation_reason = error
        .and_then(|error| error.get("details"))
        .and_then(Value::as_array)
        .is_some_and(|details| {
            details.iter().any(|detail| {
                detail.get("reason").and_then(Value::as_str) == Some("VALIDATION_REQUIRED")
            })
        });
    let validation_message = error
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .is_some_and(|message| message.to_ascii_lowercase().contains("verify your account"));
    (validation_reason || validation_message).then_some(Failure::AccountValidationRequired)
}

fn generation_probe_body(project_id: &str, session_id: &str, request_id: &str) -> Value {
    json!({
        "model": GENERATION_PROBE_MODEL,
        "project": project_id,
        "request": {
            "contents": [{
                "role": "user",
                "parts": [{"text": "Reply with OK."}]
            }],
            "generationConfig": {"maxOutputTokens": 8},
            "sessionId": session_id,
        },
        "userAgent": "antigravity",
        "requestType": "agent",
        "requestId": format!("agent-{request_id}"),
    })
}

fn validate_generation_probe_response(status: u16, body: &[u8]) -> Result<(), Failure> {
    if !(200..300).contains(&status) {
        return Err(Failure::GenerationUnavailable);
    }
    let value: Value = serde_json::from_slice(body).map_err(|_| Failure::GenerationUnavailable)?;
    let response = value
        .get("response")
        .and_then(Value::as_object)
        .ok_or(Failure::GenerationUnavailable)?;
    if response
        .get("candidates")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(Failure::GenerationUnavailable);
    }
    let usage = response
        .get("usageMetadata")
        .and_then(Value::as_object)
        .ok_or(Failure::GenerationUnavailable)?;
    let authoritative = [
        "promptTokenCount",
        "candidatesTokenCount",
        "thoughtsTokenCount",
        "toolUsePromptTokenCount",
        "totalTokenCount",
    ]
    .into_iter()
    .filter_map(|field| usage.get(field).and_then(Value::as_u64))
    .any(|tokens| tokens > 0);
    authoritative
        .then_some(())
        .ok_or(Failure::GenerationUnavailable)
}

async fn load_code_assist(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
) -> Result<LoadCodeAssistResponse, Failure> {
    for base in [
        CODE_ASSIST_PROD_URL,
        CODE_ASSIST_SANDBOX_URL,
        CODE_ASSIST_DAILY_URL,
    ] {
        match post_antigravity_json(
            client,
            access_token,
            &format!("{base}/v1internal:loadCodeAssist"),
            &load_code_assist_request_body(),
            false,
        )
        .await
        {
            Ok(response) => return Ok(response),
            Err(Failure::Temporary) => continue,
            Err(failure) => return Err(failure),
        }
    }
    Err(Failure::Temporary)
}

fn load_code_assist_request_body() -> Value {
    // Minimal official Antigravity control-plane discovery body.
    json!({"metadata": client_metadata(None)})
}

async fn onboard_antigravity(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
    tier_id: &str,
) -> Result<(), Failure> {
    let body = onboard_request_body(tier_id);
    for base in [
        CODE_ASSIST_SANDBOX_URL,
        CODE_ASSIST_DAILY_URL,
        CODE_ASSIST_PROD_URL,
    ] {
        for poll in 0..MAX_ONBOARD_POLLS {
            if poll > 0 {
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
            match post_antigravity_json::<OperationResponse>(
                client,
                access_token,
                &format!("{base}/v1internal:onboardUser"),
                &body,
                true,
            )
            .await
            {
                Ok(operation) if operation.done => {
                    if operation
                        .response
                        .as_ref()
                        .and_then(|response| {
                            project_from_value(response.get("cloudaicompanionProject"))
                        })
                        .is_some()
                    {
                        return Ok(());
                    }
                    break;
                }
                Ok(_) => continue,
                Err(Failure::Temporary) => {
                    break;
                }
                Err(failure) => return Err(failure),
            }
        }
    }
    Err(Failure::Temporary)
}

fn onboard_request_body(tier_id: &str) -> Value {
    json!({
        "tier_id": tier_id,
        "metadata": antigravity_control_metadata(),
    })
}

async fn post_antigravity_json<T: for<'de> Deserialize<'de>>(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
    url: &str,
    body: &Value,
    control_plane: bool,
) -> Result<T, Failure> {
    let authorization = Zeroizing::new(format!("Bearer {access_token}"));
    let user_agent = if control_plane {
        antigravity_control_user_agent()
    } else {
        antigravity_user_agent()
    };
    let google_api_client = control_plane.then_some(gemini_credential::ANTIGRAVITY_GOOG_API_CLIENT);
    let encoded = Zeroizing::new(serde_json::to_vec(body).map_err(|_| Failure::Temporary)?);
    let mut headers = vec![
        ("accept", "*/*"),
        ("authorization", authorization.as_str()),
        ("content-type", "application/json"),
        ("user-agent", user_agent.as_str()),
    ];
    if let Some(google_api_client) = google_api_client {
        headers.push(("x-goog-api-client", google_api_client));
    }
    let response = client
        .control_request(
            GeminiHttpMethod::Post,
            url,
            &headers,
            &encoded,
            "code_assist",
        )
        .await?;
    if !(200..300).contains(&response.status) {
        let status = response.status;
        // Private Google diagnostics can contain consumer project/account context. Use them only
        // for bounded failure classification and never copy them into journalctl or public output.
        let endpoint = url.rsplit('/').next().unwrap_or(url);
        let detail = String::from_utf8_lossy(&response.body);
        let detail: String = detail.chars().take(4_096).collect();
        eprintln!("[gemini-oauth] Code Assist {endpoint} returned HTTP {status}");
        return Err(classify_google_http_failure(status, &detail));
    }
    serde_json::from_slice(&response.body).map_err(|_| Failure::Temporary)
}

fn antigravity_user_agent() -> String {
    format!(
        "antigravity/hub/{} {}",
        gemini_credential::ANTIGRAVITY_VERSION,
        gemini_credential::ANTIGRAVITY_PLATFORM,
    )
}

fn antigravity_control_user_agent() -> String {
    format!(
        "{} google-api-nodejs-client/{}",
        antigravity_user_agent(),
        gemini_credential::ANTIGRAVITY_NODE_API_CLIENT_VERSION,
    )
}

fn legacy_google_api_client() -> String {
    format!(
        "gl-node/{}",
        gemini_credential::GEMINI_NODE_VERSION.trim_start_matches('v')
    )
}

fn token_exchange_form(
    phase: OAuthPhase,
    client_id: &str,
    verifier: &str,
    code: &str,
    redirect_uri: &str,
    client_secret: &str,
) -> Result<Zeroizing<String>, Failure> {
    // Antigravity's Go client sorts url.Values keys lexically; google-auth-library 10.9.0 inserts
    // the Gemini CLI fields in its own stable order. Preserve the client-bound wire for each phase
    // instead of pretending one authorization code can be converted across consumers.
    let fields = match phase {
        OAuthPhase::LegacyBootstrap => [
            ("client_id", client_id),
            ("code_verifier", verifier),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
            ("client_secret", client_secret),
        ],
        OAuthPhase::AntigravityFinal | OAuthPhase::DirectAntigravity => [
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("code_verifier", verifier),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ],
    };
    serde_urlencoded::to_string(fields)
        .map(Zeroizing::new)
        .map_err(|_| Failure::Authorization)
}

fn official_userinfo_headers(authorization: &str) -> [(&'static str, &str); 1] {
    // fetchAndCacheUserInfo supplies only Authorization; global fetch adds its own Undici defaults.
    [("Authorization", authorization)]
}

fn classify_google_http_failure(status: u16, detail: &str) -> Failure {
    let detail = detail.to_ascii_lowercase();
    if status == 403
        && detail.contains("cloudcode-pa.googleapis.com")
        && (detail.contains("disabled") || detail.contains("has not been used"))
    {
        Failure::CodeAssistApiDisabled
    } else if matches!(status, 401 | 403) {
        Failure::Authorization
    } else {
        Failure::Temporary
    }
}

fn client_metadata(project: Option<&str>) -> Value {
    let mut value = json!({
        "ideType": "ANTIGRAVITY"
    });
    if let Some(project) = project {
        value["duetProject"] = Value::String(project.to_string());
    }
    value
}

fn legacy_client_metadata_header() -> &'static str {
    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
}

fn antigravity_control_metadata() -> Value {
    json!({
        "ide_type": "ANTIGRAVITY",
        "ide_version": gemini_credential::ANTIGRAVITY_VERSION,
        "ide_name": "antigravity",
    })
}

fn project_from_value(value: Option<&Value>) -> Option<String> {
    let value = value?;
    let project = value
        .as_str()
        .or_else(|| value.get("id").and_then(Value::as_str))?
        .trim();
    valid_identity(project, 512).then(|| project.to_string())
}

fn fresh_uuid_v4() -> Result<String, ()> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|_| ())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Ok(format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ))
}

fn classify_tier_evidence(tier: Option<&Tier>) -> TierEvidenceClass {
    let Some(tier) = tier else {
        return TierEvidenceClass::Absent;
    };
    let tier_id = tier.id.as_deref().unwrap_or_default();
    let tier_name = tier.name.as_deref().unwrap_or_default();
    let id_plan = gemini_credential::supported_plan_for_tier_id(tier_id);
    let name_plan = gemini_credential::supported_plan_for_tier_name(tier_name);
    match (id_plan, name_plan) {
        (Some(id_plan), Some(name_plan)) if id_plan != name_plan => {
            TierEvidenceClass::KnownIdNameConflict
        }
        (Some(_), Some(_)) => TierEvidenceClass::KnownIdAndName,
        (Some(_), None) => TierEvidenceClass::KnownIdNameDrift,
        (None, Some(_)) => TierEvidenceClass::KnownNameOnly,
        (None, None) => TierEvidenceClass::Unknown,
    }
}

fn log_unsupported_plan(reason: &'static str, diagnostic: CodeAssistDiagnostic) {
    eprintln!(
        "[gemini-oauth] unsupported plan shape: reason={reason} {}",
        diagnostic.sanitized()
    );
}

/// Opt-in operator diagnostic. Tier ids and display names are product labels, not identity or
/// secrets, but the default journal deliberately carries only bounded shape classes. Without this
/// escape hatch a genuinely new Google tier can only be identified by shipping a build, so the
/// operator may enable it deliberately, per host, while investigating.
fn log_tier_evidence_if_enabled(loaded: &LoadCodeAssistResponse) {
    if std::env::var("AUTH_BOT_GEMINI_TIER_EVIDENCE").as_deref() != Ok("1") {
        return;
    }
    let render = |label: &str, tier: Option<&Tier>| match tier {
        Some(tier) => format!(
            "{label}={{id={} name={}}}",
            bounded_label(tier.id.as_deref()),
            bounded_label(tier.name.as_deref())
        ),
        None => format!("{label}=absent"),
    };
    let allowed = loaded
        .allowed_tiers
        .iter()
        .map(|tier| {
            format!(
                "{{id={} name={} default={}}}",
                bounded_label(tier.id.as_deref()),
                bounded_label(tier.name.as_deref()),
                tier.is_default
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    eprintln!(
        "[gemini-oauth] raw tier evidence: {} {} allowed=[{}]",
        render("paid", loaded.paid_tier.as_ref()),
        render("current", loaded.current_tier.as_ref()),
        allowed
    );
}

/// Keep an operator label printable and bounded: an upstream string is untrusted input, and a
/// control character or an unbounded blob in the journal is a log-injection primitive.
fn bounded_label(value: Option<&str>) -> String {
    let Some(value) = value else {
        return "<none>".into();
    };
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(96)
        .collect();
    if cleaned.is_empty() {
        "<empty>".into()
    } else {
        cleaned
    }
}

fn classify_plan(tier_id: &str, tier_name: &str, explicitly_paid: bool) -> String {
    if let Some(plan) = gemini_credential::supported_plan_for_tier(tier_id, tier_name) {
        return plan.into();
    }
    let name = tier_name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    if name.contains("workspace") && (name.contains("standard") || name.contains("plus")) {
        "workspace_ai_unsupported".into()
    } else if name.contains("expanded") {
        "ai_expanded_unsupported".into()
    } else if name.contains("plus") || name.contains("premium") {
        "google_ai_plus_unsupported".into()
    } else if explicitly_paid {
        // A newly introduced paid tier is not proof that the private Code Assist transport and
        // quotas are compatible. Future tiers require an explicit review before publication.
        "unknown_paid_unsupported".into()
    } else {
        "individual_free".into()
    }
}

/// Google can expose both `paidTier` and `currentTier`, and their shapes do not change in lock-step:
/// display names drift with marketing, and Antigravity onboarding can leave `currentTier` on a
/// different tier than the purchased `paidTier`. A reviewed tier id is authoritative through both,
/// and the paid entitlement wins when the two reviewed fields disagree. Admission still fails closed
/// when no reviewed evidence exists at all: no familiar substring can promote an unknown future tier.
fn resolve_reported_tier(
    loaded: &LoadCodeAssistResponse,
) -> Result<(String, String, String), Failure> {
    let diagnostic = CodeAssistDiagnostic::from_response(loaded);
    if matches!(diagnostic.paid_tier, TierEvidenceClass::KnownIdNameConflict)
        || matches!(
            diagnostic.current_tier,
            TierEvidenceClass::KnownIdNameConflict
        )
    {
        // Display names are marketing copy that Google rewrites without touching the tier id, so a
        // name that contradicts a reviewed id is drift, not a second entitlement. Resolution below
        // follows the id; the shape is journalled because it is the signal that our reviewed name
        // list has aged.
        eprintln!(
            "[gemini-oauth] reviewed tier id kept over drifted display name: {}",
            diagnostic.sanitized()
        );
    }
    let paid = loaded.paid_tier.as_ref();
    let current = loaded.current_tier.as_ref();
    let paid_plan = paid.and_then(|tier| {
        gemini_credential::supported_plan_for_tier(
            tier.id.as_deref().unwrap_or_default(),
            tier.name.as_deref().unwrap_or_default(),
        )
    });
    let current_plan = current.and_then(|tier| {
        gemini_credential::supported_plan_for_tier(
            tier.id.as_deref().unwrap_or_default(),
            tier.name.as_deref().unwrap_or_default(),
        )
    });
    let (tier, plan) = match (paid_plan, current_plan) {
        (Some(paid_plan), Some(current_plan)) if paid_plan != current_plan => {
            // `paidTier` is the purchased entitlement; `currentTier` is whatever tier the account
            // is onboarded to for this IDE surface, and Antigravity onboarding routinely reports a
            // different one. Rejecting the pair told a seller with a real subscription that no
            // subscription exists, so the paid entitlement wins and the disagreement is journalled.
            eprintln!(
                "[gemini-oauth] paid tier kept over disagreeing current tier: {}",
                diagnostic.sanitized()
            );
            let Some(tier) = paid else {
                log_unsupported_plan("missing_paid_tier_after_resolution", diagnostic);
                return Err(Failure::UnsupportedPlan);
            };
            (tier, paid_plan.to_string())
        }
        (Some(plan), _) => {
            let Some(tier) = paid else {
                log_unsupported_plan("missing_paid_tier_after_resolution", diagnostic);
                return Err(Failure::UnsupportedPlan);
            };
            (tier, plan.to_string())
        }
        (_, Some(plan)) => {
            if paid.is_some() {
                eprintln!(
                    "[gemini-oauth] using reviewed current tier because paid tier shape is unreviewed"
                );
            }
            let Some(tier) = current else {
                log_unsupported_plan("missing_current_tier_after_resolution", diagnostic);
                return Err(Failure::UnsupportedPlan);
            };
            (tier, plan.to_string())
        }
        (None, None) => {
            let Some(tier) = paid.or(current) else {
                log_unsupported_plan("missing_reported_tier", diagnostic);
                return Err(Failure::UnsupportedPlan);
            };
            let tier_id = tier.id.as_deref().unwrap_or_default();
            let tier_name = tier.name.as_deref().unwrap_or_default();
            (tier, classify_plan(tier_id, tier_name, paid.is_some()))
        }
    };
    Ok((
        tier.id.clone().unwrap_or_default(),
        tier.name.clone().unwrap_or_default(),
        plan,
    ))
}

fn supported_paid_plan(plan: &str) -> bool {
    gemini_credential::is_supported_paid_plan(plan)
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

/// Reject a duplicate Antigravity subject before the second consent can invalidate its currently
/// published refresh token. A legacy profile may continue to the one-way migration only through
/// its exact canonical proxy and IPRoyal order.
fn preflight_bootstrap_candidate(
    root: &Path,
    keyring: &CredentialKeyring,
    subject: &str,
    proxy: &str,
    proxy_order_id: i64,
) -> Result<(), Failure> {
    let roster_path = root.join("profiles.json");
    if !roster_path.exists() {
        return Ok(());
    }
    let credentials_dir = root.join("credentials");
    let roster: ProfilesFile =
        serde_json::from_slice(&read_private(&roster_path).map_err(|_| Failure::Storage)?)
            .map_err(|_| Failure::Storage)?;
    let candidate_proxy = normalize_proxy_url(proxy).map_err(|_| Failure::Storage)?;
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut proxies = HashSet::new();
    let mut proxy_orders = HashSet::new();
    for profile in roster.profiles {
        gemini_credential::validate_profile_id(&profile.id).map_err(|_| Failure::Storage)?;
        if !ids.insert(profile.id.clone()) {
            return Err(Failure::Storage);
        }
        let expected_path = credentials_dir.join(format!("{}.json", profile.id));
        if Path::new(&profile.credential_file) != expected_path {
            return Err(Failure::Storage);
        }
        let envelope =
            decode_envelope(&read_private(&expected_path).map_err(|_| Failure::Storage)?)
                .map_err(|_| Failure::Storage)?;
        let existing = keyring
            .open(&profile.id, &envelope)
            .map_err(|_| Failure::Storage)?;
        if !subjects.insert(existing.subject.clone()) {
            return Err(Failure::Storage);
        }
        let existing_proxy = normalize_proxy_url(&existing.proxy).map_err(|_| Failure::Storage)?;
        if !proxies.insert(existing_proxy.clone()) {
            return Err(Failure::Storage);
        }
        if existing.proxy_order_id > 0 && !proxy_orders.insert(existing.proxy_order_id) {
            return Err(Failure::Storage);
        }
        if existing.subject == subject {
            if existing.oauth_kind().map_err(|_| Failure::Storage)? == OAuthKind::Antigravity {
                return Err(Failure::Duplicate);
            }
            if existing_proxy != candidate_proxy
                || (existing.proxy_order_id > 0
                    && proxy_order_id > 0
                    && existing.proxy_order_id != proxy_order_id)
            {
                return Err(Failure::MigrationProxyMismatch);
            }
        } else if existing_proxy == candidate_proxy
            || (proxy_order_id > 0 && existing.proxy_order_id == proxy_order_id)
        {
            return Err(Failure::DuplicateProxy);
        }
    }
    Ok(())
}

fn publish(
    root: &Path,
    keyring: &CredentialKeyring,
    active_key_id: &str,
    mut credential: GeminiCredential,
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
    let mut migration = None;
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
        if !proxies.insert(existing_proxy.clone()) {
            return Err(Failure::Storage);
        }
        if existing.proxy_order_id > 0 && !proxy_orders.insert(existing.proxy_order_id) {
            return Err(Failure::Storage);
        }
        if existing.subject == credential.subject {
            if migration.is_some() {
                return Err(Failure::Storage);
            }
            migration = Some((profile.id.clone(), expected_path, existing, existing_proxy));
        }
    }
    let candidate_proxy = normalize_proxy_url(&credential.proxy).map_err(|_| Failure::Storage)?;
    if let Some((profile_id, credential_path, existing, existing_proxy)) = migration {
        let existing_kind = existing.oauth_kind().map_err(|_| Failure::Storage)?;
        let candidate_kind = credential.oauth_kind().map_err(|_| Failure::Storage)?;
        // Обратный переход на legacy-клиент запрещён: identity подписки не откатываем.
        if candidate_kind != OAuthKind::Antigravity {
            return Err(Failure::Duplicate);
        }
        // Тот же Google subject с уже опубликованным Antigravity-профилем — это НЕ дубликат, а
        // переавторизация. Повторное согласие Google выдаёт новый refresh-токен и аннулирует
        // прежний, поэтому отказ здесь оставлял бы в roster заведомо мёртвый credential: quota
        // identity одна и та же, меняется только материал. Принимаем свежий токен на место старого.
        let reauthorized = existing_kind == OAuthKind::Antigravity;
        if candidate_proxy != existing_proxy {
            return Err(Failure::MigrationProxyMismatch);
        }
        if existing.proxy_order_id > 0 {
            if credential.proxy_order_id > 0 && credential.proxy_order_id != existing.proxy_order_id
            {
                return Err(Failure::MigrationProxyMismatch);
            }
            credential.proxy_order_id = existing.proxy_order_id;
            credential.issued_at = existing.issued_at;
        }
        if !proxies.remove(&existing_proxy) {
            return Err(Failure::Storage);
        }
        if existing.proxy_order_id > 0 && !proxy_orders.remove(&existing.proxy_order_id) {
            return Err(Failure::Storage);
        }
        if proxies.contains(&candidate_proxy) {
            return Err(Failure::DuplicateProxy);
        }
        if credential.proxy_order_id > 0 && proxy_orders.contains(&credential.proxy_order_id) {
            return Err(Failure::DuplicateProxy);
        }
        let envelope = keyring
            .seal(active_key_id, &profile_id, &credential)
            .map_err(|_| Failure::Storage)?;
        let encoded = encode_envelope(&envelope).map_err(|_| Failure::Storage)?;
        atomic_private_replace(&credential_path, &encoded).map_err(|_| Failure::Storage)?;
        return Ok(PublishedProfile {
            id: profile_id,
            plan: credential.plan.clone(),
            has_proxy: !credential.proxy.is_empty(),
            migrated: !reauthorized,
            reauthorized,
        });
    }
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
        migrated: false,
        reauthorized: false,
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
            oauth_client_id: ANTIGRAVITY_CLIENT_ID.into(),
            oauth_client_secret: ANTIGRAVITY_CLIENT_SECRET.into(),
            token_uri: TOKEN_URL.into(),
            subject: subject.into(),
            email: "owner@example.com".into(),
            project_id: "managed-project".into(),
            tier_id: "g1-pro-tier".into(),
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
    fn two_stage_oauth_uses_the_pinned_legacy_then_antigravity_identities() {
        let (root, ring) = fixture();
        let database = root.join("state").join("authbot.db");
        let store = Store::open(database.to_str().unwrap()).unwrap();
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let proxy = "http://user:pass@127.0.0.1:8080";
        let links = begin(&store, &config, 1, proxy, 0).unwrap();
        let authorize = reqwest::Url::parse(&links.authorize_url).unwrap();
        assert!(authorize
            .query_pairs()
            .any(|(name, value)| name == "client_id" && value == LEGACY_CLIENT_ID));
        assert!(authorize
            .query_pairs()
            .any(|(name, value)| { name == "redirect_uri" && value == LEGACY_REDIRECT_URI }));
        assert!(links
            .submit_url
            .starts_with("https://gemini.example/oauth/callback?state="));
        let state = authorize
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        let legacy = store.claim_gemini_oauth(&state).unwrap().unwrap();
        let final_links = begin_antigravity_phase(
            &store,
            &config,
            &legacy,
            "http://user:pass@127.0.0.1:8080/",
            0,
            "google-subject",
        )
        .unwrap();
        let authorize = reqwest::Url::parse(&final_links.authorize_url).unwrap();
        assert!(authorize
            .query_pairs()
            .any(|(name, value)| name == "client_id" && value == ANTIGRAVITY_CLIENT_ID));
        assert!(authorize
            .query_pairs()
            .any(|(name, value)| { name == "redirect_uri" && value == ANTIGRAVITY_REDIRECT_URI }));
        let final_state = authorize
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        let final_session = store.claim_gemini_oauth(&final_state).unwrap().unwrap();
        let final_pending = open_pending_secret(&config, &final_session).unwrap();
        assert_eq!(final_pending.phase, OAuthPhase::AntigravityFinal);
        assert_eq!(final_pending.bootstrap_subject, "google-subject");
        for path in [
            database.clone(),
            PathBuf::from(format!("{}-wal", database.display())),
        ] {
            if let Ok(bytes) = fs::read(path) {
                for private in ["google-subject", "user:pass", "managed-project"] {
                    assert!(!bytes
                        .windows(private.len())
                        .any(|window| window == private.as_bytes()));
                }
            }
        }
        drop(store);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn onboarding_wire_identity_matches_antigravity() {
        assert_eq!(
            antigravity_user_agent(),
            "antigravity/hub/2.2.1 darwin/arm64"
        );
        assert_eq!(
            antigravity_control_user_agent(),
            "antigravity/hub/2.2.1 darwin/arm64 google-api-nodejs-client/10.3.0"
        );
        assert_eq!(
            load_code_assist_request_body(),
            json!({"metadata": {"ideType": "ANTIGRAVITY"}})
        );
        assert_eq!(
            antigravity_control_metadata(),
            json!({
                "ide_type": "ANTIGRAVITY",
                "ide_version": "2.2.1",
                "ide_name": "antigravity"
            })
        );
        assert_eq!(
            onboard_request_body("paid-tier"),
            json!({
                "tier_id": "paid-tier",
                "metadata": {
                    "ide_type": "ANTIGRAVITY",
                    "ide_version": "2.2.1",
                    "ide_name": "antigravity"
                }
            })
        );
    }

    #[test]
    fn legacy_identity_bootstrap_defers_subscription_admission_to_antigravity() {
        assert_eq!(
            post_identity_action(OAuthPhase::LegacyBootstrap),
            PostIdentityAction::StartAntigravityConsent
        );
        for phase in [OAuthPhase::AntigravityFinal, OAuthPhase::DirectAntigravity] {
            assert_eq!(
                post_identity_action(phase),
                PostIdentityAction::ResolveAntigravitySubscription
            );
        }
    }

    #[test]
    fn oauth_token_form_order_matches_each_pinned_client() {
        let form = token_exchange_form(
            OAuthPhase::AntigravityFinal,
            "client id",
            "verifier/value",
            "code+value",
            ANTIGRAVITY_REDIRECT_URI,
            "client-secret",
        )
        .unwrap();
        assert_eq!(
            form.as_str(),
            "client_id=client+id&client_secret=client-secret&code=code%2Bvalue&code_verifier=verifier%2Fvalue&grant_type=authorization_code&redirect_uri=http%3A%2F%2Flocalhost%3A51121%2Foauth-callback"
        );
        let legacy = token_exchange_form(
            OAuthPhase::LegacyBootstrap,
            "client id",
            "verifier/value",
            "code+value",
            LEGACY_REDIRECT_URI,
            "client-secret",
        )
        .unwrap();
        assert_eq!(
            legacy.as_str(),
            "client_id=client+id&code_verifier=verifier%2Fvalue&code=code%2Bvalue&grant_type=authorization_code&redirect_uri=https%3A%2F%2Fcodeassist.google.com%2Fauthcode&client_secret=client-secret"
        );
    }

    #[test]
    fn userinfo_supplies_only_the_official_fetch_authorization_header() {
        assert_eq!(
            official_userinfo_headers("Bearer redacted"),
            [("Authorization", "Bearer redacted")]
        );
    }

    #[test]
    fn oauth_start_persists_only_a_state_bound_encrypted_payload() {
        let (root, ring) = fixture();
        let state_dir = root.join("state");
        let database = state_dir.join("authbot.db");
        let store = Store::open(database.to_str().unwrap()).unwrap();
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let proxy = "http://user:pass@127.0.0.1:8080";
        let links = begin(&store, &config, 42, proxy, 777).unwrap();
        assert!(!links.authorize_url.contains("user:pass"));
        assert!(!links.authorize_url.contains(LEGACY_CLIENT_SECRET));
        assert!(!links.submit_url.contains("user:pass"));
        let url = reqwest::Url::parse(&links.authorize_url).unwrap();
        assert!(url
            .query_pairs()
            .any(|(name, value)| { name == "client_id" && value == LEGACY_CLIENT_ID }));
        let state = url
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        assert!(url
            .query_pairs()
            .any(|(name, value)| { name == "code_challenge_method" && value == "S256" }));
        let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
        assert_eq!(
            active_egress(&store, &config, 42),
            Some((format!("{proxy}/"), 777))
        );
        assert_eq!(store.interrupted_gemini_chats().unwrap(), vec![42]);
        assert!(!session.sealed_payload.contains("user:pass"));
        assert!(!session.sealed_payload.contains(LEGACY_CLIENT_SECRET));
        let envelope: SealedCredential = serde_json::from_str(&session.sealed_payload).unwrap();
        let decrypted = config
            .keyring
            .open_secret(&session.state, &envelope)
            .unwrap();
        let pending: PendingOAuthSecret = serde_json::from_str(decrypted.as_str()).unwrap();
        assert_eq!(pending.proxy, format!("{proxy}/"));
        assert_eq!(pending.proxy_order_id, 777);
        assert_eq!(pending.client_id, LEGACY_CLIENT_ID);
        assert_eq!(pending.client_secret, LEGACY_CLIENT_SECRET);
        assert_eq!(pending.redirect_uri, LEGACY_REDIRECT_URI);
        assert_eq!(pending.phase, OAuthPhase::LegacyBootstrap);
        assert!(pending.bootstrap_subject.is_empty());
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
    fn cancel_fence_restarts_with_fresh_pkce_and_rejects_the_old_generation() {
        let (root, ring) = fixture();
        let database = root.join("state").join("authbot.db");
        let store = Store::open(database.to_str().unwrap()).unwrap();
        store.register_user(42, 42, "seller").unwrap();
        let offer = store
            .create_offer_with_proxy("Google AI Pro", "$20", 999, 42, "seller", "")
            .unwrap();
        store.set_response(offer, 42, "accepted").unwrap();
        assert!(store.claim_offer_payment(offer, 42).unwrap());
        assert!(store.mark_offer_paid(offer, 42).unwrap());
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let links = begin(&store, &config, 42, "http://user:pass@127.0.0.1:8080", 777).unwrap();
        let state = reqwest::Url::parse(&links.authorize_url)
            .unwrap()
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        let session = store.claim_gemini_oauth(&state).unwrap().unwrap();
        assert!(oauth_session_handoff_is_current(&store, &session));
        assert_eq!(
            active_egress(&store, &config, 42),
            Some(("http://user:pass@127.0.0.1:8080/".into(), 777))
        );
        let job = links.job.unwrap();
        let fresh_job = store
            .rewind_handoff_step(42, &job, "gm_wait", "gm_gproxy", Some(("", 777)))
            .unwrap()
            .expect("/cancel rotates the exact seller generation");
        assert!(!oauth_session_handoff_is_current(&store, &session));
        assert!(store.active_gemini_session(42).unwrap().is_none());
        assert_ne!(fresh_job.token, job.token);

        let restarted =
            begin(&store, &config, 42, "http://user:pass@127.0.0.1:8080/", 777).unwrap();
        let restarted_state = reqwest::Url::parse(&restarted.authorize_url)
            .unwrap()
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        assert_ne!(restarted_state, state);
        assert!(store.claim_gemini_oauth(&state).unwrap().is_none());
        let restarted_session = store
            .pending_gemini_session_by_state(&restarted_state)
            .unwrap()
            .expect("fresh PKCE generation is immediately pending");
        assert!(oauth_session_handoff_is_current(&store, &restarted_session));
        assert_ne!(restarted.job.unwrap().token, fresh_job.token);
        // A late old worker cannot move the seller back onto a retry step after the restart.
        assert!(!store
            .set_handoff_state_for_seller_job(
                42,
                session.job.as_ref().unwrap(),
                "gm_gproxy",
                "http://attacker.invalid:8080",
                0,
            )
            .unwrap());
        let _ = fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn claimed_invalid_callback_finishes_outside_the_http_future() {
        let (root, ring) = fixture();
        let store = Arc::new(Store::open(root.join("state/authbot.db").to_str().unwrap()).unwrap());
        let oauth = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let links = begin(&store, &oauth, 42, "http://user:pass@127.0.0.1:8080", 777).unwrap();
        let oauth_state = reqwest::Url::parse(&links.authorize_url)
            .unwrap()
            .query_pairs()
            .find_map(|(name, value)| (name == "state").then(|| value.into_owned()))
            .unwrap();
        let bot_config = Arc::new(BotConfig {
            kimi_roster: None,
            glm_roster: None,
            admins_id: HashSet::new(),
            admins_name: HashSet::new(),
            claude_bin: String::new(),
            claude_config_dir: String::new(),
            database_url: String::new(),
            fleet: String::new(),
            bsc_python: String::new(),
            bsc_script: String::new(),
            iproyal_key: String::new(),
            codex_bin: String::new(),
            codex_homes_dir: String::new(),
            codex_roster: None,
            gemini_dir: root.join("gemini").to_string_lossy().into_owned(),
            gemini_oauth: Some(oauth.clone()),
        });
        let callback = CallbackState {
            bot: Bot::new("unused-test-token"),
            store: store.clone(),
            config: bot_config,
        };

        let response = finish_oauth(&callback, Some(&oauth_state), Some(""), None).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        tokio::time::timeout(Duration::from_secs(1), async {
            while store.active_gemini_session(42).unwrap().is_some() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached terminal task removes the claimed session");
        assert!(!oauth.abort_inflight(42));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn pending_payload_without_redirect_keeps_inflight_hosted_callback_compatible() {
        let pending: PendingOAuthSecret = serde_json::from_value(json!({
            "verifier": "legacy-verifier",
            "proxy": "http://proxy.example:8080/",
            "proxy_order_id": 0,
            "client_id": "legacy.apps.googleusercontent.com",
            "client_secret": "legacy-secret"
        }))
        .unwrap();
        assert!(pending.redirect_uri.is_empty());
        assert_eq!(pending.phase, OAuthPhase::DirectAntigravity);
        assert!(pending.bootstrap_subject.is_empty());
    }

    #[test]
    fn callback_page_is_non_cacheable_and_cannot_load_or_refer() {
        let response = status_page(StatusPage::CheckingSubscription);
        assert_eq!(response.status(), StatusCode::ACCEPTED);
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
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("style-src 'sha256-"));
        assert!(!csp.contains("'unsafe-inline'"));
        let page = page_shell(
            "wait",
            "02",
            "Callback принят",
            "Проверяем подписку",
            "Вкладку можно закрыть.",
            "Результат придёт в Telegram; /cancel начнёт всё заново.",
            2,
            "",
        );
        assert!(page.contains("viewport-fit=cover"));
        assert!(page.contains("/cancel начнёт всё заново"));
        assert!(page.contains("prefers-reduced-motion"));
    }

    #[tokio::test]
    async fn inflight_completion_is_aborted_exactly_once() {
        let (root, ring) = fixture();
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.join("gemini").to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let task = tokio::spawn(std::future::pending::<()>());
        config.register_inflight(42, "A".repeat(43), task.abort_handle());
        assert!(config.abort_inflight(42));
        assert!(!config.abort_inflight(42));
        assert!(task.await.unwrap_err().is_cancelled());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn code_form_accepts_only_generated_state_and_posts_without_query_secrets() {
        let state = "A".repeat(43);
        assert!(valid_oauth_state(&state));
        assert!(!valid_oauth_state("too-short"));
        assert!(!valid_oauth_state(&format!("{}\"", "A".repeat(42))));
        let response = code_form(&state, OAuthPhase::AntigravityFinal);
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store, max-age=0");
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("form-action 'self'"));
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        let legacy = code_form(&state, OAuthPhase::LegacyBootstrap);
        assert_eq!(legacy.status(), StatusCode::OK);
    }

    #[test]
    fn localhost_callback_submission_is_state_bound() {
        let state = "A".repeat(43);
        let callback = format!(
            "http://localhost:51121/oauth-callback?state={state}&code=4%2Fsecret-code&scope=x"
        );
        assert_eq!(
            submitted_authorization_code(&callback, &state, false).as_deref(),
            Some("4/secret-code")
        );
        assert!(submitted_authorization_code(&callback, &"B".repeat(43), false).is_none());
        assert!(submitted_authorization_code(
            &format!("https://localhost:51121/oauth-callback?state={state}&code=x"),
            &state,
            false,
        )
        .is_none());
        assert_eq!(
            submitted_authorization_code("4/direct-code", &state, true).as_deref(),
            Some("4/direct-code")
        );
        assert!(submitted_authorization_code("4/direct-code", &state, false).is_none());
        for ambiguous in [
            format!("http://localhost:51121/oauth-callback?state={state}&state={state}&code=x"),
            format!("http://localhost:51121/oauth-callback?state={state}&code=x&code=y"),
            format!("http://localhost:51121/oauth-callback?state={state}&error=x&error=y"),
        ] {
            assert!(submitted_authorization_code(&ambiguous, &state, false).is_none());
        }
        assert!(submitted_authorization_code("state=x&code=y", &state, true).is_none());
    }

    #[test]
    fn empty_caddy_transport_headers_do_not_hide_present_values_or_create_fake_values() {
        assert_eq!(first_nonempty(None, Some("state")), Some("state"));
        assert_eq!(first_nonempty(Some(""), Some("state")), Some("state"));
        assert_eq!(first_nonempty(Some("query"), Some("header")), Some("query"));
        assert_eq!(first_nonempty(None, Some("")), None);
        assert_eq!(first_nonempty(Some(""), Some("")), None);
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
    fn transport_recovery_reaches_the_observed_gateway_recovery_window_without_bursting() {
        assert_eq!(
            TRANSPORT_RECOVERY_DELAYS,
            [
                Duration::from_secs(2),
                Duration::from_secs(5),
                Duration::from_secs(10),
                Duration::from_secs(20),
            ]
        );
        assert_eq!(
            TRANSPORT_RECOVERY_DELAYS.iter().sum::<Duration>(),
            Duration::from_secs(37)
        );
    }

    #[test]
    fn generation_acceptance_surfaces_are_ordered_and_access_failures_stay_actionable() {
        assert_eq!(
            GENERATION_PROBE_SURFACES.map(|(_, host)| host),
            [CODE_ASSIST_SANDBOX_URL, CODE_ASSIST_PROD_URL],
            "the reviewed sandbox surface stays first; production is the fallback the engine serves"
        );
        let disabled = json!({
            "error": {
                "code": 403,
                "status": "PERMISSION_DENIED",
                "message": "Cloud Code Private API has not been used in project 123 before or it is disabled. Enable cloudcode-pa.googleapis.com then retry.",
            }
        });
        assert_eq!(
            classify_generation_failure(&serde_json::to_vec(&disabled).unwrap()),
            Some(Failure::CodeAssistApiDisabled)
        );
        let plain = json!({"error": {"code": 403, "status": "PERMISSION_DENIED"}});
        assert_eq!(
            classify_generation_failure(&serde_json::to_vec(&plain).unwrap()),
            None,
            "a generic access rejection must not claim the private API is disabled"
        );
        // Observed in production on 2026-08-03: an account with a live g1-pro-tier subscription is
        // refused generation on every surface until Google's own account verification is done.
        let validation = json!({
            "error": {
                "code": 403,
                "status": "PERMISSION_DENIED",
                "message": "Verify your account to continue.",
                "details": [{"@type": "type.googleapis.com/google.rpc.ErrorInfo", "reason": "VALIDATION_REQUIRED"}],
            }
        });
        assert_eq!(
            classify_generation_failure(&serde_json::to_vec(&validation).unwrap()),
            Some(Failure::AccountValidationRequired)
        );
        let message_only = json!({
            "error": {"code": 403, "status": "PERMISSION_DENIED", "message": "Verify your account to continue."}
        });
        assert_eq!(
            classify_generation_failure(&serde_json::to_vec(&message_only).unwrap()),
            Some(Failure::AccountValidationRequired),
            "the reason field is not guaranteed; the exact message alone is enough evidence"
        );
        for message in [
            Failure::AccountValidationRequired.public_message(),
            Failure::AccountValidationRequired.fixed_proxy_message(),
        ] {
            assert!(
                message.contains("повторить"),
                "the seller still needs the exact command to resume after verifying"
            );
            assert!(
                !message.contains("Подожди немного"),
                "waiting never clears an account verification requirement"
            );
        }
        assert_eq!(
            Failure::AccountValidationRequired.code(),
            "account_validation_required"
        );
        assert_eq!(bounded_label(None), "<none>");
        assert_eq!(bounded_label(Some("")), "<empty>");
        assert_eq!(
            bounded_label(Some("PERMISSION\u{7}_DENIED")),
            "PERMISSION_DENIED"
        );
        assert_eq!(bounded_label(Some(&"a".repeat(200))).len(), 96);
    }

    #[test]
    fn generation_acceptance_requires_a_wrapped_candidate_and_authoritative_usage() {
        let body = generation_probe_body(
            "managed-project",
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        );
        assert_eq!(body["model"], GENERATION_PROBE_MODEL);
        assert_eq!(body["project"], "managed-project");
        assert_eq!(body["request"]["generationConfig"]["maxOutputTokens"], 8);
        assert_eq!(body["requestType"], "agent");
        assert!(body["requestId"].as_str().unwrap().starts_with("agent-"));

        let accepted = json!({
            "response": {
                "candidates": [{"content": {"parts": [{"text": "OK"}]}}],
                "usageMetadata": {
                    "promptTokenCount": 4,
                    "candidatesTokenCount": 1,
                    "totalTokenCount": 5
                }
            }
        });
        assert!(
            validate_generation_probe_response(200, &serde_json::to_vec(&accepted).unwrap())
                .is_ok()
        );
        for (status, rejected) in [
            (503, accepted.clone()),
            (200, json!({"response": {"candidates": []}})),
            (
                200,
                json!({"response": {"candidates": [{}], "usageMetadata": {}}}),
            ),
            (200, json!({"notResponse": {}})),
        ] {
            assert_eq!(
                validate_generation_probe_response(status, &serde_json::to_vec(&rejected).unwrap()),
                Err(Failure::GenerationUnavailable)
            );
        }
        assert_eq!(
            validate_generation_probe_response(200, b"not-json"),
            Err(Failure::GenerationUnavailable)
        );
        assert!(validate_final_subject(
            OAuthPhase::AntigravityFinal,
            "same-subject",
            "same-subject"
        )
        .is_ok());
        assert_eq!(
            validate_final_subject(
                OAuthPhase::AntigravityFinal,
                "other-subject",
                "same-subject"
            ),
            Err(Failure::AccountMismatch)
        );
    }

    #[test]
    fn plan_detection_distinguishes_supported_subscriptions() {
        assert_eq!(
            classify_plan(
                "g1-pro-tier",
                "Gemini Code Assist in Google One AI Pro",
                true
            ),
            "google_ai_pro"
        );
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
        assert_eq!(
            classify_plan("future-pro", "Future Pro Trial", true),
            "unknown_paid_unsupported"
        );
        // An unreviewed id no longer suppresses an exact reviewed display name: Google introduces
        // new tier ids for the same product, and rejecting the pair blocked live subscriptions.
        assert_eq!(
            classify_plan(
                "future-pro-tier",
                "Gemini Code Assist in Google One AI Pro",
                true
            ),
            "google_ai_pro"
        );
        assert!(!supported_paid_plan("google_ai_plus_unsupported"));
        assert!(!supported_paid_plan("unknown_paid_unsupported"));
    }

    #[test]
    fn reported_tier_prefers_reviewed_ids_and_the_paid_entitlement() {
        let pro = Tier {
            id: Some("g1-pro-tier".into()),
            name: Some("Gemini Code Assist in Google One AI Pro".into()),
            is_default: false,
        };
        let ultra = Tier {
            id: Some("g1-ultra-tier".into()),
            name: Some("Gemini Code Assist in Google One AI Ultra".into()),
            is_default: false,
        };
        let drifted_paid = Tier {
            id: Some("new-paid-shape".into()),
            name: Some("New Paid Shape".into()),
            is_default: false,
        };
        let renamed_pro = Tier {
            id: Some("g1-pro-tier".into()),
            name: Some("Google AI Pro renamed after purchase".into()),
            is_default: false,
        };
        let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
            paid_tier: Some(renamed_pro),
            ..LoadCodeAssistResponse::default()
        })
        .unwrap();
        assert_eq!(resolved.0, "g1-pro-tier");
        assert_eq!(resolved.2, "google_ai_pro");

        let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
            current_tier: Some(pro.clone()),
            paid_tier: Some(drifted_paid.clone()),
            ..LoadCodeAssistResponse::default()
        })
        .unwrap();
        assert_eq!(resolved.0, "g1-pro-tier");
        assert_eq!(resolved.2, "google_ai_pro");

        // A reviewed id survives a display name that maps to another reviewed product: the name is
        // marketing copy, the id is the stable contract.
        let conflicting_name = Tier {
            id: Some("g1-pro-tier".into()),
            name: Some("Google AI Ultra".into()),
            is_default: false,
        };
        let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
            paid_tier: Some(conflicting_name),
            ..LoadCodeAssistResponse::default()
        })
        .unwrap();
        assert_eq!(resolved.2, "google_ai_pro");

        // Antigravity onboarding can leave `currentTier` on another product while the account
        // really carries the paid one; the purchased entitlement decides.
        let resolved = resolve_reported_tier(&LoadCodeAssistResponse {
            current_tier: Some(ultra),
            paid_tier: Some(pro),
            ..LoadCodeAssistResponse::default()
        })
        .unwrap();
        assert_eq!(resolved.0, "g1-pro-tier");
        assert_eq!(resolved.2, "google_ai_pro");

        // Nothing reviewed anywhere still fails closed.
        let unsupported = resolve_reported_tier(&LoadCodeAssistResponse {
            paid_tier: Some(drifted_paid),
            ..LoadCodeAssistResponse::default()
        })
        .unwrap();
        assert_eq!(unsupported.2, "unknown_paid_unsupported");
        assert!(!supported_paid_plan(&unsupported.2));
    }

    #[test]
    fn unsupported_plan_diagnostic_is_structural_and_secret_free() {
        let raw_project = "private-project-123";
        let raw_tier_id = "private-future-tier";
        let raw_tier_name = "Private Future Plan";
        let loaded = LoadCodeAssistResponse {
            current_tier: Some(Tier {
                id: Some(raw_tier_id.into()),
                name: Some(raw_tier_name.into()),
                is_default: false,
            }),
            paid_tier: Some(Tier {
                id: Some("g1-pro-tier".into()),
                name: Some("Renamed paid display".into()),
                is_default: false,
            }),
            allowed_tiers: vec![Tier::default(), Tier::default()],
            cloudaicompanion_project: Some(json!(raw_project)),
        };
        let diagnostic = CodeAssistDiagnostic::from_response(&loaded).sanitized();
        assert_eq!(
            diagnostic,
            "project=present paid=known_id_name_drift current=unknown allowed_tiers=2"
        );
        for private_value in [
            raw_project,
            raw_tier_id,
            raw_tier_name,
            "Renamed paid display",
        ] {
            assert!(!diagnostic.contains(private_value));
        }
    }

    #[test]
    fn disabled_cloud_code_api_is_actionable_instead_of_generic_auth_failure() {
        let detail = "Cloud Code Private API has not been used in project 123 before or it is disabled. Enable cloudcode-pa.googleapis.com then retry.";
        assert_eq!(
            classify_google_http_failure(403, detail),
            Failure::CodeAssistApiDisabled
        );
        assert!(Failure::CodeAssistApiDisabled
            .public_message()
            .contains("администратор проверит причину"));
        assert!(Failure::Temporary
            .fixed_proxy_message()
            .contains("менять его не нужно"));
        assert!(!Failure::Temporary
            .fixed_proxy_message()
            .contains("пришли прокси"));
        assert!(Failure::TransportUnavailable
            .fixed_proxy_message()
            .contains("CONNECT/TLS"));
        assert_ne!(
            Failure::TransportUnavailable.code(),
            Failure::Temporary.code()
        );
        for failure in [
            Failure::Authorization,
            Failure::CodeAssistApiDisabled,
            Failure::TransportUnavailable,
            Failure::Temporary,
            Failure::UnsupportedPlan,
            Failure::AccountMismatch,
            Failure::GenerationUnavailable,
            Failure::Duplicate,
            Failure::DuplicateProxy,
            Failure::MigrationProxyMismatch,
            Failure::Storage,
        ] {
            for internal_term in [
                "OAuth-клиент",
                "Cloud API",
                "consumer project",
                "managed project",
                "roster",
                "Client ID",
                "Client secret",
            ] {
                assert!(
                    !failure.public_message().contains(internal_term),
                    "seller error contains internal term {internal_term}"
                );
            }
        }
        assert_eq!(
            classify_google_http_failure(403, "permission denied"),
            Failure::Authorization
        );
        assert_eq!(
            classify_google_http_failure(500, detail),
            Failure::Temporary
        );
    }

    #[test]
    fn legacy_preflight_blocks_antigravity_duplicates_before_the_second_consent() {
        let (root, ring) = fixture();
        publish(&root, &ring, "current", credential("existing-subject")).unwrap();
        assert_eq!(
            preflight_bootstrap_candidate(
                &root,
                &ring,
                "existing-subject",
                "http://user:pass@127.0.0.1:8080/",
                42,
            ),
            Err(Failure::Duplicate)
        );
        assert_eq!(
            preflight_bootstrap_candidate(
                &root,
                &ring,
                "different-subject",
                "http://user:pass@127.0.0.1:8080/",
                42,
            ),
            Err(Failure::DuplicateProxy)
        );
        let _ = fs::remove_dir_all(root);

        let (root, ring) = fixture();
        let mut legacy = credential("legacy-subject");
        legacy.oauth_client_id = LEGACY_CLIENT_ID.into();
        legacy.oauth_client_secret = LEGACY_CLIENT_SECRET.into();
        publish(&root, &ring, "current", legacy).unwrap();
        assert!(preflight_bootstrap_candidate(
            &root,
            &ring,
            "legacy-subject",
            "http://user:pass@127.0.0.1:8080/",
            42,
        )
        .is_ok());
        assert_eq!(
            preflight_bootstrap_candidate(
                &root,
                &ring,
                "legacy-subject",
                "http://user:pass@127.0.0.2:8080/",
                42,
            ),
            Err(Failure::MigrationProxyMismatch)
        );
        let _ = fs::remove_dir_all(root);
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
        // Тот же subject через тот же прокси — переавторизация, а не дубликат: свежее согласие
        // Google уже аннулировало прежний refresh-токен, поэтому отказ оставил бы в roster
        // заведомо мёртвый credential.
        let reauthorized = publish(&root, &ring, "current", credential("subject-1")).unwrap();
        assert_eq!(reauthorized.id, first.id);
        assert!(reauthorized.reauthorized);
        assert!(!reauthorized.migrated);
        assert!(matches!(
            publish(&root, &ring, "current", credential("subject-2")),
            Err(Failure::DuplicateProxy)
        ));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn publication_migrates_legacy_profile_in_place_to_antigravity() {
        let (root, ring) = fixture();
        let mut legacy = credential("migration-subject");
        legacy.oauth_client_id = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID.into();
        legacy.oauth_client_secret = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET.into();
        let published = publish(&root, &ring, "current", legacy).unwrap();
        assert!(!published.migrated);

        let roster_path = root.join("profiles.json");
        let roster_before = fs::read(&roster_path).unwrap();
        let credential_path = root
            .join("credentials")
            .join(format!("{}.json", published.id));

        let mut antigravity = credential("migration-subject");
        antigravity.proxy = "http://user:pass@127.0.0.1:8080/".into();
        antigravity.proxy_order_id = 0;
        antigravity.issued_at = 999;
        antigravity.access_token = "new-access-token-value".into();
        antigravity.refresh_token = "new-refresh-token-value".into();
        let migrated = publish(&root, &ring, "current", antigravity).unwrap();

        assert!(migrated.migrated);
        assert_eq!(migrated.id, published.id);
        assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
        let envelope = decode_envelope(&fs::read(&credential_path).unwrap()).unwrap();
        let opened = ring.open(&migrated.id, &envelope).unwrap();
        assert_eq!(opened.oauth_kind().unwrap(), OAuthKind::Antigravity);
        assert_eq!(opened.proxy_order_id, 42);
        assert_eq!(opened.issued_at, 100);
        assert_eq!(opened.access_token, "new-access-token-value");
        assert_eq!(opened.refresh_token, "new-refresh-token-value");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    /// Повторное согласие того же аккаунта заменяет материал на месте: id профиля, roster и
    /// quota identity сохраняются, меняется только конверт. Отказ здесь оставлял бы подписку
    /// мёртвой — Google аннулирует прежний refresh-токен ещё на экране согласия.
    fn publication_reauthorizes_an_existing_antigravity_profile_in_place() {
        let (root, ring) = fixture();
        let published =
            publish(&root, &ring, "current", credential("antigravity-duplicate")).unwrap();
        let roster_path = root.join("profiles.json");
        let credential_path = root
            .join("credentials")
            .join(format!("{}.json", published.id));
        let roster_before = fs::read(&roster_path).unwrap();
        let credential_before = fs::read(&credential_path).unwrap();

        let mut duplicate = credential("antigravity-duplicate");
        duplicate.access_token = "replacement-access-token".into();
        duplicate.refresh_token = "replacement-refresh-token".into();
        let reauthorized = publish(&root, &ring, "current", duplicate).unwrap();
        assert_eq!(reauthorized.id, published.id);
        assert!(reauthorized.reauthorized);
        // Roster не меняется: профиль тот же, подменён только запечатанный материал.
        assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
        assert_ne!(fs::read(&credential_path).unwrap(), credential_before);
        let opened = ring
            .open(
                &reauthorized.id,
                &decode_envelope(&fs::read(&credential_path).unwrap()).unwrap(),
            )
            .unwrap();
        assert_eq!(opened.refresh_token, "replacement-refresh-token");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn publication_rejects_legacy_migration_through_a_different_proxy() {
        let (root, ring) = fixture();
        let mut legacy = credential("proxy-mismatch-subject");
        legacy.oauth_client_id = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID.into();
        legacy.oauth_client_secret = gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET.into();
        let published = publish(&root, &ring, "current", legacy).unwrap();
        let roster_path = root.join("profiles.json");
        let credential_path = root
            .join("credentials")
            .join(format!("{}.json", published.id));
        let roster_before = fs::read(&roster_path).unwrap();
        let credential_before = fs::read(&credential_path).unwrap();

        let mut migration = credential("proxy-mismatch-subject");
        migration.proxy = "http://user:pass@127.0.0.2:8080".into();
        assert!(matches!(
            publish(&root, &ring, "current", migration),
            Err(Failure::MigrationProxyMismatch)
        ));
        assert_eq!(fs::read(&roster_path).unwrap(), roster_before);
        assert_eq!(fs::read(&credential_path).unwrap(), credential_before);
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

    #[test]
    fn operator_proxy_replacement_is_atomic_secret_safe_and_reversible() {
        let (root, ring) = fixture();
        let published = publish(&root, &ring, "current", credential("replace-subject")).unwrap();
        let credential_path = root
            .join("credentials")
            .join(format!("{}.json", published.id));
        let rollback_path = proxy_rollback_path(&root.join("credentials"), &published.id);
        let original = fs::read(&credential_path).unwrap();
        let roster = fs::read(root.join("profiles.json")).unwrap();
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();
        let replacement = "http://other:replacement-secret@127.0.0.2:9000";

        config
            .stage_proxy_replacement(&published.id, replacement)
            .unwrap();
        assert_eq!(fs::read(root.join("profiles.json")).unwrap(), roster);
        assert_eq!(fs::read(&rollback_path).unwrap(), original);
        assert_eq!(
            fs::symlink_metadata(&rollback_path)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let staged_bytes = fs::read(&credential_path).unwrap();
        let staged_text = String::from_utf8(staged_bytes.clone()).unwrap();
        assert!(!staged_text.contains("replacement-secret"));
        let staged = config
            .keyring
            .open(&published.id, &decode_envelope(&staged_bytes).unwrap())
            .unwrap();
        assert_eq!(
            staged.proxy,
            gemini_credential::normalize_proxy_url(replacement).unwrap()
        );
        assert_eq!(staged.proxy_order_id, 0);
        assert!(config
            .stage_proxy_replacement(&published.id, replacement)
            .is_err());

        config.rollback_proxy_replacement(&published.id).unwrap();
        assert_eq!(fs::read(&credential_path).unwrap(), original);
        assert!(!rollback_path.exists());

        config
            .stage_proxy_replacement(&published.id, replacement)
            .unwrap();
        let committed = fs::read(&credential_path).unwrap();
        config.commit_proxy_replacement(&published.id).unwrap();
        assert_eq!(fs::read(&credential_path).unwrap(), committed);
        assert!(!rollback_path.exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn operator_proxy_replacement_rejects_another_profiles_egress() {
        let (root, ring) = fixture();
        let first = publish(&root, &ring, "current", credential("replace-first")).unwrap();
        let mut second_credential = credential("replace-second");
        second_credential.proxy = "http://second:secret@127.0.0.2:9000".into();
        second_credential.proxy_order_id = 43;
        let second = publish(&root, &ring, "current", second_credential).unwrap();
        let first_path = root.join("credentials").join(format!("{}.json", first.id));
        let before = fs::read(&first_path).unwrap();
        let config = Config::new(
            "https://gemini.example/oauth/callback".into(),
            "127.0.0.1:8796".parse().unwrap(),
            root.to_string_lossy().into_owned(),
            ring,
            "current".into(),
        )
        .unwrap();

        assert!(config
            .stage_proxy_replacement(&first.id, "http://second:secret@127.0.0.2:9000")
            .is_err());
        assert_eq!(fs::read(first_path).unwrap(), before);
        assert!(!proxy_rollback_path(&root.join("credentials"), &first.id).exists());
        assert!(root
            .join("credentials")
            .join(format!("{}.json", second.id))
            .exists());
        let _ = fs::remove_dir_all(root);
    }
}
