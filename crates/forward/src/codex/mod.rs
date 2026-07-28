//! OpenAI-compatible text API backed by the official Codex app-server protocol.
//!
//! The transport speaks newline-delimited JSON-RPC v2 to pinned Codex children. It never reads or
//! replays ChatGPT bearer tokens: authentication remains owned by the official Codex profiles.

mod api;
mod billing;
mod calibration;
mod chat;
mod config;
mod discovery;
mod history;
mod process;
mod runner;

pub use api::{
    delete_response as openai_delete_response, get_response as openai_get_response,
    input_tokens as openai_input_tokens, model as openai_model, models as openai_models,
    response_input_items as openai_response_input_items, responses as openai_responses,
};
pub use calibration::WindowCalibration;
pub use chat::completions as openai_chat_completions;
pub use config::{CodexConfig, CodexHomeSpec, CodexModel, CodexPrices};
pub use history::{HistoryError, StoredHistory};
pub(crate) use process::{AppServerEvent, CodexProcess, ProcessError};
pub use process::{CodexRateLimitWindow, CodexRateLimits};
pub(crate) use runner::{CodexTurnRequest, CodexTurnResult, CodexUsage, TurnUpdate};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use history::HistoryStore;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

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
        self.resolution.as_ref().map(|resolution| resolution.home.as_str())
    }

    /// Persist the affinity binding to the home that actually served the turn. Called only on
    /// success: a failed attempt must not pin a conversation to a home that could not serve it.
    async fn record_served(&mut self, served_home: &str) {
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
    /// Position in the discovered pool: explicit homes first, then scanned ones in name order.
    /// Only a tie-break for selection, so it may be renumbered freely as the pool changes; the
    /// stable identity used for labels is `spec.id`.
    order: AtomicUsize,
    cfg: Arc<CodexConfig>,
    process: Mutex<Option<Arc<CodexProcess>>>,
    process_start: Mutex<()>,
    /// Turns in flight on this home right now. Concurrency is deliberately unbounded, exactly like
    /// the Claude fleet: this counter is only a load signal for least-loaded selection and metrics,
    /// never a cap. A `TurnSlot` guard increments it on admission and decrements on drop, so a
    /// client disconnect never leaks a phantom busy slot.
    inflight: Arc<AtomicUsize>,
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
            spec,
            order: AtomicUsize::new(order),
            cfg,
            process: Mutex::new(None),
            process_start: Mutex::new(()),
            inflight: Arc::new(AtomicUsize::new(0)),
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

    /// Take an in-flight turn slot on this home. Concurrency is unbounded, so this always succeeds;
    /// the returned guard keeps the load counter accurate for selection and metrics until it drops.
    fn acquire_turn(self: &Arc<Self>) -> TurnSlot {
        self.inflight.fetch_add(1, Ordering::Relaxed);
        TurnSlot {
            inflight: self.inflight.clone(),
        }
    }

    /// Return a live, subscription-authenticated process. Startup is serialized per home, but
    /// normal JSON-RPC requests and independent turns are multiplexed over the same child.
    pub(crate) async fn process(&self) -> Result<Arc<CodexProcess>, ProcessError> {
        if let Some(process) = self.process.lock().await.as_ref().cloned() {
            if process.is_live() {
                return Ok(process);
            }
        }

        let _start = self.process_start.lock().await;
        if let Some(process) = self.process.lock().await.as_ref().cloned() {
            if process.is_live() {
                return Ok(process);
            }
        }

        let process =
            Arc::new(CodexProcess::spawn(self.cfg.clone(), &self.spec).await?);
        *self.process.lock().await = Some(process.clone());
        Ok(process)
    }

    async fn live_process(&self) -> Option<Arc<CodexProcess>> {
        self.process
            .lock()
            .await
            .as_ref()
            .filter(|process| process.is_live())
            .cloned()
    }

    pub(crate) async fn invalidate(&self, process: &Arc<CodexProcess>) {
        let _start = self.process_start.lock().await;
        process.shutdown().await;
        let mut current = self.process.lock().await;
        if current
            .as_ref()
            .is_some_and(|candidate| Arc::ptr_eq(candidate, process))
        {
            *current = None;
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

/// RAII load-counter guard for one in-flight turn. Dropping it — on success, error, or client
/// disconnect — releases the home's load slot. It is not a concurrency cap; the count only steers
/// least-loaded selection and feeds the inflight metric.
pub(crate) struct TurnSlot {
    inflight: Arc<AtomicUsize>,
}

impl Drop for TurnSlot {
    fn drop(&mut self) {
        self.inflight.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Outcome of choosing a home for one request.
enum HomeSelection {
    Ready(Arc<CodexHome>, TurnSlot),
    /// Every home is cooling or out of window headroom; `ready_at` is the soonest recovery.
    Unavailable { ready_at: Option<i64> },
}

/// Owns and restarts the pinned app-server processes for every configured home. The composition
/// layer calls `preflight` before exposing a configured provider, while later transport failures
/// are recovered lazily. Existing Claude routing is completely independent when Codex is disabled.
pub struct CodexGateway {
    cfg: Arc<CodexConfig>,
    /// Rediscovered on every health tick, so an account the authbot finishes buying joins the pool
    /// without a restart. Readers take a snapshot; the lock is never held across a turn.
    homes: RwLock<Vec<Arc<CodexHome>>>,
    history: Arc<HistoryStore>,
}

impl CodexGateway {
    pub fn new(cfg: CodexConfig) -> anyhow::Result<Self> {
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
        let specs = discovery::discover(&cfg.homes, cfg.homes_dir.as_deref());
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
            homes: RwLock::new(homes),
            history: Arc::new(history),
        })
    }

    /// Snapshot of the current pool.
    async fn homes(&self) -> Vec<Arc<CodexHome>> {
        self.homes.read().await.clone()
    }

    /// Reconcile the pool with what is on disk.
    ///
    /// An existing home keeps its live child and its cooling/auth state — rediscovery must never
    /// reset a quarantine or drop a warm process. A home whose directory disappeared leaves the
    /// pool; its child is dropped and killed with it.
    async fn rediscover(&self) {
        let specs = discovery::discover(&self.cfg.homes, self.cfg.homes_dir.as_deref());
        let mut homes = self.homes.write().await;
        let mut next: Vec<Arc<CodexHome>> = Vec::with_capacity(specs.len());
        for (order, spec) in specs.into_iter().enumerate() {
            match homes.iter().find(|home| home.path() == spec.path) {
                // Keep the live instance. A changed proxy only takes effect on the next child
                // start, which is correct: restarting a healthy child to re-read egress would
                // interrupt in-flight turns.
                Some(existing) => {
                    existing.order.store(order, Ordering::Relaxed);
                    next.push(existing.clone());
                }
                None => {
                    eprintln!("Codex home {} joined the pool", spec.id);
                    next.push(Arc::new(CodexHome::new(self.cfg.clone(), spec, order)));
                }
            }
        }
        for gone in homes.iter() {
            if !next.iter().any(|home| home.path() == gone.path()) {
                eprintln!("Codex home {} left the pool", gone.id());
            }
        }
        if next.is_empty() {
            // Never empty the pool from a scan: a transient unreadable directory would otherwise
            // take the whole provider down while the previous homes were still serving.
            eprintln!("Codex rediscovery found no homes; keeping the current pool");
            return;
        }
        *homes = next;
    }

    pub fn config(&self) -> &CodexConfig {
        &self.cfg
    }

    pub(crate) fn history(&self) -> &HistoryStore {
        &self.history
    }

    fn admit_below_percent(&self) -> i64 {
        self.cfg.admit_below_used_percent
    }

    /// Choose a usable home and take one of its turn slots.
    ///
    /// Selection is cache-first, mirroring the Claude fleet: a conversation pinned to a `preferred`
    /// home reuses that home's warm OpenAI prompt cache; failing that, a home already holding this
    /// request's shared system/tools cache root (`warm`) is preferred; only then does it fall back to
    /// the least-loaded home (lowest observed window utilisation, then fewest in-flight turns), which
    /// spreads load away from a subscription approaching its limit before it hits the wall. With a
    /// single-home pool every branch resolves to that one home, so affinity is a no-op until the pool
    /// grows. `exclude` drops homes already tried this request, so a spill or retry never re-pins the
    /// preferred home that just failed.
    async fn select_home(
        &self,
        exclude: &[String],
        preferred: Option<&str>,
        warm: &[String],
    ) -> HomeSelection {
        let now = pool::now();
        let admit_below = self.admit_below_percent();
        let pool_homes = self.homes().await;
        let mut candidates = Vec::with_capacity(pool_homes.len());
        let mut soonest: Option<i64> = None;
        for home in &pool_homes {
            if exclude.iter().any(|id| id == home.id()) {
                continue;
            }
            if home.is_cooling(now) {
                soonest = Some(soonest.map_or(home.cooling_until(), |v: i64| v.min(home.cooling_until())));
                continue;
            }
            let limits = home.rate_limits().await;
            if !CodexHome::within_headroom(limits.as_ref(), admit_below) {
                let ready_at = home.ready_at(admit_below).await;
                soonest = Some(soonest.map_or(ready_at, |v: i64| v.min(ready_at)));
                continue;
            }
            let used = limits
                .and_then(|limits: CodexRateLimits| limits.max_used_percent())
                .unwrap_or(0);
            candidates.push((used, home.inflight(), home.clone()));
        }
        if candidates.is_empty() {
            return HomeSelection::Unavailable { ready_at: soonest };
        }
        candidates.sort_by_key(|(used, inflight, home)| (*used, *inflight, home.order()));
        // 1) The conversation's pinned home, if it is currently usable. 2) Otherwise the least-loaded
        // home that already holds this request's shared cache root. 3) Otherwise the least-loaded home.
        // Concurrency per home is unbounded, so the chosen home always accepts the turn.
        let home = preferred
            .and_then(|id| {
                candidates
                    .iter()
                    .find(|(_, _, home)| home.id() == id)
                    .map(|(_, _, home)| home.clone())
            })
            .or_else(|| {
                candidates
                    .iter()
                    .find(|(_, _, home)| warm.iter().any(|id| id == home.id()))
                    .map(|(_, _, home)| home.clone())
            })
            .unwrap_or_else(|| {
                candidates
                    .into_iter()
                    .next()
                    .expect("candidates is non-empty")
                    .2
            });
        let slot = home.acquire_turn();
        HomeSelection::Ready(home, slot)
    }

    /// A process from any usable home, for provider-level reads such as model discovery.
    pub(crate) async fn any_process(&self) -> Result<Arc<CodexProcess>, ProcessError> {
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
    /// At least one home must pass. Requiring *every* home would reintroduce the single point of
    /// failure this pool exists to remove: one expired device login would block every future
    /// deployment. A home that fails here starts quarantined and is reported by the health metrics
    /// and `CodexHomeUnauthenticated` alert instead. Returning only a diagnostic class keeps account
    /// metadata, paths and child messages out of composition logs.
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
        if healthy == 0 {
            anyhow::bail!("Codex app-server preflight failed [{last_class}]");
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
        // Pick up accounts finished since the last tick before probing, so a new home is health
        // checked on the same pass that admits it.
        self.rediscover().await;
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
                Err(error) => {
                    home.note_process_error(&error);
                    home.invalidate(&process).await;
                }
            }
        }
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
            let usable = !home.is_cooling(now)
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
            ProcessError::InvalidConfig(_)
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
            | ProcessError::Rpc { .. } => {}
            other => self.note_process_error(other),
        }
    }
}
