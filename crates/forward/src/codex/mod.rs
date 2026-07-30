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
pub use chat::completions as openai_chat_completions;
pub use config::{CodexConfig, CodexHomeSpec, CodexModel, CodexPrices, CodexTransport};
pub use history::{HistoryError, StoredHistory};
pub(crate) use process::{AppServerEvent, CodexProcess, ProcessError};
pub use process::{CodexRateLimitWindow, CodexRateLimits};
pub(crate) use runner::{CodexTurnRequest, CodexTurnResult, CodexUsage, TurnUpdate};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use history::HistoryStore;
use std::collections::HashSet;
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

/// A home whose device login no longer authenticates is quarantined rather than retried in a hot
/// loop: repeatedly hammering a rejected profile is both useless and a ban signal. It matches the
/// Claude `AUTH_QUARANTINE` for the same reason. The background health probe can clear it early.
const AUTH_QUARANTINE_SECS: i64 = 900;
/// A transport fault is the child's problem, not the account's. Cool briefly so a crash-looping
/// child stops absorbing requests, but do not take its subscription capacity out of the pool.
const TRANSPORT_COOL_SECS: i64 = 10;
/// Fallback wait advertised to a client when every home is limited but no window published a reset.
const DEFAULT_LIMIT_RETRY_SECS: u64 = 60;
/// Match the Claude fleet's cache-root fanout: one shared prompt prefix is deliberately warmed on
/// two competitive homes so independent sessions do not collapse onto the first subscription.
const CACHE_ROOT_MIN_WARM_HOMES: usize = 2;
/// Reuse cache warmth only while the candidate retains at least this share of the fleet's best
/// remaining provider capacity. This is a comparison, never a time window or an availability gate.
const CACHE_ROOT_MIN_CAPACITY_RATIO: f64 = 0.70;
/// Detached public stream tasks are bounded by the server-wide admission limit in practice. This
/// larger private semaphore provides a quiescence barrier without imposing a lower runtime cap.
const MAX_BACKGROUND_TASKS: u32 = 1_000_000;
/// Once the server's advertised drain deadline has expired, cleanup itself must also be bounded.
/// systemd cannot start the replacement singleton while the old main PID remains in `deactivating`.
const FORCED_SHUTDOWN_CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Clone, Copy, Debug)]
struct CacheRootCandidate {
    warm: bool,
    free_capacity: f64,
}

/// Candidates arrive best-first. Seed a second competitive cache copy, then prefer a warm copy
/// while it remains close enough to the global capacity leader. The returned index is an immediate
/// placement decision; no sampling count, timer, background repair or minimum live-home quorum is
/// involved.
fn cache_root_primary_index(candidates: &[CacheRootCandidate]) -> usize {
    debug_assert!(!candidates.is_empty());
    let global_free = candidates[0].free_capacity.max(0.0);
    let competitive = |candidate: &CacheRootCandidate| {
        candidate.free_capacity.max(0.0) >= global_free * CACHE_ROOT_MIN_CAPACITY_RATIO
    };
    let warm_count = candidates.iter().filter(|candidate| candidate.warm).count();

    if warm_count < CACHE_ROOT_MIN_WARM_HOMES {
        if let Some((index, candidate)) = candidates
            .iter()
            .enumerate()
            .find(|(_, candidate)| !candidate.warm)
        {
            if competitive(candidate) {
                return index;
            }
        }
    }

    if let Some((index, candidate)) = candidates
        .iter()
        .enumerate()
        .find(|(_, candidate)| candidate.warm)
    {
        if competitive(candidate) {
            return index;
        }
    }
    0
}

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
    pub cooling_until: i64,
    pub inflight: usize,
    pub rate_limits: Option<CodexRateLimits>,
    /// Cumulative official-price spend this home has served through the gateway.
    pub spend_usd_total: f64,
    /// Capacity estimate per reported window slot (empty until the first snapshot arrives).
    pub capacities: Vec<CodexWindowCapacityReport>,
}

/// Measured (or prior) sellable capacity of one subscription window slot, in official-price USD.
#[derive(Clone, Debug)]
pub struct CodexWindowCapacityReport {
    /// `primary` or `secondary`, mirroring the provider's rate-limit payload.
    pub slot: &'static str,
    pub window_minutes: Option<i64>,
    pub used_percent: i64,
    /// EMA capacity estimate per full window; a prior until the first accepted sample.
    pub cap_usd: f64,
    /// `cap_usd` scaled by the unused share of the current window.
    pub remaining_usd: f64,
    /// True once at least one real sample was accepted; false means `cap_usd` is the prior.
    pub calibrated: bool,
    pub samples: u32,
}

#[derive(Clone, Debug)]
pub struct CodexOperationalStatus {
    /// True when at least one home has a live child. Kept as the provider-level liveness signal.
    pub process_live: bool,
    /// Snapshot of the most constrained home, for the provider-level rate-limit metrics.
    pub rate_limits: Option<CodexRateLimits>,
    pub homes: Vec<CodexHomeStatus>,
    /// Homes that could accept a request right now (live-or-startable, not cooling, within
    /// headroom).
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
    cooling_until: AtomicI64,
    /// Last known verdict of `account/read`. Startup and the health probe both write it.
    auth_ok: AtomicBool,
    /// Cumulative official-price cost of every turn this home served, feeding window-capacity
    /// calibration. Monotonic for the process lifetime; a restart re-anchors calibration, which is
    /// exactly the anchor mechanism's job.
    spend_nano_total: AtomicI64,
    /// Per-slot window calibration. `std` mutex: the critical section is a few float ops and never
    /// awaits, so it may be taken on the request hot path.
    calib_primary: std::sync::Mutex<WindowCalibration>,
    calib_secondary: std::sync::Mutex<WindowCalibration>,
}

impl CodexHome {
    fn new(cfg: Arc<CodexConfig>, spec: CodexHomeSpec, order: usize) -> Self {
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
            cooling_until: AtomicI64::new(0),
            auth_ok: AtomicBool::new(true),
            spend_nano_total: AtomicI64::new(0),
            calib_primary: std::sync::Mutex::new(WindowCalibration::new()),
            calib_secondary: std::sync::Mutex::new(WindowCalibration::new()),
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

    fn cooling_until(&self) -> i64 {
        self.cooling_until.load(Ordering::Relaxed)
    }

    fn is_cooling(&self, now: i64) -> bool {
        self.cooling_until() > now
    }

    /// Extend cooling to at least `until`. Cooling is never shortened by a later, weaker signal.
    fn cool_until(&self, until: i64) {
        self.cooling_until.fetch_max(until, Ordering::Relaxed);
    }

    fn cool_for(&self, secs: i64) {
        self.cool_until(pool::now().saturating_add(secs.max(1)));
    }

    /// A completed turn proves the home is healthy: clear cooling and restore the auth verdict.
    fn mark_healthy(&self) {
        self.cooling_until.store(0, Ordering::Relaxed);
        self.auth_ok.store(true, Ordering::Relaxed);
    }

    fn mark_auth_failed(&self) {
        self.auth_ok.store(false, Ordering::Relaxed);
        self.cool_for(AUTH_QUARANTINE_SECS);
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

    /// Read the cached snapshot and feed it to window-capacity calibration. Every read path
    /// funnels through here; sub-threshold deltas accumulate inside the calibration, so the
    /// observation cadence does not matter.
    async fn observed_rate_limits(&self) -> Option<CodexRateLimits> {
        let limits = match self.live_process().await {
            Some(process) => process.rate_limits().await,
            None => None,
        };
        if let Some(limits) = &limits {
            self.note_rate_limits(limits);
        }
        limits
    }

    /// Credit one completed turn's exact official-price cost to this home's calibration spend.
    /// Called for every served turn regardless of customer billing (admin turns consume the
    /// subscription window exactly the same), and only on success: a failed turn's provider-side
    /// consumption is intentionally attributed to the foreign-usage share, which keeps the
    /// calibration guard conservative.
    pub(crate) fn record_spend(&self, real_nano: i128) {
        let nano = real_nano.clamp(0, i64::MAX as i128) as i64;
        self.spend_nano_total.fetch_add(nano, Ordering::Relaxed);
    }

    fn spend_usd_total(&self) -> f64 {
        self.spend_nano_total.load(Ordering::Relaxed) as f64 / 1e9
    }

    /// Configured weekly-capacity prior scaled to this window's duration (weekly = 10080 minutes
    /// is the anchor; a 5h window starts from a proportionally smaller prior).
    fn window_prior_usd(&self, window: &CodexRateLimitWindow) -> f64 {
        let minutes = window.window_duration_mins.unwrap_or(10_080).max(1) as f64;
        self.cfg.window_cap_usd_prior * minutes / 10_080.0
    }

    fn note_rate_limits(&self, limits: &CodexRateLimits) {
        let spend = self.spend_usd_total();
        if let Some(window) = &limits.primary {
            let prior = self.window_prior_usd(window);
            self.calib_primary
                .lock()
                .expect("primary window calibration lock")
                .observe(window.used_percent, window.resets_at, spend, prior);
        }
        if let Some(window) = &limits.secondary {
            let prior = self.window_prior_usd(window);
            self.calib_secondary
                .lock()
                .expect("secondary window calibration lock")
                .observe(window.used_percent, window.resets_at, spend, prior);
        }
    }

    /// Capacity report for one window slot, read against a just-observed snapshot.
    fn capacity_report(
        &self,
        slot: &'static str,
        window: &CodexRateLimitWindow,
        calib: &std::sync::Mutex<WindowCalibration>,
    ) -> CodexWindowCapacityReport {
        let prior = self.window_prior_usd(window);
        let cal = calib.lock().expect("window calibration lock");
        CodexWindowCapacityReport {
            slot,
            window_minutes: window.window_duration_mins,
            used_percent: window.used_percent,
            cap_usd: cal.cap_usd(prior),
            remaining_usd: cal.remaining_usd(window.used_percent, prior),
            calibrated: cal.calibrated(),
            samples: cal.samples(),
        }
    }

    /// Remaining official-price capacity on the most constrained reported window. This is the
    /// Codex equivalent of Claude pool's `placement_free`: calibrated capacity when available,
    /// otherwise the duration-scaled prior. Missing observational data stays fail-open and uses the
    /// configured weekly prior, matching the existing unknown-utilisation placement behavior.
    fn placement_free_usd(&self, limits: Option<&CodexRateLimits>) -> f64 {
        let Some(limits) = limits else {
            return self.cfg.window_cap_usd_prior.max(0.0);
        };
        let primary = limits.primary.as_ref().map(|window| {
            self.capacity_report("primary", window, &self.calib_primary)
                .remaining_usd
        });
        let secondary = limits.secondary.as_ref().map(|window| {
            self.capacity_report("secondary", window, &self.calib_secondary)
                .remaining_usd
        });
        match (primary, secondary) {
            (Some(primary), Some(secondary)) => primary.min(secondary).max(0.0),
            (Some(remaining), None) | (None, Some(remaining)) => remaining.max(0.0),
            (None, None) => self.cfg.window_cap_usd_prior.max(0.0),
        }
    }

    /// Whether the cached snapshot says this home is inside the configured window headroom.
    ///
    /// A missing snapshot is treated as available on purpose: the rate-limit endpoint is
    /// observational and must never become a hard availability dependency. Discovering the wall
    /// mid-turn is still handled by cooling on `usageLimitExceeded`.
    fn within_headroom(limits: Option<&CodexRateLimits>, admit_below_percent: i64) -> bool {
        let Some(limits) = limits else {
            return true;
        };
        if limits.reached {
            return false;
        }
        match limits.max_used_percent() {
            Some(used) => used < admit_below_percent,
            None => true,
        }
    }

    async fn status(&self, admit_below_percent: i64, now: i64) -> CodexHomeStatus {
        let process_live = self.live_process().await.is_some();
        let rate_limits = self.observed_rate_limits().await;
        let mut capacities = Vec::new();
        if let Some(limits) = &rate_limits {
            if let Some(window) = &limits.primary {
                capacities.push(self.capacity_report("primary", window, &self.calib_primary));
            }
            if let Some(window) = &limits.secondary {
                capacities.push(self.capacity_report("secondary", window, &self.calib_secondary));
            }
        }
        let _ = admit_below_percent;
        let _ = now;
        CodexHomeStatus {
            id: self.spec.id.clone(),
            process_live,
            auth_ok: self.auth_ok.load(Ordering::Relaxed),
            cooling_until: self.cooling_until(),
            inflight: self.inflight(),
            rate_limits,
            spend_usd_total: self.spend_usd_total(),
            capacities,
        }
    }

    /// When this home is expected to be usable again.
    async fn ready_at(&self, admit_below_percent: i64) -> i64 {
        let cooling = self.cooling_until();
        let limited_until = match self.rate_limits().await {
            Some(limits) if !Self::within_headroom(Some(&limits), admit_below_percent) => limits
                .soonest_reset_at_or_above(admit_below_percent)
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
    fn cache_root_seeds_a_second_competitive_home() {
        let candidates = [
            CacheRootCandidate {
                warm: true,
                free_capacity: 90.0,
            },
            CacheRootCandidate {
                warm: false,
                free_capacity: 80.0,
            },
        ];
        assert_eq!(cache_root_primary_index(&candidates), 1);
    }

    #[test]
    fn cache_root_prefers_a_warm_home_when_capacity_is_close() {
        let candidates = [
            CacheRootCandidate {
                warm: false,
                free_capacity: 90.0,
            },
            CacheRootCandidate {
                warm: true,
                free_capacity: 80.0,
            },
            CacheRootCandidate {
                warm: true,
                free_capacity: 75.0,
            },
        ];
        assert_eq!(cache_root_primary_index(&candidates), 1);
    }

    #[test]
    fn cache_root_yields_to_materially_better_cold_capacity() {
        let candidates = [
            CacheRootCandidate {
                warm: false,
                free_capacity: 90.0,
            },
            CacheRootCandidate {
                warm: true,
                free_capacity: 60.0,
            },
            CacheRootCandidate {
                warm: true,
                free_capacity: 50.0,
            },
        ];
        assert_eq!(cache_root_primary_index(&candidates), 0);
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
    /// Every home is cooling or out of window headroom; `ready_at` is the soonest recovery.
    Unavailable {
        ready_at: Option<i64>,
    },
}

/// Owns and restarts the pinned app-server processes for every configured home. The composition
/// layer calls `preflight` before exposing a configured provider, while later transport failures
/// are recovered lazily. Existing Claude routing is completely independent when Codex is disabled.
pub struct CodexGateway {
    cfg: Arc<CodexConfig>,
    shutting_down: AtomicBool,
    abort_turns: AtomicBool,
    /// Rotates equal-load cold/warm candidates. Without an atomic cursor, sequential requests and
    /// concurrent bursts with identical snapshots all collapse onto the lowest discovery order.
    selection_cursor: AtomicU64,
    abort_notify: Notify,
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
            .map(|(order, spec)| Arc::new(CodexHome::new(cfg.clone(), spec, order)))
            .collect();
        Ok(Self {
            cfg,
            shutting_down: AtomicBool::new(false),
            abort_turns: AtomicBool::new(false),
            selection_cursor: AtomicU64::new(0),
            abort_notify: Notify::new(),
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
                next.push(Arc::new(CodexHome::new(self.cfg.clone(), spec, order)));
            }
            next.sort_by_key(|home| home.order());
            *self.homes.write().await = next;
            return;
        }
    }

    pub fn config(&self) -> &CodexConfig {
        &self.cfg
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

    fn admit_below_percent(&self) -> i64 {
        self.cfg.admit_below_used_percent
    }

    /// Choose a usable home and take one of its turn slots.
    ///
    /// Selection is cache-first, mirroring the Claude fleet: a conversation pinned to a `preferred`
    /// home reuses that home's warm OpenAI prompt cache. A new shared cache root is seeded onto two
    /// competitive homes, then reuses a warm home only while it retains at least 70% of the fleet's
    /// best remaining capacity. Otherwise the request immediately uses the global capacity leader.
    /// With a single-home pool every branch resolves to that one home, so affinity is a no-op until
    /// the pool grows. `exclude` drops homes already tried this request, so a spill or retry never
    /// re-pins the preferred home that just failed.
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
        let admit_below = self.admit_below_percent();
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
            if home.is_cooling(now) {
                soonest = Some(
                    soonest.map_or(home.cooling_until(), |v: i64| v.min(home.cooling_until())),
                );
                continue;
            }
            let limits = home.rate_limits().await;
            if !CodexHome::within_headroom(limits.as_ref(), admit_below) {
                let ready_at = home.ready_at(admit_below).await;
                soonest = Some(soonest.map_or(ready_at, |v: i64| v.min(ready_at)));
                continue;
            }
            let free_capacity = home.placement_free_usd(limits.as_ref());
            candidates.push((free_capacity, home.inflight(), home.clone()));
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
            |(free_a, inflight_a, home_a), (free_b, inflight_b, home_b)| {
                let rotated_a = (home_a.order() + width - cursor) % width;
                let rotated_b = (home_b.order() + width - cursor) % width;
                free_b
                    .total_cmp(free_a)
                    .then_with(|| inflight_a.cmp(inflight_b))
                    .then_with(|| rotated_a.cmp(&rotated_b))
            },
        );
        // A resolved conversation is a hard first choice. A new shared root uses the exact same
        // two-replica/capacity-ratio policy as Claude; this only selects an ordering and never waits
        // for replicas or makes them an availability requirement.
        let mut ordered: Vec<Arc<CodexHome>> = Vec::with_capacity(candidates.len());
        if let Some(id) = preferred {
            if let Some((_, _, home)) = candidates.iter().find(|(_, _, home)| home.id() == id) {
                ordered.push(home.clone());
            }
        } else if place_cache_root {
            let placement = candidates
                .iter()
                .map(|(free_capacity, _, home)| CacheRootCandidate {
                    warm: warm.iter().any(|id| id == home.id()),
                    free_capacity: *free_capacity,
                })
                .collect::<Vec<_>>();
            let primary = cache_root_primary_index(&placement);
            ordered.push(candidates[primary].2.clone());
        }
        // If the primary choice is currently sampling, another warm copy is the next best spill;
        // remaining candidates stay in global capacity order.
        if place_cache_root {
            for (_, _, home) in &candidates {
                if warm.iter().any(|id| id == home.id())
                    && !ordered.iter().any(|chosen| Arc::ptr_eq(chosen, home))
                {
                    ordered.push(home.clone());
                }
            }
        }
        for (_, _, home) in candidates {
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
        for home in &self.homes().await {
            let process = match home.process().await {
                Ok(process) => process,
                Err(error) => {
                    home.note_process_error(&error);
                    continue;
                }
            };
            match process.probe().await {
                Ok(()) => home.mark_healthy(),
                Err(ProcessError::Timeout(stage)) => {
                    // Observational timeout: keep both the authenticated child and its admission
                    // state intact. A later successful request/probe clears the uncertainty.
                    eprintln!("Codex health probe timed out during {stage}");
                }
                Err(error) => {
                    home.note_process_error(&error);
                    home.invalidate(&process).await;
                }
            }
        }
        // Model discovery is intentionally best-effort and outside customer request handling. A
        // stale last-good snapshot (or the configured catalog before the first success) is safer
        // than turning an app-server catalog hiccup into a public 503 on every SDK startup.
        self.refresh_model_catalog().await;
    }

    /// Read only cached operational state. Metrics collection never starts a provider process or
    /// triggers an authentication or network request.
    pub async fn operational_status(&self) -> CodexOperationalStatus {
        let now = pool::now();
        let admit_below = self.admit_below_percent();
        let pool_homes = self.homes().await;
        let mut homes = Vec::with_capacity(pool_homes.len());
        let mut available = 0usize;
        let mut soonest: Option<i64> = None;
        for home in &pool_homes {
            let status = home.status(admit_below, now).await;
            let usable = status.process_live
                && status.auth_ok
                && !home.is_cooling(now)
                && CodexHome::within_headroom(status.rate_limits.as_ref(), admit_below);
            if usable {
                available += 1;
            } else {
                let ready_at = home.ready_at(admit_below).await;
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
    /// Apply the cooling policy for a failure observed outside a turn (startup, probe, discovery).
    fn note_process_error(&self, error: &ProcessError) {
        match error {
            ProcessError::AuthenticationRequired | ProcessError::SubscriptionRequired => {
                self.mark_auth_failed()
            }
            ProcessError::UsageLimitExceeded { retry_after } => {
                self.cool_for(retry_after.unwrap_or(DEFAULT_LIMIT_RETRY_SECS) as i64)
            }
            // A configuration or attestation fault is not transient and must not be retried in a
            // hot loop; the deployment gate is the place that fixes it.
            ProcessError::HomeInUse
            | ProcessError::HomeLockUnavailable
            | ProcessError::InvalidConfig(_)
            | ProcessError::DigestMismatch { .. }
            | ProcessError::VersionMismatch { .. }
            | ProcessError::Disabled => self.cool_for(AUTH_QUARANTINE_SECS),
            _ => self.cool_for(TRANSPORT_COOL_SECS),
        }
    }

    /// Blame classification for a failed turn, mirroring the Claude rotation policy.
    ///
    /// A usage limit or a dead login belongs to this home's subscription, so it leaves the rotation
    /// until its window resets or an operator re-authenticates. A client fault belongs to the
    /// request and must not cool a healthy account — otherwise one malformed request could quietly
    /// drain the pool.
    pub(crate) fn note_turn_error(&self, error: &ProcessError) {
        match error {
            ProcessError::BadRequest
            | ProcessError::ContextWindowExceeded
            | ProcessError::Rpc { .. }
            // RPC deadlines are isolated to the request and do not prove either an account or
            // transport fault. Cooling the home here would turn one slow client request into a
            // provider-wide admission outage even though its shared process remains live.
            | ProcessError::Timeout(_) => {}
            other => self.note_process_error(other),
        }
    }
}
