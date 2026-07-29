//! Paid Gemini credential/project pool with per-profile health, cooling and load-aware selection.

use super::config::{GeminiConfig, GeminiProfileSpec, GeminiProfilesFile};
use anyhow::{bail, Context};
use futures_util::StreamExt;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const MAX_BACKGROUND_TASKS: u32 = 4_096;
const HEALTH_PROBE_CONCURRENCY: usize = 16;

struct ApiKey(String);

impl std::fmt::Debug for ApiKey {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ApiKey(REDACTED)")
    }
}

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

pub(crate) struct GeminiProfile {
    id: String,
    #[allow(dead_code)]
    project_id: String,
    api_key: ApiKey,
    client: wreq::Client,
    inflight: AtomicUsize,
    cooling_until: AtomicI64,
    authenticated: AtomicBool,
}

fn normalize_project_id(project_id: &str) -> anyhow::Result<String> {
    let project_id = project_id.trim().to_ascii_lowercase();
    let bytes = project_id.as_bytes();
    if !(6..=30).contains(&bytes.len())
        || !bytes.first().is_some_and(u8::is_ascii_lowercase)
        || !bytes.last().is_some_and(u8::is_ascii_alphanumeric)
        || !bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
    {
        bail!(
            "Gemini project id must be a 6..=30 byte Google Cloud project id: lowercase letters, digits and hyphens, starting with a letter and ending with a letter or digit"
        );
    }
    Ok(project_id)
}

impl GeminiProfile {
    fn new(spec: GeminiProfileSpec, cfg: &GeminiConfig) -> anyhow::Result<Self> {
        validate_profile_id(&spec.id)?;
        let project_id = normalize_project_id(&spec.project_id)
            .with_context(|| format!("invalid project id for Gemini profile {}", spec.id))?;
        let api_key = read_api_key(&spec.api_key_file)
            .with_context(|| format!("read Gemini credential for profile {}", spec.id))?;
        let mut builder = wreq::Client::builder()
            .connect_timeout(Duration::from_secs(cfg.connect_timeout_secs))
            .read_timeout(Duration::from_secs(cfg.read_timeout_secs))
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60));
        if let Some(proxy) = spec
            .proxy
            .as_deref()
            .filter(|proxy| !proxy.trim().is_empty())
        {
            builder =
                builder.proxy(wreq::Proxy::all(proxy).context("invalid Gemini profile proxy URL")?);
        }
        let client = builder.build().context("build Gemini HTTP client")?;
        Ok(Self {
            id: spec.id,
            project_id,
            api_key: ApiKey(api_key),
            client,
            inflight: AtomicUsize::new(0),
            cooling_until: AtomicI64::new(0),
            authenticated: AtomicBool::new(true),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn request(&self, method: wreq::Method, url: &str) -> wreq::RequestBuilder {
        self.client
            .request(method, url)
            .header("x-goog-api-key", &self.api_key.0)
            .header("user-agent", "apitoken-gemini-gateway/1")
            .header("accept-encoding", "gzip, br")
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
}

fn validate_profile_id(id: &str) -> anyhow::Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("Gemini profile id must match [A-Za-z0-9_-] and be 1..=64 bytes");
    }
    Ok(())
}

fn read_api_key(path: &str) -> anyhow::Result<String> {
    let key = read_private_file(path, "Gemini API key")?;
    let key = key.trim();
    if key.len() < 20 || key.len() > 512 || !key.bytes().all(|byte| byte.is_ascii_graphic()) {
        bail!("Gemini API key file has an invalid value");
    }
    Ok(key.to_string())
}

fn read_private_file(path: &str, description: &str) -> anyhow::Result<String> {
    let path = Path::new(path);
    if !path.is_absolute() {
        bail!("{description} path must be absolute");
    }
    let symlink = fs::symlink_metadata(path).with_context(|| format!("stat {description} file"))?;
    if symlink.file_type().is_symlink() || !symlink.is_file() {
        bail!("{description} path must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if symlink.permissions().mode() & 0o077 != 0 {
            bail!("{description} file must not be accessible by group or other users");
        }
    }
    fs::read_to_string(path).with_context(|| format!("read {description} file"))
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
    profiles: Vec<Arc<GeminiProfile>>,
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
        // The document can contain authenticated proxy URLs, so protect it exactly like key files.
        let raw = read_private_file(&cfg.profiles_file, "Gemini profiles")
            .with_context(|| format!("read Gemini profiles file {}", cfg.profiles_file))?;
        let mut document: GeminiProfilesFile =
            serde_json::from_str(&raw).context("parse Gemini profiles file")?;
        if document.profiles.is_empty() {
            bail!("Gemini profiles file contains no profiles");
        }
        let mut ids = HashSet::new();
        let mut projects = HashSet::new();
        for profile in &mut document.profiles {
            if !ids.insert(profile.id.clone()) {
                bail!("duplicate Gemini profile id {}", profile.id);
            }
            profile.project_id = normalize_project_id(&profile.project_id)
                .with_context(|| format!("invalid project id for Gemini profile {}", profile.id))?;
            if !projects.insert(profile.project_id.clone()) {
                bail!(
                    "duplicate Gemini project in profiles file; quotas are per project, not per API key"
                );
            }
        }
        let cfg = Arc::new(cfg);
        let profiles = document
            .profiles
            .into_iter()
            .map(|profile| GeminiProfile::new(profile, &cfg).map(Arc::new))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            cfg,
            profiles,
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

    pub(crate) fn select(&self, excluded: &HashSet<String>) -> Option<GeminiLease> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let now = pool::now();
        let offset = self.cursor.fetch_add(1, Ordering::Relaxed) as usize;
        let profile = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| {
                !excluded.contains(profile.id())
                    && profile.cooling_until.load(Ordering::Acquire) <= now
            })
            .min_by_key(|(index, profile)| {
                (
                    profile.inflight.load(Ordering::Acquire),
                    (index + self.profiles.len() - offset % self.profiles.len())
                        % self.profiles.len(),
                )
            })?
            .1
            .clone();
        profile.inflight.fetch_add(1, Ordering::AcqRel);
        Some(GeminiLease { profile })
    }

    pub(crate) fn soonest_ready(&self, excluded: &HashSet<String>) -> Option<i64> {
        let now = pool::now();
        self.profiles
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
            .profiles
            .iter()
            .map(|profile| profile.status())
            .collect();
        GeminiOperationalStatus {
            available: profiles
                .iter()
                .filter(|profile| profile.cooling_until <= now)
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
        let healthy = self.probe_profiles().await;
        if healthy == 0 {
            bail!("Gemini provider preflight found no authenticated project");
        }
        if healthy < self.profiles.len() {
            eprintln!(
                "Gemini provider starting with {healthy}/{} authenticated profiles",
                self.profiles.len()
            );
        }
        Ok(())
    }

    pub async fn probe_health(&self) {
        if !self.shutting_down.load(Ordering::Acquire) {
            self.probe_profiles().await;
        }
    }

    async fn probe_profiles(&self) -> usize {
        let mut healthy = 0usize;
        let now = pool::now();
        let url = format!("{}/v1beta/models?pageSize=1", self.cfg.upstream);
        // Pools can contain many paid projects. Probe them concurrently with a fixed bound so
        // startup/health latency is not N × timeout and a large roster cannot create an unbounded
        // connection burst.
        let probes = futures_util::stream::iter(self.profiles.iter().cloned().map(|profile| {
            let url = url.clone();
            async move {
                let response = profile
                    .request(wreq::Method::GET, &url)
                    .timeout(Duration::from_secs(20))
                    .send()
                    .await;
                (profile, response)
            }
        }))
        .buffer_unordered(HEALTH_PROBE_CONCURRENCY.min(self.profiles.len()).max(1));
        tokio::pin!(probes);
        while let Some((profile, response)) = probes.next().await {
            match response {
                Ok(response) if response.status().is_success() => {
                    profile.mark_healthy();
                    healthy += 1;
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    profile.authenticated.store(true, Ordering::Release);
                    profile.cool_until(now + self.cfg.default_rate_limit_cool_secs);
                    healthy += 1;
                }
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    profile.mark_auth_failed(now + self.cfg.auth_quarantine_secs);
                }
                Ok(_) | Err(_) => {
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
                    // Detached streams own the authoritative final usage. At the drain deadline,
                    // ask each task to stop reading, settle its last complete snapshot, and only
                    // then cross the barrier so the server cannot flush billing too early.
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
    use std::os::unix::fs::PermissionsExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture() -> (std::path::PathBuf, std::path::PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("gemini-pool-{}-{unique}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let key = dir.join("key");
        fs::write(&key, "test-api-key-that-is-long-enough\n").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        (dir, key)
    }

    fn config(profiles_file: &Path) -> GeminiConfig {
        GeminiConfig {
            enabled: true,
            upstream: "http://127.0.0.1:1".to_string(),
            profiles_file: profiles_file.to_string_lossy().into_owned(),
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

    #[test]
    fn duplicate_projects_are_rejected_because_quota_is_project_scoped() {
        let (dir, key) = fixture();
        let profiles = dir.join("profiles.json");
        fs::write(
            &profiles,
            serde_json::json!({"profiles": [
                {"id":"one","project_id":"same-project","api_key_file":key},
                {"id":"two","project_id":"same-project","api_key_file":key}
            ]})
            .to_string(),
        )
        .unwrap();
        fs::set_permissions(&profiles, fs::Permissions::from_mode(0o600)).unwrap();
        let error = GeminiGateway::new(config(&profiles)).err().unwrap();
        assert!(error.to_string().contains("quotas are per project"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn project_ids_are_normalized_before_duplicate_detection() {
        let (dir, key) = fixture();
        let profiles = dir.join("profiles.json");
        fs::write(
            &profiles,
            serde_json::json!({"profiles": [
                {"id":"one","project_id":" Paid-Project-01 ","api_key_file":key},
                {"id":"two","project_id":"paid-project-01","api_key_file":key}
            ]})
            .to_string(),
        )
        .unwrap();
        fs::set_permissions(&profiles, fs::Permissions::from_mode(0o600)).unwrap();
        let error = GeminiGateway::new(config(&profiles)).err().unwrap();
        assert!(error.to_string().contains("quotas are per project"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_google_project_ids_are_rejected() {
        for project_id in ["short", "-starts-wrong", "ends-wrong-", "has_underscore"] {
            assert!(normalize_project_id(project_id).is_err(), "{project_id}");
        }
        assert_eq!(
            normalize_project_id(" Valid-Project-01 ").unwrap(),
            "valid-project-01"
        );
    }

    #[tokio::test]
    async fn cooling_profile_is_skipped_and_ids_do_not_expose_project() {
        let (dir, key) = fixture();
        let profiles = dir.join("profiles.json");
        fs::write(
            &profiles,
            serde_json::json!({"profiles": [
                {"id":"profile_a","project_id":"secret-project-a","api_key_file":key},
                {"id":"profile_b","project_id":"secret-project-b","api_key_file":key}
            ]})
            .to_string(),
        )
        .unwrap();
        fs::set_permissions(&profiles, fs::Permissions::from_mode(0o600)).unwrap();
        let gateway = GeminiGateway::new(config(&profiles)).unwrap();
        gateway.profiles[0].cool_until(pool::now() + 60);
        let lease = gateway.select(&HashSet::new()).unwrap();
        assert_eq!(lease.profile().id(), "profile_b");
        let status = gateway.operational_status().await;
        assert!(status
            .profiles
            .iter()
            .all(|profile| !profile.id.contains("secret-project")));
        let _ = fs::remove_dir_all(dir);
    }
}
