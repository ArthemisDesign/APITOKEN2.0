//! OpenAI-compatible text API backed by the official Codex app-server protocol.
//!
//! A legacy single-owner gateway speaks newline-delimited JSON-RPC v2 to private pinned Codex
//! children. Blue-green gateway slots speak the official websocket control protocol through
//! disposable `app-server proxy` bridges to separately supervised pinned daemons. Neither mode
//! reads or replays ChatGPT bearer tokens: authentication remains owned by official Codex profiles.

mod api;
mod billing;
mod calibration;
mod chat;
mod config;
mod discovery;
mod health;
mod history;
mod openai_snapshot;
mod process;
mod runner;

pub use api::{
    delete_response as openai_delete_response, get_response as openai_get_response,
    input_tokens as openai_input_tokens, model as openai_model, models as openai_models,
    response_input_items as openai_response_input_items, responses as openai_responses,
};
pub use calibration::WindowCalibration;
pub(crate) use calibration::{apply_observation_with_history, ESTIMATOR_VERSION};
pub use chat::completions as openai_chat_completions;
pub use config::{CodexConfig, CodexHomeSpec, CodexModel, CodexPrices, CodexTransport};
pub use history::{HistoryError, StoredHistory};
pub(crate) use process::{AppServerEvent, CodexProcess, ProcessError};
pub use process::{CodexRateLimitWindow, CodexRateLimits};
pub(crate) use runner::{CodexTurnRequest, CodexTurnResult, CodexUsage, TurnUpdate};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use crate::billing::AsyncBilling;
use history::HistoryStore;
use std::collections::{BTreeMap, HashSet};
use std::fs::{File, OpenOptions, TryLockError};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, OwnedSemaphorePermit, RwLock, Semaphore};

/// Cache-first routing context for one turn, mirroring the Claude fleet's affinity flow.
///
/// It carries the tenant-scoped cache lineage (`input`), the home this conversation is already
/// pinned to (`resolution`, if any), and the homes known to hold this request's shared cache root
/// (`warm`). After a turn succeeds the gateway records the served home back into the shared
/// `AffinityStore`, so a follow-up request lands on the same home and reuses its warm prompt cache.
/// Everything here is a fail-open optimization: losing it only lowers cache-hit rate, never money or
/// capacity. Owned (not borrowed) so it can move into the detached streaming task.
#[derive(Clone)]
pub(crate) struct TurnRouting {
    store: Arc<AffinityStore>,
    input: AffinityInput,
    resolution: Option<AffinityResolution>,
    warm: Vec<String>,
}

impl TurnRouting {
    pub(crate) fn new(
        store: Arc<AffinityStore>,
        input: AffinityInput,
        resolution: Option<AffinityResolution>,
        warm: Vec<String>,
    ) -> Self {
        Self {
            store,
            input,
            resolution,
            warm,
        }
    }

    fn preferred_home(&self) -> Option<&str> {
        self.resolution
            .as_ref()
            .map(|resolution| resolution.home.as_str())
    }

    fn prompt_cache_key(&self) -> String {
        self.input.prompt_cache_key(
            self.resolution
                .as_ref()
                .map(|resolution| resolution.session_id.as_str()),
        )
    }

    /// A shared cache root influences only the first placement of a new conversation. Once the
    /// conversation has a binding, that binding remains the stronger cache-continuity signal.
    fn places_cache_root(&self) -> bool {
        self.resolution.is_none() && self.input.has_cache_root()
    }

    /// Persist the affinity binding to the home that actually served the turn. Called only on
    /// success: a failed attempt must not pin a conversation to a home that could not serve it.
    async fn record_served(&mut self, served_home: &str) {
        let new_cache_root_placement = self.places_cache_root();
        let reused_warm_root = self.warm.iter().any(|home| home == served_home);
        match self.resolution.as_mut() {
            Some(resolution) => {
                if resolution.home != served_home {
                    self.store.rebind(resolution, served_home).await;
                }
                self.store.remember(&self.input, resolution).await;
            }
            None => {
                let resolution = self.store.claim(&self.input, served_home).await;
                self.resolution = Some(resolution);
            }
        }
        if new_cache_root_placement {
            self.store
                .record_cache_root_placement(&self.input, reused_warm_root);
        }
        self.store.mark_cache_warm(&self.input, served_home);
    }
}

/// Fallback wait advertised to a client when every home is limited but no window published a reset.
const DEFAULT_LIMIT_RETRY_SECS: u64 = 60;
/// Utilisation below which quota does not influence ordering at all.
///
/// Steering exists to avoid draining a subscription that is near its wall while another has room.
/// Below this line every home has plenty of room, so ranking them against each other would only
/// break fan-out: the emptiest home would win every tie and absorb the pool, which is the same
/// herding the rotation cursor exists to prevent. An unmeasured home ranks here too — no reading is
/// no reason to steer, and the sweep supplies one within a cadence.
const QUOTA_STEER_FLOOR_PERCENT: i64 = 50;
/// Granularity of quota steering above the floor. Coarse on purpose: homes within a few points of
/// each other are equally good choices and must stay tied so the rotation cursor still spreads them.
const QUOTA_STEER_BUCKET_PERCENT: i64 = 10;

/// Ordering rank derived from window utilisation. Lower is preferred; `0` means "do not steer".
fn quota_rank(used_percent: Option<i64>) -> i64 {
    used_percent
        .unwrap_or(0)
        .saturating_sub(QUOTA_STEER_FLOOR_PERCENT)
        .max(0)
        / QUOTA_STEER_BUCKET_PERCENT
}
/// Match the Claude fleet's cache-root fanout: one shared prompt prefix is deliberately warmed on
/// two competitive homes so independent sessions do not collapse onto the first subscription.
const CACHE_ROOT_MIN_WARM_HOMES: usize = 2;
/// Detached public stream tasks are bounded by the server-wide admission limit in practice. This
/// larger private semaphore provides a quiescence barrier without imposing a lower runtime cap.
const MAX_BACKGROUND_TASKS: u32 = 1_000_000;
/// Once the server's advertised drain deadline has expired, cleanup itself must also be bounded.
/// systemd cannot start the replacement singleton while the old main PID remains in `deactivating`.
const FORCED_SHUTDOWN_CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug)]
struct ProviderLock {
    _file: File,
}

fn acquire_provider_lock(path: &str) -> Result<ProviderLock, ProcessError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| ProcessError::HomeLockUnavailable)?;
    match file.try_lock() {
        Ok(()) => Ok(ProviderLock { _file: file }),
        Err(TryLockError::WouldBlock) => Err(ProcessError::HomeInUse),
        Err(TryLockError::Error(_)) => Err(ProcessError::HomeLockUnavailable),
    }
}

#[derive(Debug)]
struct HomeIdentity {
    /// Keep the original directory inode alive so remove/recreate cannot recycle its `(dev, ino)`
    /// pair and make a replacement account look unchanged.
    #[cfg(unix)]
    _directory: File,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl HomeIdentity {
    fn capture(path: &str) -> Option<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let directory = File::open(path).ok()?;
            let metadata = directory.metadata().ok()?;
            Some(Self {
                _directory: directory,
                device: metadata.dev(),
                inode: metadata.ino(),
            })
        }
        #[cfg(not(unix))]
        {
            std::fs::metadata(path).ok()?;
            Some(Self {})
        }
    }

    fn is_current(&self, path: &str) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            std::fs::metadata(path)
                .is_ok_and(|metadata| metadata.dev() == self.device && metadata.ino() == self.inode)
        }
        #[cfg(not(unix))]
        {
            std::fs::metadata(path).is_ok()
        }
    }
}

pub(crate) fn new_id(prefix: &str) -> String {
    let mut random = [0u8; 16];
    if getrandom::fill(&mut random).is_err() {
        return format!("{prefix}_{}", crate::upstream::fresh_request_id());
    }
    let mut hex = String::with_capacity(32);
    for byte in random {
        use std::fmt::Write as _;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    format!("{prefix}_{hex}")
}

/// Per-home operational state. Homes are identified by their index in the configured list, never by
/// path or account identity: metrics and logs must not carry customer or subscription identity.
#[derive(Clone, Debug)]
pub struct CodexHomeStatus {
    /// Stable, non-identifying id. Never a path and never an account identity.
    pub id: String,
    pub process_live: bool,
    pub auth_ok: bool,
    /// `healthy` | `suspect` | `dead` — liveness of the subscription, independent of the transport.
    pub account_state: &'static str,
    /// `responsive` | `degraded` | `wedged` — responsiveness of the current app-server generation.
    /// A home can be `wedged` while its child process is perfectly alive: production hit exactly
    /// that when a daemon was replaced underneath a still-running proxy bridge.
    pub transport_state: &'static str,
    /// The single verdict operator surfaces must trust: whether selection would route here now.
    pub admitted: bool,
    /// Why the gateway is refusing to route here, when it is. Named by the same policy that made
    /// the decision, so an operator never has to reverse-engineer the verdict from raw gauges.
    pub reject_reason: Option<&'static str>,
    /// Age of the rate-limit snapshot. A growing age is the signal that the refresh path is broken,
    /// which is invisible in the snapshot's own values.
    pub snapshot_age_secs: Option<i64>,
    pub cooling_until: i64,
    pub inflight: usize,
    pub rate_limits: Option<CodexRateLimits>,
    /// True when the provider snapshot puts this home outside rotation: an explicit reached verdict
    /// or a reported window at full utilisation. Computed by the same predicate selection uses, so
    /// operator surfaces can never show `active` for a home the gateway refuses to route to.
    pub limit_reached: bool,
    /// Cumulative official-price spend this home has served through the gateway.
    pub spend_usd_total: f64,
    /// False after a persistence failure (or in an explicitly in-memory test gateway).
    pub calibration_persistence_ok: bool,
    /// Capacity estimate per reported window slot (empty until the first snapshot arrives).
    pub capacities: Vec<CodexWindowCapacityReport>,
}

/// Evidence-backed capacity of one provider-reported subscription window.
#[derive(Clone, Debug)]
pub struct CodexWindowCapacityReport {
    /// `primary` or `secondary`, mirroring the provider's rate-limit payload.
    pub slot: &'static str,
    pub window_minutes: Option<i64>,
    pub resets_at: Option<i64>,
    pub observed_at: i64,
    pub data_age_seconds: Option<i64>,
    pub used_percent: i64,
    /// `None` until a real positive utilisation movement is paired with gateway spend.
    pub cap_usd: Option<f64>,
    pub remaining_usd: Option<f64>,
    pub low_usd: Option<f64>,
    pub high_usd: Option<f64>,
    /// `measured_cumulative` or `unknown`.
    pub source: &'static str,
    pub confidence: f64,
    pub samples: i64,
}

#[derive(Clone, Debug)]
pub struct CodexOperationalStatus {
    /// True when at least one home has a live child. Kept as the provider-level liveness signal.
    pub process_live: bool,
    /// Snapshot of the most constrained home, for the provider-level rate-limit metrics.
    pub rate_limits: Option<CodexRateLimits>,
    pub homes: Vec<CodexHomeStatus>,
    /// Homes that could accept a request right now (live-or-startable, not cooling and without a
    /// provider limit — an explicit reached verdict or a window at full utilisation).
    pub available: usize,
    /// Unix time when the first unavailable home is expected back, if any is cooling.
    pub soonest_ready: Option<i64>,
}

/// One authenticated `CODEX_HOME` and the child that serves it.
pub(crate) struct CodexHome {
    spec: CodexHomeSpec,
    identity: Option<HomeIdentity>,
    /// Position in the discovered pool: explicit homes first, then scanned ones in name order.
    /// Only a tie-break for selection, so it may be renumbered freely as the pool changes; the
    /// stable identity used for labels is `spec.id`.
    order: AtomicUsize,
    cfg: Arc<CodexConfig>,
    process: Mutex<Option<Arc<CodexProcess>>>,
    process_start: Arc<Mutex<()>>,
    retired: Arc<AtomicBool>,
    /// Turns in flight on this home right now. Concurrency is deliberately unbounded: app-server
    /// serializes work per thread, while independent ephemeral API threads are multiplexed over the
    /// same authenticated process. This counter is only a load signal for selection and metrics.
    inflight: Arc<AtomicUsize>,
    turns_idle: Arc<Notify>,
    /// Health and admission policy for this home, on two independent axes (account and transport).
    /// A plain `std::sync::Mutex`: every critical section is a few field writes and is never held
    /// across an await, so an async lock would only add cost.
    health: std::sync::Mutex<health::HomeHealth>,
    /// Last durable cumulative official-price spend returned by the authority (or local total in
    /// an explicitly in-memory test gateway).
    spend_nano_total: AtomicI64,
    /// Credits that failed persistence are retried with the next successful turn.
    pending_spend_nano: AtomicI64,
    calibration_persistence_ok: AtomicBool,
    billing: Option<Arc<AsyncBilling>>,
    /// Provider slots are presentation only. Estimator identity is the actual duration; a primary
    /// and secondary window can change duration without inheriting each other's evidence.
    calibrations: std::sync::Mutex<BTreeMap<i64, WindowCalibration>>,
}

impl CodexHome {
    fn new(
        cfg: Arc<CodexConfig>,
        spec: CodexHomeSpec,
        order: usize,
        billing: Option<Arc<AsyncBilling>>,
    ) -> Self {
        Self {
            identity: HomeIdentity::capture(&spec.path),
            spec,
            order: AtomicUsize::new(order),
            cfg,
            process: Mutex::new(None),
            process_start: Arc::new(Mutex::new(())),
            retired: Arc::new(AtomicBool::new(false)),
            inflight: Arc::new(AtomicUsize::new(0)),
            turns_idle: Arc::new(Notify::new()),
            health: std::sync::Mutex::new(health::HomeHealth::new()),
            spend_nano_total: AtomicI64::new(0),
            pending_spend_nano: AtomicI64::new(0),
            calibration_persistence_ok: AtomicBool::new(billing.is_some()),
            billing,
            calibrations: std::sync::Mutex::new(BTreeMap::new()),
        }
    }

    /// Stable, non-identifying id used in logs and metric labels.
    pub(crate) fn id(&self) -> &str {
        &self.spec.id
    }

    fn path(&self) -> &str {
        &self.spec.path
    }

    fn order(&self) -> usize {
        self.order.load(Ordering::Relaxed)
    }

    fn config(&self) -> &CodexConfig {
        &self.cfg
    }

    fn inflight(&self) -> usize {
        self.inflight.load(Ordering::Relaxed)
    }

    /// Copy of the current health state. Cheap, and it keeps the lock out of the caller's hands.
    fn health(&self) -> health::HomeHealth {
        *self.health.lock().expect("codex home health lock")
    }

    /// Fold one observation into this home's health, persisting the account axis when it moves.
    ///
    /// Persistence is fire-and-forget on purpose: health must keep working when the authority is
    /// briefly unavailable. Losing a write costs one restart's worth of memory, while blocking a
    /// turn on it would make a database hiccup an availability incident.
    fn note(&self, signal: health::HealthSignal) {
        let (before, after) = {
            let mut guard = self.health.lock().expect("codex home health lock");
            let before = guard.durable();
            guard.apply(signal, pool::now());
            (before, guard.durable())
        };
        if before == after {
            return;
        }
        let (Some(billing), id) = (self.billing.clone(), self.spec.id.clone()) else {
            return;
        };
        tokio::spawn(async move {
            if let Err(error) = billing
                .save_codex_health(&id, durable_health_row(after), pool::now())
                .await
            {
                eprintln!("Codex home {id} health persistence failed [{error:#}]");
            }
        });
    }

    /// Recover this home's durable account verdict from the authority.
    ///
    /// Called as a home joins the pool. Without it every restart — and blue-green makes those
    /// routine — re-admitted a subscription that had already been corroborated dead or found
    /// quota-exhausted, so the pool rediscovered the same failure using customer traffic.
    async fn hydrate_health(&self) {
        let Some(billing) = self.billing.as_ref() else {
            return;
        };
        match billing.load_codex_health(self.id()).await {
            Ok(row) => {
                let durable = health::DurableAccountHealth {
                    account: health::AccountState::from_str(&row.account_state),
                    auth_fail_streak: row.auth_fail_streak,
                    first_auth_fail_ts: row.first_auth_fail_ts,
                    cooling_until: row.cooling_until,
                };
                self.health
                    .lock()
                    .expect("codex home health lock")
                    .restore(durable);
            }
            // Fail open: an unreadable verdict must not keep a working subscription out of the
            // pool. The next probe re-establishes the truth either way.
            Err(error) => eprintln!(
                "Codex home {} health could not be recovered [{error:#}]",
                self.id()
            ),
        }
    }

    fn cooling_until(&self) -> i64 {
        self.health().cooling_until
    }

    fn is_cooling(&self, now: i64) -> bool {
        self.health().is_cooling(now)
    }

    /// A completed probe proves both axes at once: the account answered and the transport carried
    /// the answer. Nothing else is allowed to clear health, so a home cannot be declared well by
    /// anything weaker than actually serving.
    fn mark_healthy(&self) {
        self.note(health::HealthSignal::ProbeOk);
    }

    /// A completed customer turn proves exactly the same two facts, earned from real traffic
    /// instead of an extra probe. This is the Codex counterpart of the Claude path harvesting live
    /// limits from every upstream response: healthy homes are kept verified by the work they do,
    /// so the background sweep only has to carry homes that are idle or already suspicious.
    fn mark_turn_healthy(&self) {
        self.note(health::HealthSignal::TurnOk);
    }

    /// Take an in-flight turn slot on this home. This is a load counter, not an admission cap.
    fn acquire_turn(self: &Arc<Self>) -> Option<TurnSlot> {
        if self.retired.load(Ordering::Acquire) {
            return None;
        }
        self.inflight.fetch_add(1, Ordering::AcqRel);
        if self.retired.load(Ordering::Acquire) {
            release_turn(&self.inflight, &self.turns_idle);
            return None;
        }
        Some(TurnSlot {
            inflight: self.inflight.clone(),
            idle: self.turns_idle.clone(),
        })
    }

    fn identity_is_current(&self) -> bool {
        match &self.identity {
            Some(identity) => identity.is_current(&self.spec.path),
            None => std::fs::metadata(&self.spec.path).is_err(),
        }
    }

    async fn wait_for_turns(&self) {
        loop {
            let notified = self.turns_idle.notified();
            if self.inflight.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }

    /// Return a live, subscription-authenticated process. Startup is serialized per home, but
    /// normal JSON-RPC requests and independent turns are multiplexed over the same child.
    pub(crate) async fn process(&self) -> Result<Arc<CodexProcess>, ProcessError> {
        if self.cfg.transport == CodexTransport::SharedDaemonProxy
            && discovery::daemon_draining(std::path::Path::new(&self.spec.path))
        {
            return Err(ProcessError::Closed);
        }
        if !self.identity_is_current() {
            self.retired.store(true, Ordering::Release);
        }
        if !self.retired.load(Ordering::Acquire) {
            if let Some(process) = self.process.lock().await.as_ref().cloned() {
                if process.is_ready() {
                    return Ok(process);
                }
            }
        }

        let _start = self.process_start.clone().lock_owned().await;
        if !self.identity_is_current() {
            self.retired.store(true, Ordering::Release);
        }
        let stale = {
            let mut current = self.process.lock().await;
            if !self.retired.load(Ordering::Acquire) {
                if let Some(process) = current.as_ref().filter(|process| process.is_ready()) {
                    return Ok(process.clone());
                }
            }
            current.take()
        };
        if let Some(stale) = stale {
            if let Err(error) = stale.shutdown().await {
                self.retired.store(true, Ordering::Release);
                *self.process.lock().await = Some(stale);
                return Err(error);
            }
        }
        if self.retired.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }

        // Publish the child before its first initialization await. If this future is cancelled, the
        // next caller finds the unready generation and reaps it instead of starting alongside it.
        let mut current = self.process.lock().await;
        let process = Arc::new(CodexProcess::launch(self.cfg.clone(), &self.spec).await?);
        *current = Some(process.clone());
        drop(current);
        if let Err(error) = process.start().await {
            if let Err(shutdown_error) = process.shutdown().await {
                self.retired.store(true, Ordering::Release);
                return Err(shutdown_error);
            }
            let mut current = self.process.lock().await;
            if current
                .as_ref()
                .is_some_and(|candidate| Arc::ptr_eq(candidate, &process))
            {
                *current = None;
            }
            return Err(error);
        }
        Ok(process)
    }

    async fn retire(&self) -> Result<(), ProcessError> {
        self.retired.store(true, Ordering::Release);
        // A turn can invalidate its process, so wait for all turn guards before taking process_start.
        self.wait_for_turns().await;
        let _start = self.process_start.clone().lock_owned().await;
        let process = self.process.lock().await.take();
        if let Some(process) = process {
            process.shutdown().await?;
        }
        Ok(())
    }

    async fn live_process(&self) -> Option<Arc<CodexProcess>> {
        self.process
            .lock()
            .await
            .as_ref()
            .filter(|process| process.is_ready())
            .cloned()
    }

    pub(crate) async fn invalidate(&self, process: &Arc<CodexProcess>) {
        let start = self.process_start.clone().lock_owned().await;
        // Claim this generation before awaiting shutdown. Concurrent failed turns can all observe
        // the same dead child; only one may kill/reap it. Clearing the published pointer first also
        // prevents a new selector from attaching work to a generation already being retired.
        let claimed = {
            let mut current = self.process.lock().await;
            if current
                .as_ref()
                .is_some_and(|candidate| Arc::ptr_eq(candidate, process))
            {
                *current = None;
                true
            } else {
                false
            }
        };
        if !claimed {
            return;
        }
        let process = process.clone();
        let retired = self.retired.clone();
        let shutdown = tokio::spawn(async move {
            // Keep the per-home generation fence held even if the request that noticed the dead
            // transport is cancelled by shutdown. A replacement cannot start until the detached,
            // cancellation-safe child reaper has completed.
            let _start = start;
            let result = process.shutdown().await;
            if result.is_err() {
                retired.store(true, Ordering::Release);
            }
            result
        });
        let result = match shutdown.await {
            Ok(result) => result,
            Err(error) => {
                self.retired.store(true, Ordering::Release);
                Err(ProcessError::Spawn(format!(
                    "Codex invalidation task failed: {error}"
                )))
            }
        };
        if let Err(error) = result {
            eprintln!(
                "Codex child invalidation failed [{}]",
                error.diagnostic_class()
            );
            return;
        }
    }

    async fn rate_limits(&self) -> Option<CodexRateLimits> {
        self.observed_rate_limits().await
    }

    /// Read the cached snapshot and feed it to duration-keyed window calibration.
    async fn observed_rate_limits(&self) -> Option<CodexRateLimits> {
        let limits = match self.live_process().await {
            Some(process) => process.rate_limits().await,
            None => None,
        };
        if let Some(limits) = &limits {
            self.note_rate_limits(limits).await;
        }
        limits
    }

    /// Credit one completed turn's exact official-price cost to this home's calibration spend.
    /// Called for every served turn regardless of customer billing (admin turns consume the
    /// subscription window exactly the same). Failed persistence is retained as a pending delta
    /// and retried on the next successful turn.
    pub(crate) async fn record_spend(&self, real_nano: i128) {
        let nano = real_nano.clamp(0, i64::MAX as i128) as i64;
        if nano == 0 {
            return;
        }
        let Some(billing) = &self.billing else {
            self.spend_nano_total.fetch_add(nano, Ordering::Relaxed);
            self.calibration_persistence_ok
                .store(false, Ordering::Relaxed);
            return;
        };
        self.pending_spend_nano.fetch_add(nano, Ordering::AcqRel);
        let pending = self.pending_spend_nano.swap(0, Ordering::AcqRel);
        match billing
            .credit_codex_spend(self.id(), pending, pool::now())
            .await
        {
            Ok(total) => {
                self.spend_nano_total.fetch_max(total, Ordering::Relaxed);
                self.calibration_persistence_ok.store(
                    self.pending_spend_nano.load(Ordering::Acquire) == 0,
                    Ordering::Relaxed,
                );
            }
            Err(error) => {
                self.pending_spend_nano.fetch_add(pending, Ordering::AcqRel);
                self.calibration_persistence_ok
                    .store(false, Ordering::Relaxed);
                eprintln!(
                    "Codex calibration spend persistence failed [{}]",
                    error.root_cause()
                );
            }
        }
    }

    fn spend_usd_total(&self) -> f64 {
        self.spend_nano_total.load(Ordering::Relaxed) as f64 / 1e9
    }

    async fn note_rate_limits(&self, limits: &CodexRateLimits) {
        let mut all_persisted = self.billing.is_some();
        for window in limits.primary.iter().chain(limits.secondary.iter()) {
            let (Some(duration), Some(resets_at)) = (window.window_duration_mins, window.resets_at)
            else {
                continue;
            };
            if duration <= 0 || resets_at <= 0 {
                continue;
            }
            let persisted = match &self.billing {
                Some(billing) => {
                    billing
                        .observe_codex_window(
                            self.id(),
                            duration,
                            resets_at,
                            window.used_percent,
                            limits.observed_at,
                        )
                        .await
                }
                None => Err(anyhow::anyhow!("in-memory calibration")),
            };
            match persisted {
                Ok((spend_nano, row)) => {
                    self.spend_nano_total
                        .fetch_max(spend_nano, Ordering::Relaxed);
                    self.calibrations
                        .lock()
                        .expect("Codex calibration map lock")
                        .insert(duration, WindowCalibration::from_row(row));
                }
                Err(error) => {
                    all_persisted = false;
                    let spend_nano = self.spend_nano_total.load(Ordering::Relaxed);
                    let observation = registry::CodexWindowObservation {
                        home_id: self.id().to_owned(),
                        window_duration_mins: duration,
                        resets_at,
                        observed_at: limits.observed_at,
                        used_percent: window.used_percent,
                        gateway_spend_nano: spend_nano,
                    };
                    let mut calibrations = self
                        .calibrations
                        .lock()
                        .expect("Codex calibration map lock");
                    let existing = calibrations.remove(&duration).map(|cal| cal.into_row());
                    let mut row = calibration::apply_observation(existing, &observation);
                    if row.version == 0 {
                        row.version = 1;
                    }
                    calibrations.insert(duration, WindowCalibration::from_row(row));
                    if self.billing.is_some() {
                        eprintln!(
                            "Codex window calibration persistence failed [{}]",
                            error.root_cause()
                        );
                    }
                }
            }
        }
        self.calibration_persistence_ok.store(
            all_persisted && self.pending_spend_nano.load(Ordering::Acquire) == 0,
            Ordering::Relaxed,
        );
    }

    /// Capacity report for one provider slot. Slot names are presentation only; evidence lookup is
    /// exclusively by the provider-reported duration.
    fn capacity_report(
        &self,
        slot: &'static str,
        window: &CodexRateLimitWindow,
        snapshot_observed_at: i64,
    ) -> CodexWindowCapacityReport {
        let calibrations = self
            .calibrations
            .lock()
            .expect("Codex calibration map lock");
        let calibration = window
            .window_duration_mins
            .and_then(|duration| calibrations.get(&duration));
        let estimate = calibration.and_then(WindowCalibration::estimate);
        let nano_to_usd = |nano: i64| nano as f64 / 1e9;
        // Keep the timestamp of the latest accepted complete interval instead of making cumulative
        // evidence look fresh merely because the provider emitted another snapshot.
        let observed_at = estimate
            .map(|value| value.measured_at)
            .unwrap_or(snapshot_observed_at);
        CodexWindowCapacityReport {
            slot,
            window_minutes: window.window_duration_mins,
            resets_at: window.resets_at,
            observed_at,
            data_age_seconds: (observed_at > 0)
                .then(|| pool::now().saturating_sub(observed_at).max(0)),
            used_percent: window.used_percent,
            cap_usd: estimate.map(|value| nano_to_usd(value.capacity_nano)),
            remaining_usd: calibration
                .and_then(|cal| cal.remaining_nano(window.used_percent))
                .map(nano_to_usd),
            low_usd: estimate.and_then(|value| value.low_nano).map(nano_to_usd),
            high_usd: estimate.and_then(|value| value.high_nano).map(nano_to_usd),
            source: estimate.map_or("unknown", |value| value.source.as_str()),
            confidence: estimate.map_or(0.0, |value| value.confidence_bp as f64 / 10_000.0),
            samples: calibration.map_or(0, |cal| cal.row().samples),
        }
    }

    /// Only an explicit provider reached verdict, or a reported window at full utilisation, blocks
    /// selection.
    ///
    /// `usedPercent` is quantised to whole percent, so `100` can arrive slightly before the true
    /// wall. That remainder is not worth selling: a subscription the provider reports as full stops
    /// answering, and every turn routed to it burns a customer request on a home that cannot serve
    /// it. Selection is fail-closed — an excluded home turns into a real `429 + Retry-After` on the
    /// window reset — so the cost of leaving early is bounded by one window's rounding remainder.
    fn within_provider_limit(limits: Option<&CodexRateLimits>) -> bool {
        let Some(limits) = limits else {
            return true;
        };
        if limits.reached {
            return false;
        }
        limits.max_used_percent().is_none_or(|used| used < 100)
    }

    /// Normalise a protocol snapshot into the plain view the health policy consumes, so `health.rs`
    /// stays free of transport types and remains testable without the app-server protocol.
    fn limit_view(limits: Option<&CodexRateLimits>) -> Option<health::LimitView> {
        limits.map(|limits| health::LimitView {
            reached: limits.reached,
            max_used_percent: limits.max_used_percent(),
            observed_at: limits.observed_at,
            soonest_reset_at: limits.soonest_reset_at_or_above(100),
        })
    }

    /// Sweep cadence, used to express snapshot staleness in probe intervals rather than a constant.
    fn probe_interval_secs(&self) -> i64 {
        self.cfg.health_probe_interval_secs.max(1) as i64
    }

    /// This home's admission verdict right now.
    fn admission(&self, limits: Option<&CodexRateLimits>, now: i64) -> health::Admission {
        self.health().admission(
            Self::limit_view(limits).as_ref(),
            now,
            self.probe_interval_secs(),
        )
    }

    async fn status(&self) -> CodexHomeStatus {
        let process_live = self.live_process().await.is_some();
        let rate_limits = self.observed_rate_limits().await;
        let health = self.health();
        let now = pool::now();
        let mut capacities = Vec::new();
        if let Some(limits) = &rate_limits {
            if let Some(window) = &limits.primary {
                capacities.push(self.capacity_report("primary", window, limits.observed_at));
            }
            if let Some(window) = &limits.secondary {
                capacities.push(self.capacity_report("secondary", window, limits.observed_at));
            }
        }
        let admission = health.admission(
            Self::limit_view(rate_limits.as_ref()).as_ref(),
            now,
            self.probe_interval_secs(),
        );
        CodexHomeStatus {
            id: self.spec.id.clone(),
            process_live,
            auth_ok: health.account != health::AccountState::Dead,
            account_state: health.account.as_str(),
            transport_state: health.transport.as_str(),
            // Reported by the same predicate selection uses, so an operator surface can never show
            // a home as routable while the gateway refuses to route to it.
            admitted: admission.is_admitted(),
            reject_reason: match admission {
                health::Admission::Reject { reason, .. } => Some(reason.as_str()),
                health::Admission::Admit { .. } => None,
            },
            cooling_until: health.cooling_until,
            inflight: self.inflight(),
            limit_reached: !Self::within_provider_limit(rate_limits.as_ref()),
            snapshot_age_secs: rate_limits
                .as_ref()
                .map(|limits| now.saturating_sub(limits.observed_at)),
            rate_limits,
            spend_usd_total: self.spend_usd_total(),
            calibration_persistence_ok: self.calibration_persistence_ok.load(Ordering::Relaxed),
            capacities,
        }
    }

    /// When this home is expected to be usable again.
    async fn ready_at(&self) -> i64 {
        let cooling = self.cooling_until();
        let limited_until = match self.rate_limits().await {
            Some(limits) if !Self::within_provider_limit(Some(&limits)) => limits
                .soonest_reset_at_or_above(100)
                .unwrap_or_else(|| pool::now().saturating_add(DEFAULT_LIMIT_RETRY_SECS as i64)),
            _ => 0,
        };
        cooling.max(limited_until)
    }
}

#[cfg(test)]
mod provider_lock_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_HOME: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn admission_rejects_full_windows_and_explicit_provider_limit() {
        let observed_hundred = CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_percent: 100,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            reached: false,
            observed_at: 100,
        };
        // A window the provider reports as full leaves rotation immediately, without waiting for
        // the explicit reached verdict that may never arrive.
        assert!(!CodexHome::within_provider_limit(Some(&observed_hundred)));
        assert!(!CodexHome::within_provider_limit(Some(&CodexRateLimits {
            reached: true,
            ..observed_hundred.clone()
        })));
        // Anything below the wall stays routable, and a missing snapshot stays fail-open.
        assert!(CodexHome::within_provider_limit(Some(&CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_percent: 99,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            ..observed_hundred.clone()
        })));
        assert!(CodexHome::within_provider_limit(Some(&CodexRateLimits {
            primary: None,
            ..observed_hundred
        })));
        assert!(CodexHome::within_provider_limit(None));
    }

    #[test]
    fn secondary_window_at_the_wall_also_blocks_admission() {
        let limits = CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_percent: 32,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(CodexRateLimitWindow {
                used_percent: 100,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_444_800),
            }),
            reached: false,
            observed_at: 100,
        };
        assert!(!CodexHome::within_provider_limit(Some(&limits)));
        // The client-facing wait is the reset of the window that is actually full.
        assert_eq!(
            limits.soonest_reset_at_or_above(100),
            Some(4_102_444_800),
            "retry-after follows the exhausted window"
        );
    }

    fn workspace(label: &str) -> std::path::PathBuf {
        let suffix = NEXT_HOME.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "claude-api-codex-{label}-{}-{suffix}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn provider_lock_fences_concurrent_owner_and_releases_on_drop() {
        let root = workspace("provider-lock");
        let path = root.join("ownership.lock");
        std::fs::write(&path, []).unwrap();
        let path = path.to_str().unwrap();

        let owner = acquire_provider_lock(path).unwrap();
        assert!(matches!(
            acquire_provider_lock(path),
            Err(ProcessError::HomeInUse)
        ));
        drop(owner);
        drop(acquire_provider_lock(path).unwrap());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replacing_a_home_does_not_replace_the_provider_fence() {
        let root = workspace("home-replace");
        let home = root.join("homes/account");
        std::fs::create_dir_all(&home).unwrap();
        let lock = root.join("ownership.lock");
        std::fs::write(&lock, []).unwrap();

        let owner = acquire_provider_lock(lock.to_str().unwrap()).unwrap();
        std::fs::remove_dir_all(&home).unwrap();
        std::fs::create_dir(&home).unwrap();
        assert!(matches!(
            acquire_provider_lock(lock.to_str().unwrap()),
            Err(ProcessError::HomeInUse)
        ));

        drop(owner);
        std::fs::remove_dir_all(root).unwrap();
    }
}

/// Translate the policy's durable slice into the authority row. Kept here so `health.rs` never
/// depends on a storage type and stays testable as pure policy.
fn durable_health_row(durable: health::DurableAccountHealth) -> registry::CodexHomeHealthRow {
    registry::CodexHomeHealthRow {
        account_state: durable.account.as_str().to_string(),
        auth_fail_streak: durable.auth_fail_streak,
        first_auth_fail_ts: durable.first_auth_fail_ts,
        cooling_until: durable.cooling_until,
    }
}

/// RAII load-counter guard for one in-flight turn. Dropping it — on success, error, cancellation
/// or client disconnect — updates the home's selection signal and metrics.
pub(crate) struct TurnSlot {
    inflight: Arc<AtomicUsize>,
    idle: Arc<Notify>,
}

fn release_turn(inflight: &AtomicUsize, idle: &Notify) {
    if inflight.fetch_sub(1, Ordering::AcqRel) == 1 {
        idle.notify_waiters();
    }
}

impl Drop for TurnSlot {
    fn drop(&mut self) {
        release_turn(&self.inflight, &self.idle);
    }
}

/// Outcome of choosing a home for one request.
enum HomeSelection {
    Ready(Arc<CodexHome>, TurnSlot),
    /// Every home is cooling or explicitly provider-limited; `ready_at` is the soonest recovery.
    Unavailable {
        ready_at: Option<i64>,
    },
}

/// Owns and restarts the pinned app-server processes for every configured home. The composition
/// layer calls `preflight` before exposing a configured provider, while later transport failures
/// are recovered lazily. Existing Claude routing is completely independent when Codex is disabled.
pub struct CodexGateway {
    cfg: Arc<CodexConfig>,
    calibration_store: Option<Arc<AsyncBilling>>,
    shutting_down: AtomicBool,
    abort_turns: AtomicBool,
    /// Rotates equal-load cold/warm candidates. Without an atomic cursor, sequential requests and
    /// concurrent bursts with identical snapshots all collapse onto the lowest discovery order.
    selection_cursor: AtomicU64,
    abort_notify: Notify,
    /// Raised when a home's health asks to be re-checked ahead of the sweep cadence.
    ///
    /// This is the Codex counterpart of the Claude pool's `request_probe` + `probe_poke` pair: a bad
    /// outcome on the data path immediately queues a control-plane check instead of waiting a full
    /// interval for the background loop to notice. Without it, the only thing that could discover a
    /// silent home was the sweep that the silent home was already stalling.
    probe_poke: Notify,
    background_tasks: Arc<Semaphore>,
    rediscover_lock: Mutex<()>,
    /// Rediscovered on every health tick, so an account the authbot finishes buying joins the pool
    /// without a restart. Readers take a snapshot; the lock is never held across a turn.
    homes: RwLock<Vec<Arc<CodexHome>>>,
    /// Last successful live `model/list` snapshot. The public OpenAI model-list route reads this
    /// cache and never waits on an app-server RPC; before the first successful refresh it falls back
    /// to the locally configured billing catalog.
    model_catalog: RwLock<Option<HashSet<String>>>,
    /// SIGUSR2 reconciliation and the periodic health loop may overlap. Keep live catalog refresh
    /// single-flight so a transient child stall cannot multiply background RPCs.
    model_catalog_refresh: Mutex<()>,
    history: Arc<HistoryStore>,
    /// Declared last so Rust drops every home/child before releasing the process-wide fence.
    /// A per-home fence can split a multi-home pool between two provider generations, while this
    /// fixed root-owned path cannot be replaced by a home rename or authbot publication.
    _ownership_lock: Option<ProviderLock>,
}

impl CodexGateway {
    pub fn new(cfg: CodexConfig) -> anyhow::Result<Self> {
        Self::new_with_calibration(cfg, None)
    }

    pub fn new_with_calibration(
        cfg: CodexConfig,
        calibration_store: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        let ownership_lock = cfg
            .transport
            .owns_home()
            .then(|| acquire_provider_lock(&cfg.ownership_lock_file))
            .transpose()?;
        let history = HistoryStore::new(
            cfg.history_redis_url.as_deref(),
            cfg.history_secret.as_deref(),
            cfg.history_ttl_secs,
            cfg.history_local_cap,
            cfg.history_redis_timeout_ms,
        )?;
        if cfg.homes.is_empty() && cfg.homes_dir.is_none() {
            anyhow::bail!(
                "Codex provider requires at least one authenticated CODEX_HOME or a homes directory"
            );
        }
        let cfg = Arc::new(cfg);
        let specs = discovery::discover(
            &cfg.homes,
            cfg.homes_dir.as_deref(),
            cfg.transport.owns_home(),
        );
        if specs.is_empty() {
            anyhow::bail!("Codex provider found no authenticated CODEX_HOME to serve from");
        }
        let homes = specs
            .into_iter()
            .enumerate()
            .map(|(order, spec)| {
                Arc::new(CodexHome::new(
                    cfg.clone(),
                    spec,
                    order,
                    calibration_store.clone(),
                ))
            })
            .collect();
        Ok(Self {
            cfg,
            calibration_store,
            shutting_down: AtomicBool::new(false),
            abort_turns: AtomicBool::new(false),
            selection_cursor: AtomicU64::new(0),
            abort_notify: Notify::new(),
            probe_poke: Notify::new(),
            background_tasks: Arc::new(Semaphore::new(MAX_BACKGROUND_TASKS as usize)),
            rediscover_lock: Mutex::new(()),
            homes: RwLock::new(homes),
            model_catalog: RwLock::new(None),
            model_catalog_refresh: Mutex::new(()),
            history: Arc::new(history),
            _ownership_lock: ownership_lock,
        })
    }

    /// Snapshot of the current pool.
    async fn homes(&self) -> Vec<Arc<CodexHome>> {
        self.homes.read().await.clone()
    }

    /// Reconcile the pool with what is on disk.
    ///
    /// An unchanged home keeps its live child and cooling/auth state. Removed, replaced, or
    /// reconfigured homes are retired explicitly after active turns finish.
    async fn rediscover(&self) {
        let _reconcile = self.rediscover_lock.lock().await;
        loop {
            if self.shutting_down.load(Ordering::Acquire) {
                return;
            }
            let specs = discovery::discover(
                &self.cfg.homes,
                self.cfg.homes_dir.as_deref(),
                self.cfg.transport.owns_home(),
            );
            if specs.is_empty() {
                // Never empty the pool from a scan: a transient unreadable directory would otherwise
                // take the whole provider down while the previous homes were still serving.
                eprintln!("Codex rediscovery found no homes; keeping the current pool");
                return;
            }
            let mut homes = self.homes.write().await;
            let mut next: Vec<Arc<CodexHome>> = Vec::with_capacity(specs.len());
            let mut retiring: Vec<Arc<CodexHome>> = Vec::new();
            let mut joining: Vec<(usize, CodexHomeSpec)> = Vec::new();
            for (order, spec) in specs.into_iter().enumerate() {
                match homes.iter().find(|home| {
                    home.path() == spec.path
                        && !home.retired.load(Ordering::Acquire)
                        && home.identity_is_current()
                        && home.spec.proxy == spec.proxy
                }) {
                    Some(existing) => {
                        existing.order.store(order, Ordering::Relaxed);
                        next.push(existing.clone());
                    }
                    None => {
                        if let Some(replaced) = homes.iter().find(|home| home.path() == spec.path) {
                            replaced.retired.store(true, Ordering::Release);
                            retiring.push(replaced.clone());
                            eprintln!("Codex home {} configuration changed", replaced.id());
                        }
                        joining.push((order, spec));
                    }
                }
            }
            for gone in homes.iter() {
                if !next.iter().any(|home| home.path() == gone.path())
                    && !retiring
                        .iter()
                        .any(|candidate| Arc::ptr_eq(candidate, gone))
                {
                    gone.retired.store(true, Ordering::Release);
                    retiring.push(gone.clone());
                    eprintln!("Codex home {} left the pool", gone.id());
                }
            }
            // Remove retiring homes from selection before waiting on their active turns. Replacements
            // are published only after the old child has exited, so one auth store never has two live
            // generations in this process.
            let had_retiring = !retiring.is_empty();
            *homes = next.clone();
            drop(homes);
            for home in retiring {
                if let Err(error) = home.retire().await {
                    eprintln!(
                        "Codex home retirement failed [{}]",
                        error.diagnostic_class()
                    );
                    return;
                }
            }
            if self.shutting_down.load(Ordering::Acquire) {
                return;
            }
            if had_retiring {
                // Retirement can last an entire active turn. Re-read auth and proxy metadata before
                // publishing a replacement rather than serving a stale pre-retirement snapshot.
                continue;
            }
            for (order, spec) in joining {
                eprintln!("Codex home {} joined the pool", spec.id);
                let home = Arc::new(CodexHome::new(
                    self.cfg.clone(),
                    spec,
                    order,
                    self.calibration_store.clone(),
                ));
                // Recover the durable account verdict before this home can be selected, so a
                // subscription already known to be dead or spent is not re-admitted by a restart
                // and rediscovered with customer traffic.
                home.hydrate_health().await;
                next.push(home);
            }
            next.sort_by_key(|home| home.order());
            *self.homes.write().await = next;
            return;
        }
    }

    pub fn config(&self) -> &CodexConfig {
        &self.cfg
    }

    /// Wake the background sweep early when any home asked to be re-checked.
    ///
    /// Called on the data path after a turn failure: the request that just hit the problem is the
    /// freshest evidence the pool will get, and waiting a full cadence to act on it is exactly how
    /// a wedged home kept receiving customer traffic.
    pub(crate) async fn poke_probe_if_requested(&self) {
        for home in self.homes().await {
            if home.health().wants_probe() {
                self.probe_poke.notify_one();
                return;
            }
        }
    }

    /// Await a forced-probe request. The composition layer races this against the sweep interval,
    /// so a healthy pool keeps its steady cadence while a suspicious one is re-checked immediately.
    pub async fn probe_requested(&self) {
        self.probe_poke.notified().await;
    }

    /// Clear pending forced-probe requests as the sweep picks them up, so one bad turn cannot make
    /// the loop spin: the request is consumed whether or not the probe ultimately succeeds.
    async fn take_probe_requests(&self) {
        for home in self.homes().await {
            home.health
                .lock()
                .expect("codex home health lock")
                .take_probe_request();
        }
    }

    pub(crate) async fn cached_model_catalog(&self) -> Option<HashSet<String>> {
        self.model_catalog.read().await.clone()
    }

    /// Refresh the best-effort live model snapshot outside the request path. A failed refresh keeps
    /// the last successful snapshot; an empty successful snapshot is retained intentionally because
    /// the app-server may explicitly report that no models are currently available.
    pub async fn refresh_model_catalog(&self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let _refresh = self.model_catalog_refresh.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        match api::available_upstream_models(self).await {
            Ok(available) => {
                *self.model_catalog.write().await = Some(available);
            }
            Err(error) => {
                eprintln!(
                    "Codex model catalog refresh failed [{}]",
                    error.diagnostic_class()
                );
            }
        }
    }

    pub(crate) fn history(&self) -> &HistoryStore {
        &self.history
    }

    /// Reserve one detached stream task in the shutdown barrier. The second shutdown check closes
    /// the race where shutdown starts after the first check but before the permit is acquired.
    pub(crate) fn track_background_task(&self) -> Result<OwnedSemaphorePermit, ProcessError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        let permit = self
            .background_tasks
            .clone()
            .try_acquire_owned()
            .map_err(|_| ProcessError::Closed)?;
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        Ok(permit)
    }

    async fn turn_abort_requested(&self) {
        loop {
            let notified = self.abort_notify.notified();
            if self.abort_turns.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    fn abort_active_turns(&self) {
        self.abort_turns.store(true, Ordering::Release);
        self.abort_notify.notify_waiters();
    }

    /// Wait for detached turns to persist and settle until the server drain deadline, then cancel
    /// any remainder before stopping every child. The process-wide ownership lock remains held by
    /// `self` until the server process exits.
    pub async fn shutdown(&self) {
        self.shutdown_until(None).await;
    }

    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.shutting_down.store(true, Ordering::Release);
        let cleanup_deadline = deadline.map(|deadline| deadline + FORCED_SHUTDOWN_CLEANUP_GRACE);
        let barrier = self
            .background_tasks
            .clone()
            .acquire_many_owned(MAX_BACKGROUND_TASKS);
        let _background_barrier = match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, barrier).await {
                Ok(permit) => permit.ok(),
                Err(_) => {
                    self.abort_active_turns();
                    // A task can be stuck on downstream backpressure or in cancellation cleanup.
                    // Waiting for its semaphore permit again without a deadline recreated the
                    // ten-minute singleton outage this deadline exists to prevent.
                    eprintln!(
                        "Codex forced shutdown: abandoning residual tracked tasks after deadline"
                    );
                    None
                }
            },
            None => barrier.await.ok(),
        };
        // Also cancel an untracked provider operation such as a health probe before retirement.
        self.abort_active_turns();
        let _reconcile = match cleanup_deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, self.rediscover_lock.lock())
                .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    eprintln!("Codex forced shutdown: rediscovery lock exceeded cleanup deadline");
                    return;
                }
            },
            None => self.rediscover_lock.lock().await,
        };
        for home in self.homes().await {
            let result = match cleanup_deadline {
                Some(deadline) => match tokio::time::timeout_at(deadline, home.retire()).await {
                    Ok(result) => result,
                    Err(_) => {
                        eprintln!(
                            "Codex forced shutdown: child cleanup exceeded the hard deadline"
                        );
                        break;
                    }
                },
                None => home.retire().await,
            };
            if let Err(error) = result {
                eprintln!("Codex child shutdown failed [{}]", error.diagnostic_class());
            }
        }
    }

    /// Choose a usable home and take one of its turn slots.
    ///
    /// A conversation-pinned home is first. Other homes are ordered only by current in-flight load
    /// and a rotating tie-break; calibration is reporting evidence and never an admission/routing
    /// restriction. New shared roots still seed two homes, but no capacity ratio can veto warmth.
    async fn select_home(
        &self,
        exclude: &[String],
        preferred: Option<&str>,
        warm: &[String],
        place_cache_root: bool,
        advance_cursor: bool,
    ) -> HomeSelection {
        if self.shutting_down.load(Ordering::Acquire) {
            return HomeSelection::Unavailable { ready_at: None };
        }
        let now = pool::now();
        let pool_homes = self.homes().await;
        let mut candidates = Vec::with_capacity(pool_homes.len());
        let mut soonest: Option<i64> = None;
        for home in &pool_homes {
            if home.retired.load(Ordering::Acquire) {
                continue;
            }
            if self.cfg.transport == CodexTransport::SharedDaemonProxy
                && discovery::daemon_draining(std::path::Path::new(home.path()))
            {
                continue;
            }
            if exclude.iter().any(|id| id == home.id()) {
                continue;
            }
            // One admission verdict for every reason a home can be unroutable: a dead account, a
            // wedged or degraded transport, cooling, or a fresh full window. Previously each of
            // these was decided at a different place with a different rule, and a home that had
            // simply stopped answering matched none of them.
            let limits = home.rate_limits().await;
            match home.admission(limits.as_ref(), now) {
                health::Admission::Admit { snapshot_stale } => {
                    // Remaining window, normalised per subscription by the provider itself. That
                    // normalisation is what makes this tier-aware for free: 40% of a small plan and
                    // 40% of a large one are equally close to their own wall, which is the thing
                    // selection must equalise. The USD calibration answers a different question —
                    // how much capacity is left in money — and stays empty until a window turns over.
                    let used =
                        quota_rank(limits.as_ref().and_then(|limits| limits.max_used_percent()));
                    candidates.push((snapshot_stale, home.inflight(), used, home.clone()));
                }
                health::Admission::Reject { ready_at, .. } => {
                    if let Some(ready_at) = ready_at {
                        soonest = Some(soonest.map_or(ready_at, |v: i64| v.min(ready_at)));
                    }
                }
            }
        }
        if candidates.is_empty() {
            return HomeSelection::Unavailable { ready_at: soonest };
        }
        let width = pool_homes.len().max(1);
        // Streaming capacity preflight must inspect the same next choice without consuming it.
        // Otherwise every `preflight -> turn` pair advances twice; with two equal homes all real
        // turns would land on the same half of the rotation.
        let cursor = if advance_cursor {
            self.selection_cursor.fetch_add(1, Ordering::Relaxed)
        } else {
            self.selection_cursor.load(Ordering::Relaxed)
        } as usize
            % width;
        candidates.sort_by(
            |(stale_a, inflight_a, used_a, home_a), (stale_b, inflight_b, used_b, home_b)| {
                let rotated_a = (home_a.order() + width - cursor) % width;
                let rotated_b = (home_b.order() + width - cursor) % width;
                // A home whose quota evidence has gone stale is still routable — the snapshot is
                // observational and must never become a hard dependency — but it must never win a tie
                // against a home whose evidence is current. A frozen reading looks arbitrarily
                // optimistic, and ranking on it is how one unresponsive home absorbed the whole pool.
                //
                // Below that, ordering mirrors the Claude pool's `select_best`: the load envelope is
                // a full tier of its own, and only within it does remaining window decide. Ranking on
                // capacity first would pile concurrent turns onto whichever subscription is emptiest.
                // Quota steering is bucketed and only engages near the wall, so comparable homes stay
                // tied and the rotation cursor keeps spreading them.
                stale_a
                    .cmp(stale_b)
                    .then_with(|| inflight_a.cmp(inflight_b))
                    .then_with(|| used_a.cmp(used_b))
                    .then_with(|| rotated_a.cmp(&rotated_b))
            },
        );
        // A resolved conversation is a hard first choice. For a new shared root, seed a cold home
        // until two copies exist, then prefer the least-loaded warm home.
        let mut ordered: Vec<Arc<CodexHome>> = Vec::with_capacity(candidates.len());
        if let Some(id) = preferred {
            if let Some((_, _, _, home)) = candidates.iter().find(|(_, _, _, home)| home.id() == id)
            {
                ordered.push(home.clone());
            }
        } else if place_cache_root {
            let warm_count = candidates
                .iter()
                .filter(|(_, _, _, home)| warm.iter().any(|id| id == home.id()))
                .count();
            let primary = if warm_count < CACHE_ROOT_MIN_WARM_HOMES {
                candidates
                    .iter()
                    .position(|(_, _, _, home)| !warm.iter().any(|id| id == home.id()))
            } else {
                candidates
                    .iter()
                    .position(|(_, _, _, home)| warm.iter().any(|id| id == home.id()))
            }
            .unwrap_or(0);
            ordered.push(candidates[primary].3.clone());
        }
        // If the primary choice is currently sampling, another warm copy is the next best spill;
        // remaining candidates stay in global capacity order.
        if place_cache_root {
            for (_, _, _, home) in &candidates {
                if warm.iter().any(|id| id == home.id())
                    && !ordered.iter().any(|chosen| Arc::ptr_eq(chosen, home))
                {
                    ordered.push(home.clone());
                }
            }
        }
        for (_, _, _, home) in candidates {
            if !ordered.iter().any(|chosen| Arc::ptr_eq(chosen, &home)) {
                ordered.push(home);
            }
        }
        for home in ordered {
            if let Some(slot) = home.acquire_turn() {
                return HomeSelection::Ready(home, slot);
            }
        }
        HomeSelection::Unavailable { ready_at: soonest }
    }

    /// Read-only capacity pre-check for the streaming path. Streaming establishes the SSE body
    /// before the turn runs, so pool exhaustion discovered inside the turn can only surface as an
    /// in-stream failure on an HTTP 200 — which status-code-driven SDK retry logic never treats as
    /// retryable. Calling this before opening the stream lets the handler reject with a real
    /// `429 + Retry-After` (soonest reset) exactly like a non-streaming request would. It runs the
    /// same selection a real turn would and releases the acquired load slot immediately. A home can
    /// still exhaust between this check and the turn; that rare race stays an in-stream failure.
    pub(crate) async fn preflight_capacity(&self) -> Result<(), ProcessError> {
        match self.select_home(&[], None, &[], false, false).await {
            HomeSelection::Ready(_home, _slot) => Ok(()),
            HomeSelection::Unavailable { ready_at } => Err(ProcessError::UsageLimitExceeded {
                retry_after: ready_at
                    .map(|at| at.saturating_sub(pool::now()).clamp(1, 7 * 24 * 3600) as u64),
            }),
        }
    }

    /// A process from any usable home, for provider-level reads such as model discovery.
    pub(crate) async fn any_process(&self) -> Result<Arc<CodexProcess>, ProcessError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        let now = pool::now();
        let mut last_error = ProcessError::Closed;
        let pool_homes = self.homes().await;
        let mut ordered: Vec<_> = pool_homes.iter().collect();
        ordered.sort_by_key(|home| (home.is_cooling(now), home.order()));
        for home in ordered {
            match home.process().await {
                Ok(process) => return Ok(process),
                Err(error) => {
                    home.note_process_error(&error);
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    /// Verify the pinned executable, start app-server, complete protocol initialization and prove
    /// that each dedicated profile is authenticated with a ChatGPT subscription.
    ///
    /// Every transport keeps a one-home service floor: a single working subscription is still
    /// customer capacity. Blue-green compares the candidate's authenticated home set with the old
    /// generation outside this process; it must not encode rollout redundancy by making a healthy
    /// one-home runtime return 503. A home that fails here starts quarantined and is reported by the
    /// health metrics and `CodexHomeUnauthenticated` alert instead. Returning only a diagnostic
    /// class keeps account metadata, paths and child messages out of composition logs.
    pub async fn preflight(&self) -> anyhow::Result<()> {
        let mut healthy = 0usize;
        let mut last_class = "closed";
        let pool_homes = self.homes().await;
        // The initial pool is constructed synchronously, and rediscovery deliberately keeps
        // existing instances, so startup is the only place these homes can recover their durable
        // verdict. Without it a restart would silently re-admit an already-condemned subscription.
        for home in &pool_homes {
            home.hydrate_health().await;
        }
        for home in &pool_homes {
            match home.process().await {
                Ok(_) => {
                    home.mark_healthy();
                    healthy += 1;
                }
                Err(error) => {
                    last_class = error.diagnostic_class();
                    home.note_process_error(&error);
                    eprintln!("Codex home {} failed preflight [{}]", home.id(), last_class);
                }
            }
        }
        let required = self.cfg.transport.minimum_ready_homes();
        if healthy < required {
            anyhow::bail!(
                "Codex app-server preflight admitted {healthy}/{} homes, but {required} are required [{last_class}]",
                pool_homes.len()
            );
        }
        if healthy < pool_homes.len() {
            eprintln!(
                "Codex provider starting with {healthy}/{} authenticated homes",
                pool_homes.len()
            );
        }
        Ok(())
    }

    /// Read-only health sweep over every home, run by the composition layer's background loop.
    ///
    /// A device login expires with no traffic on it, so a home that is never selected would
    /// otherwise stay silently dead until the pool needed it. Quarantined homes are probed too, so
    /// a re-authenticated profile returns to service without a restart.
    pub async fn probe_health(&self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        // Pick up accounts finished since the last tick before probing, so a new home is health
        // checked on the same pass that admits it.
        self.rediscover().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        self.take_probe_requests().await;
        // Probe every home concurrently.
        //
        // A sequential sweep costs the sum of its slowest homes: in production two unresponsive
        // homes each burned a full RPC deadline, stretching a ten-second cadence past forty seconds
        // and starving the *healthy* home's snapshot of refreshes. One sick subscription must not
        // degrade the observability of the rest of the pool, so the sweep now costs its slowest
        // home rather than their sum.
        let probes = self
            .homes()
            .await
            .into_iter()
            .map(|home| async move {
                let process = match home.process().await {
                    Ok(process) => process,
                    Err(error) => {
                        // A home that cannot re-establish its transport is the state the sweep
                        // exists to surface, and it was the one state the sweep said nothing about.
                        // In production a recycled home sat unroutable for over half an hour while
                        // every ten-second sweep failed here in silence, and the only evidence was
                        // a gauge that had stopped moving. A failed probe is loud; a failed
                        // *reconnect* has to be too, or the recovery path is invisible precisely
                        // when it is not working.
                        eprintln!(
                            "Codex home {} could not re-establish its transport [{}]",
                            home.id(),
                            error.diagnostic_class()
                        );
                        home.note_process_error(&error);
                        return;
                    }
                };
                match process.probe().await {
                    Ok(()) => home.mark_healthy(),
                    Err(error) => {
                        // A deadline is recorded like any other failure. It still changes no verdict
                        // on its own — `health.rs` requires a streak to close admission and a
                        // time-corroborated streak to recycle — but it is no longer discarded.
                        // Silently dropping it is what let a home that had stopped answering stay
                        // routable indefinitely: the one failure mode with no consequence became
                        // the one that took the pool down.
                        let deadline = matches!(error, ProcessError::Timeout(_));
                        // A busy home is not a silent one.
                        //
                        // The app-server serializes work per home, so while a turn is generating —
                        // and a turn may legitimately run for minutes — a control call queues behind
                        // it and cannot answer inside the probe deadline. Treating that as evidence
                        // of a dead transport is backwards: the home is proving its liveness by
                        // serving, and the only thing the timeout measures is our own traffic.
                        //
                        // In production this took out exactly the homes that were working. The
                        // freshest, highest-capacity home took the load, missed three probes behind
                        // its own turns, and was recycled for it, which moved the load to the next
                        // home and repeated. A probe deadline therefore only counts against a home
                        // that has nothing in flight.
                        let busy = home.inflight() > 0;
                        if deadline && busy {
                            eprintln!(
                                "Codex home {} probe deferred while serving {} turn(s)",
                                home.id(),
                                home.inflight()
                            );
                            return;
                        }
                        home.note_process_error(&error);
                        // Every probe failure is reported, not only the deadlines. Logging one
                        // class and swallowing the rest produced a home whose quota reading had
                        // been frozen for five hours with nothing in the journal to explain it:
                        // the probe was failing before it ever reached the refresh, and that exit
                        // left no trace at all. A failure the sweep acts on must be a failure the
                        // sweep can be questioned about.
                        eprintln!(
                            "Codex home {} probe failed [{}]{}",
                            home.id(),
                            error.diagnostic_class(),
                            if deadline {
                                format!(" (deadline streak {})", home.health().deadline_streak)
                            } else {
                                String::new()
                            }
                        );
                        // Replace the generation only once the policy says it is provably unusable.
                        // Recycling on a single deadline would kill every sibling turn multiplexed
                        // over the same child; recycling never is how a bridge to a replaced daemon
                        // survived for hours while still reporting itself live.
                        if home.health().needs_recycle() {
                            eprintln!("Codex home {} transport wedged; recycling", home.id());
                            home.invalidate(&process).await;
                        }
                    }
                }
            })
            .collect::<Vec<_>>();
        futures_util::future::join_all(probes).await;
        // Model discovery is intentionally best-effort and outside customer request handling. A
        // stale last-good snapshot (or the configured catalog before the first success) is safer
        // than turning an app-server catalog hiccup into a public 503 on every SDK startup.
        self.refresh_model_catalog().await;
    }

    /// Read only cached operational state. Metrics collection never starts a provider process or
    /// triggers an authentication or network request.
    pub async fn operational_status(&self) -> CodexOperationalStatus {
        let now = pool::now();
        let pool_homes = self.homes().await;
        let mut homes = Vec::with_capacity(pool_homes.len());
        let mut available = 0usize;
        let mut soonest: Option<i64> = None;
        for home in &pool_homes {
            let status = home.status().await;
            // `available` now means exactly "selection would route here", because it is computed by
            // the same predicate selection uses. It previously re-derived its own weaker rule, so a
            // home that had stopped answering still counted as available — which is why the
            // `CodexNoAvailableHomes` alert stayed silent while the pool served nothing.
            if status.process_live && status.admitted {
                available += 1;
            } else {
                let ready_at = home.ready_at().await;
                if ready_at > now {
                    soonest = Some(soonest.map_or(ready_at, |v: i64| v.min(ready_at)));
                }
            }
            homes.push(status);
        }
        // Provider-level snapshot reports the most constrained home: that is the one an operator
        // needs to see before the pool loses capacity.
        let rate_limits = homes
            .iter()
            .filter_map(|home| home.rate_limits.clone())
            .max_by_key(|limits| limits.max_used_percent().unwrap_or(0));
        CodexOperationalStatus {
            process_live: homes.iter().any(|home| home.process_live),
            rate_limits,
            available,
            soonest_ready: soonest,
            homes,
        }
    }
}

impl CodexHome {
    /// Translate a transport-level failure into the health signal it actually proves.
    ///
    /// The mapping is the whole point: previously each error class carried its own ad-hoc cooling
    /// constant, and a deadline carried none at all. Now every class names the evidence it provides
    /// and the policy in `health.rs` decides what that evidence is worth.
    fn note_process_error(&self, error: &ProcessError) {
        let signal = match error {
            // A subscription the provider says is absent needs an operator, not corroboration.
            ProcessError::SubscriptionRequired => {
                health::HealthSignal::AuthRejected { permanent: true }
            }
            ProcessError::AuthenticationRequired => {
                health::HealthSignal::AuthRejected { permanent: false }
            }
            ProcessError::UsageLimitExceeded { retry_after } => {
                health::HealthSignal::UsageLimited {
                    retry_after_secs: Some(retry_after.unwrap_or(DEFAULT_LIMIT_RETRY_SECS) as i64),
                }
            }
            // A missed deadline is now evidence rather than nothing. On its own it still changes
            // no verdict; a streak of them closes admission, and a corroborated streak recycles.
            ProcessError::Timeout(_) => health::HealthSignal::Deadline,
            // Ownership of the home is held by another generation. This is transport state, not a
            // verdict about the subscription: it resolves the moment the other owner exits, which
            // is exactly what happens during a blue-green handoff or after an invalidated child is
            // reaped. Condemning the account for it was wrong twice over — the account is fine, and
            // the verdict is durable, so a few seconds of ordinary lock contention would have
            // outlived the restart that cleared it and kept a healthy subscription out of the pool.
            ProcessError::HomeInUse | ProcessError::HomeLockUnavailable => {
                health::HealthSignal::TransportClosed
            }
            // A configuration or attestation fault is not transient and must not be retried in a
            // hot loop; the deployment gate is the place that fixes it. Treated as a permanent
            // account-level verdict so the home stays out until an operator acts.
            ProcessError::InvalidConfig(_)
            | ProcessError::DigestMismatch { .. }
            | ProcessError::VersionMismatch { .. }
            | ProcessError::Disabled => health::HealthSignal::AuthRejected { permanent: true },
            // EOF, protocol violation, or a generation that is provably gone.
            _ => health::HealthSignal::TransportClosed,
        };
        self.note(signal);
    }

    /// Blame classification for a failed turn, mirroring the Claude rotation policy.
    ///
    /// A usage limit or a dead login belongs to this home's subscription, so it leaves the rotation
    /// until its window resets or an operator re-authenticates. A client fault belongs to the
    /// request and must not stain a healthy account — otherwise one malformed request could quietly
    /// drain the pool.
    pub(crate) fn note_turn_error(&self, error: &ProcessError) {
        match error {
            // Deterministic client faults: another home would reject them identically.
            ProcessError::BadRequest
            | ProcessError::ContextWindowExceeded
            | ProcessError::Rpc { .. } => {}
            other => self.note_process_error(other),
        }
    }
}
