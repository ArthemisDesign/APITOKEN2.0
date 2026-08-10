//! OpenAI-compatible text API backed by the native ChatGPT Codex backend.
//!
//! A profile pool speaks Responses-over-HTTPS to `chatgpt.com/backend-api/codex` through
//! per-profile clients pinned to the official client identity. OAuth material lives only inside
//! AEAD credential envelopes (`codex-credential`): the roster lists opaque profile ids and file
//! paths, and tokens are decrypted into memory, never into logs or metrics.

mod api;
mod billing;
mod calibration;
mod chat;
mod claudestore;
mod config;
mod discovery;
mod health;
pub mod history;
mod image_api;
mod images;
mod openai_image_snapshot;
mod runner;
mod skin;
mod transport;

pub use api::{
    delete_response as openai_delete_response, get_response as openai_get_response,
    input_tokens as openai_input_tokens, model as openai_model, models as openai_models,
    response_input_items as openai_response_input_items, responses as openai_responses,
};
pub(crate) use calibration::{apply_observation_with_history, ESTIMATOR_VERSION};
pub use calibration::{fraction_resolution_units, WindowCalibration};
pub use chat::completions as openai_chat_completions;
pub use config::{CodexConfig, CodexModel, CodexPrices, CodexProfileSpec, CodexProfilesFile};
pub use history::{HistoryError, StoredHistory};
pub use image_api::{edits as openai_image_edits, generations as openai_image_generations};
pub use images::{
    CodexImageError, CodexImageResult, GptImage2, ImageBackground, ImageEditRequest,
    ImageErrorContext, ImageGenerationRequest, ImageQuality, ImageReference, ImageSize,
    ImageTurnId, GPT_IMAGE_2, PUBLIC_IMAGE_MODEL_IDS,
};
pub(crate) use runner::{CodexTurnRequest, CodexTurnResult, CodexUsage, TurnUpdate};
pub use skin::{count_tokens as codex_messages_count_tokens, messages as codex_messages_skin};
pub use transport::RATE_LIMIT_FRACTION_SCALE;
pub(crate) use transport::{
    AppServerEvent, AuthContext, CodexModelCatalog, ImageDispatchError, ProfileTransport,
    TurnEvents,
};
pub use transport::{CodexRateLimitWindow, CodexRateLimits, ProcessError};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use crate::billing::AsyncBilling;
use crate::config::ClaudeStoreFallbackConfig;
use crate::metrics::Metrics;
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};
use codex_credential::{CodexCredential, SecretString};
use futures_util::StreamExt as _;
use history::HistoryStore;
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify, RwLock};

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
const MAX_PENDING_CALIBRATION_EVENTS: usize = 4_096;
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

/// Lower ranks are preferred for a requested Fast turn. The profile catalogue is the only useful
/// account-specific capability signal: ChatGPT's completed response tier is not an end-to-end Fast
/// verdict and must never defeat affinity or demote a profile that advertises Fast.
fn fast_route_rank(catalog_available: Option<bool>, catalog_fast_supported: Option<bool>) -> u8 {
    match (catalog_available, catalog_fast_supported) {
        (Some(true), Some(true)) => 0,
        (None, _) => 1,
        (Some(true), _) => 2,
        (Some(false), _) => 3,
    }
}
/// Match the Claude fleet's cache-root fanout: one shared prompt prefix is deliberately warmed on
/// two competitive homes so independent sessions do not collapse onto the first subscription.
const CACHE_ROOT_MIN_WARM_HOMES: usize = 2;
/// Bounded concurrency for startup preflight and the health sweep: at fleet scale (hundreds of
/// profiles) an unbounded burst of upstream calls is itself an outage and a ban signal.
const PREFLIGHT_CONCURRENCY: usize = 16;
const SWEEP_CONCURRENCY: usize = 32;
/// Once the server's advertised drain deadline has expired, cleanup itself must also be bounded.
/// systemd cannot start the replacement singleton while the old main PID remains in `deactivating`.
const FORCED_SHUTDOWN_CLEANUP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);
/// Refresh skew: an access token this close to expiry is replaced before it is sent upstream.
const ACCESS_TOKEN_SKEW_SECS: i64 = 300;
/// Idle healthy homes are re-probed no faster than this. Live traffic keeps their snapshot fresh,
/// so a cadence-level sweep over a large fleet would only burn upstream quota and look like a bot.
const HEALTHY_IDLE_PROBE_MIN_SECS: i64 = 60;

/// Soft window reserve, mirroring the Claude fleet's `pool::Reserve`: never route above
/// `1 − base` of a window (5h default 10%, 7d default 3%) so subscriptions stay off their hard
/// wall — fewer provider 429s, no quota-maxing automaton fingerprint, and the measured wall stays
/// evidence rather than a routine event. The threshold is jittered deterministically per profile
/// so the fleet does not cut at one percent; under peak the filter relaxes to FULL (fail open:
/// serving at 99% beats a synthetic 429 for the client).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct WindowReserve {
    pub base5h: f64,
    pub base7d: f64,
    pub jitter: f64,
}

impl WindowReserve {
    pub(crate) const FULL: Self = Self {
        base5h: 0.0,
        base7d: 0.0,
        jitter: 0.0,
    };

    /// Per-profile (cap_5h, cap_7d) as fractions in (0, 1]. The jitter is deterministic by
    /// (home id, window), stable over time, so each subscription has its own cutoff.
    fn caps(&self, home_id: &str) -> (f64, f64) {
        let jit = |salt: u8| -> f64 {
            if self.jitter <= 0.0 {
                return 0.0;
            }
            let mut material = home_id.as_bytes().to_vec();
            material.push(salt);
            let digest = blake3::hash(&material);
            let raw = u64::from_le_bytes(digest.as_bytes()[..8].try_into().unwrap_or([0; 8]));
            (raw % 2001) as f64 / 1000.0 - 1.0
        };
        let c5 = (1.0 - (self.base5h + jit(5) * self.jitter)).clamp(0.5, 1.0);
        let c7 = (1.0 - (self.base7d + jit(7) * self.jitter)).clamp(0.5, 1.0);
        (c5, c7)
    }

    /// Cap for one reported window: durations up to a day take the 5h reserve, longer windows
    /// (the weekly bucket) take the 7d reserve.
    fn window_cap(&self, window: &CodexRateLimitWindow, home_id: &str) -> f64 {
        let (c5, c7) = self.caps(home_id);
        match window.window_duration_mins {
            Some(duration) if duration > 24 * 60 => c7,
            _ => c5,
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

/// Operator-facing account hint for the control plane. Keep only the first four local-part
/// characters: this is enough to match a purchased account to its opaque home while the full
/// ChatGPT identity stays inside the sealed credential and never reaches logs, metrics or JSON.
fn mask_codex_email(email: &str) -> String {
    let local = email.split_once('@').map_or(email, |(local, _)| local);
    let head: String = local.chars().take(4).collect();
    format!("{head}…")
}

/// Per-home operational state. Homes are identified by their configured profile id, never by
/// path or account identity: metrics and logs must not carry customer or subscription identity.
#[derive(Clone, Debug)]
pub struct CodexHomeStatus {
    /// Stable, non-identifying id. Never a path and never an account identity.
    pub id: String,
    /// Privacy-safe operator hint derived from the credential email. The full address never leaves
    /// the sealed credential; this value contains at most four local-part characters.
    pub masked_email: String,
    /// Reviewed paid-plan identity from the sealed credential; safe for commercial aggregation.
    pub plan: String,
    /// Immutable credential issue time and the derived 30-day subscription horizon. Invalid or
    /// unavailable source values remain `None` rather than becoming sentinel timestamps.
    pub acquired_at: Option<i64>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_days_left: Option<f64>,
    /// The profile's credential opened and its transport was built.
    pub process_live: bool,
    /// This generation has proved the profile works (token plus one usage read or served turn).
    /// The deploy gate needs every home ready on the candidate before it will cut over, so this is
    /// the signal that predicts a blocked deploy.
    pub ready_published: bool,
    pub auth_ok: bool,
    /// `healthy` | `suspect` | `dead` — liveness of the subscription, independent of the transport.
    pub account_state: &'static str,
    /// `responsive` | `degraded` | `wedged` — responsiveness of the current transport generation.
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
    pub spend_nano_total: i64,
    pub spend_usd_total: f64,
    pub spend_nanocredits_total: Option<i64>,
    pub credit_tracking_started_ts: Option<i64>,
    pub calibration_pending_events: usize,
    pub calibration_dropped_events: u64,
    /// False after a persistence failure (or in an explicitly in-memory test gateway).
    pub calibration_persistence_ok: bool,
    /// Capacity estimate per reported window slot (empty until the first snapshot arrives).
    pub capacities: Vec<CodexWindowCapacityReport>,
    /// Per-model Fast capability, effective accepted Fast turns, and the provider's separate
    /// completed-response diagnostic tier.
    pub fast_tiers: Vec<CodexFastTierStatus>,
}

/// Privacy-safe Fast evidence for one configured upstream model on one opaque home.
#[derive(Clone, Debug)]
pub struct CodexFastTierStatus {
    pub model: String,
    /// `None` before this home has supplied a model catalogue.
    pub catalog_available: Option<bool>,
    /// `None` when the model catalogue is unknown or does not contain this model.
    pub catalog_fast_supported: Option<bool>,
    /// Backward-compatible effective tier: a successful requested Fast turn is `priority`.
    pub served_tier: Option<&'static str>,
    /// Raw completed-response tier for diagnostics. ChatGPT currently reports `default` even when
    /// the accepted priority request runs at the documented Fast cadence.
    pub provider_reported_tier: Option<&'static str>,
    pub observed_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ProviderReportedTier {
    Priority,
    Default,
}

impl ProviderReportedTier {
    fn as_str(self) -> &'static str {
        match self {
            Self::Priority => "priority",
            Self::Default => "default",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FastTierObservation {
    provider_reported: Option<ProviderReportedTier>,
    observed_at: i64,
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
    pub used_fraction_units: i64,
    pub used_percent: i64,
    /// Numeric resolution recoverable from the provider value. Whole percentages report
    /// `1_000_000` fraction units even though storage itself supports single-unit precision.
    pub measurement_resolution_fraction_units: i64,
    /// `None` until a real positive utilisation movement is paired with gateway spend.
    pub capacity_nano: Option<i64>,
    pub remaining_nano: Option<i64>,
    pub low_nano: Option<i64>,
    pub high_nano: Option<i64>,
    pub remaining_low_nano: Option<i64>,
    pub remaining_high_nano: Option<i64>,
    pub cap_usd: Option<f64>,
    pub remaining_usd: Option<f64>,
    pub low_usd: Option<f64>,
    pub high_usd: Option<f64>,
    pub remaining_low_usd: Option<f64>,
    pub remaining_high_usd: Option<f64>,
    pub capacity_nanocredits: Option<i64>,
    pub remaining_nanocredits: Option<i64>,
    pub low_nanocredits: Option<i64>,
    pub high_nanocredits: Option<i64>,
    pub remaining_low_nanocredits: Option<i64>,
    pub remaining_high_nanocredits: Option<i64>,
    pub observed_spend_nanocredits: Option<i64>,
    pub credit_samples: Option<i64>,
    pub unattributed_fraction_units: Option<i64>,
    pub observed_spend_nano: i64,
    pub observed_fraction_units: i64,
    /// `workload_blend` or `unknown`.
    pub source: &'static str,
    pub confidence: f64,
    pub samples: i64,
}

#[derive(Clone, Debug)]
pub struct CodexOperationalStatus {
    /// True when at least one home has a working credential and transport.
    pub process_live: bool,
    /// Snapshot of the most constrained home, for the provider-level rate-limit metrics.
    pub rate_limits: Option<CodexRateLimits>,
    pub homes: Vec<CodexHomeStatus>,
    /// Homes that could accept a request right now (credential loaded, not cooling and without a
    /// provider limit — an explicit reached verdict or a window at full utilisation).
    pub available: usize,
    /// Unix time when the first unavailable home is expected back, if any is cooling.
    pub soonest_ready: Option<i64>,
}

/// One sealed ChatGPT profile and the native client that serves it.
pub(crate) struct CodexHome {
    spec: CodexProfileSpec,
    /// Bounded operator hint, never the full credential identity. It is refreshed whenever a
    /// republished credential replaces the in-memory identity.
    masked_email: std::sync::RwLock<String>,
    /// Non-secret paid-plan classification, refreshed with the sealed credential.
    plan: std::sync::RwLock<String>,
    /// Position in the discovered pool: roster order. Only a tie-break for selection, so it may be
    /// renumbered freely as the pool changes; the stable identity used for labels is `spec.id`.
    order: AtomicUsize,
    cfg: Arc<CodexConfig>,
    /// Envelope key id this credential was sealed with; refresh re-seals under the same key.
    key_id: String,
    credential: Mutex<CodexCredential>,
    /// Modification time of the credential file at load, so a republished roster entry is picked
    /// up without a restart. Atomic authbot publication makes a torn read a non-case.
    credential_mtime: std::sync::Mutex<Option<std::time::SystemTime>>,
    transport: std::sync::Mutex<ProfileTransport>,
    /// Latest window snapshot, fed by usage probes and by live response headers.
    rate_limits: Arc<Mutex<Option<CodexRateLimits>>>,
    /// Last catalogue fetched through this exact profile. The provider may roll out model/tier
    /// availability account by account, so a fleet-wide set is insufficient for Fast routing.
    model_catalog: std::sync::RwLock<Option<CodexModelCatalog>>,
    /// Successful requested Fast outcomes, keyed by upstream model. The provider-reported tier is
    /// retained inside each observation for diagnostics but never drives selection.
    fast_observations: std::sync::Mutex<BTreeMap<String, FastTierObservation>>,
    /// Set once this generation proved the profile works (token plus usage read or a served turn).
    ready: AtomicBool,
    retired: Arc<AtomicBool>,
    /// Turns in flight on this home right now. Concurrency is deliberately unbounded: the native
    /// backend multiplexes independent requests over the same account. This counter is only a load
    /// signal for selection and metrics.
    inflight: Arc<AtomicUsize>,
    turns_idle: Arc<Notify>,
    /// Health and admission policy for this home, on two independent axes (account and transport).
    /// A plain `std::sync::Mutex`: every critical section is a few field writes and is never held
    /// across an await, so an async lock would only add cost.
    health: std::sync::Mutex<health::HomeHealth>,
    /// Last durable cumulative official-price spend returned by the authority (or local total in
    /// an explicitly in-memory test gateway).
    spend_nano_total: AtomicI64,
    spend_nanocredits_total: AtomicI64,
    credit_tracking_started_ts: AtomicI64,
    /// Exact failed events are retained in FIFO order; amount-only retries would destroy model and
    /// token-class evidence and could not be made idempotent across home retries.
    pending_calibration_events: std::sync::Mutex<VecDeque<registry::CodexTurnCalibrationEvent>>,
    calibration_flush: tokio::sync::Mutex<()>,
    calibration_dropped_events: AtomicU64,
    calibration_persistence_ok: AtomicBool,
    billing: Option<Arc<AsyncBilling>>,
    /// Provider slots are presentation only. Estimator identity is the actual duration; a primary
    /// and secondary window can change duration without inheriting each other's evidence.
    calibrations: std::sync::Mutex<BTreeMap<i64, WindowCalibration>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CalibrationFlushOutcome {
    queue_empty: bool,
    dropped_events: u64,
}

impl CodexHome {
    fn load(
        cfg: Arc<CodexConfig>,
        spec: CodexProfileSpec,
        order: usize,
        billing: Option<Arc<AsyncBilling>>,
    ) -> Result<Self, ProcessError> {
        let (key_id, credential, mtime) = open_credential(&cfg, &spec)?;
        let masked_email = mask_codex_email(&credential.email);
        let plan = credential.plan.clone();
        let transport = std::sync::Mutex::new(ProfileTransport::new(
            cfg.clone(),
            Some(credential.proxy.as_str()),
        )?);
        Ok(Self {
            spec,
            masked_email: std::sync::RwLock::new(masked_email),
            plan: std::sync::RwLock::new(plan),
            order: AtomicUsize::new(order),
            cfg,
            key_id,
            credential: Mutex::new(credential),
            credential_mtime: std::sync::Mutex::new(mtime),
            transport,
            rate_limits: Arc::new(Mutex::new(None)),
            model_catalog: std::sync::RwLock::new(None),
            fast_observations: std::sync::Mutex::new(BTreeMap::new()),
            ready: AtomicBool::new(false),
            retired: Arc::new(AtomicBool::new(false)),
            inflight: Arc::new(AtomicUsize::new(0)),
            turns_idle: Arc::new(Notify::new()),
            health: std::sync::Mutex::new(health::HomeHealth::new()),
            spend_nano_total: AtomicI64::new(0),
            spend_nanocredits_total: AtomicI64::new(0),
            credit_tracking_started_ts: AtomicI64::new(0),
            pending_calibration_events: std::sync::Mutex::new(VecDeque::new()),
            calibration_flush: tokio::sync::Mutex::new(()),
            calibration_dropped_events: AtomicU64::new(0),
            calibration_persistence_ok: AtomicBool::new(billing.is_some()),
            billing,
            calibrations: std::sync::Mutex::new(BTreeMap::new()),
        })
    }

    /// Stable, non-identifying id used in logs and metric labels.
    pub(crate) fn id(&self) -> &str {
        &self.spec.id
    }

    fn order(&self) -> usize {
        self.order.load(Ordering::Relaxed)
    }

    fn config(&self) -> &CodexConfig {
        &self.cfg
    }

    /// Current transport generation (clone is an Arc bump; a recycle swaps the value).
    fn transport(&self) -> ProfileTransport {
        self.transport.lock().expect("codex transport lock").clone()
    }

    fn inflight(&self) -> usize {
        self.inflight.load(Ordering::Relaxed)
    }

    /// New, unpinned conversations should give every healthy home at least one immutable turn.
    /// This is only a tie-break inside the normal health/Fast/load policy; an established affinity
    /// remains hard and a broken evidence writer is never preferred merely to generate more loss.
    fn calibration_seed_rank(&self) -> u8 {
        if self.calibration_persistence_ok.load(Ordering::Relaxed)
            && self.calibration_dropped_events.load(Ordering::Relaxed) == 0
            && self.credit_tracking_started_ts.load(Ordering::Relaxed) == 0
        {
            0
        } else {
            1
        }
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
                elog::error("codex", format!("Codex home {id} health persistence failed [{error:#}]"));
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
            Err(error) => elog::error(
                "codex",
                format!("Codex home {} health could not be recovered [{error:#}]", self.id()),
            ),
        }
    }

    fn cooling_until(&self) -> i64 {
        self.health().effective_cooling_until()
    }

    fn is_cooling(&self, now: i64) -> bool {
        self.health().effective_cooling_until() > now
    }

    /// A completed probe proves both axes at once: the account answered and the transport carried
    /// the answer. Nothing else is allowed to clear health, so a home cannot be declared well by
    /// anything weaker than actually serving.
    fn mark_healthy(&self) {
        self.ready.store(true, Ordering::Release);
        self.note(health::HealthSignal::ProbeOk);
    }

    /// A completed customer turn proves exactly the same two facts, earned from real traffic
    /// instead of an extra probe. This is the Codex counterpart of the Claude path harvesting live
    /// limits from every upstream response: healthy homes are kept verified by the work they do,
    /// so the background sweep only has to carry homes that are idle or already suspicious.
    fn mark_turn_healthy(&self) {
        self.ready.store(true, Ordering::Release);
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

    async fn wait_for_turns(&self) {
        loop {
            let notified = self.turns_idle.notified();
            if self.inflight.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }

    async fn retire(&self) -> Result<(), ProcessError> {
        self.retired.store(true, Ordering::Release);
        self.wait_for_turns().await;
        // No new turn can enter after `retired` flips. Flush the last completed turn before this
        // generation disappears so a blue-green handoff cannot strand evidence in process memory.
        self.retry_pending_calibration().await;
        Ok(())
    }

    /// Single-flight access token. The credential mutex serializes expiry checks and refresh, so
    /// a burst after expiry produces exactly one upstream refresh. OpenAI rotates the refresh
    /// token on every refresh with strict reuse detection, so the rotated material is re-sealed to
    /// disk before the lock is released; a concurrent engine generation that lost the rotation
    /// race reloads the winner's envelope from disk and retries exactly once.
    pub(crate) async fn access_token(&self) -> Result<SecretString, ProcessError> {
        let mut credential = self.credential.lock().await;
        let now = pool::now();
        if credential.expires_at > now.saturating_add(ACCESS_TOKEN_SKEW_SECS)
            && !credential.access_token.is_empty()
        {
            return Ok(SecretString::new(credential.access_token.clone()));
        }
        self.refresh_locked(&mut credential).await
    }

    /// Reuse the winner after a 401 burst: if a concurrent caller already replaced the rejected
    /// token, take it instead of serially refreshing once per rejected request.
    pub(crate) async fn access_token_after_rejection(
        &self,
        rejected_token: &str,
    ) -> Result<SecretString, ProcessError> {
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
        credential: &mut CodexCredential,
    ) -> Result<SecretString, ProcessError> {
        match self.refresh_once(credential).await {
            Ok(token) => Ok(token),
            Err(ProcessError::AuthenticationRequired) => {
                // We may hold a refresh token another engine generation already rotated. The
                // disk envelope is the shared authority: reload it and retry exactly once when
                // it carries different material.
                let Ok((_key_id, fresh, mtime)) = open_credential(&self.cfg, &self.spec) else {
                    return Err(ProcessError::AuthenticationRequired);
                };
                if fresh.refresh_token == credential.refresh_token {
                    return Err(ProcessError::AuthenticationRequired);
                }
                let masked_email = mask_codex_email(&fresh.email);
                let plan = fresh.plan.clone();
                *credential = fresh;
                *self.masked_email.write().expect("Codex masked email lock") = masked_email;
                *self.plan.write().expect("Codex plan lock") = plan;
                *self
                    .credential_mtime
                    .lock()
                    .expect("codex credential mtime lock") = mtime;
                // The peer that rotated first may also have left a perfectly valid access token:
                // reusing it is one rotation fewer for a family OpenAI throttles aggressively.
                let now = pool::now();
                if credential.expires_at > now.saturating_add(ACCESS_TOKEN_SKEW_SECS)
                    && !credential.access_token.is_empty()
                {
                    return Ok(SecretString::new(credential.access_token.clone()));
                }
                self.refresh_once(credential).await
            }
            Err(error) => Err(error),
        }
    }

    async fn refresh_once(
        &self,
        credential: &mut CodexCredential,
    ) -> Result<SecretString, ProcessError> {
        let tokens = self
            .transport()
            .refresh(
                &credential.token_uri,
                &credential.oauth_client_id,
                &credential.refresh_token,
            )
            .await?;
        credential.access_token = tokens.access_token;
        credential.expires_at = pool::now()
            .saturating_add(tokens.expires_in)
            .saturating_sub(60);
        if let Some(refresh) = tokens.refresh_token {
            // OpenAI rotates on every refresh; the new material must reach durable storage before
            // any other holder of the old token can burn the family.
            credential.refresh_token = refresh;
        }
        self.persist_credential(credential).await?;
        Ok(SecretString::new(credential.access_token.clone()))
    }

    /// Re-seal the credential and atomically replace its envelope (tmp file + rename), so a crash
    /// never leaves a half-written credential and a blue-green peer always reads a complete one.
    /// A persistence failure is transient: the rotated material stays in memory (it is ours alone
    /// already), the next refresh retries the write, and the account is never quarantined for a
    /// disk hiccup.
    async fn persist_credential(&self, credential: &CodexCredential) -> Result<(), ProcessError> {
        let envelope = self
            .cfg
            .credential_keys
            .seal(&self.key_id, &self.spec.id, credential)
            .map_err(|error| {
                ProcessError::InvalidConfig(format!("re-seal credential: {error:#}"))
            })?;
        let encoded = codex_credential::encode_envelope(&envelope).map_err(|error| {
            ProcessError::InvalidConfig(format!("encode credential: {error:#}"))
        })?;
        let path = std::path::PathBuf::from(&self.spec.credential_file);
        let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
        let write_result = async {
            tokio::fs::write(&tmp, &encoded).await?;
            tokio::fs::rename(&tmp, &path).await
        }
        .await;
        if let Err(error) = write_result {
            elog::error(
                "codex",
                format!("Codex home {} credential persistence failed [{error}]", self.id()),
            );
            return Err(ProcessError::Timeout("credential persist"));
        }
        if let Ok(metadata) = std::fs::metadata(&path) {
            *self
                .credential_mtime
                .lock()
                .expect("codex credential mtime lock") = metadata.modified().ok();
        }
        Ok(())
    }

    /// Pick up a republished envelope: new proxy or account material joins without a restart.
    /// A reload failure keeps the working in-memory credential — the file may belong to a
    /// half-finished purchase, and the next tick retries.
    async fn reload_if_republished(&self) {
        let mtime = std::fs::metadata(&self.spec.credential_file)
            .ok()
            .and_then(|metadata| metadata.modified().ok());
        let current = *self
            .credential_mtime
            .lock()
            .expect("codex credential mtime lock");
        if mtime.is_none() || mtime == current {
            return;
        }
        let Ok((_key_id, fresh, fresh_mtime)) = open_credential(&self.cfg, &self.spec) else {
            elog::warn(
                "codex",
                format!("Codex home {} credential reload failed; keeping current", self.id()),
            );
            return;
        };
        let mut credential = self.credential.lock().await;
        // Never move backwards in the rotation: our in-memory material may be newer than the file
        // if a persist raced this reload. The reload exists for *other* writers' updates.
        if fresh.refresh_token != credential.refresh_token
            || fresh.proxy != credential.proxy
            || fresh.email != credential.email
            || fresh.plan != credential.plan
            || fresh.issued_at != credential.issued_at
        {
            let masked_email = mask_codex_email(&fresh.email);
            let plan = fresh.plan.clone();
            *credential = fresh;
            *self.masked_email.write().expect("Codex masked email lock") = masked_email;
            *self.plan.write().expect("Codex plan lock") = plan;
            *self
                .credential_mtime
                .lock()
                .expect("codex credential mtime lock") = fresh_mtime;
        }
    }

    /// Pure read of the latest window snapshot. Selection, status and metrics paths use this:
    /// reading must never write, or every routed request would cost the fleet a calibration
    /// transaction per home.
    async fn cached_rate_limits(&self) -> Option<CodexRateLimits> {
        self.rate_limits.lock().await.clone()
    }

    fn cached_profile_catalog(&self) -> Option<CodexModelCatalog> {
        self.model_catalog
            .read()
            .expect("Codex model catalog lock")
            .clone()
    }

    fn publish_profile_catalog(&self, catalog: CodexModelCatalog) {
        *self
            .model_catalog
            .write()
            .expect("Codex model catalog lock") = Some(catalog);
    }

    /// Ordering signal for Fast requests. Catalogue support is the only account-specific
    /// capability evidence; the completed response's tier is diagnostic and never affects
    /// placement because ChatGPT reports `default` for measurably accelerated Fast turns.
    fn fast_route_rank(&self, model: &str) -> u8 {
        let catalog = self.model_catalog.read().expect("Codex model catalog lock");
        let catalog_available = catalog
            .as_ref()
            .map(|catalog| catalog.models.contains(model));
        let catalog_fast_supported = catalog.as_ref().and_then(|catalog| {
            catalog
                .models
                .contains(model)
                .then(|| catalog.fast_models.contains(model))
        });
        fast_route_rank(catalog_available, catalog_fast_supported)
    }

    fn observe_fast_result(&self, model: &str, provider_reported_tier: Option<&str>) {
        let provider_reported = match provider_reported_tier {
            Some("priority") => Some(ProviderReportedTier::Priority),
            Some("default") => Some(ProviderReportedTier::Default),
            _ => None,
        };
        self.fast_observations
            .lock()
            .expect("Codex Fast observation lock")
            .insert(
                model.to_string(),
                FastTierObservation {
                    provider_reported,
                    observed_at: pool::now(),
                },
            );
    }

    fn fast_tier_statuses(&self) -> Vec<CodexFastTierStatus> {
        let catalog = self.model_catalog.read().expect("Codex model catalog lock");
        let observations = self
            .fast_observations
            .lock()
            .expect("Codex Fast observation lock");
        let mut models = BTreeMap::new();
        for model in self.cfg.models.iter().filter(|model| model.supports_fast()) {
            models.entry(model.upstream.as_str()).or_insert_with(|| {
                let catalog_available = catalog
                    .as_ref()
                    .map(|catalog| catalog.models.contains(&model.upstream));
                let catalog_fast_supported = catalog.as_ref().and_then(|catalog| {
                    catalog
                        .models
                        .contains(&model.upstream)
                        .then(|| catalog.fast_models.contains(&model.upstream))
                });
                let observation = observations.get(&model.upstream).copied();
                CodexFastTierStatus {
                    model: model.upstream.clone(),
                    catalog_available,
                    catalog_fast_supported,
                    served_tier: observation.map(|_| "priority"),
                    provider_reported_tier: observation
                        .and_then(|observation| observation.provider_reported)
                        .map(ProviderReportedTier::as_str),
                    observed_at: observation.map(|observation| observation.observed_at),
                }
            });
        }
        models.into_values().collect()
    }

    /// Ingest one fresh snapshot from the wire (usage probe, response headers or SSE): store it
    /// and feed duration-keyed calibration. This is the ONLY path that persists observations.
    async fn ingest_rate_limits(&self, limits: CodexRateLimits) {
        *self.rate_limits.lock().await = Some(limits.clone());
        self.note_rate_limits(&limits).await;
    }

    /// Feed calibration from whatever the last turn's response headers published. One entry per
    /// served turn — the same cadence as spend crediting, never more.
    pub(crate) async fn ingest_turn_snapshot(&self) {
        if let Some(limits) = self.cached_rate_limits().await {
            self.note_rate_limits(&limits).await;
        }
    }

    /// Probe-time read used by status surfaces. Reads are free; persistence is not their job.
    async fn rate_limits(&self) -> Option<CodexRateLimits> {
        self.cached_rate_limits().await
    }

    pub(crate) async fn usage_limit_retry_after(&self) -> Option<u64> {
        let limits = self.rate_limits.lock().await.clone()?;
        let at = limits
            .soonest_reset_at_or_above(100)
            .unwrap_or_else(|| pool::now().saturating_add(DEFAULT_LIMIT_RETRY_SECS as i64));
        Some(at.saturating_sub(pool::now()).clamp(1, 7 * 24 * 3600) as u64)
    }

    /// Record one successful turn, retrying exact immutable evidence in FIFO order. Request-id
    /// idempotency in registry makes a lost writer reply and a home retry safe. `true` means the
    /// exact cumulative ledgers already include this turn, so a quota snapshot may safely consume
    /// them; `false` deliberately keeps that snapshot out of the estimator until background retry.
    pub(crate) async fn record_calibration_event(
        &self,
        event: registry::CodexTurnCalibrationEvent,
    ) -> bool {
        let dropped_before = self.calibration_dropped_events.load(Ordering::Relaxed);
        {
            let mut pending = self
                .pending_calibration_events
                .lock()
                .expect("Codex calibration event queue lock");
            if pending.len() >= MAX_PENDING_CALIBRATION_EVENTS {
                self.calibration_dropped_events
                    .fetch_add(1, Ordering::Relaxed);
                self.calibration_persistence_ok
                    .store(false, Ordering::Relaxed);
                elog::warn("codex", "Codex calibration event queue is full; evidence dropped");
                return false;
            }
            pending.push_back(event);
        }
        let outcome = self.flush_pending_calibration_events().await;
        outcome.queue_empty && outcome.dropped_events == dropped_before
    }

    /// Drain exact turn evidence without requiring another customer request. A transient authority
    /// outage must not strand one home in `calibration storage` forever merely because affinity
    /// sends its next turn elsewhere.
    async fn flush_pending_calibration_events(&self) -> CalibrationFlushOutcome {
        let _flush = self.calibration_flush.lock().await;
        let Some(billing) = &self.billing else {
            loop {
                let event = self
                    .pending_calibration_events
                    .lock()
                    .expect("Codex calibration event queue lock")
                    .pop_front();
                let Some(event) = event else {
                    break;
                };
                let api = self.spend_nano_total.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |current| current.checked_add(event.api_total_nanousd),
                );
                let credits = self.spend_nanocredits_total.fetch_update(
                    Ordering::AcqRel,
                    Ordering::Acquire,
                    |current| current.checked_add(event.chatgpt_total_nanocredits),
                );
                if api.is_err() || credits.is_err() {
                    self.calibration_dropped_events
                        .fetch_add(1, Ordering::Relaxed);
                }
                let _ = self.credit_tracking_started_ts.compare_exchange(
                    0,
                    event.completed_at,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
            self.calibration_persistence_ok
                .store(false, Ordering::Relaxed);
            return CalibrationFlushOutcome {
                queue_empty: true,
                dropped_events: self.calibration_dropped_events.load(Ordering::Relaxed),
            };
        };
        loop {
            let event = self
                .pending_calibration_events
                .lock()
                .expect("Codex calibration event queue lock")
                .front()
                .cloned();
            let Some(event) = event else {
                self.calibration_persistence_ok.store(
                    self.calibration_dropped_events.load(Ordering::Relaxed) == 0,
                    Ordering::Relaxed,
                );
                break;
            };
            match billing.record_codex_turn(event.clone()).await {
                Ok(total) => {
                    self.spend_nano_total
                        .store(total.spent_nano, Ordering::Relaxed);
                    if let Some(credits) = total.spent_nanocredits {
                        self.spend_nanocredits_total
                            .store(credits, Ordering::Relaxed);
                    }
                    if let Some(started) = total.credit_tracking_started_ts {
                        self.credit_tracking_started_ts
                            .store(started, Ordering::Relaxed);
                    }
                    let mut pending = self
                        .pending_calibration_events
                        .lock()
                        .expect("Codex calibration event queue lock");
                    if pending
                        .front()
                        .is_some_and(|front| front.request_id == event.request_id)
                    {
                        pending.pop_front();
                    }
                }
                Err(error) if registry::is_codex_turn_calibration_replay_conflict(&error) => {
                    let mut pending = self
                        .pending_calibration_events
                        .lock()
                        .expect("Codex calibration event queue lock");
                    if pending
                        .front()
                        .is_some_and(|front| front.request_id == event.request_id)
                    {
                        pending.pop_front();
                    }
                    self.calibration_dropped_events
                        .fetch_add(1, Ordering::Relaxed);
                    self.calibration_persistence_ok
                        .store(false, Ordering::Relaxed);
                    elog::warn("codex", "Codex calibration event quarantined after immutable replay conflict");
                }
                Err(error) => {
                    self.calibration_persistence_ok
                        .store(false, Ordering::Relaxed);
                    elog::error(
                        "codex",
                        format!(
                            "Codex calibration event persistence failed [{}]",
                            error.root_cause()
                        ),
                    );
                    break;
                }
            }
        }
        CalibrationFlushOutcome {
            queue_empty: self
                .pending_calibration_events
                .lock()
                .expect("Codex calibration event queue lock")
                .is_empty(),
            dropped_events: self.calibration_dropped_events.load(Ordering::Relaxed),
        }
    }

    /// Retry a failed exact event first and only then replay the cached post-turn quota snapshot.
    /// This ordering prevents a database hiccup from permanently classifying real gateway spend as
    /// foreign usage.
    async fn retry_pending_calibration(&self) {
        let had_pending = !self
            .pending_calibration_events
            .lock()
            .expect("Codex calibration event queue lock")
            .is_empty();
        if !had_pending {
            return;
        }
        let dropped_before = self.calibration_dropped_events.load(Ordering::Relaxed);
        let outcome = self.flush_pending_calibration_events().await;
        if outcome.queue_empty && outcome.dropped_events == dropped_before {
            self.ingest_turn_snapshot().await;
        }
    }

    pub(crate) fn reject_calibration_event(&self, error: &anyhow::Error) {
        self.calibration_dropped_events
            .fetch_add(1, Ordering::Relaxed);
        self.calibration_persistence_ok
            .store(false, Ordering::Relaxed);
        elog::warn(
            "codex",
            format!("Codex calibration evidence rejected [{}]", error.root_cause()),
        );
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
                            window.used_fraction_units,
                            limits.observed_at,
                        )
                        .await
                }
                None => Err(anyhow::anyhow!("in-memory calibration")),
            };
            match persisted {
                Ok((spend, row)) => {
                    self.spend_nano_total
                        .fetch_max(spend.spent_nano, Ordering::Relaxed);
                    if let Some(credits) = spend.spent_nanocredits {
                        self.spend_nanocredits_total
                            .fetch_max(credits, Ordering::Relaxed);
                    }
                    if let Some(started) = spend.credit_tracking_started_ts {
                        let _ = self.credit_tracking_started_ts.compare_exchange(
                            0,
                            started,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                    }
                    match WindowCalibration::from_row(row) {
                        Ok(calibration) => {
                            self.calibrations
                                .lock()
                                .expect("Codex calibration map lock")
                                .insert(duration, calibration);
                        }
                        Err(error) => {
                            all_persisted = false;
                            elog::error(
                                "codex",
                                format!(
                                    "Codex window calibration state invalid [{}]",
                                    error.root_cause()
                                ),
                            );
                        }
                    }
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
                        used_fraction_units: window.used_fraction_units,
                        gateway_spend_nano: spend_nano,
                        gateway_spend_nanocredits: (self
                            .credit_tracking_started_ts
                            .load(Ordering::Relaxed)
                            > 0)
                        .then(|| self.spend_nanocredits_total.load(Ordering::Relaxed)),
                    };
                    let mut calibrations = self
                        .calibrations
                        .lock()
                        .expect("Codex calibration map lock");
                    let existing = calibrations.remove(&duration).map(|cal| cal.into_row());
                    match calibration::apply_observation(existing, &observation) {
                        Ok(mut row) => {
                            if row.version == 0 {
                                row.version = 1;
                            }
                            match WindowCalibration::from_row(row) {
                                Ok(calibration) => {
                                    calibrations.insert(duration, calibration);
                                }
                                Err(state_error) => elog::error(
                                    "codex",
                                    format!(
                                        "Codex in-memory calibration state invalid [{}]",
                                        state_error.root_cause()
                                    ),
                                ),
                            }
                        }
                        Err(state_error) => elog::error(
                            "codex",
                            format!(
                                "Codex in-memory calibration observation invalid [{}]",
                                state_error.root_cause()
                            ),
                        ),
                    }
                    if self.billing.is_some() {
                        elog::error(
                            "codex",
                            format!(
                                "Codex window calibration persistence failed [{}]",
                                error.root_cause()
                            ),
                        );
                    }
                }
            }
        }
        self.calibration_persistence_ok.store(
            all_persisted
                && self
                    .pending_calibration_events
                    .lock()
                    .expect("Codex calibration event queue lock")
                    .is_empty()
                && self.calibration_dropped_events.load(Ordering::Relaxed) == 0,
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
        let remaining_nano =
            calibration.and_then(|cal| cal.remaining_nano(window.used_fraction_units));
        let remaining_low_nano =
            calibration.and_then(|cal| cal.remaining_low_nano(window.used_fraction_units));
        let remaining_high_nano =
            calibration.and_then(|cal| cal.remaining_high_nano(window.used_fraction_units));
        let credit_estimate = calibration.and_then(WindowCalibration::credit_estimate);
        let remaining_nanocredits =
            calibration.and_then(|cal| cal.remaining_nanocredits(window.used_fraction_units));
        let remaining_low_nanocredits =
            calibration.and_then(|cal| cal.remaining_low_nanocredits(window.used_fraction_units));
        let remaining_high_nanocredits =
            calibration.and_then(|cal| cal.remaining_high_nanocredits(window.used_fraction_units));
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
            used_fraction_units: window.used_fraction_units,
            used_percent: window.used_percent,
            measurement_resolution_fraction_units: fraction_resolution_units(
                window.used_fraction_units,
            ),
            capacity_nano: estimate.map(|value| value.capacity_nano),
            remaining_nano,
            low_nano: estimate.and_then(|value| value.low_nano),
            high_nano: estimate.and_then(|value| value.high_nano),
            remaining_low_nano,
            remaining_high_nano,
            cap_usd: estimate.map(|value| nano_to_usd(value.capacity_nano)),
            remaining_usd: remaining_nano.map(nano_to_usd),
            low_usd: estimate.and_then(|value| value.low_nano).map(nano_to_usd),
            high_usd: estimate.and_then(|value| value.high_nano).map(nano_to_usd),
            remaining_low_usd: remaining_low_nano.map(nano_to_usd),
            remaining_high_usd: remaining_high_nano.map(nano_to_usd),
            capacity_nanocredits: credit_estimate.map(|value| value.capacity_nanocredits),
            remaining_nanocredits,
            low_nanocredits: credit_estimate.and_then(|value| value.low_nanocredits),
            high_nanocredits: credit_estimate.and_then(|value| value.high_nanocredits),
            remaining_low_nanocredits,
            remaining_high_nanocredits,
            observed_spend_nanocredits: calibration
                .and_then(|cal| cal.row().observed_spend_nanocredits),
            credit_samples: calibration.and_then(|cal| cal.row().credit_samples),
            unattributed_fraction_units: calibration
                .and_then(|cal| cal.row().unattributed_fraction_units),
            observed_spend_nano: calibration.map_or(0, |cal| cal.row().observed_spend_nano),
            observed_fraction_units: calibration.map_or(0, |cal| cal.row().observed_fraction_units),
            source: estimate.map_or("unknown", |value| value.source.as_str()),
            confidence: estimate.map_or(0.0, |value| value.confidence_bp as f64 / 10_000.0),
            samples: calibration.map_or(0, |cal| cal.row().samples),
        }
    }

    /// Soft-reserve verdict: the earliest reset among windows that already sit at or above their
    /// jittered cap for this home, if any. Selection treats it as a rejection until that reset.
    fn reserve_blocked_until(
        &self,
        limits: Option<&CodexRateLimits>,
        reserve: &WindowReserve,
    ) -> Option<i64> {
        reserve_blocked(limits, self.id(), reserve)
    }

    /// Only an explicit provider verdict blocks selection: `limit_reached` or `allowed: false`.
    ///
    /// A window at `usedPercent=100` with `allowed: true` still serves — verified live on
    /// 2026-07-31: the provider's percentage can include usage outside this gateway, and the
    /// provider's own `allowed`/`limit_reached` fields are the only authoritative stop signal.
    /// Steering away from a near-full window is the soft reserve's job (97%/98% caps), not a hard
    /// exclusion's: killing a serving account would burn real capacity and strand conversations.
    fn within_provider_limit(limits: Option<&CodexRateLimits>) -> bool {
        let Some(limits) = limits else {
            return true;
        };
        !limits.reached
    }

    /// Normalise a protocol snapshot into the plain view the health policy consumes, so `health.rs`
    /// stays free of transport types and remains testable without the wire protocol.
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

    fn admission_ignoring_soft_cooling(
        &self,
        limits: Option<&CodexRateLimits>,
        now: i64,
    ) -> health::Admission {
        self.health().admission_ignoring_soft_cooling(
            Self::limit_view(limits).as_ref(),
            now,
            self.probe_interval_secs(),
        )
    }

    async fn status(&self) -> CodexHomeStatus {
        let rate_limits = self.cached_rate_limits().await;
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
        // Read issued_at from the active sealed credential so a republished envelope updates the
        // lifecycle atomically with the material it describes.
        let lifecycle =
            crate::lifecycle::fixed_days(self.credential.lock().await.issued_at, 30, now);
        CodexHomeStatus {
            id: self.spec.id.clone(),
            masked_email: self
                .masked_email
                .read()
                .expect("Codex masked email lock")
                .clone(),
            plan: self.plan.read().expect("Codex plan lock").clone(),
            acquired_at: lifecycle.acquired_at,
            subscription_expires_at: lifecycle.subscription_expires_at,
            subscription_days_left: lifecycle.subscription_days_left,
            process_live: true,
            ready_published: self.ready.load(Ordering::Acquire),
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
            cooling_until: health.effective_cooling_until(),
            inflight: self.inflight(),
            limit_reached: !Self::within_provider_limit(rate_limits.as_ref()),
            snapshot_age_secs: rate_limits
                .as_ref()
                .map(|limits| now.saturating_sub(limits.observed_at)),
            rate_limits,
            spend_nano_total: self.spend_nano_total.load(Ordering::Relaxed),
            spend_usd_total: self.spend_usd_total(),
            spend_nanocredits_total: (self.credit_tracking_started_ts.load(Ordering::Relaxed) > 0)
                .then(|| self.spend_nanocredits_total.load(Ordering::Relaxed)),
            credit_tracking_started_ts: {
                let started = self.credit_tracking_started_ts.load(Ordering::Relaxed);
                (started > 0).then_some(started)
            },
            calibration_pending_events: self
                .pending_calibration_events
                .lock()
                .expect("Codex calibration event queue lock")
                .len(),
            calibration_dropped_events: self.calibration_dropped_events.load(Ordering::Relaxed),
            calibration_persistence_ok: self.calibration_persistence_ok.load(Ordering::Relaxed),
            capacities,
            fast_tiers: self.fast_tier_statuses(),
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

    /// Whether the background sweep should spend a probe on this home right now.
    ///
    /// Live turns are the freshest evidence there is, so a busy home costs nothing to skip.
    /// Healthy idle homes are probed at a slow floor cadence; anything suspicious, stale,
    /// flagged, or never probed is checked every tick so recovery is quick to see.
    fn probe_due(&self, now: i64) -> bool {
        if self.retired.load(Ordering::Acquire) {
            return false;
        }
        // A transient window-observation failure gets another real provider snapshot on the next
        // sweep. Permanent quarantined/dropped evidence remains visible to operators without
        // turning the health loop into an endless upstream hot poll.
        if !self.calibration_persistence_ok.load(Ordering::Relaxed)
            && self.calibration_dropped_events.load(Ordering::Relaxed) == 0
        {
            return true;
        }
        let health = self.health();
        if health.wants_probe() {
            return true;
        }
        if self.inflight() > 0 {
            return false;
        }
        let healthy = matches!(
            (health.account, health.transport),
            (
                health::AccountState::Healthy,
                health::TransportState::Responsive
            )
        );
        let threshold = if healthy {
            self.probe_interval_secs().max(HEALTHY_IDLE_PROBE_MIN_SECS)
        } else {
            self.probe_interval_secs()
        };
        match self.rate_limits.try_lock() {
            Ok(guard) => match guard.as_ref() {
                None => true,
                Some(limits) => now.saturating_sub(limits.observed_at) >= threshold,
            },
            Err(_) => true,
        }
    }

    /// Read-only probe: prove the token refreshes (or is valid) and the account answers, and
    /// harvest the window snapshot for calibration and steering.
    async fn probe(&self) -> Result<(), ProcessError> {
        let token = self.access_token().await?;
        let auth = AuthContext {
            access_token: token,
            account_id: self.credential.lock().await.account_id.clone(),
        };
        let limits = self.transport().fetch_usage(&auth).await?;
        self.ingest_rate_limits(limits).await;
        Ok(())
    }

    /// Read the live availability catalogue through this home's identity.
    async fn fetch_models(&self) -> Result<CodexModelCatalog, ProcessError> {
        let token = self.access_token().await?;
        let auth = AuthContext {
            access_token: token,
            account_id: self.credential.lock().await.account_id.clone(),
        };
        let catalog = self.transport().fetch_models(&auth).await?;
        self.publish_profile_catalog(catalog.clone());
        Ok(catalog)
    }

    /// Translate a transport-level failure into the health signal it actually proves.
    ///
    /// The mapping is the whole point: every error class names the evidence it provides and the
    /// policy in `health.rs` decides what that evidence is worth.
    fn note_transport_error(&self, error: &ProcessError) {
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
            // A missed deadline is evidence on the transport axis; a streak closes admission.
            ProcessError::Timeout(_) => health::HealthSignal::Deadline,
            // A configuration fault is not transient and must not be retried in a hot loop; the
            // deployment gate is the place that fixes it. Treated as a permanent account-level
            // verdict so the home stays out until an operator acts.
            ProcessError::InvalidConfig(_) | ProcessError::Disabled => {
                health::HealthSignal::AuthRejected { permanent: true }
            }
            // EOF or a protocol violation: the current transport generation is suspect.
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
            ProcessError::BadRequest | ProcessError::ContextWindowExceeded => {}
            other => self.note_transport_error(other),
        }
    }

    /// A wedged transport generation is replaced by swapping in a fresh client. Unlike the
    /// app-server era there is no child to reap; in-flight turns hold their own cloned handle
    /// and finish on the old connections.
    fn recycle_transport(&self) {
        let proxy = self
            .credential
            .try_lock()
            .map(|credential| credential.proxy.clone())
            .unwrap_or_default();
        match ProfileTransport::new(self.cfg.clone(), Some(proxy.as_str())) {
            Ok(transport) => {
                *self.transport.lock().expect("codex transport lock") = transport;
                elog::warn(
                    "codex",
                    format!("Codex home {} transport recycled after wedge verdict", self.id()),
                );
            }
            Err(error) => {
                elog::error(
                    "codex",
                    format!(
                        "Codex home {} transport recycle failed [{}]",
                        self.id(),
                        error.diagnostic_class()
                    ),
                );
            }
        }
    }
}

/// Open and validate one sealed credential from the roster. The profile id is the AEAD associated
/// data, so an envelope copied between profiles fails closed here.
fn open_credential(
    cfg: &CodexConfig,
    spec: &CodexProfileSpec,
) -> Result<(String, CodexCredential, Option<std::time::SystemTime>), ProcessError> {
    let bytes = std::fs::read(&spec.credential_file)
        .map_err(|error| ProcessError::InvalidConfig(format!("credential unreadable: {error}")))?;
    let envelope = codex_credential::decode_envelope(&bytes).map_err(|error| {
        ProcessError::InvalidConfig(format!("credential undecodable: {error:#}"))
    })?;
    let credential = cfg
        .credential_keys
        .open(&spec.id, &envelope)
        .map_err(|error| {
            ProcessError::InvalidConfig(format!("credential unsealable: {error:#}"))
        })?;
    let mtime = std::fs::metadata(&spec.credential_file)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    Ok((envelope.key_id, credential, mtime))
}

#[cfg(test)]
mod admission_tests {
    use super::*;

    #[test]
    fn aggregate_catalog_keeps_only_a_cross_profile_context_guarantee() {
        let catalog = |model: &str, limit: Option<u64>| CodexModelCatalog {
            models: HashSet::from([model.to_string()]),
            input_token_limits: limit
                .map(|limit| std::collections::HashMap::from([(model.to_string(), limit)]))
                .unwrap_or_default(),
            ..Default::default()
        };

        let aggregate = aggregate_model_catalogs(
            [
                catalog("gpt-shared", Some(400_000)),
                catalog("gpt-shared", Some(272_000)),
                catalog("gpt-other", Some(128_000)),
            ],
            true,
        );
        assert_eq!(aggregate.input_token_limits["gpt-shared"], 272_000);
        assert_eq!(aggregate.input_token_limits["gpt-other"], 128_000);

        let uncertain = aggregate_model_catalogs(
            [
                catalog("gpt-shared", Some(272_000)),
                catalog("gpt-shared", None),
                catalog("gpt-shared", Some(400_000)),
            ],
            true,
        );
        assert!(uncertain.models.contains("gpt-shared"));
        assert!(!uncertain.input_token_limits.contains_key("gpt-shared"));

        let incomplete = aggregate_model_catalogs([catalog("gpt-shared", Some(272_000))], false);
        assert!(incomplete.models.contains("gpt-shared"));
        assert!(incomplete.input_token_limits.is_empty());
    }

    #[test]
    fn aggregate_catalog_keeps_only_non_conflicting_provider_display_names() {
        let catalog = |name: Option<&str>| CodexModelCatalog {
            models: HashSet::from(["gpt-shared".to_string()]),
            display_names: name
                .map(|name| {
                    std::collections::HashMap::from([("gpt-shared".to_string(), name.to_string())])
                })
                .unwrap_or_default(),
            ..Default::default()
        };

        let matching = aggregate_model_catalogs(
            [catalog(Some("GPT Shared")), catalog(Some("GPT Shared"))],
            true,
        );
        assert_eq!(matching.display_names["gpt-shared"], "GPT Shared");

        let conflicting = aggregate_model_catalogs(
            [
                catalog(Some("GPT Shared")),
                catalog(Some("Different rollout name")),
                catalog(Some("GPT Shared")),
            ],
            true,
        );
        assert!(!conflicting.display_names.contains_key("gpt-shared"));
    }

    #[test]
    fn operator_email_hint_never_exposes_the_full_identity() {
        let masked = mask_codex_email("owner.account@example.com");
        assert_eq!(masked, "owne…");
        assert!(!masked.contains('@'));
        assert!(!masked.contains("example.com"));
    }

    #[test]
    fn fast_routing_uses_catalog_capability_without_false_response_demotion() {
        assert_eq!(fast_route_rank(Some(true), Some(true)), 0);
        assert_eq!(fast_route_rank(None, None), 1);
        assert_eq!(fast_route_rank(Some(true), Some(false)), 2);
        assert_eq!(fast_route_rank(Some(false), None), 3);
    }

    #[test]
    fn admission_trusts_explicit_provider_limit_over_numeric_fullness() {
        let observed_hundred = CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_fraction_units: 100_000_000,
                used_percent: 100,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            reached: false,
            observed_at: 100,
        };
        // An explicit provider reached verdict leaves rotation immediately. A window merely
        // reporting 100% with `allowed: true` stays routable — the soft reserve steers around it.
        assert!(CodexHome::within_provider_limit(Some(&observed_hundred)));
        assert!(!CodexHome::within_provider_limit(Some(&CodexRateLimits {
            reached: true,
            ..observed_hundred.clone()
        })));
        assert!(CodexHome::within_provider_limit(Some(&CodexRateLimits {
            primary: None,
            ..observed_hundred
        })));
        assert!(CodexHome::within_provider_limit(None));
    }

    #[test]
    fn secondary_fullness_only_drives_retry_hint_until_provider_reaches_wall() {
        let limits = CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_fraction_units: 32_000_000,
                used_percent: 32,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(CodexRateLimitWindow {
                used_fraction_units: 100_000_000,
                used_percent: 100,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_444_800),
            }),
            reached: false,
            observed_at: 100,
        };
        assert!(CodexHome::within_provider_limit(Some(&limits)));
        assert!(!CodexHome::within_provider_limit(Some(&CodexRateLimits {
            reached: true,
            ..limits.clone()
        })));
        // The client-facing wait is the reset of the window that is actually full.
        assert_eq!(
            limits.soonest_reset_at_or_above(100),
            Some(4_102_444_800),
            "retry-after follows the exhausted window"
        );
    }

    fn reserve_limits(five_h: i64, weekly: i64) -> CodexRateLimits {
        CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_fraction_units: five_h * 1_000_000,
                used_percent: five_h,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: Some(CodexRateLimitWindow {
                used_fraction_units: weekly * 1_000_000,
                used_percent: weekly,
                window_duration_mins: Some(10_080),
                resets_at: Some(4_102_500_000),
            }),
            reached: false,
            observed_at: 100,
        }
    }

    #[test]
    fn reserve_blocks_above_cap_and_fails_open_below_it() {
        let reserve = WindowReserve {
            base5h: 0.10,
            base7d: 0.03,
            jitter: 0.0,
        };
        // 99% of the 5h window (cap 98%) blocks even though the provider wall (100%) is near.
        let reserve98 = WindowReserve {
            base5h: 0.02,
            base7d: 0.03,
            jitter: 0.0,
        };
        assert_eq!(
            reserve_blocked(Some(&reserve_limits(99, 10)), "home-a", &reserve98),
            Some(4_102_444_800)
        );
        assert_eq!(
            reserve_blocked(Some(&reserve_limits(98, 10)), "home-a", &reserve98),
            None
        );
        // 98% of the weekly window (cap 97%) blocks on the weekly reset.
        assert_eq!(
            reserve_blocked(Some(&reserve_limits(20, 98)), "home-a", &reserve),
            Some(4_102_500_000)
        );
        // At or below both caps the home stays routable; FULL never blocks, even at the wall.
        assert_eq!(
            reserve_blocked(Some(&reserve_limits(90, 97)), "home-a", &reserve),
            None
        );
        assert_eq!(
            reserve_blocked(
                Some(&reserve_limits(100, 100)),
                "home-a",
                &WindowReserve::FULL
            ),
            None
        );
        // No evidence and missing resets never invent a block.
        assert_eq!(reserve_blocked(None, "home-a", &reserve), None);
    }

    #[test]
    fn reserve_jitter_is_deterministic_per_home() {
        let reserve = WindowReserve {
            base5h: 0.10,
            base7d: 0.03,
            jitter: 0.02,
        };
        let (a5, a7) = reserve.caps("home-a");
        let (b5, b7) = reserve.caps("home-b");
        assert_eq!(
            reserve.caps("home-a"),
            (a5, a7),
            "caps must be stable per home"
        );
        assert!(
            (0.86..=0.94).contains(&a5),
            "5h cap stays near its base: {a5}"
        );
        assert!(
            (0.95..=0.99).contains(&a7),
            "weekly cap stays near 0.97: {a7}"
        );
        // The fleet must not cut at one percent: different homes get different thresholds.
        assert!(
            a5 != b5 || a7 != b7,
            "jitter spreads thresholds across homes"
        );
    }
}

#[cfg(test)]
mod calibration_integration_tests {
    use super::*;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    struct TestFleet {
        gateway: Arc<CodexGateway>,
        billing: Arc<AsyncBilling>,
        metrics: Arc<Metrics>,
        root: PathBuf,
        database: PathBuf,
    }

    impl TestFleet {
        async fn close(self) {
            self.gateway.shutdown().await;
            self.billing.flush().await.unwrap();
            drop(self.gateway);
            drop(self.billing);
            let _ = std::fs::remove_dir_all(self.root);
            let _ = std::fs::remove_file(self.database);
        }
    }

    fn test_models() -> Vec<CodexModel> {
        let advertised = [
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
            "gpt-5.4",
        ];
        metering::codex_catalog_at(i64::MAX)
            .into_iter()
            .filter(|model| advertised.contains(&model.id))
            .map(|model| CodexModel {
                id: model.id.to_string(),
                upstream: model.upstream.to_string(),
                created: 0,
                owned_by: "test".to_string(),
                max_output_tokens: model.max_output_tokens,
                reasoning_efforts: model
                    .reasoning_efforts
                    .iter()
                    .map(|effort| (*effort).to_string())
                    .collect(),
                input_modalities: vec!["text".to_string(), "image".to_string()],
                output_modalities: vec!["text".to_string()],
                tool_calling: true,
                structured_outputs: true,
                fast_multiplier_basis_points: model.subscription_fast_multiplier_basis_points,
                prices: model.prices,
            })
            .collect()
    }

    fn test_fleet(home_count: usize, base_url: &str) -> TestFleet {
        test_fleet_with_fallback(home_count, base_url, None)
    }

    fn test_fleet_with_fallback(
        home_count: usize,
        base_url: &str,
        fallback: Option<ClaudeStoreFallbackConfig>,
    ) -> TestFleet {
        static TEST_FLEET_SEQUENCE: AtomicU64 = AtomicU64::new(0);
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = TEST_FLEET_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "claude-api-codex-calibration-fleet-{}-{unique}-{sequence}",
            std::process::id(),
        ));
        let credentials = root.join("credentials");
        std::fs::create_dir_all(&credentials).unwrap();
        let keyring =
            codex_credential::CredentialKeyring::parse(&format!("current:{}", "ab".repeat(32)))
                .unwrap();
        let mut profiles = Vec::new();
        for index in 0..home_count {
            let id = format!("home-{index}");
            let credential = codex_credential::CodexCredential {
                version: 1,
                access_token: format!("test-access-{index}"),
                refresh_token: format!("test-refresh-{index}"),
                expires_at: i64::MAX / 2,
                oauth_client_id: codex_credential::CODEX_OFFICIAL_OAUTH_CLIENT_ID.to_string(),
                token_uri: codex_credential::CODEX_OFFICIAL_TOKEN_URI.to_string(),
                account_id: format!("acct_test_{index}"),
                email: format!("owner{index}@example.test"),
                plan: "chatgpt_pro".to_string(),
                proxy: String::new(),
                proxy_order_id: 0,
                issued_at: 1,
            };
            let envelope = keyring.seal("current", &id, &credential).unwrap();
            let path = credentials.join(format!("{id}.json"));
            std::fs::write(&path, codex_credential::encode_envelope(&envelope).unwrap()).unwrap();
            profiles.push(serde_json::json!({
                "id": id,
                "credential_file": path.to_str().unwrap(),
            }));
        }
        let roster = root.join("profiles.json");
        std::fs::write(
            &roster,
            serde_json::to_vec(&serde_json::json!({"profiles": profiles})).unwrap(),
        )
        .unwrap();
        let database = root.with_extension("sqlite");
        let billing =
            Arc::new(AsyncBilling::start(database.to_string_lossy().into_owned(), 1).unwrap());
        let metrics = Arc::new(Metrics::new());
        let gateway = Arc::new(
            CodexGateway::new_with_calibration_and_fallback(
                CodexConfig {
                    smooth_wait_ms: 0,
                    enabled: true,
                    base_url: base_url.to_string(),
                    profiles_file: roster.to_string_lossy().into_owned(),
                    credential_keys: keyring,
                    cli_version: codex_credential::CODEX_CLI_VERSION.to_string(),
                    request_timeout_ms: 5_000,
                    turn_timeout_ms: 5_000,
                    turn_silence_timeout_ms: 5_000,
                    health_probe_interval_secs: 10,
                    reserve_5h: 0.10,
                    reserve_7d: 0.03,
                    reserve_jitter: 0.0,
                    reserve_overhead_tokens: 0,
                    history_ttl_secs: 60,
                    history_local_cap: 16,
                    history_redis_url: None,
                    history_secret: Some("test-history-secret".to_string()),
                    history_redis_timeout_ms: 10,
                    default_proxy_env: BTreeMap::new(),
                    models: test_models(),
                },
                Some(billing.clone()),
                fallback,
                metrics.clone(),
            )
            .unwrap(),
        );
        TestFleet {
            gateway,
            billing,
            metrics,
            root,
            database,
        }
    }

    fn calibration_event(
        request_id: &str,
        home_id: &str,
        api_total_nanousd: i64,
        chatgpt_total_nanocredits: i64,
        completed_at: i64,
    ) -> registry::CodexTurnCalibrationEvent {
        registry::CodexTurnCalibrationEvent {
            request_id: request_id.to_string(),
            home_id: home_id.to_string(),
            model_id: "gpt-5.6-terra".to_string(),
            service_tier: "standard".to_string(),
            provider_reported_tier: Some("default".to_string()),
            api_tariff_schedule_id: "openai/test/v1".to_string(),
            credit_schedule_id: "chatgpt/test/v1".to_string(),
            completed_at,
            input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_input_tokens: 0,
            output_tokens: 0,
            reasoning_output_tokens: 0,
            api_input_nanousd: api_total_nanousd,
            api_cached_input_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 0,
            api_total_nanousd,
            chatgpt_input_nanocredits: chatgpt_total_nanocredits,
            chatgpt_cached_input_nanocredits: 0,
            chatgpt_output_nanocredits: 0,
            chatgpt_total_nanocredits,
        }
    }

    fn limits(used_percent: i64, observed_at: i64) -> CodexRateLimits {
        CodexRateLimits {
            primary: Some(CodexRateLimitWindow {
                used_fraction_units: used_percent * 1_000_000,
                used_percent,
                window_duration_mins: Some(300),
                resets_at: Some(4_102_444_800),
            }),
            secondary: None,
            reached: false,
            observed_at,
        }
    }

    fn expected_turn_prices(model: &str, tier: &str) -> ([i64; 4], [i64; 3]) {
        let (api, credits, api_fast_bp, credit_fast_bp) = match model {
            "gpt-5.6-sol" => (
                [350_000, 10_000, 62_500, 600_000],
                [10_000_000, 250_000, 15_000_000],
                20_000,
                25_000,
            ),
            "gpt-5.6-terra" => (
                [140_000, 4_000, 25_000, 240_000],
                [4_000_000, 100_000, 6_000_000],
                20_000,
                25_000,
            ),
            "gpt-5.6-luna" => (
                [14_000, 400, 2_500, 24_000],
                [400_000, 10_000, 600_000],
                20_000,
                25_000,
            ),
            "gpt-5.5" => (
                [350_000, 10_000, 50_000, 600_000],
                [10_000_000, 250_000, 15_000_000],
                25_000,
                25_000,
            ),
            "gpt-5.4" => (
                [175_000, 5_000, 25_000, 300_000],
                [5_000_000, 125_000, 7_500_000],
                20_000,
                20_000,
            ),
            other => panic!("missing independent price fixture for {other}"),
        };
        let (api_multiplier, credit_multiplier) = if tier == "fast" {
            (api_fast_bp, credit_fast_bp)
        } else {
            (10_000, 10_000)
        };
        (
            api.map(|value| value * api_multiplier / 10_000),
            credits.map(|value| value * credit_multiplier / 10_000),
        )
    }

    async fn mock_turn_upstream_with_usage(
        zero_usage: bool,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let accounts = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = accounts.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 4_096];
                    let header_end = loop {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                        if let Some(offset) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            break offset + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    let account = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("chatgpt-account-id")
                                .then(|| value.trim().to_string())
                        })
                        .unwrap_or_else(|| "missing-account".to_string());
                    while request.len() < header_end.saturating_add(content_length) {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                    }
                    seen.lock().unwrap().push(account);
                    // Keep the turn in flight briefly so concurrent-selection regressions are
                    // observable instead of being hidden by an immediate response.
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                    let body = if zero_usage {
                        concat!(
                            "event: response.output_text.delta\n",
                            "data: {\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
                            "event: response.output_item.done\n",
                            "data: {\"item\":{\"type\":\"message\",\"id\":\"msg_1\",",
                            "\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",",
                            "\"text\":\"ok\"}]}}\n\n",
                            "event: response.completed\n",
                            "data: {\"response\":{\"service_tier\":\"default\",\"usage\":{",
                            "\"input_tokens\":0,\"output_tokens\":0,\"total_tokens\":0}}}\n\n"
                        )
                    } else {
                        concat!(
                            "event: response.output_text.delta\n",
                            "data: {\"item_id\":\"msg_1\",\"delta\":\"ok\"}\n\n",
                            "event: response.output_item.done\n",
                            "data: {\"item\":{\"type\":\"message\",\"id\":\"msg_1\",",
                            "\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",",
                            "\"text\":\"ok\"}]}}\n\n",
                            "event: response.completed\n",
                            "data: {\"response\":{\"service_tier\":\"default\",\"usage\":{",
                            "\"input_tokens\":100,\"input_tokens_details\":{",
                            "\"cached_tokens\":20,\"cache_write_tokens\":10},",
                            "\"output_tokens\":20,\"output_tokens_details\":{",
                            "\"reasoning_tokens\":5},\"total_tokens\":120}}}\n\n"
                        )
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{address}/codex"), accounts)
    }

    async fn mock_turn_upstream() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        mock_turn_upstream_with_usage(false).await
    }

    async fn mock_terminal_upstream(status: u16) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 4_096];
                    loop {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    let response = format!(
                        "HTTP/1.1 {status} terminal\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{address}/codex")
    }

    async fn mock_claudestore_responses(
        status: u16,
        authoritative_usage: bool,
    ) -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let requests = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = requests.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 4_096];
                    let header_end = loop {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                        if let Some(offset) =
                            request.windows(4).position(|window| window == b"\r\n\r\n")
                        {
                            break offset + 4;
                        }
                    };
                    let head = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = head
                        .lines()
                        .find_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            name.eq_ignore_ascii_case("content-length")
                                .then(|| value.trim().parse::<usize>().ok())
                                .flatten()
                        })
                        .unwrap_or(0);
                    while request.len() < header_end.saturating_add(content_length) {
                        let Ok(read) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if read == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..read]);
                    }
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&request).to_string());
                    if status != 200 {
                        let response = format!(
                            "HTTP/1.1 {status} failed\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                        return;
                    }
                    let usage = if authoritative_usage {
                        "\"input_tokens\":100,\"output_tokens\":20,\"total_tokens\":120"
                    } else {
                        "\"input_tokens\":0,\"output_tokens\":0,\"total_tokens\":0"
                    };
                    let body = format!(
                        "event: response.output_text.delta\n\
                         data: {{\"item_id\":\"msg_fallback\",\"delta\":\"fallback\"}}\n\n\
                         event: response.output_item.done\n\
                         data: {{\"item\":{{\"type\":\"message\",\"id\":\"msg_fallback\",\"role\":\"assistant\",\"content\":[{{\"type\":\"output_text\",\"text\":\"fallback\"}}]}}}}\n\n\
                         event: response.completed\n\
                         data: {{\"response\":{{\"service_tier\":\"default\",\"usage\":{{{usage}}}}}}}\n\n"
                    );
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\n\
                         content-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{address}"), requests)
    }

    fn install_calibration_write_failure(database: &std::path::Path) {
        let connection = registry::open(database.to_str().unwrap()).unwrap();
        connection
            .execute_batch(
                "CREATE TRIGGER reject_codex_calibration_test \
                 BEFORE INSERT ON codex_turn_calibration_events \
                 BEGIN SELECT RAISE(FAIL, 'injected calibration write failure'); END;",
            )
            .unwrap();
    }

    fn remove_calibration_write_failure(database: &std::path::Path) {
        let connection = registry::open(database.to_str().unwrap()).unwrap();
        connection
            .execute_batch("DROP TRIGGER reject_codex_calibration_test")
            .unwrap();
    }

    async fn turn_routing(
        store: Arc<AffinityStore>,
        model: &CodexModel,
        session_id: &str,
    ) -> TurnRouting {
        let input = store
            .infer_codex(
                "calibration-test-tenant",
                &axum::http::HeaderMap::new(),
                &model.id,
                None,
                &[],
                &[serde_json::json!({"type": "text", "text": "measure"})],
                Some(session_id),
            )
            .unwrap();
        let resolution = store.resolve(&input).await;
        let warm = if resolution.is_none() {
            store.warm_homes(&input).await
        } else {
            Vec::new()
        };
        TurnRouting::new(store, input, resolution, warm)
    }

    fn turn(model: CodexModel, fast: bool, sequence: usize) -> CodexTurnRequest {
        CodexTurnRequest {
            model,
            prompt_cache_key: Some(format!("calibration-integration-{sequence}")),
            base_instructions: None,
            developer_instructions: None,
            injected_items: Vec::new(),
            turn_input: vec![serde_json::json!({"type": "text", "text": "measure"})],
            dynamic_tools: Vec::new(),
            service_tier: fast.then(|| "priority".to_string()),
            reasoning_effort: Some("none".to_string()),
            reasoning_summary: None,
            output_schema: None,
            verbosity: None,
        }
    }

    #[tokio::test]
    async fn exhausted_local_pool_uses_claudestore_once_without_local_identity_or_calibration() {
        let local = mock_terminal_upstream(429).await;
        let (external, requests) = mock_claudestore_responses(200, true).await;
        let fleet = test_fleet_with_fallback(
            1,
            &local,
            Some(ClaudeStoreFallbackConfig::for_test(external)),
        );
        let model = fleet
            .gateway
            .config()
            .models
            .iter()
            .find(|model| model.id == "gpt-5.5")
            .unwrap()
            .clone();

        let result = fleet
            .gateway
            .run_turn(turn(model, false, 1), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 100);
        assert_eq!(result.usage.output_tokens, 20);
        assert_eq!(result.output[0]["content"][0]["text"], "fallback");
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_attempts),
            1
        );
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_successes),
            1
        );
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_failures),
            0
        );

        let captured = requests.lock().unwrap();
        assert_eq!(captured.len(), 1);
        let wire = captured[0].to_ascii_lowercase();
        assert!(wire.starts_with("post /v1/responses http/1.1"));
        assert!(wire.contains("authorization: bearer sk-cs4-test-only-placeholder"));
        assert!(!wire.contains("chatgpt-account-id"));
        assert!(!wire.contains("originator:"));
        assert!(!wire.contains("client_metadata"));
        let body = captured[0].split("\r\n\r\n").nth(1).unwrap();
        let body: serde_json::Value = serde_json::from_str(body).unwrap();
        assert_eq!(body["model"], "gpt-5.5");
        assert_eq!(body["stream"], true);
        drop(captured);

        fleet.billing.flush().await.unwrap();
        assert!(fleet
            .billing
            .codex_calibration_report()
            .await
            .unwrap()
            .is_empty());
        fleet.close().await;
    }

    #[tokio::test]
    async fn healthy_local_codex_home_never_calls_claudestore() {
        let (local, _) = mock_turn_upstream().await;
        let (external, requests) = mock_claudestore_responses(200, true).await;
        let fleet = test_fleet_with_fallback(
            1,
            &local,
            Some(ClaudeStoreFallbackConfig::for_test(external)),
        );
        let model = fleet
            .gateway
            .config()
            .models
            .iter()
            .find(|model| model.id == "gpt-5.5")
            .unwrap()
            .clone();

        let result = fleet
            .gateway
            .run_turn(turn(model, false, 5), None, None)
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 100);
        assert!(requests.lock().unwrap().is_empty());
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_attempts),
            0
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn claudestore_is_not_attempted_for_models_outside_the_reviewed_allow_list() {
        let local = mock_terminal_upstream(429).await;
        let (external, requests) = mock_claudestore_responses(200, true).await;
        let fleet = test_fleet_with_fallback(
            1,
            &local,
            Some(ClaudeStoreFallbackConfig::for_test(external)),
        );
        let model = fleet
            .gateway
            .config()
            .models
            .iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .unwrap()
            .clone();

        assert!(matches!(
            fleet
                .gateway
                .run_turn(turn(model, false, 2), None, None)
                .await,
            Err(ProcessError::UsageLimitExceeded { .. })
        ));
        assert!(requests.lock().unwrap().is_empty());
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_attempts),
            0
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn failed_claudestore_attempt_keeps_local_terminal_and_is_counted_once() {
        let local = mock_terminal_upstream(429).await;
        let (external, requests) = mock_claudestore_responses(500, false).await;
        let fleet = test_fleet_with_fallback(
            1,
            &local,
            Some(ClaudeStoreFallbackConfig::for_test(external)),
        );
        let model = fleet
            .gateway
            .config()
            .models
            .iter()
            .find(|model| model.id == "gpt-5.4")
            .unwrap()
            .clone();

        let error = fleet
            .gateway
            .run_turn(turn(model, false, 3), None, None)
            .await
            .unwrap_err();
        assert!(matches!(
            error,
            ProcessError::ExternalFallbackFailed { local }
                if matches!(*local, ProcessError::UsageLimitExceeded { .. })
        ));
        assert_eq!(requests.lock().unwrap().len(), 1);
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_attempts),
            1
        );
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_successes),
            0
        );
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_failures),
            1
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn claudestore_success_without_terminal_usage_fails_closed() {
        let local = mock_terminal_upstream(429).await;
        let (external, _) = mock_claudestore_responses(200, false).await;
        let fleet = test_fleet_with_fallback(
            1,
            &local,
            Some(ClaudeStoreFallbackConfig::for_test(external)),
        );
        let model = fleet
            .gateway
            .config()
            .models
            .iter()
            .find(|model| model.id == "gpt-5.5")
            .unwrap()
            .clone();

        assert!(matches!(
            fleet
                .gateway
                .run_turn(turn(model, false, 4), None, None)
                .await,
            Err(ProcessError::ExternalFallbackFailed { .. })
        ));
        assert_eq!(
            Metrics::get(&fleet.metrics.claudestore_fallback_failures),
            1
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn real_turns_seed_every_home_and_persist_every_model_tier_and_usage_leg() {
        let (base_url, accounts) = mock_turn_upstream().await;
        let fleet = test_fleet(4, &base_url);
        let models = fleet.gateway.config().models.clone();
        assert_eq!(models.len(), 5);

        let mut sequence = 0usize;
        for model in &models {
            for fast in [false, true] {
                // Sequential first turns are intentional: the test proves evidence seeding, not
                // accidental fan-out caused only by concurrent in-flight load.
                for _ in 0..4 {
                    fleet
                        .gateway
                        .run_turn(turn(model.clone(), fast, sequence), None, None)
                        .await
                        .unwrap();
                    sequence += 1;
                }
            }
        }

        let report = fleet.billing.codex_calibration_report().await.unwrap();
        assert_eq!(report.len(), 4 * 5 * 2);
        assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 40);
        for home_index in 0..4 {
            let home_id = format!("home-{home_index}");
            let rows: Vec<_> = report.iter().filter(|row| row.home_id == home_id).collect();
            assert_eq!(rows.len(), 10, "{home_id} must see every model/tier");
            for model in &models {
                for tier in ["standard", "fast"] {
                    let row = rows
                        .iter()
                        .find(|row| row.model_id == model.id && row.service_tier == tier)
                        .unwrap_or_else(|| panic!("missing {home_id} {} {tier}", model.id));
                    assert_eq!(row.turns, 1);
                    assert_eq!(row.input_tokens, 100);
                    assert_eq!(row.cached_input_tokens, 20);
                    assert_eq!(row.cache_write_input_tokens, 10);
                    assert_eq!(row.output_tokens, 20);
                    assert_eq!(row.reasoning_output_tokens, 5);
                    let (api, credits) = expected_turn_prices(&model.id, tier);
                    assert_eq!(row.api_input_nanousd, api[0]);
                    assert_eq!(row.api_cached_input_nanousd, api[1]);
                    assert_eq!(row.api_cache_write_nanousd, api[2]);
                    assert_eq!(row.api_output_nanousd, api[3]);
                    assert_eq!(row.api_total_nanousd, api.into_iter().sum::<i64>());
                    assert_eq!(row.chatgpt_input_nanocredits, credits[0]);
                    assert_eq!(row.chatgpt_cached_input_nanocredits, credits[1]);
                    assert_eq!(row.chatgpt_output_nanocredits, credits[2]);
                    assert_eq!(
                        row.chatgpt_total_nanocredits,
                        credits.into_iter().sum::<i64>()
                    );
                }
            }
        }

        let account_counts = accounts.lock().unwrap().iter().cloned().fold(
            BTreeMap::<String, usize>::new(),
            |mut counts, account| {
                *counts.entry(account).or_default() += 1;
                counts
            },
        );
        assert_eq!(account_counts.len(), 4);
        assert!(account_counts.values().all(|count| *count == 10));

        let status = fleet.gateway.operational_status().await;
        assert_eq!(status.homes.len(), 4);
        assert!(status.homes.iter().all(|home| {
            home.credit_tracking_started_ts.is_some()
                && home.calibration_pending_events == 0
                && home.calibration_dropped_events == 0
                && home.calibration_persistence_ok
        }));
        fleet.close().await;
    }

    #[tokio::test]
    async fn downstream_disconnect_still_persists_authoritative_turn_evidence() {
        let (base_url, _) = mock_turn_upstream().await;
        let fleet = test_fleet(1, &base_url);
        let (updates, receiver) = tokio::sync::mpsc::channel(1);
        drop(receiver);
        let result = fleet
            .gateway
            .run_turn(
                turn(fleet.gateway.config().models[0].clone(), false, 0),
                Some(updates),
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.usage.input_tokens, 100);
        assert_eq!(result.usage.cache_write_input_tokens, 10);
        let report = fleet.billing.codex_calibration_report().await.unwrap();
        assert_eq!(report.len(), 1);
        assert_eq!(report[0].turns, 1);
        assert_eq!(report[0].api_total_nanousd, 1_022_500);
        assert_eq!(report[0].chatgpt_total_nanocredits, 25_250_000);
        fleet.close().await;
    }

    #[tokio::test]
    async fn pending_turn_recovery_persists_spend_before_replaying_quota() {
        let fleet = test_fleet(1, "http://127.0.0.1:1/codex");
        let home = fleet.gateway.homes().await.into_iter().next().unwrap();

        assert!(
            home.record_calibration_event(calibration_event(
                "anchor-turn",
                home.id(),
                10_000_000_000,
                1_000_000_000,
                100,
            ))
            .await
        );
        home.note_rate_limits(&limits(10, 100)).await;

        // Model the exact production failure: the successful turn is queued, while its freshest
        // provider snapshot already sits in memory. The retry must never observe 12% against the
        // old cumulative spend.
        home.pending_calibration_events
            .lock()
            .unwrap()
            .push_back(calibration_event(
                "recovered-turn",
                home.id(),
                40_000_000_000,
                4_000_000_000,
                101,
            ));
        home.calibration_persistence_ok
            .store(false, Ordering::Relaxed);
        *home.rate_limits.lock().await = Some(limits(12, 101));

        home.retry_pending_calibration().await;

        let status = home.status().await;
        assert_eq!(status.calibration_pending_events, 0);
        assert_eq!(status.calibration_dropped_events, 0);
        assert!(status.calibration_persistence_ok);
        assert_eq!(status.spend_nano_total, 50_000_000_000);
        assert_eq!(status.spend_nanocredits_total, Some(5_000_000_000));
        let primary = status
            .capacities
            .iter()
            .find(|window| window.slot == "primary")
            .unwrap();
        assert_eq!(primary.observed_fraction_units, 2_000_000);
        assert_eq!(primary.observed_spend_nano, 40_000_000_000);
        assert_eq!(primary.observed_spend_nanocredits, Some(4_000_000_000));
        assert_eq!(primary.capacity_nano, Some(2_000_000_000_000));
        assert_eq!(primary.capacity_nanocredits, Some(200_000_000_000));

        let report = fleet.billing.codex_calibration_report().await.unwrap();
        assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 2);
        fleet.close().await;
    }

    #[tokio::test]
    async fn health_probe_retries_transient_writer_failure_without_customer_traffic() {
        let fleet = test_fleet(1, "http://127.0.0.1:1/codex");
        let home = fleet.gateway.homes().await.into_iter().next().unwrap();
        let now = pool::now();

        assert!(
            home.record_calibration_event(calibration_event(
                "probe-anchor",
                home.id(),
                10_000_000_000,
                1_000_000_000,
                now,
            ))
            .await
        );
        home.note_rate_limits(&limits(10, now)).await;

        install_calibration_write_failure(&fleet.database);
        assert!(
            !home
                .record_calibration_event(calibration_event(
                    "probe-retry",
                    home.id(),
                    40_000_000_000,
                    4_000_000_000,
                    now.saturating_add(1),
                ))
                .await
        );
        *home.rate_limits.lock().await = Some(limits(12, now.saturating_add(1)));
        let failed = home.status().await;
        assert_eq!(failed.calibration_pending_events, 1);
        assert!(!failed.calibration_persistence_ok);
        remove_calibration_write_failure(&fleet.database);

        // No second customer request is made. The ordinary health sweep must drain local
        // authority work even though the home has a fresh, healthy provider snapshot.
        fleet.gateway.probe_health().await;

        let recovered = home.status().await;
        assert_eq!(recovered.calibration_pending_events, 0);
        assert_eq!(recovered.calibration_dropped_events, 0);
        assert!(recovered.calibration_persistence_ok);
        let primary = recovered
            .capacities
            .iter()
            .find(|window| window.slot == "primary")
            .unwrap();
        assert_eq!(primary.observed_spend_nano, 40_000_000_000);
        assert_eq!(primary.observed_spend_nanocredits, Some(4_000_000_000));
        assert_eq!(
            fleet
                .billing
                .codex_calibration_report()
                .await
                .unwrap()
                .iter()
                .map(|row| row.turns)
                .sum::<i64>(),
            2
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn retire_flushes_last_pending_event_after_writer_recovers() {
        let fleet = test_fleet(1, "http://127.0.0.1:1/codex");
        let home = fleet.gateway.homes().await.into_iter().next().unwrap();
        install_calibration_write_failure(&fleet.database);
        assert!(
            !home
                .record_calibration_event(calibration_event(
                    "retire-retry",
                    home.id(),
                    7_000_000_000,
                    700_000_000,
                    pool::now(),
                ))
                .await
        );
        assert_eq!(home.status().await.calibration_pending_events, 1);
        remove_calibration_write_failure(&fleet.database);

        home.retire().await.unwrap();

        let retired = home.status().await;
        assert_eq!(retired.calibration_pending_events, 0);
        assert!(retired.calibration_persistence_ok);
        let report = fleet.billing.codex_calibration_report().await.unwrap();
        assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 1);
        assert_eq!(report[0].api_total_nanousd, 7_000_000_000);
        assert_eq!(report[0].chatgpt_total_nanocredits, 700_000_000);
        fleet.close().await;
    }

    #[tokio::test]
    async fn exact_replay_is_idempotent_and_conflict_does_not_block_later_turns() {
        let fleet = test_fleet(1, "http://127.0.0.1:1/codex");
        let home = fleet.gateway.homes().await.into_iter().next().unwrap();
        let first = calibration_event(
            "immutable-turn",
            home.id(),
            11_000_000_000,
            1_100_000_000,
            100,
        );
        assert!(home.record_calibration_event(first.clone()).await);
        assert!(home.record_calibration_event(first.clone()).await);

        let mut conflict = first;
        conflict.api_input_nanousd = conflict.api_input_nanousd.saturating_add(1);
        conflict.api_total_nanousd = conflict.api_total_nanousd.saturating_add(1);
        assert!(!home.record_calibration_event(conflict).await);
        assert!(
            home.record_calibration_event(calibration_event(
                "after-conflict",
                home.id(),
                13_000_000_000,
                1_300_000_000,
                101,
            ))
            .await
        );

        let status = home.status().await;
        assert_eq!(status.calibration_pending_events, 0);
        assert_eq!(status.calibration_dropped_events, 1);
        assert!(!status.calibration_persistence_ok);
        assert_eq!(status.spend_nano_total, 24_000_000_000);
        assert_eq!(status.spend_nanocredits_total, Some(2_400_000_000));
        let report = fleet.billing.codex_calibration_report().await.unwrap();
        assert_eq!(report.iter().map(|row| row.turns).sum::<i64>(), 2);
        assert_eq!(
            report.iter().map(|row| row.api_total_nanousd).sum::<i64>(),
            24_000_000_000
        );
        fleet.close().await;
    }

    #[tokio::test]
    async fn failed_and_zero_usage_turns_never_create_calibration_evidence() {
        let failed_fleet = test_fleet(1, "http://127.0.0.1:1/codex");
        assert!(failed_fleet
            .gateway
            .run_turn(
                turn(failed_fleet.gateway.config().models[0].clone(), false, 0),
                None,
                None,
            )
            .await
            .is_err());
        assert!(failed_fleet
            .billing
            .codex_calibration_report()
            .await
            .unwrap()
            .is_empty());
        failed_fleet.close().await;

        let (base_url, _) = mock_turn_upstream_with_usage(true).await;
        let zero_fleet = test_fleet(1, &base_url);
        let result = zero_fleet
            .gateway
            .run_turn(
                turn(zero_fleet.gateway.config().models[0].clone(), false, 0),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(result.usage, CodexUsage::default());
        assert!(zero_fleet
            .billing
            .codex_calibration_report()
            .await
            .unwrap()
            .is_empty());
        let status = zero_fleet.gateway.operational_status().await;
        assert_eq!(status.homes[0].calibration_pending_events, 0);
        assert_eq!(status.homes[0].calibration_dropped_events, 1);
        assert!(!status.homes[0].calibration_persistence_ok);
        zero_fleet.close().await;
    }

    #[tokio::test]
    async fn resolved_affinity_stays_on_its_home_while_new_sessions_seed_every_home() {
        let (base_url, accounts) = mock_turn_upstream().await;
        let fleet = test_fleet(4, &base_url);
        let model = fleet.gateway.config().models[0].clone();
        let store = Arc::new(
            AffinityStore::new(None, Some("calibration-affinity-secret"), 60, 60, 10).unwrap(),
        );

        for sequence in 0..4 {
            let session = format!("new-session-{sequence}");
            let routing = turn_routing(store.clone(), &model, &session).await;
            fleet
                .gateway
                .run_turn(turn(model.clone(), false, sequence), None, Some(routing))
                .await
                .unwrap();
        }
        let seeded = accounts.lock().unwrap().clone();
        assert_eq!(seeded.len(), 4);
        assert_eq!(seeded.iter().collect::<HashSet<_>>().len(), 4);

        let routing = turn_routing(store, &model, "new-session-0").await;
        fleet
            .gateway
            .run_turn(turn(model, false, 4), None, Some(routing))
            .await
            .unwrap();
        let served = accounts.lock().unwrap().clone();
        assert_eq!(served.len(), 5);
        assert_eq!(served[4], served[0]);

        let report = fleet.billing.codex_calibration_report().await.unwrap();
        let home_turns = report.iter().fold(BTreeMap::new(), |mut turns, row| {
            *turns.entry(row.home_id.clone()).or_insert(0i64) += row.turns;
            turns
        });
        assert_eq!(home_turns.len(), 4);
        assert_eq!(home_turns.values().copied().sum::<i64>(), 5);
        assert_eq!(home_turns.values().filter(|turns| **turns == 2).count(), 1);
        assert_eq!(home_turns.values().filter(|turns| **turns == 1).count(), 3);
        fleet.close().await;
    }
}

/// Pure soft-reserve core (testable without a home): earliest reset among windows at or above
/// their jittered cap, `None` when every reported window is below its cap or no evidence exists.
fn reserve_blocked(
    limits: Option<&CodexRateLimits>,
    home_id: &str,
    reserve: &WindowReserve,
) -> Option<i64> {
    let limits = limits?;
    limits
        .primary
        .iter()
        .chain(limits.secondary.iter())
        .filter(|window| window.used_fraction() > reserve.window_cap(window, home_id))
        .filter_map(|window| window.resets_at)
        .min()
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

/// Owns the sealed profiles for every roster entry. The composition layer calls `preflight`
/// before exposing a configured provider, while later failures are recovered lazily. Existing
/// Claude routing is completely independent when Codex is disabled.
pub struct CodexGateway {
    cfg: Arc<CodexConfig>,
    /// Optional one-shot external transport. It is not a home and therefore owns no affinity,
    /// health, quota or calibration state.
    claudestore_fallback: Option<claudestore::ClaudeStoreCodexFallback>,
    metrics: Arc<Metrics>,
    calibration_store: Option<Arc<AsyncBilling>>,
    shutting_down: AtomicBool,
    abort_turns: AtomicBool,
    /// Rotates equal-load cold/warm candidates. Without an atomic cursor, sequential requests and
    /// concurrent bursts with identical snapshots all collapse onto the lowest discovery order.
    selection_cursor: AtomicU64,
    /// Samples one profile catalogue per refresh pass. This learns account-scoped rollout metadata
    /// across the fleet without multiplying the health cadence into one `/models` call per home.
    model_catalog_cursor: AtomicU64,
    abort_notify: Notify,
    /// Raised when a home's health asks to be re-checked ahead of the sweep cadence.
    ///
    /// This is the Codex counterpart of the Claude pool's `request_probe` + `probe_poke` pair: a bad
    /// outcome on the data path immediately queues a control-plane check instead of waiting a full
    /// interval for the background loop to notice.
    probe_poke: Notify,
    background_tasks: Arc<ActiveTaskTracker>,
    rediscover_lock: Mutex<()>,
    /// Rediscovered on every health tick, so an account the authbot finishes buying joins the pool
    /// without a restart. Readers take a snapshot; the lock is never held across a turn.
    homes: RwLock<Vec<Arc<CodexHome>>>,
    /// Last successful live model snapshot. The public OpenAI model-list route reads this cache and
    /// never waits on an upstream call; before the first successful refresh it falls back to the
    /// locally configured billing catalog.
    model_catalog: RwLock<Option<CodexModelCatalog>>,
    /// SIGUSR2 reconciliation and the periodic health loop may overlap. Keep live catalog refresh
    /// single-flight so a transient stall cannot multiply background requests.
    model_catalog_refresh: Mutex<()>,
    history: Arc<HistoryStore>,
}

fn aggregate_model_catalogs(
    catalogs: impl IntoIterator<Item = CodexModelCatalog>,
    all_profiles_observed: bool,
) -> CodexModelCatalog {
    let mut aggregate = CodexModelCatalog::default();
    let mut missing_limit = HashSet::new();
    let mut conflicting_display_name = HashSet::new();
    for catalog in catalogs {
        aggregate.fast_models.extend(catalog.fast_models);
        for (model, display_name) in catalog.display_names {
            if conflicting_display_name.contains(&model) {
                continue;
            }
            match aggregate.display_names.get(&model) {
                Some(current) if current != &display_name => {
                    aggregate.display_names.remove(&model);
                    conflicting_display_name.insert(model);
                }
                Some(_) => {}
                None => {
                    aggregate.display_names.insert(model, display_name);
                }
            }
        }
        for model in catalog.models {
            aggregate.models.insert(model.clone());
            if missing_limit.contains(&model) {
                continue;
            }
            let Some(limit) = catalog.input_token_limits.get(&model).copied() else {
                aggregate.input_token_limits.remove(&model);
                missing_limit.insert(model);
                continue;
            };
            aggregate
                .input_token_limits
                .entry(model)
                .and_modify(|current| *current = (*current).min(limit))
                .or_insert(limit);
        }
    }
    if !all_profiles_observed {
        aggregate.input_token_limits.clear();
    }
    aggregate
}

impl CodexGateway {
    pub fn new(cfg: CodexConfig) -> anyhow::Result<Self> {
        Self::new_with_calibration(cfg, None)
    }

    pub fn new_with_calibration(
        cfg: CodexConfig,
        calibration_store: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        Self::new_with_calibration_and_fallback(
            cfg,
            calibration_store,
            None,
            Arc::new(Metrics::new()),
        )
    }

    pub fn new_with_calibration_and_fallback(
        cfg: CodexConfig,
        calibration_store: Option<Arc<AsyncBilling>>,
        claudestore_fallback: Option<ClaudeStoreFallbackConfig>,
        metrics: Arc<Metrics>,
    ) -> anyhow::Result<Self> {
        let fallback_transport = claudestore_fallback
            .map(|fallback| {
                claudestore::ClaudeStoreCodexFallback::new(
                    fallback,
                    cfg.request_timeout_ms,
                    cfg.turn_timeout_ms,
                    cfg.turn_silence_timeout_ms,
                )
            })
            .transpose()
            .map_err(|error| anyhow::anyhow!(error))?;
        let history = HistoryStore::new(
            cfg.history_redis_url.as_deref(),
            cfg.history_secret.as_deref(),
            cfg.history_ttl_secs,
            cfg.history_local_cap,
            cfg.history_redis_timeout_ms,
        )?;
        let cfg = Arc::new(cfg);
        let specs = discovery::discover(&cfg);
        let mut homes = Vec::new();
        for (order, spec) in specs.into_iter().enumerate() {
            match CodexHome::load(cfg.clone(), spec.clone(), order, calibration_store.clone()) {
                Ok(home) => homes.push(Arc::new(home)),
                Err(error) => {
                    elog::error(
                        "codex",
                        format!(
                            "Codex profile {} could not be loaded [{}]",
                            spec.id,
                            error.diagnostic_class()
                        ),
                    );
                }
            }
        }
        if homes.is_empty() {
            anyhow::bail!("Codex provider found no sealed profile to serve from");
        }
        Ok(Self {
            cfg,
            claudestore_fallback: fallback_transport,
            metrics,
            calibration_store,
            shutting_down: AtomicBool::new(false),
            abort_turns: AtomicBool::new(false),
            selection_cursor: AtomicU64::new(0),
            model_catalog_cursor: AtomicU64::new(0),
            abort_notify: Notify::new(),
            probe_poke: Notify::new(),
            background_tasks: Arc::new(ActiveTaskTracker::default()),
            rediscover_lock: Mutex::new(()),
            homes: RwLock::new(homes),
            model_catalog: RwLock::new(None),
            model_catalog_refresh: Mutex::new(()),
            history: Arc::new(history),
        })
    }

    /// Snapshot of the current pool.
    async fn homes(&self) -> Vec<Arc<CodexHome>> {
        self.homes.read().await.clone()
    }

    /// Reconcile the pool with the roster on disk.
    ///
    /// An unchanged home keeps its credential and health state. Removed profiles are retired after
    /// active turns finish; republished envelopes are picked up by `reload_if_republished`.
    async fn rediscover(&self) {
        let _reconcile = self.rediscover_lock.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let specs = discovery::discover(&self.cfg);
        if specs.is_empty() {
            // Never empty the pool from a scan: a transient unreadable roster would otherwise take
            // the whole provider down while the previous homes were still serving.
            elog::warn("codex", "Codex rediscovery found no profiles; keeping the current pool");
            return;
        }
        let mut homes = self.homes.write().await;
        let mut next: Vec<Arc<CodexHome>> = Vec::with_capacity(specs.len());
        let mut retiring: Vec<Arc<CodexHome>> = Vec::new();
        let mut joining: Vec<(usize, CodexProfileSpec)> = Vec::new();
        for (order, spec) in specs.into_iter().enumerate() {
            match homes
                .iter()
                .find(|home| home.id() == spec.id && !home.retired.load(Ordering::Acquire))
            {
                Some(existing) => {
                    existing.order.store(order, Ordering::Relaxed);
                    next.push(existing.clone());
                }
                None => joining.push((order, spec)),
            }
        }
        for gone in homes.iter() {
            if !next.iter().any(|home| Arc::ptr_eq(home, gone)) {
                gone.retired.store(true, Ordering::Release);
                retiring.push(gone.clone());
                elog::warn("codex", format!("Codex home {} left the pool", gone.id()));
            }
        }
        *homes = next.clone();
        drop(homes);
        for home in retiring {
            if let Err(error) = home.retire().await {
                elog::error(
                    "codex",
                    format!("Codex home retirement failed [{}]", error.diagnostic_class()),
                );
            }
        }
        for home in &next {
            home.reload_if_republished().await;
        }
        for (order, spec) in joining {
            match CodexHome::load(
                self.cfg.clone(),
                spec.clone(),
                order,
                self.calibration_store.clone(),
            ) {
                Ok(home) => {
                    elog::info("codex", format!("Codex home {} joined the pool", spec.id));
                    let home = Arc::new(home);
                    // Recover the durable account verdict before this home can be selected, so a
                    // subscription already known to be dead or spent is not re-admitted by a
                    // restart and rediscovered with customer traffic.
                    home.hydrate_health().await;
                    next.push(home);
                }
                Err(error) => {
                    elog::error(
                        "codex",
                        format!(
                            "Codex profile {} could not join the pool [{}]",
                            spec.id,
                            error.diagnostic_class()
                        ),
                    );
                }
            }
        }
        next.sort_by_key(|home| home.order());
        *self.homes.write().await = next;
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

    pub(crate) async fn cached_model_catalog(&self) -> Option<CodexModelCatalog> {
        self.model_catalog.read().await.clone()
    }

    /// Refresh the best-effort live model snapshot outside the request path. A failed refresh keeps
    /// the last successful snapshot; an empty successful snapshot is retained intentionally because
    /// the provider may explicitly report that no models are currently available.
    pub async fn refresh_model_catalog(&self) {
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        let _refresh = self.model_catalog_refresh.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        match self.fetch_live_models().await {
            Ok(available) => {
                *self.model_catalog.write().await = Some(available);
            }
            Err(error) => {
                elog::warn(
                    "codex",
                    format!(
                        "Codex model catalog refresh failed [{}]",
                        error.diagnostic_class()
                    ),
                );
            }
        }
    }

    /// A model catalogue read through any usable home, for provider-level discovery.
    pub(crate) async fn fetch_live_models(&self) -> Result<CodexModelCatalog, ProcessError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        let now = pool::now();
        let mut last_error = ProcessError::Closed;
        let pool_homes = self.homes().await;
        let width = pool_homes.len().max(1);
        let cursor = self.model_catalog_cursor.fetch_add(1, Ordering::Relaxed) as usize % width;
        let mut ordered: Vec<_> = pool_homes.iter().collect();
        ordered.sort_by_key(|home| {
            (
                home.is_cooling(now),
                (home.order() + width - cursor) % width,
            )
        });
        for home in ordered {
            match home.fetch_models().await {
                Ok(catalog) => {
                    // The public list is the union of last-good account snapshots: a model rolled
                    // out to only part of the fleet remains usable, while selection keeps its
                    // per-profile capability evidence for Fast placement. Context metadata is a
                    // guarantee rather than a routing hint, so profiles which can serve the same
                    // model are merged at their smallest proved input ceiling. Missing metadata
                    // on any serving profile withdraws the guarantee instead of guessing it.
                    let mut catalogs = vec![catalog];
                    let mut all_profiles_observed = true;
                    for candidate in &pool_homes {
                        match candidate.cached_profile_catalog() {
                            Some(snapshot) => catalogs.push(snapshot),
                            None => all_profiles_observed = false,
                        }
                    }
                    return Ok(aggregate_model_catalogs(catalogs, all_profiles_observed));
                }
                Err(error) => {
                    home.note_transport_error(&error);
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    pub(crate) fn history(&self) -> &HistoryStore {
        &self.history
    }

    /// Shared response-history counters for `/metrics`. Unlike affinity, a lost entry here becomes
    /// a customer-visible 400, so these are exported rather than kept internal.
    pub fn history_stats(&self) -> history::HistoryStats {
        self.history.stats()
    }

    pub fn history_redis_configured(&self) -> bool {
        self.history.redis_configured()
    }

    /// Register one detached stream task in the shutdown barrier without a semaphore or wait.
    pub(crate) fn track_background_task(&self) -> Result<ActiveTaskGuard, ProcessError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ProcessError::Closed);
        }
        self.background_tasks.track().ok_or(ProcessError::Closed)
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
    /// any remainder.
    pub async fn shutdown(&self) {
        self.shutdown_until(None).await;
    }

    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.shutting_down.store(true, Ordering::Release);
        self.background_tasks.close();
        let cleanup_deadline = deadline.map(|deadline| deadline + FORCED_SHUTDOWN_CLEANUP_GRACE);
        match deadline {
            Some(deadline) => {
                if tokio::time::timeout_at(deadline, self.background_tasks.wait_idle())
                    .await
                    .is_err()
                {
                    self.abort_active_turns();
                    // A task can be stuck on downstream backpressure or in cancellation cleanup.
                    // Waiting again without a deadline recreated the ten-minute singleton outage
                    // this deadline exists to prevent.
                    elog::warn(
                        "codex",
                        "Codex forced shutdown: abandoning residual tracked tasks after deadline"
                    );
                }
            }
            None => self.background_tasks.wait_idle().await,
        }
        // Also cancel an untracked provider operation such as a health probe before retirement.
        self.abort_active_turns();
        let _reconcile = match cleanup_deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, self.rediscover_lock.lock())
                .await
            {
                Ok(guard) => guard,
                Err(_) => {
                    elog::warn("codex", "Codex forced shutdown: rediscovery lock exceeded cleanup deadline");
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
                        elog::warn(
                            "codex",
                            "Codex forced shutdown: home retirement exceeded the hard deadline"
                        );
                        break;
                    }
                },
                None => home.retire().await,
            };
            if let Err(error) = result {
                elog::error("codex", format!("Codex home shutdown failed [{}]", error.diagnostic_class()));
            }
        }
    }

    /// Choose a usable home and take one of its turn slots.
    ///
    /// A conversation-pinned home is first inside the best Fast-capability class. Other homes are
    /// ordered by current in-flight load and a rotating tie-break; calibration is reporting
    /// evidence and never an admission/routing restriction. New shared roots still seed two homes,
    /// but no capacity ratio can veto warmth.
    /// Collect routable candidates for one selection pass under a window-reserve policy.
    ///
    /// The reserve keeps each subscription below its jittered 5h/weekly cap: a home beyond its
    /// cap is treated as rejected until that window resets, so the fleet walks subscriptions off
    /// the hard provider wall instead of discovering it with customer traffic.
    async fn selection_candidates(
        &self,
        pool_homes: &[Arc<CodexHome>],
        exclude: &[String],
        now: i64,
        reserve: &WindowReserve,
        fast_model: Option<&str>,
        ignore_soft_cooling: bool,
    ) -> (Vec<(u8, bool, usize, u8, i64, Arc<CodexHome>)>, Option<i64>) {
        let mut candidates = Vec::with_capacity(pool_homes.len());
        let mut soonest: Option<i64> = None;
        for home in pool_homes {
            if home.retired.load(Ordering::Acquire) {
                continue;
            }
            if exclude.iter().any(|id| id == home.id()) {
                continue;
            }
            // One admission verdict for every reason a home can be unroutable: a dead account, a
            // wedged or degraded transport, cooling, or a fresh full window.
            let limits = home.rate_limits().await;
            let verdict = if ignore_soft_cooling {
                home.admission_ignoring_soft_cooling(limits.as_ref(), now)
            } else {
                home.admission(limits.as_ref(), now)
            };
            match verdict {
                health::Admission::Admit { snapshot_stale } => {
                    if let Some(blocked_until) =
                        home.reserve_blocked_until(limits.as_ref(), reserve)
                    {
                        soonest =
                            Some(soonest.map_or(blocked_until, |v: i64| v.min(blocked_until)));
                        continue;
                    }
                    // Remaining window, normalised per subscription by the provider itself. That
                    // normalisation is what makes this tier-aware for free: 40% of a small plan and
                    // 40% of a large one are equally close to their own wall, which is the thing
                    // selection must equalise. The USD calibration answers a different question —
                    // how much capacity is left in money — and stays empty until a window turns over.
                    let used =
                        quota_rank(limits.as_ref().and_then(|limits| limits.max_used_percent()));
                    let fast_rank = fast_model.map_or(0, |model| home.fast_route_rank(model));
                    candidates.push((
                        fast_rank,
                        snapshot_stale,
                        home.inflight(),
                        home.calibration_seed_rank(),
                        used,
                        home.clone(),
                    ));
                }
                health::Admission::Reject { ready_at, .. } => {
                    if let Some(ready_at) = ready_at {
                        soonest = Some(soonest.map_or(ready_at, |v: i64| v.min(ready_at)));
                    }
                }
            }
        }
        (candidates, soonest)
    }

    async fn select_home(
        &self,
        exclude: &[String],
        preferred: Option<&str>,
        warm: &[String],
        place_cache_root: bool,
        advance_cursor: bool,
        fast_model: Option<&str>,
    ) -> HomeSelection {
        if self.shutting_down.load(Ordering::Acquire) {
            return HomeSelection::Unavailable { ready_at: None };
        }
        let now = pool::now();
        let pool_homes = self.homes().await;
        let (mut candidates, mut soonest) = self
            .selection_candidates(
                &pool_homes,
                exclude,
                now,
                &self.cfg.window_reserve(),
                fast_model,
                false,
            )
            .await;
        if candidates.is_empty() && self.cfg.window_reserve() != WindowReserve::FULL {
            // Peak escape hatch, mirroring the Claude fleet: when every home is past its soft
            // reserve, serve at up to the provider's own wall rather than fail the client.
            let (relaxed, relaxed_soonest) = self
                .selection_candidates(
                    &pool_homes,
                    exclude,
                    now,
                    &WindowReserve::FULL,
                    fast_model,
                    false,
                )
                .await;
            candidates = relaxed;
            if candidates.is_empty() {
                soonest = relaxed_soonest.or(soonest);
            }
        }
        if candidates.is_empty() {
            // Second escape hatch, for the other kind of rest. The pass above relaxes how much
            // window we are willing to spend; this one relaxes cooling we inferred ourselves rather
            // than cooling the provider reported. A home the provider called dead or full is still
            // excluded, so a genuinely limited plane still answers 429 instead of burning a turn.
            let (unchilled, unchilled_soonest) = self
                .selection_candidates(
                    &pool_homes,
                    exclude,
                    now,
                    &WindowReserve::FULL,
                    fast_model,
                    true,
                )
                .await;
            candidates = unchilled;
            if candidates.is_empty() {
                soonest = unchilled_soonest.or(soonest);
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
            |(fast_a, stale_a, inflight_a, seed_a, used_a, home_a),
             (fast_b, stale_b, inflight_b, seed_b, used_b, home_b)| {
                let rotated_a = (home_a.order() + width - cursor) % width;
                let rotated_b = (home_b.order() + width - cursor) % width;
                // Fast capability is a service-quality boundary, not a small balancing hint.
                // Catalogue support outranks unknown or unsupported profiles before cache warmth,
                // load or quota. Within the same Fast class, ordinary fleet discipline remains.
                fast_a
                    .cmp(fast_b)
                    .then_with(|| stale_a.cmp(stale_b))
                    // A home whose quota evidence has gone stale is still routable — the snapshot is
                    // observational and must never become a hard dependency — but it must never win a
                    // tie against a home whose evidence is current. A frozen reading looks arbitrarily
                    // optimistic, and ranking on it is how one unresponsive home absorbed the whole pool.
                    //
                    // Below that, ordering mirrors the Claude pool's `select_best`: the load envelope is
                    // a full tier of its own, and only within it does remaining window decide. Ranking on
                    // capacity first would pile concurrent turns onto whichever subscription is emptiest.
                    // Quota steering is bucketed and only engages near the wall, so comparable homes stay
                    // tied and the rotation cursor keeps spreading them.
                    .then_with(|| inflight_a.cmp(inflight_b))
                    // A new conversation seeds exact evidence on every home before revisiting an
                    // already-observed peer. Never apply this to resolved affinity: conversation
                    // continuity and cache ownership remain stronger than calibration coverage.
                    .then_with(|| {
                        if preferred.is_none() {
                            seed_a.cmp(seed_b)
                        } else {
                            std::cmp::Ordering::Equal
                        }
                    })
                    .then_with(|| used_a.cmp(used_b))
                    .then_with(|| rotated_a.cmp(&rotated_b))
            },
        );
        let best_fast_rank = candidates[0].0;
        // A resolved conversation is a hard first choice. For a new shared root, seed a cold home
        // until two copies exist, then prefer the least-loaded warm home. Fast requests keep that
        // affinity only inside the best currently known Fast-capability class.
        let mut ordered: Vec<Arc<CodexHome>> = Vec::with_capacity(candidates.len());
        if let Some(id) = preferred {
            if let Some((_, _, _, _, _, home)) = candidates
                .iter()
                .find(|(fast, _, _, _, _, home)| *fast == best_fast_rank && home.id() == id)
            {
                ordered.push(home.clone());
            }
        } else if place_cache_root {
            let warm_count = candidates
                .iter()
                .filter(|(fast, _, _, _, _, home)| {
                    *fast == best_fast_rank && warm.iter().any(|id| id == home.id())
                })
                .count();
            let primary = if warm_count < CACHE_ROOT_MIN_WARM_HOMES {
                candidates.iter().position(|(fast, _, _, _, _, home)| {
                    *fast == best_fast_rank && !warm.iter().any(|id| id == home.id())
                })
            } else {
                candidates.iter().position(|(fast, _, _, _, _, home)| {
                    *fast == best_fast_rank && warm.iter().any(|id| id == home.id())
                })
            }
            .unwrap_or(0);
            ordered.push(candidates[primary].5.clone());
        }
        // If the primary choice is currently sampling, another warm copy is the next best spill;
        // remaining candidates stay in global capacity order.
        if place_cache_root {
            for (fast, _, _, _, _, home) in &candidates {
                if *fast == best_fast_rank
                    && warm.iter().any(|id| id == home.id())
                    && !ordered.iter().any(|chosen| Arc::ptr_eq(chosen, home))
                {
                    ordered.push(home.clone());
                }
            }
        }
        for (_, _, _, _, _, home) in candidates {
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
    pub(crate) async fn preflight_capacity(&self, model: &CodexModel) -> Result<(), ProcessError> {
        match self.select_home(&[], None, &[], false, false, None).await {
            HomeSelection::Ready(_home, _slot) => Ok(()),
            HomeSelection::Unavailable { ready_at: _ }
                if self
                    .claudestore_fallback
                    .as_ref()
                    .is_some_and(|fallback| fallback.supports_model(&model.id)) =>
            {
                Ok(())
            }
            HomeSelection::Unavailable { ready_at } => Err(ProcessError::UsageLimitExceeded {
                retry_after: ready_at
                    .map(|at| at.saturating_sub(pool::now()).clamp(1, 7 * 24 * 3600) as u64),
            }),
        }
    }

    /// Open every sealed credential, prove the token refreshes or is valid, and read one usage
    /// snapshot per profile.
    ///
    /// Every transport keeps a one-home service floor: a single working subscription is still
    /// customer capacity. A home that fails here starts quarantined and is reported by the health
    /// metrics instead of blocking the provider. Returning only a diagnostic class keeps account
    /// metadata, paths and upstream messages out of composition logs.
    pub async fn preflight(&self) -> anyhow::Result<()> {
        let pool_homes = self.homes().await;
        // The initial pool is constructed synchronously, and rediscovery deliberately keeps
        // existing instances, so startup is the only place these homes can recover their durable
        // verdict. Without it a restart would silently re-admit an already-condemned subscription.
        for home in &pool_homes {
            home.hydrate_health().await;
        }
        // Bounded concurrency: a large fleet must not turn startup into an upstream burst.
        let outcomes: Vec<(Arc<CodexHome>, Result<(), ProcessError>)> =
            futures_util::stream::iter(pool_homes.iter().cloned().map(|home| async move {
                let outcome = home.probe().await;
                (home, outcome)
            }))
            .buffer_unordered(PREFLIGHT_CONCURRENCY)
            .collect()
            .await;
        let mut healthy = 0usize;
        let mut last_class = "closed";
        for (home, outcome) in outcomes {
            match outcome {
                Ok(()) => {
                    home.mark_healthy();
                    healthy += 1;
                }
                Err(error) => {
                    last_class = error.diagnostic_class();
                    home.note_transport_error(&error);
                    elog::error("codex", format!("Codex home {} failed preflight [{}]", home.id(), last_class));
                }
            }
        }
        if healthy == 0 {
            anyhow::bail!(
                "Codex preflight admitted 0/{} homes, but at least one working subscription is required [{last_class}]",
                pool_homes.len()
            );
        }
        if healthy < pool_homes.len() {
            elog::info(
                "codex",
                format!("Codex provider starting with {healthy}/{} authenticated homes", pool_homes.len()),
            );
        }
        Ok(())
    }

    /// Read-only health sweep over every home, run by the composition layer's background loop.
    ///
    /// A login expires with no traffic on it, so a home that is never selected would otherwise stay
    /// silently dead until the pool needed it. Quarantined homes are probed too, so a
    /// re-authenticated profile returns to service without a restart.
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
        let homes = self.homes().await;
        // Exact event retry is local authority work, not an upstream probe. Run it for every home,
        // including busy/healthy ones that the network sweep correctly skips. If the FIFO drains,
        // the home replays its cached post-turn quota only after the durable spend ledger catches up.
        futures_util::stream::iter(homes.iter().cloned().map(|home| async move {
            home.retry_pending_calibration().await;
        }))
        .buffer_unordered(SWEEP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        // Probe only the homes that are due. Live traffic keeps busy homes' snapshots fresh, so
        // sweeping them again on every tick would multiply upstream calls without learning
        // anything: at fleet scale the sweep itself becomes the load problem and a ban signal.
        // Healthy homes with a fresh snapshot are skipped; anything stale, unprobed, flagged, or
        // already suspicious is probed every tick. Concurrency is bounded so one sick egress
        // cannot stretch the sweep, and so the fleet never fans out an upstream burst.
        let now = pool::now();
        let due: Vec<Arc<CodexHome>> = homes
            .into_iter()
            .filter(|home| home.probe_due(now))
            .collect();
        futures_util::stream::iter(due.into_iter().map(|home| async move {
            match home.probe().await {
                Ok(()) => home.mark_healthy(),
                Err(error) => {
                    home.note_transport_error(&error);
                    elog::error(
                        "codex",
                        format!(
                            "Codex home {} probe failed [{}]",
                            home.id(),
                            error.diagnostic_class(),
                        ),
                    );
                    if home.health().needs_recycle() {
                        home.recycle_transport();
                    }
                }
            }
        }))
        .buffer_unordered(SWEEP_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;
        // Model discovery is intentionally best-effort and outside customer request handling. A
        // stale last-good snapshot (or the configured catalog before the first success) is safer
        // than turning a catalog hiccup into a public 503 on every SDK startup.
        self.refresh_model_catalog().await;
    }

    /// Read only cached operational state. Metrics collection never starts a network request.
    pub async fn operational_status(&self) -> CodexOperationalStatus {
        let now = pool::now();
        let pool_homes = self.homes().await;
        let mut homes = Vec::with_capacity(pool_homes.len());
        let mut available = 0usize;
        let mut soonest: Option<i64> = None;
        for home in &pool_homes {
            let status = home.status().await;
            // `available` means exactly "selection would route here", because it is computed by the
            // same predicate selection uses.
            if status.admitted {
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
            process_live: !homes.is_empty(),
            rate_limits,
            available,
            soonest_ready: soonest,
            homes,
        }
    }
}
