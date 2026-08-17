//! Single-consent Google OAuth producer for paid Antigravity-backed Gemini subscriptions.
//!
//! One Antigravity consent establishes everything admission needs: verified userinfo, the Google
//! subject, the exact tier/project and one real generation probe. Browser authorization and the
//! HTTPS code-entry form are state+PKCE protected. Only the encrypted Antigravity credential is
//! published; account email, Google subject, refresh tokens, authenticated proxy and PKCE material
//! never enter the roster, Telegram messages, filenames or logs. The legacy Gemini CLI bootstrap
//! phase is retired — its types survive only so a callback already in flight across a deploy can
//! still finish.

use crate::db::{GeminiOAuthSession, GeminiPendingVerification, SellerJobRef, Store};
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
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

const AUTHORIZE_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = gemini_credential::GEMINI_OFFICIAL_TOKEN_URI;
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
/// Every Code Assist call — discovery, onboarding and the acceptance generation — walks this one
/// ordered list, and its origins come from `gemini_credential` so Auth Bot and the engine cannot
/// drift apart again. They already had: acceptance probed the legacy host while the pool served
/// from the runtime origin, so a live Google AI Pro subscription was refused on a host whose quota
/// it never needed, and the seller was told their subscription had none.
const CODE_ASSIST_SURFACES: [(&str, &str); 3] = [
    ("runtime", gemini_credential::CODE_ASSIST_RUNTIME_ORIGIN),
    ("daily", gemini_credential::CODE_ASSIST_DAILY_ORIGIN),
    ("legacy", gemini_credential::CODE_ASSIST_LEGACY_ORIGIN),
];
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
    /// The only phase new handoffs create: one Antigravity consent that is itself the admission.
    #[default]
    DirectAntigravity,
    /// Retired. Kept so a legacy-bootstrap session sealed by an older binary can still complete
    /// across a deploy instead of stranding a seller mid-flow.
    LegacyBootstrap,
    /// Retired second phase of the former two-stage flow; same deploy-overlap compatibility.
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

#[derive(Clone, PartialEq, Eq)]
pub struct LifecycleProfile {
    pub profile_id: String,
    pub account_email: String,
    pub order_id: i64,
    pub issued_at: i64,
    pub canonical_plan: String,
    pub canonical_ip: Option<std::net::IpAddr>,
}

/// Backward-compatible name for the projection consumed by the existing proxy-admin integration.
pub type IproyalLease = LifecycleProfile;

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

    /// Read every sealed roster profile, including its full account email, without exposing Google
    /// subject, project, tokens or proxy credentials. Only a literal host from the canonical proxy
    /// URL is projected; no DNS occurs.
    pub fn lifecycle_profiles(&self) -> anyhow::Result<Vec<LifecycleProfile>> {
        let roster_path = self.root.join("profiles.json");
        if !roster_path.exists() {
            return Ok(Vec::new());
        }
        let credentials_dir = self.root.join("credentials");
        let roster: ProfilesFile = serde_json::from_slice(&read_private(&roster_path)?)
            .context("parse Gemini roster for proxy lifecycle")?;
        let mut ids = HashSet::new();
        let mut subjects = HashSet::new();
        let mut exact_managed_bindings = HashSet::new();
        let mut profiles = Vec::new();
        for profile in roster.profiles {
            gemini_credential::validate_profile_id(&profile.id)?;
            if !ids.insert(profile.id.clone()) {
                bail!("duplicate Gemini profile id in proxy lifecycle");
            }
            let expected = credentials_dir.join(format!("{}.json", profile.id));
            if Path::new(&profile.credential_file) != expected {
                bail!("Gemini credential path is outside the lifecycle roster layout");
            }
            let envelope = decode_envelope(&read_private(&expected)?)?;
            let credential = self.keyring.open(&profile.id, &envelope)?;
            credential.validate()?;
            if !subjects.insert(credential.subject.clone()) {
                bail!("duplicate Gemini subscription in proxy lifecycle");
            }
            let canonical = gemini_credential::normalize_proxy_url(&credential.proxy)?;
            let canonical_ip = canonical_proxy_ip(&canonical)?;
            if credential.proxy_order_id > 0
                && canonical_ip.is_some()
                && !exact_managed_bindings.insert((credential.proxy_order_id, canonical_ip))
            {
                bail!("ambiguous duplicate Gemini managed order and proxy IP");
            }
            profiles.push(LifecycleProfile {
                profile_id: profile.id,
                account_email: credential.email.clone(),
                order_id: credential.proxy_order_id,
                issued_at: credential.issued_at,
                canonical_plan: credential.plan.clone(),
                canonical_ip,
            });
        }
        Ok(profiles)
    }

    /// Preserve the existing integration contract: only managed profiles with a literal IP can be
    /// bound automatically. The complete, order-zero-inclusive view is [`Self::lifecycle_profiles`].
    pub fn iproyal_leases(&self) -> anyhow::Result<Vec<IproyalLease>> {
        Ok(self
            .lifecycle_profiles()?
            .into_iter()
            .filter(|profile| profile.order_id > 0 && profile.canonical_ip.is_some())
            .collect())
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

fn canonical_proxy_ip(proxy: &str) -> anyhow::Result<Option<std::net::IpAddr>> {
    Ok(reqwest::Url::parse(proxy)
        .context("parse canonical Gemini proxy URL")?
        .host_str()
        .map(|host| host.trim_matches(['[', ']']))
        .and_then(|host| host.parse().ok()))
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

/// Create the restart-safe, one-use PKCE transaction for the Antigravity consent that actually
/// produces the published subscription.
///
/// The former legacy Gemini CLI bootstrap is gone: it cost the seller a second Google consent and
/// a second code-entry form while proving nothing that this consent does not prove on its own —
/// verified userinfo, subject, tier, project and one real generation are all established here, and
/// `publish` is the authority on duplicate subjects and proxies. Fewer `select_account consent`
/// screens is also fewer chances to confirm the wrong account in one browser profile.
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
        OAuthPhase::DirectAntigravity,
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
    elog::info("authbot", format!("[gemini-oauth] chat={} proxy_order={} started Antigravity consent", chat_id, proxy_order_id));
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
    elog::info("authbot", format!("[gemini-oauth] chat={} proxy_order={} legacy bootstrap passed; started Antigravity final phase", previous.chat_id, proxy_order_id));
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
    let loopback = url.scheme() == "http"
        && url.host_str() == Some("localhost")
        && url.port_or_known_default() == Some(51_121);
    // Google renders the code on its own hosted callback page for clients that ask for that redirect
    // instead of the loopback one. We keep asking for loopback, so this address is not one we
    // produced — but a seller who copies the address bar rather than the code otherwise gets an
    // opaque `authorization` failure with nothing to act on. Accepting the shape costs nothing:
    // what authorises a submission is the state binding checked below, and it is identical either
    // way, so a code minted for any other transaction is still refused.
    let hosted = url.scheme() == "https"
        && url.host_str() == Some("antigravity.google")
        && url.port().is_none();
    if !(loopback || hosted)
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
        elog::error("authbot", format!("[gemini-oauth] chat={} rejected stale seller generation before code exchange", session.chat_id));
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
            fail_callback(&state, &session, failure, None).await;
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
            elog::error("authbot", format!("[gemini-oauth] chat={} detached completion panicked; restarting the exact handoff generation", chat_id));
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
        fail_callback(state, session, Failure::Interrupted, None).await;
        return;
    };
    let exchange_redirect = if pending.redirect_uri.is_empty() {
        oauth.redirect_uri.as_str()
    } else {
        pending.redirect_uri.as_str()
    };
    let mut verification_url = None;
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
        &mut verification_url,
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
                        .send(
                            session.chat_id,
                            &format!(
                                "✅ <b>Gemini CLI инициализировал подписку.</b> Теперь, не меняя Google-аккаунт, антидетект-профиль и прокси, выдай финальный доступ по официальной ссылке: <a href=\"{}\">авторизация Google для Antigravity</a>.",
                                crate::bot::esc(&links.authorize_url)
                            ),
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
                    elog::error("authbot", format!("[gemini-oauth] chat={} Antigravity phase became stale immediately after transition", session.chat_id));
                }
                Err(_) => {
                    fail_callback(state, &session, Failure::Storage, None).await;
                }
            }
        }
        Ok(Completion::Published(profile, _terminal_guard)) => {
            let _ = state.store.finish_gemini_oauth(&session.state);
            announce_publication(
                &state.bot,
                &state.store,
                &state.config,
                session.chat_id,
                session.job.clone(),
                &profile,
            )
            .await;
        }
        Err(failure) => {
            fail_callback(state, &session, failure, verification_url.as_deref()).await;
        }
    }
}

fn oauth_session_handoff_is_current(store: &Store, session: &GeminiOAuthSession) -> bool {
    handoff_is_current(store, session.chat_id, session.job.as_ref())
}

/// A handoff with no seller job is an admin/self-serve connection and is always current; a job-bound
/// one must still be the exact activation generation that started this attempt.
fn handoff_is_current(store: &Store, chat_id: i64, job: Option<&SellerJobRef>) -> bool {
    job.is_none()
        || crate::bot::seller_handoff_is_current(
            store,
            chat_id,
            job,
            crate::bot::HandoffKind::Gemini,
        )
}

/// Announce a published profile and settle the seller's job. Shared by the OAuth callback and the
/// post-verification retry so both paths pay out through exactly one code path.
pub(crate) async fn announce_publication(
    bot: &Bot,
    store: &Arc<Store>,
    config: &Arc<BotConfig>,
    chat_id: i64,
    job: Option<SellerJobRef>,
    profile: &PublishedProfile,
) {
    let binding = match (profile.proxy_order_id, profile.canonical_ip) {
        (0, _) => Ok(()),
        (_, Some(allocation_ip)) => store
            .upsert_proxy_binding_allocation(
                "gemini",
                &profile.id,
                profile.proxy_order_id,
                &allocation_ip.to_string(),
                profile.issued_at,
                crate::db::ProxyAuthorityStatus::Local,
            )
            .map(|_| ()),
        (_, None) => Err(anyhow::anyhow!(
            "managed Gemini proxy host is not a literal allocation IP"
        )),
    };
    if binding.is_err() {
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} profile {} is published but lifecycle binding needs reconciliation", profile.id));
        for admin in &config.admins_id {
            let _ = bot
                .send(
                    *admin,
                    "⚠️ Gemini опубликован в roster, но lifecycle binding не записан. Сделка оставлена незавершённой; публикацию не откатывать, требуется reconciliation.",
                )
                .await;
        }
        return;
    }
    if handoff_is_current(store, chat_id, job.as_ref()) {
        let seller_outcome = if profile.reauthorized {
            "переподключена"
        } else if profile.migrated {
            "переведена на Antigravity"
        } else {
            "подключена"
        };
        let _ = bot
            .send(
                chat_id,
                &format!(
                    "✅ <b>Gemini-подписка {seller_outcome}.</b> План: <b>{}</b>. Профиль <code>{}</code> опубликован в отдельном Gemini-пуле.",
                    plan_label(&profile.plan),
                    profile.id
                ),
            )
            .await;
        for admin in &config.admins_id {
            let admin_outcome = if profile.reauthorized {
                "переавторизован; прежний токен был аннулирован Google, конверт заменён атомарно"
            } else if profile.migrated {
                "переведён на Antigravity; профиль обновлён атомарно"
            } else {
                "получен; аккаунт добавлен в пул"
            };
            let _ = bot
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
        bot,
        store,
        config,
        chat_id,
        job,
        crate::bot::HandoffKind::Gemini,
    )
    .await;
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
            "Подключение подписки",
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
        "Не меняйте Google-аккаунт, профиль браузера или прокси до конца проверки.",
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

async fn fail_callback(
    state: &CallbackState,
    session: &GeminiOAuthSession,
    failure: Failure,
    verification_url: Option<&str>,
) {
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
        elog::error("authbot", format!("[gemini-oauth] chat={} stale callback failed: {} (seller state unchanged)", session.chat_id,
            failure.code()));
        return;
    }
    // The authorization code and its encrypted PKCE transaction cannot be reused after this point;
    // the proxy remains available for a new generation or can be explicitly replaced by its owner.
    let base = if accepts_proxy_input {
        failure.public_message()
    } else {
        failure.fixed_proxy_message()
    };
    // The link is account-bound and must be opened from the same browser profile and egress as the
    // account itself, so it is offered as copyable text rather than a tappable link that Telegram
    // would open in its own in-app browser.
    // Google's own instructions, already rendered and escaped. They are forwarded whatever the
    // surrounding verdict classified as: a quota refusal answers before the validation gate is even
    // reached and carries no `reason`, yet the account is still held and these are still the only
    // way past it.
    let message = match verification_url {
        Some(guidance) => format!("{base}\n\n{guidance}"),
        _ => base.to_string(),
    };
    // The button exists only while the account is actually parked: offering a retry we cannot honour
    // would send the seller back into two consents believing one press was enough.
    let parked = failure == Failure::AccountValidationRequired
        && matches!(
            state.store.gemini_verification_is_parked(session.chat_id),
            Ok(true)
        );
    if parked {
        let _ = state
            .bot
            .send_kb(
                session.chat_id,
                &message,
                Some(&crate::bot::gemini_verified_kb()),
            )
            .await;
    } else {
        let _ = state.bot.send(session.chat_id, &message).await;
    }
    if failure.operator_action_required() {
        for admin in &state.config.admins_id {
            let _ = state.bot.send(*admin, failure.operator_message()).await;
        }
    }
    elog::error("authbot", format!("[gemini-oauth] chat={} callback failed: {}{}", session.chat_id,
        failure.code(),
        if failure.operator_action_required() {
            " (operator notified)"
        } else {
            ""
        }));
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
pub(crate) enum Failure {
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
            Self::AccountValidationRequired => "❌ Google держит сам аккаунт на проверке и не выполняет генерацию. Это отдельная проверка — обычный Gemini на сайте при этом может работать, и повтор без её прохождения ничего не изменит.

Ниже — то, что Google сообщает по этому аккаунту: его текст и его ссылки, дословно. Открывай их СТРОГО в том же профиле антидетект-браузера и через тот же прокси, где выдавал согласие, — с другого устройства или IP проверка не засчитается. Дальше Google проведёт по шагам сам.

Прошёл проверку — нажми кнопку ниже, бот перепроверит немедленно. Кнопки нет или сутки автопроверки уже вышли — отправь <code>повторить</code>, бот выдаст новые ссылки авторизации на том же прокси.

Профиль не опубликован и сделка не завершена. Прокси закреплён за этой позицией.",
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
            Self::AccountValidationRequired => "❌ Google держит сам аккаунт на проверке: генерация отклонена с «Verify your account to continue». Это отдельная проверка — обычный Gemini на сайте при этом может работать, и повтор без её прохождения ничего не изменит.

Сделай по порядку, СТРОГО в том же профиле антидетект-браузера и через тот же прокси:
1️⃣ Привяжи и подтверди номер телефона: <code>myaccount.google.com/signinoptions/rescuephone</code>
2️⃣ Открой <code>youtube.com</code> под этим аккаунтом и пройди проверку, если она появится.
3️⃣ Открой персональную ссылку Google, если она пришла ниже.
4️⃣ Дальше жди: статус у Google обновляется не мгновенно, бот сам повторяет проверку каждые 5 минут в течение суток и присылает свежую ссылку раз в полчаса.

Прошёл проверку — нажми кнопку ниже, бот перепроверит немедленно. Кнопки нет или сутки автопроверки уже вышли — отправь <code>повторить</code>, бот выдаст новые ссылки авторизации на том же прокси.

Профиль не опубликован и сделка не завершена. Прокси закреплён за этой позицией.",
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

    /// Does this verdict end automatic retries for parked material?
    ///
    /// Only outcomes that no later attempt can change: the token family is dead, the consent named
    /// a different account, or the account/proxy conflicts with a published profile. Everything
    /// else — a held account, a tier Google has not finished provisioning, an unhappy surface, a
    /// throttled egress — is exactly what the 24-hour window exists for. The credential itself is
    /// kept on record either way.
    fn stops_automatic_probing(self) -> bool {
        matches!(
            self,
            Self::Authorization
                | Self::AccountMismatch
                | Self::Duplicate
                | Self::DuplicateProxy
                | Self::MigrationProxyMismatch
                | Self::StaleHandoff
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

pub(crate) struct PublishedProfile {
    id: String,
    plan: String,
    has_proxy: bool,
    proxy_order_id: i64,
    issued_at: i64,
    canonical_ip: Option<std::net::IpAddr>,
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
            elog::error("authbot", format!("[gemini-oauth] chat={} attested OAuth transport startup failed: {}", chat_id,
                crate::gemini_transport::diagnostic_kind(&error),));
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
                elog::error("authbot", format!("[gemini-oauth] chat={} OAuth transport restart failed: {}", self.chat_id,
                    crate::gemini_transport::diagnostic_kind(&error),));
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
                        elog::warn("authbot", format!("[gemini-oauth] chat={} token transport recovered on attempt {}/{}", self.chat_id,
                            attempt,
                            TRANSPORT_RECOVERY_DELAYS.len() + 1,));
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let diagnostic = crate::gemini_transport::diagnostic_kind(&error);
                    let retryable = crate::gemini_transport::failure_kind(&error)
                        .is_some_and(|kind| kind.safe_to_retry_before_target());
                    elog::warn("authbot", format!("[gemini-oauth] chat={} token transport attempt {}/{} failed: {}", self.chat_id,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,));
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
                    elog::warn("authbot", format!("[gemini-oauth] chat={} {} transport attempt {}/{} failed: {}", self.chat_id,
                        phase,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,));
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
    /// lost, so an ambiguous post-send outcome is never replayed.
    ///
    /// A CONNECT-stage refusal is a different fact: the tunnel to Google was never established, so
    /// the request provably did not reach the model and no paid generation exists to protect. Those
    /// bounded classes (`proxy_throttle`, `proxy_timeout`, `proxy_upstream`, `proxy_connect`,
    /// `proxy_eof`, `tls` — exactly `safe_to_retry_before_target`) get the same bounded recovery as
    /// the token exchange. Without this a residential gateway throttling one CONNECT burned the
    /// seller's whole acceptance attempt and was reported as `generation_unavailable`, which reads
    /// as a verdict about the subscription rather than about our egress.
    async fn generation_request(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Result<crate::gemini_transport::Response, Failure> {
        for attempt in 1..=TRANSPORT_RECOVERY_DELAYS.len() + 1 {
            match self
                .inner
                .request(GeminiHttpMethod::Post, url, headers, body)
                .await
            {
                Ok(response) => {
                    if attempt > 1 {
                        elog::warn("authbot", format!("[gemini-oauth] chat={} generation transport recovered on attempt {}/{}", self.chat_id,
                            attempt,
                            TRANSPORT_RECOVERY_DELAYS.len() + 1,));
                    }
                    return Ok(response);
                }
                Err(error) => {
                    let diagnostic = crate::gemini_transport::diagnostic_kind(&error);
                    let before_target = crate::gemini_transport::failure_kind(&error)
                        .is_some_and(|kind| kind.safe_to_retry_before_target());
                    elog::warn("authbot", format!("[gemini-oauth] chat={} generation acceptance transport attempt {}/{} failed: {}", self.chat_id,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,));
                    if !before_target || attempt > TRANSPORT_RECOVERY_DELAYS.len() {
                        return Err(Failure::GenerationUnavailable);
                    }
                    tokio::time::sleep(TRANSPORT_RECOVERY_DELAYS[attempt - 1]).await;
                    self.reconnect().await?;
                }
            }
        }
        unreachable!("bounded generation transport recovery loop always returns")
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
                    elog::warn("authbot", format!("[gemini-oauth] chat={} userinfo transport attempt {}/{} failed: {}", self.chat_id,
                        attempt,
                        TRANSPORT_RECOVERY_DELAYS.len() + 1,
                        diagnostic,));
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
    // Set when Google refuses the acceptance generation until the seller's own Google account is
    // verified; carries that account's verification link back to the Telegram answer.
    verification_url: &mut Option<String>,
) -> Result<Completion, Failure> {
    elog::info("authbot", format!("[gemini-oauth] chat={} proxy_order={} finalizing: exchanging Google authorization code", session.chat_id, proxy_order_id));
    if session.expires_ts < now() {
        elog::error("authbot", format!("[gemini-oauth] chat={} aborted: OAuth session expired before the callback arrived", session.chat_id));
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
        elog::error("authbot", format!("[gemini-oauth] chat={} Google rejected the token exchange: HTTP {}", session.chat_id, response.status));
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
    elog::info("authbot", format!("[gemini-oauth] chat={} Google granted scopes: {}", session.chat_id,
        token.scope.as_deref().unwrap_or("<none>")));
    let authorization = Zeroizing::new(format!("Bearer {}", token.access_token));
    let headers = official_userinfo_headers(authorization.as_str());
    let user_info_response = client.userinfo_request(USERINFO_URL, &headers).await?;
    if !(200..300).contains(&user_info_response.status) {
        elog::error("authbot", format!("[gemini-oauth] chat={} Google userinfo call failed: HTTP {}", session.chat_id, user_info_response.status));
        return Err(Failure::Authorization);
    }
    let mut user: UserInfo =
        serde_json::from_slice(&user_info_response.body).map_err(|_| Failure::Temporary)?;
    if !user.verified_email || !valid_identity(&user.id, 512) || !valid_identity(&user.email, 512) {
        elog::error("authbot", format!("[gemini-oauth] chat={} rejected: Google account is unverified or malformed", session.chat_id));
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
        elog::info("authbot", format!("[gemini-oauth] chat={} legacy identity bootstrap passed; final subscription admission remains pending", session.chat_id));
        return Ok(Completion::LegacyBootstrap {
            subject: Zeroizing::new(std::mem::take(&mut user.id)),
        });
    }
    let refresh_token = token.refresh_token.take().ok_or(Failure::Authorization)?;
    if validate_final_subject(phase, &user.id, bootstrap_subject).is_err() {
        elog::error("authbot", format!("[gemini-oauth] chat={} rejected: Antigravity subject differs from legacy bootstrap", session.chat_id));
        return Err(Failure::AccountMismatch);
    }
    // Consent succeeded, so this token family is the only copy of a subscription the seller already
    // paid for and Google has already annulled any previous one. Seal it BEFORE anything that can
    // fail — tier resolution, the paid generation, the roster write — so no verdict, timeout or
    // throttled proxy can lose it. Parking is not publication: nothing reaches `profiles.json` and
    // the payout does not complete until an acceptance generation passes.
    let mut credential = GeminiCredential {
        version: 1,
        access_token: std::mem::take(&mut token.access_token),
        refresh_token,
        expires_at: now().saturating_add(token.expires_in).saturating_sub(60),
        oauth_client_id: client_id.to_string(),
        oauth_client_secret: client_secret.to_string(),
        token_uri: TOKEN_URL.to_string(),
        subject: std::mem::take(&mut user.id),
        email: std::mem::take(&mut user.email),
        // Tier and project are resolved below and re-sealed; an attempt that never got that far
        // resolves them on its next automatic retry instead of discarding the account.
        project_id: String::new(),
        tier_id: String::new(),
        tier_name: String::new(),
        plan: String::new(),
        proxy: proxy.to_string(),
        proxy_order_id,
        issued_at: now(),
    };
    park_verification(
        store,
        config,
        session.chat_id,
        session.job.as_ref(),
        &credential,
    );
    // Resolution and the paid probe share one code path with every later automatic attempt, so a
    // credential admitted at 03:00 by the sweep goes through exactly the checks the callback would
    // have applied.
    if let Err(failure) = resolve_and_probe(
        &mut client,
        &mut credential,
        session.chat_id,
        verification_url,
    )
    .await
    {
        reseal_parked_credential(store, config, session.chat_id, &credential);
        record_probe_outcome(store, session.chat_id, failure);
        return Err(failure);
    }
    elog::info("authbot", format!("[gemini-oauth] chat={} Google account and exact generation verified, plan={}, sealing credential", session.chat_id,
        plan_label(&credential.plan)));
    let published = publish_verified_credential(
        store,
        config,
        session.chat_id,
        session.job.as_ref(),
        credential,
    )
    .await;
    match &published {
        // The roster now owns this material, so the parked copy has no reason to exist and must
        // not keep the automatic sweep probing a subscription that is already serving traffic.
        Ok(_) => {
            let _ = store.clear_gemini_verification(session.chat_id);
        }
        Err(failure) => record_probe_outcome(store, session.chat_id, *failure),
    }
    published
}

/// Publication tail shared by the callback and the seller's post-verification retry.
async fn publish_verified_credential(
    store: &Store,
    config: &Config,
    chat_id: i64,
    job: Option<&SellerJobRef>,
    credential: GeminiCredential,
) -> Result<Completion, Failure> {
    let plan = credential.plan.clone();
    let terminal_guard = config.terminal_guard().await;
    // Generation acceptance may take long enough for the seller to cancel, rewind or replace the
    // exact job generation. Re-check after waiting for the filesystem publication lock and as close
    // as possible to the credential write; SQLite and the roster cannot form one atomic transaction.
    if !handoff_is_current(store, chat_id, job) {
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} rejected stale seller generation immediately before publication"));
        return Err(Failure::StaleHandoff);
    }
    let _ = plan;
    let root = config.root.clone();
    let ring = config.keyring.clone();
    let active = config.active_key_id.clone();
    let published = tokio::task::spawn_blocking(move || publish(&root, &ring, &active, credential))
        .await
        .map_err(|_| Failure::Storage)?;
    match &published {
        Ok(profile) if profile.reauthorized => elog::info("authbot", format!("[gemini-oauth] chat={} reauthorized profile {} in place (plan {}); the previous refresh token was invalidated by Google", chat_id, profile.id, plan_label(&profile.plan))),
        Ok(profile) if profile.migrated => elog::info("authbot", format!("[gemini-oauth] chat={} atomically migrated profile {} to Antigravity (plan {})", chat_id, profile.id, plan_label(&profile.plan))),
        Ok(profile) => elog::info("authbot", format!("[gemini-oauth] chat={} sealed and published profile {} (plan {}) into the Gemini roster", chat_id, profile.id, plan_label(&profile.plan))),
        Err(failure) => elog::error("authbot", format!("[gemini-oauth] chat={} sealing/publishing the profile failed: {}", chat_id,
            failure.code())),
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

/// How long the sealed credential of a consented-but-not-yet-admitted account stays on record.
/// Deliberately far longer than the automatic window: the seller already paid for this
/// subscription, and an operator investigating why it never passed needs the material to still
/// exist.
const VERIFICATION_PARK_SECS: i64 = 7 * 24 * 3600;

/// One automatic acceptance attempt every five minutes, for twenty-four hours after consent.
/// Google's own holds (account verification, tier provisioning) and a throttled residential egress
/// all clear on that timescale, and a `gemini-2.5-flash-lite` probe capped at 8 output tokens is
/// negligible against the subscription it is proving.
/// Attempts between two reminders while an account sits parked.
///
/// Silence is right for an unchanged verdict — 288 identical messages a day are noise — but not
/// forever: Google mints a fresh `validation_url` for every rejection, so the one the seller was
/// given first goes stale while they are still working through the checklist. Every sixth attempt
/// is a reminder every half hour, carrying the link that is current right now.
const VERIFICATION_REMINDER_EVERY: i64 = 6;
const VERIFICATION_PROBE_INTERVAL_SECS: i64 = 300;
const VERIFICATION_PROBE_WINDOW_SECS: i64 = 24 * 3600;

/// AEAD context for a parked account. The keyring restricts a context id to `[A-Za-z0-9_-]`, so the
/// separator is a dash — a colon here silently fails the seal and loses the parked account.
fn verification_aad(chat_id: i64) -> String {
    format!("gemini-verification-{chat_id}")
}

fn seal_parked_credential(
    config: &Config,
    chat_id: i64,
    credential: &GeminiCredential,
) -> Option<String> {
    let payload = Zeroizing::new(serde_json::to_string(credential).ok()?);
    config
        .keyring
        .seal_secret(
            &config.active_key_id,
            &verification_aad(chat_id),
            payload.as_str(),
        )
        .ok()
        .and_then(|envelope| serde_json::to_string(&envelope).ok())
}

/// Seal a consented account and start its automatic acceptance window. The envelope uses the same
/// keyring and AEAD as a published credential and is bound to this chat, so it cannot be moved to
/// another seller, and it never enters `profiles.json`.
fn park_verification(
    store: &Store,
    config: &Config,
    chat_id: i64,
    job: Option<&SellerJobRef>,
    credential: &GeminiCredential,
) {
    let Some(sealed) = seal_parked_credential(config, chat_id, credential) else {
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} could not seal the consented account; it will have to be authorized again"));
        return;
    };
    let now = now();
    match store.park_gemini_verification(
        chat_id,
        &sealed,
        now.saturating_add(VERIFICATION_PARK_SECS),
        now.saturating_add(VERIFICATION_PROBE_WINDOW_SECS),
        now.saturating_add(VERIFICATION_PROBE_INTERVAL_SECS),
        job,
    ) {
        Ok(()) => elog::info("authbot", format!("[gemini-oauth] chat={chat_id} recorded the consented account; automatic acceptance runs every {VERIFICATION_PROBE_INTERVAL_SECS}s for {VERIFICATION_PROBE_WINDOW_SECS}s")),
        Err(_) => elog::error("authbot", format!("[gemini-oauth] chat={chat_id} could not record the consented account; it will have to be authorized again")),
    }
}

/// Persist material the attempt improved (a refreshed access token, a resolved project/tier)
/// without touching the schedule the claim already advanced.
fn reseal_parked_credential(
    store: &Store,
    config: &Config,
    chat_id: i64,
    credential: &GeminiCredential,
) {
    if let Some(sealed) = seal_parked_credential(config, chat_id, credential) {
        let _ = store.reseal_gemini_verification(chat_id, &sealed);
    }
}

/// Record what one attempt concluded. A verdict no retry can change stops the sweep; the sealed
/// credential stays on record either way, so nothing the seller paid for is thrown away.
fn record_probe_outcome(store: &Store, chat_id: i64, failure: Failure) {
    if failure.stops_automatic_probing() {
        let _ = store.schedule_gemini_probe(chat_id, 0, Some(0), failure.code());
        elog::warn("authbot", format!("[gemini-oauth] chat={chat_id} automatic acceptance stopped on a terminal verdict: {}; the credential stays recorded", failure.code()));
    } else {
        let _ = store.schedule_gemini_probe(
            chat_id,
            now().saturating_add(VERIFICATION_PROBE_INTERVAL_SECS),
            None,
            failure.code(),
        );
    }
}

/// Everything between a consented token family and publication: refresh the bearer if it aged out,
/// resolve tier/project when a previous attempt could not, then run exactly one paid acceptance
/// generation. Shared by the callback, the seller's button and the background sweep so all three
/// admit on identical evidence.
async fn resolve_and_probe(
    client: &mut RecoveringClient<'_>,
    credential: &mut GeminiCredential,
    chat_id: i64,
    verification_url: &mut Option<String>,
) -> Result<(), Failure> {
    if credential.expires_at <= now() {
        refresh_parked_access_token(client, credential).await?;
    }
    // Resolved on every attempt, not only when the project is still unknown. The stored project id
    // never changes, but the account's tier does: Google can provision the purchased entitlement
    // hours after consent, and `resolve_antigravity_account` can move an account off the free tier
    // it was parked on. Skipping this while a project existed is what made a parked account retry the same
    // refused generation for a day without anything being able to change its outcome. One
    // control-plane read per five-minute attempt is what that visibility costs.
    let resolved = match resolve_antigravity_account(client, &credential.access_token).await {
        Ok(resolved) => resolved,
        Err(failure) => {
            elog::error("authbot", format!("[gemini-oauth] chat={chat_id} tier/project resolution failed: {}", failure.code()));
            return Err(failure);
        }
    };
    if !supported_paid_plan(&resolved.plan) {
        log_unsupported_plan("unreviewed_reported_tier", resolved.diagnostic);
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} rejected: unsupported Google plan {}", plan_label(&resolved.plan)));
        return Err(Failure::UnsupportedPlan);
    }
    credential.project_id = resolved.project_id;
    credential.tier_id = resolved.tier_id;
    credential.tier_name = resolved.tier_name;
    credential.plan = resolved.plan;
    generation_probe(
        client,
        &credential.access_token,
        &credential.project_id,
        chat_id,
        verification_url,
    )
    .await
}

/// The seller pressed “I verified the account”. One press = one real acceptance generation with the
/// tokens their consent already produced; the deal is settled here on success, and on a repeated
/// hold the same button is offered again with whatever fresh verification link Google returned.
/// Tell the seller what a parked attempt just decided, with whatever link it carried.
///
/// Shared by the seller's button and the background sweep so both surfaces say the same thing about
/// the same verdict, and so the sweep can hand over a link that is current instead of leaving the
/// seller with the stale one from the first rejection.
async fn report_parked_rejection(
    bot: &Bot,
    store: &Arc<Store>,
    config: &Arc<BotConfig>,
    chat_id: i64,
    job: Option<&SellerJobRef>,
    failure: Failure,
    verification_url: Option<&str>,
) {
    let accepts_proxy_input = job
        .is_some_and(|expected| crate::bot::gemini_job_accepts_proxy_input(store, expected, 0));
    let base = if accepts_proxy_input {
        failure.public_message()
    } else {
        failure.fixed_proxy_message()
    };
    // Google's own instructions, already rendered and escaped, follow whatever verdict text this
    // refusal produced — including one classified as something else, because the account is still
    // held and those instructions are still the only way past it.
    let message = match verification_url {
        Some(guidance) => format!("{base}\n\n{guidance}"),
        None => base.to_string(),
    };
    // The seller does not have to babysit this: the same acceptance runs automatically every five
    // minutes for a day, and a late success publishes and pays out on its own.
    let parked = matches!(store.gemini_verification_is_parked(chat_id), Ok(true));
    let message = if parked {
        format!(
            "{message}\n\n🔄 Доступ сохранён у нас: бот сам повторяет проверку каждые 5 минут в течение суток и завершит сделку автоматически, как только она пройдёт. Кнопкой ниже можно проверить немедленно."
        )
    } else {
        message
    };
    if parked {
        let _ = bot
            .send_kb(chat_id, &message, Some(&crate::bot::gemini_verified_kb()))
            .await;
    } else {
        let _ = bot.send(chat_id, &message).await;
    }
    if failure.operator_action_required() {
        for admin in &config.admins_id {
            let _ = bot.send(*admin, failure.operator_message()).await;
        }
    }
}

pub(crate) async fn finish_parked_verification(
    bot: &Bot,
    store: &Arc<Store>,
    config: &Arc<BotConfig>,
    oauth: &Config,
    chat_id: i64,
) {
    let job = store
        .active_seller_job(chat_id)
        .ok()
        .flatten()
        .map(|job| job.reference);
    match retry_parked_verification(store, oauth, chat_id).await {
        VerificationRetry::Published(profile) => {
            announce_publication(bot, store, config, chat_id, job, &profile).await;
        }
        VerificationRetry::Rejected(failure, verification_url) => {
            report_parked_rejection(
                bot,
                store,
                config,
                chat_id,
                job.as_ref(),
                failure,
                verification_url.as_deref(),
            )
            .await;
            elog::error("authbot", format!("[gemini-oauth] chat={chat_id} post-verification retry failed: {}", failure.code()));
        }
        VerificationRetry::Missing => {
            let _ = bot
                .send(
                    chat_id,
                    "Эта кнопка уже неактивна: сохранённого доступа для проверки нет. Отправь <code>повторить</code> — бот выдаст новые ссылки авторизации на том же прокси.",
                )
                .await;
        }
    }
}

/// Result of the seller pressing “I verified the account”.
pub(crate) enum VerificationRetry {
    Published(PublishedProfile),
    Rejected(Failure, Option<String>),
    /// Nothing is parked for this chat, or it belongs to a deal that has since moved on.
    Missing,
}

/// Re-run the paid acceptance generation for a consented account that has not been admitted yet,
/// using the tokens that consent already produced. Every call is exactly one real generation: the
/// account either passes now or waits for the next attempt, and nothing else about the deal
/// changes. Shared by the seller's button and the background sweep.
pub(crate) async fn retry_parked_verification(
    store: &Store,
    config: &Config,
    chat_id: i64,
) -> VerificationRetry {
    let next_probe = now().saturating_add(VERIFICATION_PROBE_INTERVAL_SECS);
    let Ok(Some(parked)) = store.claim_gemini_verification(chat_id, next_probe) else {
        return VerificationRetry::Missing;
    };
    if !handoff_is_current(store, chat_id, parked.job.as_ref()) {
        let _ = store.clear_gemini_verification(chat_id);
        elog::info("authbot", format!("[gemini-oauth] chat={chat_id} dropped a parked account whose seller generation moved on"));
        return VerificationRetry::Missing;
    }
    let Some(mut credential) = open_parked_credential(config, &parked) else {
        let _ = store.clear_gemini_verification(chat_id);
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} could not open the parked account envelope"));
        return VerificationRetry::Missing;
    };
    elog::info("authbot", format!("[gemini-oauth] chat={chat_id} re-running acceptance for the recorded account (attempt {})", parked.attempts));
    // The client borrows the egress for its whole lifetime, so hand it an owned copy: the parked
    // credential itself still has to be mutable for the token refresh below.
    let proxy = credential.proxy.clone();
    let mut client = match RecoveringClient::connect(&proxy, chat_id).await {
        Ok(client) => client,
        Err(failure) => {
            record_probe_outcome(store, chat_id, failure);
            return VerificationRetry::Rejected(failure, None);
        }
    };
    let mut verification_url = None;
    if let Err(failure) =
        resolve_and_probe(&mut client, &mut credential, chat_id, &mut verification_url).await
    {
        // Keep the material. A refused generation is a statement about the account's current state
        // — held for verification, tier not provisioned yet, a surface having a bad minute, a
        // throttled CONNECT — and none of those are reasons to destroy tokens the seller was paid
        // for. Erasing here is exactly what turned one throttled proxy into a dead button.
        reseal_parked_credential(store, config, chat_id, &credential);
        record_probe_outcome(store, chat_id, failure);
        return VerificationRetry::Rejected(failure, verification_url);
    }
    match publish_verified_credential(store, config, chat_id, parked.job.as_ref(), credential).await
    {
        Ok(Completion::Published(profile, _guard)) => {
            let _ = store.clear_gemini_verification(chat_id);
            VerificationRetry::Published(profile)
        }
        Ok(Completion::LegacyBootstrap { .. }) => VerificationRetry::Missing,
        Err(failure) => {
            record_probe_outcome(store, chat_id, failure);
            VerificationRetry::Rejected(failure, None)
        }
    }
}

/// One pass of the automatic acceptance window over every recorded account that is due.
///
/// Attempts are sequential on purpose: each one is a real paid generation through a per-account
/// authenticated CONNECT, and the parallel version of this would look like a burst from one egress.
/// A late success publishes and settles the deal through exactly the callback's code path, so a
/// subscription that Google unblocks at 04:00 is in the pool at 04:05 without anyone pressing
/// anything.
pub(crate) async fn sweep_recorded_verifications(
    bot: &Bot,
    store: &Arc<Store>,
    config: &Arc<BotConfig>,
) {
    let Some(oauth) = config.gemini_oauth.as_ref() else {
        return;
    };
    for chat_id in store.due_gemini_verifications().unwrap_or_default() {
        let job = store
            .active_seller_job(chat_id)
            .ok()
            .flatten()
            .map(|job| job.reference);
        // Read before the attempt: claiming the row increments the counter and overwrites the
        // verdict, and both are needed to decide whether this attempt is worth a message.
        let previous = store.gemini_verification_progress(chat_id).unwrap_or_default();
        match retry_parked_verification(store, oauth, chat_id).await {
            VerificationRetry::Published(profile) => {
                elog::info("authbot", format!("[gemini-oauth] chat={chat_id} automatic acceptance passed; publishing profile {}", profile.id));
                announce_publication(bot, store, config, chat_id, job, &profile).await;
            }
            VerificationRetry::Rejected(failure, verification_url) => {
                elog::warn("authbot", format!("[gemini-oauth] chat={chat_id} automatic acceptance attempt failed: {}", failure.code()));
                // Mostly silent by design — 288 identical messages a day would be noise — but not
                // in the two cases where silence costs the deal. A changed verdict is news the
                // seller cannot get anywhere else: a window that turned terminal looks exactly like
                // one still working. And a periodic reminder carries the link that is current now,
                // because Google mints a new one per rejection and the seller's first one goes stale
                // long before they finish the checklist.
                let (attempts_before, previous_verdict) = previous.unwrap_or_default();
                // An empty previous verdict is the parking row as the callback left it, not a
                // change: the seller already has that message and its link from seconds ago.
                let verdict_changed =
                    !previous_verdict.is_empty() && previous_verdict != failure.code();
                let reminder_due = (attempts_before + 1) % VERIFICATION_REMINDER_EVERY == 0;
                if verdict_changed || reminder_due {
                    report_parked_rejection(
                        bot,
                        store,
                        config,
                        chat_id,
                        job.as_ref(),
                        failure,
                        verification_url.as_deref(),
                    )
                    .await;
                }
            }
            VerificationRetry::Missing => {}
        }
    }
    for chat_id in store.expired_gemini_probe_windows().unwrap_or_default() {
        if !store
            .mark_gemini_probe_window_notified(chat_id)
            .unwrap_or(false)
        {
            continue;
        }
        elog::info("authbot", format!("[gemini-oauth] chat={chat_id} automatic acceptance window closed"));
        let _ = bot
            .send(
                chat_id,
                "⏳ <b>Автоматическая проверка этого Google-аккаунта закончилась.</b> Сутки бот повторял тестовую генерацию каждые 5 минут, и она так и не прошла — доступ в пул не добавлен и выплата не завершена. Сам доступ сохранён: если ты завершил проверку Google, нажми кнопку проверки ещё раз или отправь <code>повторить</code> для новой авторизации.",
            )
            .await;
        for admin in &config.admins_id {
            let _ = bot
                .send(
                    *admin,
                    &format!(
                        "⏳ Gemini: 24-часовое окно автопроверки закрыто для chat=<code>{chat_id}</code>. Конверт остаётся записанным, профиль не опубликован, выплата не завершена."
                    ),
                )
                .await;
        }
    }
}

fn open_parked_credential(
    config: &Config,
    parked: &GeminiPendingVerification,
) -> Option<GeminiCredential> {
    let envelope: SealedCredential = serde_json::from_str(&parked.sealed_payload).ok()?;
    let payload = config
        .keyring
        .open_secret(&verification_aad(parked.chat_id), &envelope)
        .ok()?;
    serde_json::from_str::<GeminiCredential>(payload.as_str()).ok()
}

/// Trade the parked refresh token for a fresh access token over the account's own egress. Google
/// rotates nothing here, so a failure is a verdict about the account, not a lost credential.
async fn refresh_parked_access_token(
    client: &mut RecoveringClient<'_>,
    credential: &mut GeminiCredential,
) -> Result<(), Failure> {
    // Antigravity's own refresh keeps the Go client's lexical field order.
    let form = serde_urlencoded::to_string([
        ("client_id", credential.oauth_client_id.as_str()),
        ("client_secret", credential.oauth_client_secret.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", credential.refresh_token.as_str()),
    ])
    .map(Zeroizing::new)
    .map_err(|_| Failure::Authorization)?;
    let response = client
        .token_request(
            &[
                (
                    "content-type",
                    "application/x-www-form-urlencoded;charset=UTF-8",
                ),
                ("user-agent", "Go-http-client/2.0"),
            ],
            form.as_bytes(),
        )
        .await?;
    if !(200..300).contains(&response.status) {
        elog::error("authbot", format!("[gemini-oauth] chat={} refreshing the parked access token failed: HTTP {}", client.chat_id, response.status));
        return Err(if matches!(response.status, 400 | 401) {
            Failure::Authorization
        } else {
            Failure::Temporary
        });
    }
    let mut token: TokenResponse =
        serde_json::from_slice(&response.body).map_err(|_| Failure::Temporary)?;
    if !valid_oauth_value(&token.access_token, 16_384) || !(60..=86_400).contains(&token.expires_in)
    {
        return Err(Failure::Authorization);
    }
    credential.access_token = std::mem::take(&mut token.access_token);
    credential.expires_at = now().saturating_add(token.expires_in).saturating_sub(60);
    Ok(())
}

/// Projects this process has already tried to move onto a paid tier.
///
/// The parked sweep re-resolves every five minutes for a day, so without this an account Google
/// insists on keeping below its entitlement would be sent an `onboardUser` write on all 288 of them.
/// One attempt per project per process is what unsticking an account our own default-tier choice
/// parked there actually needs, and a deploy restart is a natural bounded retry.
static REONBOARDED_PROJECTS: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

/// Claim the single re-onboarding attempt for this project, returning false if it is already spent
/// or the memo is poisoned — both mean "do not write to Google again".
fn claim_reonboarding_attempt(project_id: &str) -> bool {
    let claimed = REONBOARDED_PROJECTS.get_or_init(|| Mutex::new(HashSet::new()));
    let Ok(mut claimed) = claimed.lock() else {
        return false;
    };
    claimed.insert(project_id.to_string())
}

/// Does this tier map to a reviewed paid plan?
fn tier_is_supported_paid(tier: &Tier) -> bool {
    gemini_credential::supported_plan_for_tier(
        tier.id.as_deref().unwrap_or_default(),
        tier.name.as_deref().unwrap_or_default(),
    )
    .is_some_and(supported_paid_plan)
}

/// Choose the tier to onboard an Antigravity account onto.
///
/// Google marks the free tier `is_default`, so taking the default put a seller's paid subscription
/// on `free-tier`: the account then holds a real entitlement while every acceptance generation is
/// refused with `RESOURCE_EXHAUSTED`, which reads as an exhausted account rather than as a tier we
/// chose for it. Prefer a tier that maps to a reviewed paid plan and fall back to Google's own
/// default ordering only when none of the offered tiers is one.
///
/// The candidate is always a tier Google itself listed for this account. The reported `paidTier` is
/// preferred only when it also appears in `allowedTiers`, so onboarding never sends an id Google did
/// not offer — an entitlement Antigravity has not provisioned yet is not an onboarding target.
fn preferred_onboarding_tier(loaded: &LoadCodeAssistResponse) -> Option<Tier> {
    let entitled_id = loaded
        .paid_tier
        .as_ref()
        .filter(|tier| tier_is_supported_paid(tier))
        .and_then(|tier| tier.id.clone());
    if let Some(entitled_id) = entitled_id {
        if let Some(offered) = loaded
            .allowed_tiers
            .iter()
            .find(|tier| tier.id.as_deref() == Some(entitled_id.as_str()))
        {
            return Some(offered.clone());
        }
    }
    loaded
        .allowed_tiers
        .iter()
        .find(|tier| tier_is_supported_paid(tier))
        .or_else(|| loaded.allowed_tiers.iter().find(|tier| tier.is_default))
        .or(loaded.current_tier.as_ref())
        .or_else(|| loaded.allowed_tiers.first())
        .cloned()
}

fn valid_identity(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

async fn resolve_antigravity_account(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
) -> Result<ResolvedAccount, Failure> {
    let mut loaded = load_code_assist(client, access_token).await?;
    let mut onboarded = false;
    if project_from_value(loaded.cloudaicompanion_project.as_ref()).is_none() {
        let Some(tier) = preferred_onboarding_tier(&loaded) else {
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
        onboarded = true;
    }
    // An account that was already onboarded before this preference existed still sits on whatever
    // tier Google marked default, which is the free one. It holds a real paid entitlement and is
    // refused on every generation with `RESOURCE_EXHAUSTED`, and nothing about waiting changes that:
    // the tier is only ever chosen while the account has no project. Move it once, here, whenever
    // Google is still offering a reviewed paid tier it is not on.
    //
    // Best-effort on purpose. A refused or unavailable re-onboarding must leave the account exactly
    // where it was and still let the acceptance generation run, so a tier we could not improve never
    // becomes a new way to fail an account that was merely waiting.
    if !onboarded && !loaded.current_tier.as_ref().is_some_and(tier_is_supported_paid) {
        let project = project_from_value(loaded.cloudaicompanion_project.as_ref());
        let target = preferred_onboarding_tier(&loaded)
            .filter(tier_is_supported_paid)
            .and_then(|tier| tier.id.clone());
        if let (Some(project), Some(tier_id)) = (project, target) {
            if claim_reonboarding_attempt(&project) {
                elog::warn("authbot", format!("[gemini-oauth] account is onboarded below its entitlement; re-onboarding onto {}: {}", bounded_label(Some(&tier_id)),
                    CodeAssistDiagnostic::from_response(&loaded).sanitized()));
                match onboard_antigravity(client, access_token, &tier_id).await {
                    Ok(()) => loaded = load_code_assist(client, access_token).await?,
                    Err(failure) => elog::warn("authbot", format!("[gemini-oauth] re-onboarding onto {} did not take: {}", bounded_label(Some(&tier_id)),
                        failure.code())),
                }
            }
        }
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

async fn generation_probe(
    client: &mut RecoveringClient<'_>,
    access_token: &str,
    project_id: &str,
    chat_id: i64,
    verification_url: &mut Option<String>,
) -> Result<(), Failure> {
    let mut last = Failure::GenerationUnavailable;
    for (surface, host) in CODE_ASSIST_SURFACES {
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
                elog::info("authbot", format!("[gemini-oauth] chat={chat_id} exact generation acceptance passed on {surface}"));
                return Ok(());
            }
            Err(failure) => {
                log_generation_failure(chat_id, surface, response.status, &response.body);
                // Google puts the account's own verification link in the rejection metadata and
                // only the bare sentence in `message`. Surfacing the link is the difference between
                // an actionable instruction and a dead end, because the seller cannot reach this
                // particular check from a normal Gemini session — theirs already works.
                //
                // The link is read from every rejection, not only from the one that classifies as
                // `AccountValidationRequired`. Google answers whichever refusal wins the race, and a
                // quota refusal wins before the validation gate is ever evaluated: it arrives as
                // `RESOURCE_EXHAUSTED` with no `reason`, the classifier returns `None`, and gating
                // the lookup on that classification threw away any link the rejection still carried.
                // `validation_guidance_from_body` stays fail-closed on anything that is not
                // literally a Google sign-in URL, so a rejection without one simply leaves this
                // `None`. The first surface that carries instructions keeps them: a later host
                // answering without any is not evidence that the account stopped being held.
                if verification_url.is_none() {
                    *verification_url = validation_guidance_from_body(&response.body);
                }
                elog::warn("authbot", format!("[gemini-oauth] chat={chat_id} account verification link is {} after the {surface} rejection", if verification_url.is_some() {
                        "present in the rejection metadata"
                    } else {
                        "absent from the rejection metadata"
                    }));
                // Journal the link itself, not only that it exists. It reaches exactly one person in
                // exactly one message, Google mints a new one per rejection, and the bounded
                // `details` dump truncates before its token — so without this an operator cannot
                // recover a link the seller lost. `valid_verification_url` has already fail-closed it
                // to a real Google sign-in URL, and the same operator switch gates it as the rest of
                // the evidence.
                if let Some(url) = verification_url.as_deref() {
                    if std::env::var("AUTH_BOT_GEMINI_TIER_EVIDENCE").as_deref() == Ok("1") {
                        elog::warn("authbot", format!("[gemini-oauth] chat={chat_id} account verification link: {url}"));
                    }
                }
                // An account-level rejection is the same on every host, so stop asking. A 2xx that
                // fails acceptance already consumed a paid generation, and any other status is not
                // evidence that a different host would answer differently. Only a refusal that
                // provably ran no model may try the next reviewed surface: 403/404 are access
                // rejections, and 429 is a quota refusal, which is a statement about this host's
                // quota for this account and says nothing about the host whose quota it holds.
                // Letting 429 end the whole probe is what made a working subscription look exhausted.
                if let Some(classified) = classify_generation_failure(&response.body) {
                    return Err(classified);
                }
                if !matches!(response.status, 403 | 404 | 429) {
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
    elog::error("authbot", format!("[gemini-oauth] chat={chat_id} generation acceptance failed on {surface}: HTTP {status} google_status={} reason={}", bounded_label(google_status),
        bounded_label(reason)));
    if std::env::var("AUTH_BOT_GEMINI_TIER_EVIDENCE").as_deref() == Ok("1") {
        let message = error
            .and_then(|error| error.get("message"))
            .and_then(Value::as_str);
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} generation acceptance detail: {}", bounded_label(message)));
        // The rejection metadata is the only place a per-account verification link ever appears, so
        // when Google refuses for a reason this code does not model yet, the operator has to be able
        // to see the shape it actually sent. Same opt-in switch and the same control-character
        // stripping as the message above, with a wider bound because `details` is a structure rather
        // than a sentence.
        let details = error.and_then(|error| error.get("details")).map(Value::to_string);
        elog::error("authbot", format!("[gemini-oauth] chat={chat_id} generation acceptance details: {}", bounded_evidence(details.as_deref(), 1_024)));
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

/// Render everything Google said about an account it is holding, in Google's own words.
///
/// The rejection carries a labelled instruction set, not just an address: its own sentence, the
/// text it puts on the primary action, that action's URL, and a secondary "learn more" pointing at
/// its help centre. Forwarding only the bare URL threw the labels away, and the message had to
/// guess out loud which check Google wanted — a guess is how a seller ends up doing something
/// Google never asked for. Google walks the account owner through the actual challenge once they
/// open its own link, so the honest thing to forward is its instructions, verbatim.
///
/// Every piece is optional and independently fail-closed: upstream text is bounded and stripped,
/// the action must be a real Google sign-in URL and the help link must be Google's help centre, or
/// our own Telegram message becomes a credible phishing vector against the seller whose account we
/// just touched. Requiring the full `https://accounts.google.com/` prefix also rules out lookalike
/// hosts such as `accounts.google.com.example.net`. A rejection carrying less simply says less.
fn validation_guidance_from_body(body: &[u8]) -> Option<String> {
    let parsed = serde_json::from_slice::<Value>(body).ok()?;
    let details = parsed.pointer("/error/details")?.as_array()?;
    let metadata = details
        .iter()
        .find_map(|detail| detail.pointer("/metadata")?.as_object());
    let field = |name: &str| -> Option<String> {
        metadata
            .and_then(|metadata| metadata.get(name))
            .and_then(Value::as_str)
            .map(bounded_instruction)
            .filter(|value| !value.is_empty())
    };
    let action_url = field("validation_url").filter(|url| valid_verification_url(url))?;
    let mut block = String::from("📋 <b>Что сообщает Google по этому аккаунту:</b>");
    if let Some(message) = field("validation_error_message") {
        block.push_str(&format!("\n«{}»", crate::bot::esc(&message)));
    }
    let action_text =
        field("validation_url_link_text").unwrap_or_else(|| "Verify your account".into());
    block.push_str(&format!(
        "\n\n🔗 <b>{}</b>:\n<code>{}</code>",
        crate::bot::esc(&action_text),
        crate::bot::esc(&action_url)
    ));
    if let Some(help_url) = field("validation_learn_more_url").filter(|url| valid_help_url(url)) {
        let help_text =
            field("validation_learn_more_link_text").unwrap_or_else(|| "Learn more".into());
        block.push_str(&format!(
            "\n\nℹ️ <b>{}</b>:\n<code>{}</code>",
            crate::bot::esc(&help_text),
            crate::bot::esc(&help_url)
        ));
    }
    Some(block)
}

/// Google's help centre is the only host besides the sign-in one this message ever points at.
fn valid_help_url(url: &str) -> bool {
    url.len() <= 2048
        && url.starts_with("https://support.google.com/")
        && !url.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '"' | '\'' | '<' | '>' | '\\')
        })
}

/// Upstream instruction text forwarded to a human: bounded and stripped of control characters.
fn bounded_instruction(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_control())
        .take(300)
        .collect()
}

fn valid_verification_url(url: &str) -> bool {
    url.len() <= 2048
        && url.starts_with("https://accounts.google.com/")
        && !url.chars().any(|character| {
            character.is_control()
                || character.is_whitespace()
                || matches!(character, '"' | '\'' | '<' | '>' | '\\')
        })
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
    for (_, base) in CODE_ASSIST_SURFACES {
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
    for (_, base) in CODE_ASSIST_SURFACES {
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
        elog::error("authbot", format!("[gemini-oauth] Code Assist {endpoint} returned HTTP {status}"));
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
    elog::error("authbot", format!("[gemini-oauth] unsupported plan shape: reason={reason} {}", diagnostic.sanitized()));
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
    elog::info("authbot", format!("[gemini-oauth] raw tier evidence: {} {} allowed=[{}]", render("paid", loaded.paid_tier.as_ref()),
        render("current", loaded.current_tier.as_ref()),
        allowed));
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

/// `bounded_label` is sized for one enum-shaped field, so operator evidence that is a structure
/// rather than a sentence needs a wider bound while keeping the same control-character stripping.
fn bounded_evidence(value: Option<&str>, limit: usize) -> String {
    let Some(value) = value else {
        return "<none>".into();
    };
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(limit)
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
        elog::warn("authbot", format!("[gemini-oauth] reviewed tier id kept over drifted display name: {}", diagnostic.sanitized()));
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
            elog::warn("authbot", format!("[gemini-oauth] paid tier kept over disagreeing current tier: {}", diagnostic.sanitized()));
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
                elog::warn("authbot", "[gemini-oauth] using reviewed current tier because paid tier shape is unreviewed");
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
        if existing.proxy_order_id > 0
            && credential.proxy_order_id > 0
            && credential.proxy_order_id != existing.proxy_order_id
        {
            return Err(Failure::MigrationProxyMismatch);
        }
        if existing.proxy_order_id > 0 {
            credential.proxy_order_id = existing.proxy_order_id;
        }
        // Reauthorization preserves the lifecycle age regardless of whether the egress is managed
        // (`order > 0`) or manual (`order == 0`).
        credential.issued_at = existing.issued_at;
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
        let canonical_ip = canonical_proxy_ip(&candidate_proxy).map_err(|_| Failure::Storage)?;
        return Ok(PublishedProfile {
            id: profile_id,
            plan: credential.plan.clone(),
            has_proxy: !credential.proxy.is_empty(),
            proxy_order_id: credential.proxy_order_id,
            issued_at: credential.issued_at,
            canonical_ip,
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
    let canonical_ip = canonical_proxy_ip(&candidate_proxy).map_err(|_| Failure::Storage)?;
    Ok(PublishedProfile {
        id: profile_id,
        plan: credential.plan.clone(),
        has_proxy: !credential.proxy.is_empty(),
        proxy_order_id: credential.proxy_order_id,
        issued_at: credential.issued_at,
        canonical_ip,
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
mod tests;
