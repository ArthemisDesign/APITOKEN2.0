//! Encrypted multi-account Gemini OAuth pool with single-flight refresh and bounded health probes.

use super::config::{GeminiConfig, GeminiProfileSpec, GeminiProfilesFile};
use anyhow::{bail, Context};
use futures_util::StreamExt;
use gemini_credential::{
    decode_envelope, GeminiCredential, SecretString, GEMINI_PROVIDER_USER_AGENT,
};
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

const MAX_BACKGROUND_TASKS: u32 = 4_096;
const HEALTH_PROBE_CONCURRENCY: usize = 16;
const ACCESS_TOKEN_SKEW_SECS: i64 = 120;

#[derive(Clone, Debug)]
pub struct GeminiProfileStatus {
    pub id: String,
    pub authenticated: bool,
    pub cooling_until: i64,
    pub inflight: usize,
}

#[derive(Clone, Debug)]
pub struct GeminiOperationalStatus {
    pub profiles: Vec<GeminiProfileStatus>,
    pub available: usize,
    pub authenticated: usize,
    pub soonest_ready: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenError {
    Invalid,
    Temporary,
}

struct LoadedProfile {
    source: GeminiProfileSpec,
    credential: GeminiCredential,
    fingerprint: [u8; 32],
}

pub(crate) struct GeminiProfile {
    source: GeminiProfileSpec,
    fingerprint: [u8; 32],
    id: String,
    credential: tokio::sync::Mutex<GeminiCredential>,
    client: reqwest::Client,
    inflight: AtomicUsize,
    cooling_until: AtomicI64,
    authenticated: AtomicBool,
}

impl GeminiProfile {
    fn new(loaded: LoadedProfile, cfg: &GeminiConfig) -> anyhow::Result<Self> {
        gemini_credential::validate_profile_id(&loaded.source.id)?;
        if loaded.credential.proxy.trim().is_empty() && cfg.upstream.starts_with("https://") {
            bail!("Gemini production profile requires a dedicated proxy");
        }
        let mut builder = reqwest::Client::builder()
            // Ambient HTTP(S)_PROXY/NO_PROXY state must not override or bypass the sealed
            // per-profile route. Failure of that route is a profile transport fault, not a signal
            // to fall back to the host's direct egress.
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(cfg.connect_timeout_secs))
            .read_timeout(Duration::from_secs(cfg.read_timeout_secs))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .user_agent(GEMINI_PROVIDER_USER_AGENT);
        if !loaded.credential.proxy.trim().is_empty() {
            builder = builder.proxy(
                reqwest::Proxy::all(&loaded.credential.proxy)
                    .context("invalid encrypted Gemini profile proxy URL")?,
            );
        }
        let client = builder.build().context("build Gemini OAuth HTTP client")?;
        Ok(Self {
            id: loaded.source.id.clone(),
            source: loaded.source,
            fingerprint: loaded.fingerprint,
            credential: tokio::sync::Mutex::new(loaded.credential),
            client,
            inflight: AtomicUsize::new(0),
            cooling_until: AtomicI64::new(0),
            authenticated: AtomicBool::new(true),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    fn matches(&self, loaded: &LoadedProfile) -> bool {
        self.source == loaded.source && self.fingerprint == loaded.fingerprint
    }

    pub(crate) fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        access_token: &str,
    ) -> reqwest::RequestBuilder {
        self.client
            .request(method, url)
            .bearer_auth(access_token)
            // Truthful service identity. Do not impersonate Gemini CLI or replay client headers.
            .header(
                "client-metadata",
                r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#,
            )
    }

    pub(crate) async fn access_token(
        &self,
        force_refresh: bool,
    ) -> Result<SecretString, TokenError> {
        // One mutex owns both the expiry check and refresh. A burst after expiry produces exactly
        // one Google token request; all other turns reuse its result.
        let mut credential = self.credential.lock().await;
        let now = pool::now();
        if !force_refresh
            && credential.expires_at > now.saturating_add(ACCESS_TOKEN_SKEW_SECS)
            && !credential.access_token.is_empty()
        {
            return Ok(SecretString::new(credential.access_token.clone()));
        }
        self.refresh_locked(&mut credential).await
    }

    /// Refresh after a concrete bearer token was rejected. If another concurrent request already
    /// replaced that token, reuse the winner instead of serially refreshing once per rejected
    /// request. This preserves single-flight behaviour for both expiry and 401 bursts.
    pub(crate) async fn access_token_after_rejection(
        &self,
        rejected_token: &str,
    ) -> Result<SecretString, TokenError> {
        let mut credential = self.credential.lock().await;
        let now = pool::now();
        if credential.access_token != rejected_token
            && credential.expires_at > now.saturating_add(ACCESS_TOKEN_SKEW_SECS)
            && !credential.access_token.is_empty()
        {
            return Ok(SecretString::new(credential.access_token.clone()));
        }
        self.refresh_locked(&mut credential).await
    }

    async fn refresh_locked(
        &self,
        credential: &mut GeminiCredential,
    ) -> Result<SecretString, TokenError> {
        let now = pool::now();
        let form = serde_urlencoded::to_string([
            ("grant_type", "refresh_token"),
            ("client_id", credential.oauth_client_id.as_str()),
            ("client_secret", credential.oauth_client_secret.as_str()),
            ("refresh_token", credential.refresh_token.as_str()),
        ])
        .map_err(|_| TokenError::Invalid)?;
        let response = self
            .client
            .post(&credential.token_uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .header("accept", "application/json")
            .body(form)
            .send()
            .await
            .map_err(|_| TokenError::Temporary)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(if matches!(status, 400 | 401 | 403) {
                TokenError::Invalid
            } else {
                TokenError::Temporary
            });
        }
        let bytes = response.bytes().await.map_err(|_| TokenError::Temporary)?;
        let token: RefreshResponse =
            serde_json::from_slice(&bytes).map_err(|_| TokenError::Temporary)?;
        if token.access_token.len() < 8
            || token.access_token.len() > 16_384
            || !token
                .access_token
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
            || !(60..=86_400).contains(&token.expires_in)
        {
            return Err(TokenError::Invalid);
        }
        credential.access_token = token.access_token;
        credential.expires_at = now.saturating_add(token.expires_in).saturating_sub(60);
        if let Some(refresh) = token
            .refresh_token
            .filter(|value| !value.is_empty() && value.len() <= 16_384)
        {
            // Google normally keeps the refresh token stable. Retain an unexpected rotation in
            // memory for the process lifetime; the encrypted producer remains the disk authority.
            credential.refresh_token = refresh;
        }
        Ok(SecretString::new(credential.access_token.clone()))
    }

    pub(crate) async fn project_id(&self) -> String {
        self.credential.lock().await.project_id.clone()
    }

    pub(crate) fn mark_healthy(&self) {
        self.authenticated.store(true, Ordering::Release);
        self.cooling_until.store(0, Ordering::Release);
    }

    pub(crate) fn cool_until(&self, until: i64) {
        self.cooling_until.fetch_max(until, Ordering::AcqRel);
    }

    pub(crate) fn mark_auth_failed(&self, until: i64) {
        self.authenticated.store(false, Ordering::Release);
        self.cool_until(until);
    }

    fn status(&self) -> GeminiProfileStatus {
        GeminiProfileStatus {
            id: self.id.clone(),
            authenticated: self.authenticated.load(Ordering::Acquire),
            cooling_until: self.cooling_until.load(Ordering::Acquire),
            inflight: self.inflight.load(Ordering::Acquire),
        }
    }

    async fn probe(&self, cfg: &GeminiConfig) -> ProbeResult {
        let mut token = match self.access_token(false).await {
            Ok(token) => token,
            Err(TokenError::Invalid) => return ProbeResult::Invalid,
            Err(TokenError::Temporary) => return ProbeResult::Temporary,
        };
        for attempt in 0..=1 {
            let url = format!("{}/v1internal:loadCodeAssist", cfg.upstream);
            let body = json!({
                "cloudaicompanionProject": self.project_id().await,
                "metadata": {
                    "ideType": "IDE_UNSPECIFIED",
                    "platform": "PLATFORM_UNSPECIFIED",
                    "pluginType": "GEMINI"
                },
                "mode": "HEALTH_CHECK"
            });
            let response = self
                .request(reqwest::Method::POST, &url, &token)
                .header("content-type", "application/json")
                .timeout(Duration::from_secs(20))
                .json(&body)
                .send()
                .await;
            match response {
                Ok(response) if response.status().is_success() => return ProbeResult::Healthy,
                Ok(response) if response.status().as_u16() == 429 => {
                    return ProbeResult::RateLimited
                }
                Ok(response) if response.status().as_u16() == 401 && attempt == 0 => {
                    token = match self.access_token(true).await {
                        Ok(token) => token,
                        Err(TokenError::Invalid) => return ProbeResult::Invalid,
                        Err(TokenError::Temporary) => return ProbeResult::Temporary,
                    };
                }
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    return ProbeResult::Invalid
                }
                Ok(_) | Err(_) => return ProbeResult::Temporary,
            }
        }
        ProbeResult::Invalid
    }
}

#[derive(Deserialize)]
struct RefreshResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Clone, Copy)]
enum ProbeResult {
    Healthy,
    RateLimited,
    Invalid,
    Temporary,
}

fn read_private_file(path: &str, description: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(path);
    if !path.is_absolute() {
        bail!("{description} path must be absolute");
    }
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat {description} file"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{description} path must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("{description} file must not be accessible by group or other users");
        }
    }
    fs::read(path).with_context(|| format!("read {description} file"))
}

fn load_profiles(cfg: &GeminiConfig) -> anyhow::Result<Vec<LoadedProfile>> {
    let roster_path = Path::new(&cfg.profiles_file);
    if !roster_path.exists() {
        return Ok(Vec::new());
    }
    let raw = read_private_file(&cfg.profiles_file, "Gemini profiles")
        .with_context(|| format!("read Gemini profiles file {}", cfg.profiles_file))?;
    let document: GeminiProfilesFile =
        serde_json::from_slice(&raw).context("parse Gemini profiles file")?;
    let credential_root = roster_path
        .parent()
        .context("Gemini profiles file has no parent")?
        .join("credentials");
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut proxies = HashSet::new();
    let mut proxy_orders = HashSet::new();
    let mut profiles = Vec::with_capacity(document.profiles.len());
    for source in document.profiles {
        gemini_credential::validate_profile_id(&source.id)?;
        if !ids.insert(source.id.clone()) {
            bail!("duplicate Gemini profile id");
        }
        let expected = credential_root.join(format!("{}.json", source.id));
        if Path::new(&source.credential_file) != expected {
            bail!("Gemini credential path does not match the sealed roster layout");
        }
        let encrypted = read_private_file(&source.credential_file, "Gemini credential")?;
        let envelope = decode_envelope(&encrypted).context("parse Gemini credential envelope")?;
        let credential = cfg
            .credential_keys
            .open(&source.id, &envelope)
            .context("open Gemini credential envelope")?;
        if !subjects.insert(credential.subject.clone()) {
            bail!("duplicate Gemini subscription identity in profiles file");
        }
        if !credential.proxy.is_empty() && !proxies.insert(credential.proxy.clone()) {
            bail!("duplicate Gemini profile proxy in profiles file");
        }
        if credential.proxy_order_id > 0 && !proxy_orders.insert(credential.proxy_order_id) {
            bail!("duplicate Gemini IPRoyal order id in profiles file");
        }
        profiles.push(LoadedProfile {
            source,
            credential,
            fingerprint: *blake3::hash(&encrypted).as_bytes(),
        });
    }
    Ok(profiles)
}

pub(crate) struct GeminiLease {
    profile: Arc<GeminiProfile>,
}

impl GeminiLease {
    pub(crate) fn profile(&self) -> &Arc<GeminiProfile> {
        &self.profile
    }
}

impl Drop for GeminiLease {
    fn drop(&mut self) {
        self.profile.inflight.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct GeminiGateway {
    cfg: Arc<GeminiConfig>,
    profiles: RwLock<Vec<Arc<GeminiProfile>>>,
    cursor: AtomicU64,
    shutting_down: AtomicBool,
    abort_streams: AtomicBool,
    abort_notify: tokio::sync::Notify,
    background_tasks: Arc<tokio::sync::Semaphore>,
}

impl GeminiGateway {
    pub fn new(cfg: GeminiConfig) -> anyhow::Result<Self> {
        if !cfg.enabled {
            bail!("Gemini provider is disabled");
        }
        if cfg.models.is_empty() {
            bail!("Gemini model allowlist is empty");
        }
        let loaded = load_profiles(&cfg)?;
        let cfg = Arc::new(cfg);
        let profiles = loaded
            .into_iter()
            .map(|profile| GeminiProfile::new(profile, &cfg).map(Arc::new))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            cfg,
            profiles: RwLock::new(profiles),
            cursor: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            abort_streams: AtomicBool::new(false),
            abort_notify: tokio::sync::Notify::new(),
            background_tasks: Arc::new(tokio::sync::Semaphore::new(MAX_BACKGROUND_TASKS as usize)),
        })
    }

    pub fn config(&self) -> &GeminiConfig {
        &self.cfg
    }

    fn profiles_snapshot(&self) -> Vec<Arc<GeminiProfile>> {
        self.profiles
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn reload_profiles(&self) -> anyhow::Result<bool> {
        let loaded = load_profiles(&self.cfg)?;
        let current = self.profiles_snapshot();
        let mut next = Vec::with_capacity(loaded.len());
        for loaded_profile in loaded {
            if let Some(profile) = current
                .iter()
                .find(|profile| profile.matches(&loaded_profile))
            {
                next.push(profile.clone());
            } else {
                let profile = Arc::new(GeminiProfile::new(loaded_profile, &self.cfg)?);
                profile.authenticated.store(false, Ordering::Release);
                profile.cool_until(pool::now() + 1);
                next.push(profile);
            }
        }
        if next.len() == current.len()
            && next
                .iter()
                .zip(&current)
                .all(|(left, right)| Arc::ptr_eq(left, right))
        {
            return Ok(false);
        }
        *self
            .profiles
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
        Ok(true)
    }

    pub(crate) fn select(
        &self,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
    ) -> Option<GeminiLease> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let profiles = self.profiles_snapshot();
        let now = pool::now();
        let ready = |profile: &&Arc<GeminiProfile>| {
            !excluded.contains(profile.id())
                && profile.cooling_until.load(Ordering::Acquire) <= now
                && profile.authenticated.load(Ordering::Acquire)
        };
        let preferred = preferred_id.and_then(|id| {
            profiles
                .iter()
                .find(|profile| profile.id() == id && ready(profile))
        });
        let profile = if let Some(profile) = preferred {
            profile.clone()
        } else {
            let offset = self.cursor.fetch_add(1, Ordering::Relaxed) as usize;
            profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| ready(profile))
                .min_by_key(|(index, profile)| {
                    (
                        profile.inflight.load(Ordering::Acquire),
                        (index + profiles.len() - offset % profiles.len()) % profiles.len(),
                    )
                })?
                .1
                .clone()
        };
        profile.inflight.fetch_add(1, Ordering::AcqRel);
        Some(GeminiLease { profile })
    }

    pub(crate) fn profile_id_for_home(
        &self,
        affinity: &crate::AffinityStore,
        home: &str,
    ) -> Option<String> {
        self.profiles_snapshot()
            .iter()
            .find(|profile| affinity.home_id(profile.id()) == home)
            .map(|profile| profile.id().to_string())
    }

    pub(crate) fn soonest_ready(&self, excluded: &HashSet<String>) -> Option<i64> {
        let now = pool::now();
        self.profiles_snapshot()
            .iter()
            .filter(|profile| !excluded.contains(profile.id()))
            .map(|profile| profile.cooling_until.load(Ordering::Acquire))
            .filter(|until| *until > now)
            .min()
    }

    pub(crate) fn track_background_task(
        &self,
    ) -> anyhow::Result<tokio::sync::OwnedSemaphorePermit> {
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("Gemini provider is shutting down");
        }
        let permit = self
            .background_tasks
            .clone()
            .try_acquire_owned()
            .context("Gemini background task limit reached")?;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("Gemini provider is shutting down");
        }
        Ok(permit)
    }

    pub(crate) async fn stream_abort_requested(&self) {
        loop {
            let notified = self.abort_notify.notified();
            if self.abort_streams.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    fn abort_active_streams(&self) {
        self.abort_streams.store(true, Ordering::Release);
        self.abort_notify.notify_waiters();
    }

    pub async fn operational_status(&self) -> GeminiOperationalStatus {
        let now = pool::now();
        let profiles: Vec<_> = self
            .profiles_snapshot()
            .iter()
            .map(|profile| profile.status())
            .collect();
        GeminiOperationalStatus {
            available: profiles
                .iter()
                .filter(|profile| profile.authenticated && profile.cooling_until <= now)
                .count(),
            authenticated: profiles
                .iter()
                .filter(|profile| profile.authenticated)
                .count(),
            soonest_ready: profiles
                .iter()
                .map(|profile| profile.cooling_until)
                .filter(|until| *until > now)
                .min(),
            profiles,
        }
    }

    pub async fn preflight(&self) -> anyhow::Result<()> {
        let profile_count = self.profiles_snapshot().len();
        if profile_count == 0 {
            eprintln!("Gemini OAuth provider starting with an empty encrypted roster");
            return Ok(());
        }
        let healthy = self.probe_profiles().await;
        if healthy == 0 {
            bail!("Gemini provider preflight found no authenticated subscription");
        }
        if healthy < profile_count {
            eprintln!(
                "Gemini provider starting with {healthy}/{} authenticated profiles",
                profile_count
            );
        }
        Ok(())
    }

    pub async fn probe_health(&self) {
        if !self.shutting_down.load(Ordering::Acquire) {
            if let Err(error) = self.reload_profiles() {
                // The message is deliberately generic. Parser/decryption errors may otherwise
                // reveal a deployment path or key id through journalctl.
                let _ = error;
                eprintln!("Gemini encrypted roster reload skipped");
            }
            self.probe_profiles().await;
        }
    }

    pub async fn refresh_profiles(&self) -> bool {
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        match self.reload_profiles() {
            Ok(true) => {
                self.probe_profiles().await;
                true
            }
            Ok(false) => false,
            Err(_) => {
                eprintln!("Gemini encrypted roster refresh skipped");
                false
            }
        }
    }

    async fn probe_profiles(&self) -> usize {
        let mut healthy = 0usize;
        let now = pool::now();
        let profiles = self.profiles_snapshot();
        if profiles.is_empty() {
            return 0;
        }
        let probes = futures_util::stream::iter(profiles.iter().cloned().map(|profile| {
            let cfg = self.cfg.clone();
            async move {
                let result = profile.probe(&cfg).await;
                (profile, result)
            }
        }))
        .buffer_unordered(HEALTH_PROBE_CONCURRENCY.min(profiles.len()));
        tokio::pin!(probes);
        while let Some((profile, result)) = probes.next().await {
            match result {
                ProbeResult::Healthy => {
                    profile.mark_healthy();
                    healthy += 1;
                }
                ProbeResult::RateLimited => {
                    profile.authenticated.store(true, Ordering::Release);
                    profile.cool_until(now + self.cfg.default_rate_limit_cool_secs);
                    healthy += 1;
                }
                ProbeResult::Invalid => {
                    profile.mark_auth_failed(now + self.cfg.auth_quarantine_secs);
                }
                ProbeResult::Temporary => {
                    profile.cool_until(now + self.cfg.transport_cool_secs);
                }
            }
        }
        healthy
    }

    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.shutting_down.store(true, Ordering::Release);
        let barrier = self
            .background_tasks
            .clone()
            .acquire_many_owned(MAX_BACKGROUND_TASKS);
        let _background_barrier = match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, barrier).await {
                Ok(permit) => permit.ok(),
                Err(_) => {
                    self.abort_active_streams();
                    self.background_tasks
                        .clone()
                        .acquire_many_owned(MAX_BACKGROUND_TASKS)
                        .await
                        .ok()
                }
            },
            None => barrier.await.ok(),
        };
        self.abort_active_streams();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gemini_credential::{encode_envelope, CredentialKeyring};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    fn fixture() -> (PathBuf, CredentialKeyring) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "gemini-oauth-pool-{}-{}",
            std::process::id(),
            u64::from_le_bytes(random)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let ring = CredentialKeyring::parse(&format!("current:{}", "66".repeat(32))).unwrap();
        (dir, ring)
    }

    fn credential(subject: &str, proxy: &str) -> GeminiCredential {
        GeminiCredential {
            version: 1,
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at: i64::MAX / 2,
            oauth_client_id: "client.apps.googleusercontent.com".into(),
            oauth_client_secret: "client-secret".into(),
            token_uri: "https://oauth2.googleapis.com/token".into(),
            subject: subject.into(),
            email: "owner@example.com".into(),
            project_id: "managed-project".into(),
            tier_id: "paid-tier".into(),
            tier_name: "Google AI Pro".into(),
            plan: "google_ai_pro".into(),
            proxy: proxy.into(),
            proxy_order_id: 0,
            issued_at: 1,
        }
    }

    fn write_credential(dir: &Path, ring: &CredentialKeyring, id: &str, subject: &str) -> PathBuf {
        write_credential_with_proxy(dir, ring, id, subject, "")
    }

    fn write_credential_with_proxy(
        dir: &Path,
        ring: &CredentialKeyring,
        id: &str,
        subject: &str,
        proxy: &str,
    ) -> PathBuf {
        write_credential_with_proxy_order(dir, ring, id, subject, proxy, 0)
    }

    fn write_credential_with_proxy_order(
        dir: &Path,
        ring: &CredentialKeyring,
        id: &str,
        subject: &str,
        proxy: &str,
        proxy_order_id: i64,
    ) -> PathBuf {
        let credential_dir = dir.join("credentials");
        fs::create_dir_all(&credential_dir).unwrap();
        let path = credential_dir.join(format!("{id}.json"));
        let mut credential = credential(subject, proxy);
        credential.proxy_order_id = proxy_order_id;
        let envelope = ring.seal("current", id, &credential).unwrap();
        fs::write(&path, encode_envelope(&envelope).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn config(profiles_file: &Path, ring: CredentialKeyring) -> GeminiConfig {
        GeminiConfig {
            enabled: true,
            upstream: "http://127.0.0.1:1".to_string(),
            profiles_file: profiles_file.to_string_lossy().into_owned(),
            credential_keys: ring,
            models: vec![super::super::config::GeminiModel {
                id: "gemini-test".to_string(),
                display_name: "Gemini Test".to_string(),
                input_token_limit: 100,
                output_token_limit: 10,
                prices: metering::GeminiPrices {
                    input: 1,
                    audio_input: 1,
                    cached_input: 1,
                    cached_audio_input: 1,
                    output: 1,
                    long_context_threshold: u64::MAX,
                    long_input: 1,
                    long_audio_input: 1,
                    long_cached_input: 1,
                    long_cached_audio_input: 1,
                    long_output: 1,
                    search: metering::GeminiSearchBilling::PerQuery { nano: 1 },
                },
            }],
            connect_timeout_secs: 1,
            read_timeout_secs: 1,
            max_transport_retries: 1,
            auth_quarantine_secs: 900,
            transport_cool_secs: 5,
            default_rate_limit_cool_secs: 60,
            health_probe_interval_secs: 60,
            reserve_overhead_tokens: 10,
        }
    }

    fn write_roster(path: &Path, profiles: &[(&str, &Path)]) {
        let profiles = profiles
            .iter()
            .map(|(id, credential)| {
                json!({"id": id, "credential_file": credential.to_string_lossy()})
            })
            .collect::<Vec<_>>();
        fs::write(path, json!({"profiles": profiles}).to_string()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    #[test]
    fn duplicate_accounts_are_rejected_even_with_different_projects_or_files() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "one", "same-subject");
        let second = write_credential(&dir, &ring, "two", "same-subject");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini subscription"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_profile_proxies_are_rejected_before_joining_rotation() {
        let (dir, ring) = fixture();
        let proxy = "http://user:pass@127.0.0.1:8080/";
        let first = write_credential_with_proxy(&dir, &ring, "one", "subject-one", proxy);
        let second = write_credential_with_proxy(&dir, &ring, "two", "subject-two", proxy);
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini profile proxy"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_iproyal_orders_are_rejected_before_joining_rotation() {
        let (dir, ring) = fixture();
        let first = write_credential_with_proxy_order(
            &dir,
            &ring,
            "one",
            "subject-one",
            "http://user:pass@127.0.0.1:8080/",
            42,
        );
        let second = write_credential_with_proxy_order(
            &dir,
            &ring,
            "two",
            "subject-two",
            "http://user:pass@127.0.0.2:8080/",
            42,
        );
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini IPRoyal order"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn production_profile_without_a_dedicated_proxy_fails_closed() {
        let (dir, ring) = fixture();
        let credential = write_credential(&dir, &ring, "one", "subject-one");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &credential)]);
        let mut cfg = config(&roster, ring);
        cfg.upstream = "https://cloudcode-pa.googleapis.com".into();
        let error = GeminiGateway::new(cfg).err().unwrap();
        assert!(error.to_string().contains("requires a dedicated proxy"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cooling_profile_is_skipped_and_identity_is_not_in_status() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "secret-subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "secret-subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        gateway.profiles_snapshot()[0].cool_until(pool::now() + 60);
        let lease = gateway.select(&HashSet::new(), None).unwrap();
        assert_eq!(lease.profile().id(), "profile_b");
        let status = format!("{:?}", gateway.operational_status().await);
        assert!(!status.contains("secret-subject"));
        assert!(!status.contains("owner@example.com"));
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn atomically_published_profile_is_reloaded_without_restart() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring.clone())).unwrap();
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let staging = dir.join(".profiles.pending");
        write_roster(&staging, &[("profile_a", &first), ("profile_b", &second)]);
        fs::rename(staging, &roster).unwrap();
        assert!(gateway.reload_profiles().unwrap());
        assert_eq!(gateway.operational_status().await.profiles.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_roster_can_boot_before_the_first_subscription_arrives() {
        let (dir, ring) = fixture();
        let missing = dir.join("profiles.json");
        let gateway = GeminiGateway::new(config(&missing, ring)).unwrap();
        assert!(gateway.profiles_snapshot().is_empty());
        let _ = fs::remove_dir_all(dir);
    }
}
