//! Live Suno (suno.com) generation-lifecycle gateway.
//!
//! Suno is a task-based media API on a session-credential pool, not a chat protocol
//! (manifest §4), so this gateway owns the whole upstream generation lifecycle: reserve →
//! select profile → hCaptcha pre-check (never solved: `required: true` soft-cools and rotates)
//! → attribution baseline → create → detached poll → immediate artifact download into our own
//! storage → settlement (the attributed post-turn credit delta when unambiguous, else the
//! documented conservative reserve) → immutable turn event through the bounded FIFO → paired
//! post-turn quota observation. The money boundary is a successful upstream creation: rotation
//! across profiles is legal only before it; after it the generation belongs to the creating
//! profile's account and a client disconnect never cancels the drain.
//!
//! Calibration follows the Codex/Gemini pairing discipline (immutable per-turn evidence plus a
//! quota read taken in the turn's wake, persisted by one writer command whose cumulative
//! ledgers already include that turn), NOT the split KIMI/GLM ordering. The free periodic
//! quota poll still runs the turn-before-quota gate: it never reads with a pending FIFO head.
//!
//! Session mechanics (manifest §2): JWTs are minted on demand through the profile's pinned
//! egress under the per-profile single-flight (`session.rs`); `set-cookie` rotations are merged
//! and re-sealed before the flight releases. Every wire fact is `oss-hypothesis` and fails
//! closed.
//!
//! Contract: `docs/engine/SUNO_PROVIDER.md` §4/§5 and `docs/engine/PROVIDER_ONBOARDING.md`
//! §8/§9/§10. The plane is backend-only and off by default (manifest §0).

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::anyhow;
use axum::body::Body;
use axum::http::StatusCode;
use axum::response::Response;
use bytes::Bytes;
use metering::suno::{suno_cost_nanodollars, suno_paid_model, SUNO_TARIFF_SCHEDULE_ID};
use registry::{ExecutionAttempt, SunoTurnCalibrationEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::billing::AsyncBilling;
use crate::proxy::HoldGuard;
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};

use super::artifacts::{artifact_path, store_artifact, store_payload};
use super::client::{self, BillingProbe, BillingSnapshot, ClipLifecycle, ClipState};
use super::config::{readiness, NotReady, SunoPlaneConfig};
use super::pool::{decide, AttemptPolicy, NextStep, Phase, ProfileEffect};
use super::queue::{DeliveryHealth, PendingTurn, TurnQueue, WriteOutcome, DEFAULT_QUEUE_CAPACITY};
use super::roster::{load_roster, load_roster_for_reload, SunoProfile};
use super::selection::{select, select_ignoring_soft, Candidate, Hard, Soft};
use super::session::SessionManager;
use super::transport::{classify_status, parse_retry_after, SunoHosts, UpstreamVerdict};
use super::upload::{store_upload, upload_path, UPLOAD_MAX_BYTES};

const ERROR_BODY_LIMIT: usize = 64 * 1024;
/// Create/poll/billing JSON answers are small; anything larger is a contract anomaly.
const RESPONSE_BODY_LIMIT: usize = 1024 * 1024;
/// The poll cadence is undocumented (`oss-hypothesis` only); 3 s inside a bounded deadline is
/// our conservative choice, not a provider contract.
const POLL_INTERVAL: Duration = Duration::from_secs(3);
/// A generation's total drain is bounded: on expiry the reservation stays with the reconciler
/// under its lease rather than polling forever.
const POLL_DEADLINE: Duration = Duration::from_secs(1_800);
/// The reservation lease covers the whole bounded drain with headroom.
const RESERVATION_LEASE_SECS: i64 = 3_600;
const TRANSPORT_COOL_SECS: i64 = 15;
/// Soft auth axis: exponential backoff from a small base, reset on proven success
/// (PROVIDER_ONBOARDING §8.4). Flat, long cooling is what turns one bad wave into an outage.
const AUTH_SOFT_BASE_SECS: i64 = 15;
const AUTH_SOFT_MAX_SECS: i64 = 900;
/// A 429 wall with no parseable `Retry-After` (none is documented for this provider): bounded
/// guess the next quota probe replaces.
const RATE_LIMIT_FALLBACK_COOL_SECS: i64 = 30;
/// A CAPTCHA-required pre-check cools the profile briefly and rotates; a persistent gate is an
/// operational state (manifest §4), not a customer error.
const CAPTCHA_COOL_SECS: i64 = 30;
/// Removal from routable needs an unambiguous provider verdict: a persistent Clerk auth failure
/// corroborated by streak AND elapsed time (§8.4 discipline; a single refusal never removes).
const AUTH_DEAD_STREAK: u32 = 8;
const AUTH_DEAD_ELAPSED_SECS: i64 = 3_600;
/// The tracked-generation projection is a read model; money never depends on it. The bound
/// evicts the oldest FINALIZED record first (live generations are never evicted while
/// draining).
const GENERATION_TRACK_CAPACITY: usize = 65_536;

/// Customer money context already authenticated by the plane's shared authorization.
#[derive(Clone)]
pub(crate) struct SunoBillingInput {
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub available_nano: i64,
}

/// Customer-facing read model of one generation record. Only our own bounded fields; the
/// upstream clip ids, profile id and upstream URLs never appear (the upstream id is audit
/// metadata in the durable calibration event, not a serving identity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoGenerationView {
    /// Our internal request id — the public generation identity of this plane.
    pub generation_id: String,
    pub operation: String,
    /// `created` until the first poll answer, then the provider lifecycle (`queued`,
    /// `streaming`, `complete`, `error`), `expired` on a poll deadline.
    pub status: String,
    /// Artifact names fetchable through `GET /v1/audio/generations/{id}/artifact/{name}`.
    pub artifacts: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Bounded terminal class, never provider text.
    pub error: Option<&'static str>,
}

/// Per-profile operational projection for readiness, metrics and the admin endpoint.
///
/// Privacy by construction: the opaque roster id and the plan are the only identities this
/// struct can carry. The subject, cookie, session id, proxy, credential paths and raw provider
/// errors never enter it — `RuntimeProfile.subject_id` stays private to the gateway. The
/// bonus-drip state is deliberately not modelled at all (manifest §1: not saleable capacity),
/// so it cannot leak here either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoProfileStatus {
    /// Opaque roster id. Documented safe for logs, metrics and admin projections.
    pub id: String,
    /// Declared paid plan (`Pro`/`Premier`), corroborated at intake.
    pub plan: String,
    /// In the serving generation and not removed by a corroborated Clerk verdict.
    pub routable: bool,
    /// Authenticated and serving (a billing probe passed on this runtime generation).
    pub live: bool,
    /// Cooling axes as unix seconds; `None` means the axis is not cooling right now.
    pub rate_limit_cool_until: Option<i64>,
    /// HARD quota verdict: resting until a billing probe shows credits again.
    pub quota_walled: bool,
    /// SOFT axes with their deadlines.
    pub auth_cool_until: Option<i64>,
    pub captcha_cool_until: Option<i64>,
    pub transport_cool_until: Option<i64>,
    pub inflight: u32,
    /// Latest quota evidence: raw counters verbatim; unknown stays `None`, never `0`
    /// (manifest §5.2).
    pub quota_observed_at: Option<i64>,
    pub total_credits_left: Option<i64>,
    pub monthly_limit: Option<i64>,
    pub monthly_usage: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoOperationalStatus {
    pub total_profiles: usize,
    pub live_profiles: usize,
    /// Eligible right now under the strict pass (no hard and no soft axis active).
    pub available_profiles: usize,
    pub rate_limited_profiles: usize,
    pub quota_walled_profiles: usize,
    pub auth_cooling_profiles: usize,
    pub captcha_cooling_profiles: usize,
    pub transport_cooling_profiles: usize,
    pub inflight_requests: u64,
    /// Detached drains currently polling/downloading/settling.
    pub inflight_drains: u64,
    pub tracked_generations: usize,
    /// Finalized generations whose credit movement could not be attributed unambiguously
    /// (concurrent traffic on the same profile, or an unreadable post-turn snapshot). Each
    /// settled at the documented conservative reserve; the movement is recorded unattributed.
    pub unattributed_settlements: u64,
    /// Attributed deltas above the admitted reserve bound — a typed anomaly (quarantine),
    /// never silent acceptance.
    pub tariff_anomaly: u64,
    /// Artifact downloads that failed before the terminal record published.
    pub artifact_failures: u64,
    /// Wall time of the last completed per-profile quota sweep, milliseconds.
    pub quota_sweep_ms: u64,
    pub profiles: Vec<SunoProfileStatus>,
    pub delivery: DeliveryHealth,
}

/// A cooling deadline is only meaningful while it is still in the future; an expired or
/// never-set axis is "not cooling", not a timestamp in the past.
fn active_cooling(until: i64, now: i64) -> Option<i64> {
    (until > now).then_some(until)
}

#[derive(Default)]
struct ProfileHealth {
    authenticated: bool,
    /// HARD axes (provider verdicts only).
    rate_limit_cool_until: i64,
    quota_walled: bool,
    /// SOFT axes (our own inferences; never deny admission on their own).
    auth_cool_until: i64,
    auth_soft_streak: u32,
    auth_first_fault_at: Option<i64>,
    /// Corroborated persistent Clerk failure: removed from routable until a probe succeeds.
    auth_dead: bool,
    captcha_cool_until: i64,
    transport_cool_until: i64,
    /// Latest quota evidence; the raw counters stay `None` while unread/unproven.
    total_credits_left: Option<i64>,
    monthly_limit: Option<i64>,
    monthly_usage: Option<i64>,
    period_raw: Option<String>,
    quota_observed_at: Option<i64>,
}

struct RuntimeProfile {
    id: String,
    subject_id: String,
    /// Declared paid plan — the calibration cohort key and the published window limit source.
    plan: suno_credential::SunoPlan,
    hosts: SunoHosts,
    client: wreq::Client,
    session: SessionManager,
    health: Mutex<ProfileHealth>,
    inflight: AtomicU32,
    /// Monotonic start boundary for attribution and quota polls. If any customer lease starts
    /// while a quota GET is in flight, that snapshot is discarded even when the turn finishes
    /// before it returns; a turn whose window saw another lease start settles unattributed.
    turn_epoch: AtomicU64,
}

impl RuntimeProfile {
    fn from_roster(profile: SunoProfile, config: &SunoPlaneConfig) -> anyhow::Result<Arc<Self>> {
        Self::from_roster_on(profile, config, SunoHosts::official())
    }

    /// Crate-internal constructor with an explicit host pair: production passes
    /// [`SunoHosts::official`] only; the gateway tests inject a loopback mock. Not an operator
    /// knob — no env or config path reaches it.
    pub(crate) fn from_roster_on(
        profile: SunoProfile,
        config: &SunoPlaneConfig,
        hosts: SunoHosts,
    ) -> anyhow::Result<Arc<Self>> {
        let client = client::build_client(
            &profile.credential.proxy_url,
            Duration::from_secs(10),
            config.transport.request_timeout,
        )?;
        let session = SessionManager::new(
            &profile.id,
            &profile.credential_key_id,
            &config.roster_dir,
            config.keyring.clone(),
            profile.credential,
        );
        Ok(Arc::new(Self {
            id: profile.id,
            subject_id: profile.subject_id,
            plan: profile.plan,
            hosts,
            client,
            session,
            health: Mutex::new(ProfileHealth::default()),
            inflight: AtomicU32::new(0),
            turn_epoch: AtomicU64::new(0),
        }))
    }

    /// Latest PROVEN remaining credits: `total_credits_left` preferred, else
    /// `monthly_limit − monthly_usage`. Both halves unknown → `None` (inert, never zero).
    fn proven_remaining_credits(health: &ProfileHealth) -> Option<i64> {
        if let Some(left) = health.total_credits_left {
            return Some(left);
        }
        match (health.monthly_limit, health.monthly_usage) {
            (Some(limit), Some(used)) => Some(limit.saturating_sub(used)),
            _ => None,
        }
    }

    fn candidate(&self, now: i64, reserve_credits: i64) -> Option<Candidate> {
        let health = self.health.lock().expect("Suno profile health lock");
        if health.auth_dead {
            // Removed from routable on the corroborated Clerk verdict; a passing probe
            // re-admits (publish_quota clears it).
            return None;
        }
        // The axes stay deliberately separate (PROVIDER_ONBOARDING §8.4): hard provider
        // verdicts, soft inferences and proven quota shortfall clear independently.
        let hard = if health.rate_limit_cool_until > now {
            Some(Hard::RateLimited)
        } else if health.quota_walled {
            Some(Hard::QuotaExhausted)
        } else if Self::proven_remaining_credits(&health)
            .is_some_and(|remaining| remaining < reserve_credits)
        {
            Some(Hard::QuotaShortfall)
        } else {
            None
        };
        let soft = if health.auth_cool_until > now {
            Some(Soft::AuthCooling)
        } else if health.captcha_cool_until > now {
            Some(Soft::CaptchaRequired)
        } else if health.transport_cool_until > now {
            Some(Soft::TransportWedged)
        } else {
            None
        };
        Some(Candidate {
            profile_id: self.id.clone(),
            hard,
            soft,
            inflight: self.inflight.load(Ordering::Acquire),
        })
    }

    fn apply_effect(&self, effect: ProfileEffect, now: i64, retry_after: Option<i64>) {
        let mut health = self.health.lock().expect("Suno profile health lock");
        match effect {
            ProfileEffect::None => {}
            ProfileEffect::CoolRateLimited => {
                // The exact `Retry-After` the provider named, else a bounded guess the next
                // probe replaces.
                health.rate_limit_cool_until = retry_after
                    .filter(|until| *until > now)
                    .unwrap_or_else(|| now.saturating_add(RATE_LIMIT_FALLBACK_COOL_SECS));
            }
            ProfileEffect::RestForQuota => {
                // A money verdict: no timer clears it, only a billing probe showing credits.
                health.quota_walled = true;
            }
            ProfileEffect::SoftAuthFault => {
                health.auth_soft_streak = health.auth_soft_streak.saturating_add(1);
                if health.auth_first_fault_at.is_none() {
                    health.auth_first_fault_at = Some(now);
                }
                // Removal from routable only on the corroborated verdict: a persistent Clerk
                // failure proven by streak AND elapsed time, never one refusal.
                if health.auth_soft_streak >= AUTH_DEAD_STREAK
                    && now.saturating_sub(health.auth_first_fault_at.unwrap_or(now))
                        >= AUTH_DEAD_ELAPSED_SECS
                {
                    health.auth_dead = true;
                }
                let shift = health.auth_soft_streak.min(6);
                let backoff = AUTH_SOFT_BASE_SECS
                    .saturating_mul(1i64 << shift)
                    .min(AUTH_SOFT_MAX_SECS);
                health.auth_cool_until = now.saturating_add(backoff);
            }
            ProfileEffect::SoftCaptchaGate => {
                health.captcha_cool_until = now.saturating_add(CAPTCHA_COOL_SECS);
            }
            ProfileEffect::TransportFault => {
                health.transport_cool_until = now.saturating_add(TRANSPORT_COOL_SECS);
            }
        }
    }

    /// A successful creation is proven success: it rehabilitates the soft axes (including a
    /// corroborated Clerk verdict). The hard axes deliberately survive — only their own
    /// evidence clears them.
    fn mark_generation_success(&self) {
        let mut health = self.health.lock().expect("Suno profile health lock");
        health.authenticated = true;
        health.auth_cool_until = 0;
        health.auth_soft_streak = 0;
        health.auth_first_fault_at = None;
        health.auth_dead = false;
        health.captcha_cool_until = 0;
        health.transport_cool_until = 0;
    }

    /// A passing free billing probe is auth/quota evidence. It clears the soft axes and the
    /// quota wall when the reading shows credits; the rate-limit axis clears only on its own
    /// clock (the wall was time-boxed by the provider, not by quota).
    fn publish_quota(&self, snapshot: &BillingSnapshot, observed_at: i64) {
        let mut health = self.health.lock().expect("Suno profile health lock");
        health.authenticated = true;
        health.auth_cool_until = 0;
        health.auth_soft_streak = 0;
        health.auth_first_fault_at = None;
        health.auth_dead = false;
        health.captcha_cool_until = 0;
        health.transport_cool_until = 0;
        health.total_credits_left = snapshot.total_credits_left;
        health.monthly_limit = snapshot.monthly_limit;
        health.monthly_usage = snapshot.monthly_usage;
        health.period_raw = snapshot.period_raw.clone();
        health.quota_observed_at = Some(observed_at);
        // Explicit quota exhaustion visible via the billing probe zeroing is the hard verdict
        // (§4.1); a probe showing credits is the only thing that clears it.
        if Self::proven_remaining_credits(&health).is_some_and(|remaining| remaining <= 0) {
            health.quota_walled = true;
        } else if Self::proven_remaining_credits(&health).is_some() {
            health.quota_walled = false;
        }
    }

    fn authenticated(&self) -> bool {
        self.health
            .lock()
            .expect("Suno profile health lock")
            .authenticated
    }

    fn matches_roster(&self, profile: &SunoProfile) -> bool {
        if self.id != profile.id
            || self.subject_id != profile.subject_id
            || self.plan != profile.plan
            || self.session.credential_key_id() != profile.credential_key_id
        {
            return false;
        }
        let (cookie, session_id) = self.session.material();
        cookie == profile.credential.cookie
            && session_id == profile.credential.session_id.as_deref().unwrap_or("")
            && self.session.proxy_url() == profile.credential.proxy_url
    }
}

struct ProfileLease {
    profile: Arc<RuntimeProfile>,
}

impl ProfileLease {
    fn new(profile: Arc<RuntimeProfile>) -> Self {
        // Publish the live lease before its epoch. Pollers and attribution read in the opposite
        // order, so every interleaving observes either in-flight work or an epoch change, never
        // a false idle gap.
        profile.inflight.fetch_add(1, Ordering::SeqCst);
        profile.turn_epoch.fetch_add(1, Ordering::SeqCst);
        Self { profile }
    }
}

impl Drop for ProfileLease {
    fn drop(&mut self) {
        self.profile.inflight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// One reservation: the conservative hold pinned at admission, settled at generation end —
/// exactly from the attributed credit delta when unambiguous, at the reserve otherwise.
struct Reservation {
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
}

/// The pre-create quota read that makes the post-turn delta attributable. Valid only while no
/// other lease overlaps the window: recorded with the profile's turn epoch at read time.
struct AttributionBaseline {
    snapshot: BillingSnapshot,
    epoch: u64,
}

/// What the detached drain needs to finish the generation without the client.
struct DrainContext {
    request_id: String,
    kind: OperationKind,
    requested_model: Option<String>,
    reserve_credits: i64,
    priced_ts: i64,
    profile: Arc<RuntimeProfile>,
    reservation: Option<Reservation>,
    baseline: Option<AttributionBaseline>,
}

/// The tracked read model of one admitted generation. Money never lives here — the
/// reservation and the calibration FIFO are the money state; this is only what
/// `GET /v1/audio/generations/*` serves.
#[derive(Clone)]
struct GenerationRecord {
    kind: OperationKind,
    /// The creating account; `None` for an admin (unmetered) generation. Read isolation keys
    /// on it.
    account_id: Option<String>,
    status: &'static str,
    artifacts: Vec<String>,
    created_at: i64,
    updated_at: i64,
    error: Option<&'static str>,
    finalized: bool,
}

/// Default-off live Suno pool. Dedicated plane only (`ProviderMode::Suno`); it has no public
/// catalogue and never rides the Anthropic Messages surface.
pub struct SunoGateway {
    config: Arc<SunoPlaneConfig>,
    profiles: RwLock<Vec<Arc<RuntimeProfile>>>,
    cursor: AtomicU64,
    reload_lock: AsyncMutex<()>,
    billing: Option<Arc<AsyncBilling>>,
    turn_queue: Mutex<TurnQueue>,
    turn_drain: AsyncMutex<()>,
    quota_sweep: AsyncMutex<()>,
    maintenance_abort: Notify,
    background: Arc<ActiveTaskTracker>,
    shutting_down: AtomicBool,
    abort_drains: AtomicBool,
    abort_notify: Notify,
    live_profiles: AtomicUsize,
    generations: Mutex<HashMap<String, GenerationRecord>>,
    unattributed_settlements: AtomicU64,
    tariff_anomaly: AtomicU64,
    artifact_failures: AtomicU64,
    quota_sweep_ms: AtomicU64,
}

impl SunoGateway {
    pub fn new_with_calibration(
        config: SunoPlaneConfig,
        billing: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Arc<Self>> {
        let roster = load_roster(&config.roster_dir, &config.keyring)?;
        let mut profiles = Vec::with_capacity(roster.len());
        for profile in roster {
            profiles.push(RuntimeProfile::from_roster(profile, &config)?);
        }
        Ok(Arc::new(Self::from_profiles(config, billing, profiles)))
    }

    /// Keep the plane fail-closed when the initial roster cannot be opened: a degraded gateway
    /// has zero capacity until a later last-good roster reload checkpoint can recover it.
    pub fn new_degraded(config: SunoPlaneConfig, billing: Option<Arc<AsyncBilling>>) -> Self {
        Self::from_profiles(config, billing, Vec::new())
    }

    fn from_profiles(
        config: SunoPlaneConfig,
        billing: Option<Arc<AsyncBilling>>,
        profiles: Vec<Arc<RuntimeProfile>>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            profiles: RwLock::new(profiles),
            cursor: AtomicU64::new(0),
            reload_lock: AsyncMutex::new(()),
            billing,
            turn_queue: Mutex::new(TurnQueue::new(DEFAULT_QUEUE_CAPACITY)),
            turn_drain: AsyncMutex::new(()),
            quota_sweep: AsyncMutex::new(()),
            maintenance_abort: Notify::new(),
            background: Arc::new(ActiveTaskTracker::default()),
            shutting_down: AtomicBool::new(false),
            abort_drains: AtomicBool::new(false),
            abort_notify: Notify::new(),
            live_profiles: AtomicUsize::new(0),
            generations: Mutex::new(HashMap::new()),
            unattributed_settlements: AtomicU64::new(0),
            tariff_anomaly: AtomicU64::new(0),
            artifact_failures: AtomicU64::new(0),
            quota_sweep_ms: AtomicU64::new(0),
        }
    }

    fn profiles_snapshot(&self) -> Vec<Arc<RuntimeProfile>> {
        self.profiles.read().expect("Suno profiles lock").clone()
    }

    /// Atomically adopt one fully validated roster generation and retain the last-good snapshot
    /// on every read, decrypt, client-build or quota-probe failure.
    ///
    /// Unchanged profiles keep their exact `Arc`, preserving health, session state, in-flight
    /// accounting and HTTP state. Changed/new profiles authenticate through the free billing
    /// probe before publication. A final roster re-read prevents a snapshot that went stale
    /// during the probe from replacing a credential the Auth Bot (or a session re-seal)
    /// republished meanwhile. A removed profile closes to new admission immediately; its
    /// in-flight generations drain on their own `Arc`.
    pub async fn refresh_profiles(&self) -> bool {
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        let _reload = self.reload_lock.lock().await;
        match self.reload_profiles().await {
            Ok(changed) => changed,
            Err(_) => {
                // Do not render the error: malformed proxy URLs and credential envelopes may
                // contain private egress or session material.
                elog::warn(
                    "suno",
                    "Suno encrypted roster refresh skipped; last-good capacity retained",
                );
                false
            }
        }
    }

    async fn reload_profiles(&self) -> anyhow::Result<bool> {
        const MAX_SNAPSHOT_RACES: usize = 3;
        for _ in 0..MAX_SNAPSHOT_RACES {
            let current = self.profiles_snapshot();
            let loaded = self.load_reload_snapshot(!current.is_empty()).await?;
            let mut next = Vec::with_capacity(loaded.len());
            let mut needs_probe = Vec::new();
            for loaded_profile in loaded {
                let mut reused = None;
                for existing in &current {
                    if existing.matches_roster(&loaded_profile) {
                        reused = Some(existing.clone());
                        break;
                    }
                }
                match reused {
                    Some(existing) => next.push(existing),
                    None => {
                        let profile = RuntimeProfile::from_roster(loaded_profile, &self.config)?;
                        needs_probe.push(profile.clone());
                        next.push(profile);
                    }
                }
            }

            if same_profile_generation(&current, &next) {
                return Ok(false);
            }

            // Every new or changed credential passes the free billing probe BEFORE it joins the
            // serving generation. A session the provider rejects (401/403) must not start
            // carrying traffic.
            for profile in needs_probe {
                self.probe_profile(&profile).await.map_err(|error| {
                    anyhow!("Suno reload billing-probe class={}", error.class())
                })?;
            }

            let verified = self.load_reload_snapshot(!current.is_empty()).await?;
            if !profiles_match_roster(&next, &verified) {
                continue;
            }
            if self.shutting_down.load(Ordering::Acquire) {
                return Ok(false);
            }
            let live = next
                .iter()
                .filter(|profile| profile.authenticated())
                .count();
            *self.profiles.write().expect("Suno profiles lock") = next;
            self.live_profiles.store(live, Ordering::Release);
            return Ok(true);
        }
        anyhow::bail!("Suno roster changed repeatedly during reload")
    }

    async fn load_reload_snapshot(
        &self,
        has_last_good_capacity: bool,
    ) -> anyhow::Result<Vec<SunoProfile>> {
        let root = self.config.roster_dir.clone();
        let keyring = self.config.keyring.clone();
        tokio::task::spawn_blocking(move || {
            load_roster_for_reload(&root, &keyring, has_last_good_capacity)
        })
        .await
        .map_err(|_| anyhow!("Suno roster reader stopped"))?
    }

    /// Startup validation: open the keyring roster and billing-probe every profile. A profile
    /// whose session is rejected is soft-quarantined on its own — one dead cookie never takes
    /// the rest of the fleet, let alone the whole gateway, down with it.
    pub async fn preflight(&self) -> usize {
        let mut live = 0usize;
        for profile in self.profiles_snapshot() {
            match self.probe_profile(&profile).await {
                Ok(()) => live += 1,
                Err(error) => {
                    let effect = match error.verdict() {
                        UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
                        UpstreamVerdict::RateLimitedHard => ProfileEffect::CoolRateLimited,
                        UpstreamVerdict::QuotaExhausted => ProfileEffect::RestForQuota,
                        UpstreamVerdict::CaptchaRequired => ProfileEffect::SoftCaptchaGate,
                        UpstreamVerdict::Transport | UpstreamVerdict::Protocol => {
                            ProfileEffect::TransportFault
                        }
                        UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
                    };
                    profile.apply_effect(effect, now_unix(), None);
                    // Classification only: never print provider bodies, subject, proxy or
                    // session material.
                    elog::warn(
                        "suno",
                        format!(
                            "Suno billing preflight failed profile={} class={}",
                            profile.id,
                            error.class()
                        ),
                    );
                }
            }
        }
        let live = self
            .profiles_snapshot()
            .iter()
            .filter(|profile| profile.authenticated())
            .count()
            .max(live);
        self.live_profiles.store(live, Ordering::Release);
        live
    }

    pub fn quota_poll_interval(&self) -> Duration {
        self.config.quota_poll_interval
    }

    /// Poll every currently published idle profile. Any lease that starts while the billing GET
    /// is in flight invalidates that profile's snapshot; customer traffic is never queued or
    /// rejected merely because maintenance is reading quota.
    pub async fn poll_quotas(&self) -> usize {
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let started = std::time::Instant::now();
        let _sweep = self.quota_sweep.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let published = self.poll_quota_generation(false).await;
        self.quota_sweep_ms.store(
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            Ordering::Release,
        );
        published
    }

    async fn poll_quota_generation(&self, during_shutdown: bool) -> usize {
        let mut published = 0usize;
        for profile in self.profiles_snapshot() {
            if !during_shutdown && self.shutting_down.load(Ordering::Acquire) {
                break;
            }
            if self.poll_profile_quota(&profile, during_shutdown).await {
                published += 1;
            }
        }
        published
    }

    /// One idle-only quota read with the turn-before-quota ordering: drain the bounded turn
    /// FIFO, read the provider snapshot, re-check the generation epoch, drain again under the
    /// FIFO barrier, then let the serial writer pair cumulative dual-ledger spend with the
    /// observation/CAS. Quota steering publishes only after the observation is durable.
    async fn poll_profile_quota(
        &self,
        profile: &Arc<RuntimeProfile>,
        during_shutdown: bool,
    ) -> bool {
        let turn_epoch = profile.turn_epoch.load(Ordering::SeqCst);
        if profile.inflight.load(Ordering::SeqCst) != 0 {
            return false;
        }

        // Do not spend a provider request while an already-known turn head is undelivered.
        {
            let _drain = self.turn_drain.lock().await;
            if !self.drain_turn_queue_locked().await {
                return false;
            }
        }

        let snapshot = match self.fetch_quota(profile, during_shutdown).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let effect = match error.verdict() {
                    UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
                    UpstreamVerdict::RateLimitedHard => ProfileEffect::CoolRateLimited,
                    UpstreamVerdict::QuotaExhausted => ProfileEffect::RestForQuota,
                    UpstreamVerdict::CaptchaRequired => ProfileEffect::SoftCaptchaGate,
                    UpstreamVerdict::Transport | UpstreamVerdict::Protocol => {
                        ProfileEffect::TransportFault
                    }
                    UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
                };
                profile.apply_effect(effect, now_unix(), None);
                self.refresh_live_profile_count();
                elog::warn(
                    "suno",
                    format!(
                        "Suno quota poll failed profile={} class={}",
                        profile.id,
                        error.class()
                    ),
                );
                return false;
            }
        };
        if profile.turn_epoch.load(Ordering::SeqCst) != turn_epoch
            || profile.inflight.load(Ordering::SeqCst) != 0
        {
            // The provider snapshot can already include that generation while its durable spend
            // is not yet paired. Discard the whole read; the next idle poll will observe both.
            return false;
        }
        let observed_at = now_unix();

        // Enqueue takes this same barrier before pushing a turn. Once acquired, a full drain
        // stays full until the observation has crossed the serial writer and its CAS.
        let _drain = self.turn_drain.lock().await;
        if profile.turn_epoch.load(Ordering::SeqCst) != turn_epoch
            || profile.inflight.load(Ordering::SeqCst) != 0
        {
            return false;
        }
        if !self.drain_turn_queue_locked().await {
            return false;
        }
        let Some(billing) = &self.billing else {
            return false;
        };
        if let Err(error) = billing
            .observe_suno_quota(
                &profile.subject_id,
                profile.plan.as_str(),
                crate::billing::SunoQuotaSnapshot {
                    observed_at,
                    total_credits_left: snapshot.total_credits_left,
                    monthly_limit: snapshot.monthly_limit,
                    monthly_usage: snapshot.monthly_usage,
                    period_raw: snapshot.period_raw.clone(),
                },
            )
            .await
        {
            elog::error(
                "suno",
                format!(
                    "Suno quota observation persistence deferred profile={}: {error:#}",
                    profile.id
                ),
            );
            return false;
        }

        // Steering sees a snapshot only after the observation is durable. A transient
        // turn/observation/CAS failure therefore retains the exact previous quota generation.
        profile.publish_quota(&snapshot, observed_at);
        self.refresh_live_profile_count();
        true
    }

    fn refresh_live_profile_count(&self) {
        self.live_profiles.store(
            self.profiles_snapshot()
                .iter()
                .filter(|candidate| candidate.authenticated())
                .count(),
            Ordering::Release,
        );
    }

    /// Free billing read used by preflight and roster reload: the session is valid or it is
    /// not.
    async fn probe_profile(&self, profile: &Arc<RuntimeProfile>) -> Result<(), GatewayFailure> {
        let snapshot = self.fetch_quota(profile, false).await?;
        let observed_at = now_unix();
        // A probe during reload/preflight publishes steering directly: it carries no durable
        // observation (those come only from the FIFO-gated poll path), exactly so a probe can
        // never pair quota with a stale spend total.
        profile.publish_quota(&snapshot, observed_at);
        Ok(())
    }

    /// GET the provider's free billing endpoint: mint a JWT on demand (single-flight) and read
    /// the raw counters. A 401/403 after a successful mint is the soft auth axis, never a
    /// verdict on its own.
    async fn fetch_quota(
        &self,
        profile: &Arc<RuntimeProfile>,
        during_shutdown: bool,
    ) -> Result<BillingSnapshot, GatewayFailure> {
        let jwt = profile
            .session
            .jwt(&profile.client, &profile.hosts)
            .await
            .map_err(|error| GatewayFailure::from_verdict(error.verdict(), 502))?;
        let (cookie, _) = profile.session.material();
        let send = profile
            .client
            .get(profile.hosts.billing_info_url())
            .bearer_auth(&jwt)
            .header(wreq::header::COOKIE, cookie)
            .header("accept", "application/json")
            .header("x-requested-with", "com.suno.android")
            .send();
        tokio::pin!(send);
        let response = if during_shutdown {
            send.await
        } else {
            tokio::select! {
                response = &mut send => response,
                _ = self.maintenance_shutdown_requested() => {
                    return Err(GatewayFailure::Unavailable("suno_shutdown"));
                }
            }
        }
        .map_err(|_| GatewayFailure::Transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
        profile
            .session
            .observe_response(&headers)
            .await
            .map_err(|_| GatewayFailure::Transport)?;
        match client::parse_billing_probe(status, &body) {
            Ok(BillingProbe::Valid(snapshot)) => Ok(snapshot),
            Ok(BillingProbe::Invalid) => Err(GatewayFailure::Auth),
            Err(_) => Err(GatewayFailure::from_verdict(
                classify_status(status),
                status,
            )),
        }
    }

    async fn maintenance_shutdown_requested(&self) {
        loop {
            let notified = self.maintenance_abort.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.shutting_down.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }
}

// ── serving: admission → pre-check → create → detached drain → settlement ────

impl SunoGateway {
    /// Strict-then-relaxed selection (PROVIDER_ONBOARDING §8.4): the strict pass honors both
    /// cooling axes; when it is empty the relaxed pass ignores only the SOFT axis, so a
    /// full-soft fleet still serves while a full-hard fleet honestly selects nothing. Profiles
    /// removed by a corroborated Clerk verdict are invisible to both passes.
    fn select_profile(
        &self,
        excluded: &HashSet<String>,
        reserve_credits: i64,
    ) -> Option<Arc<RuntimeProfile>> {
        let now = now_unix();
        let profiles = self.profiles_snapshot();
        let candidates = profiles
            .iter()
            .filter(|profile| !excluded.contains(&profile.id))
            .filter_map(|profile| profile.candidate(now, reserve_credits))
            .collect::<Vec<_>>();
        let cursor = self.cursor.fetch_add(1, Ordering::Relaxed);
        let selected =
            select(&candidates, cursor).or_else(|| select_ignoring_soft(&candidates, cursor))?;
        profiles
            .into_iter()
            .find(|profile| profile.id == selected.profile_id)
    }

    /// Profiles still tryable after `current` under the same pass selection would use next:
    /// strict-eligible first, then merely hard-eligible. Zero means real provider limits — the
    /// honest 429, never an invented 503.
    fn remaining_count(
        &self,
        excluded: &HashSet<String>,
        current: &str,
        reserve_credits: i64,
    ) -> usize {
        let now = now_unix();
        let candidates = self
            .profiles_snapshot()
            .iter()
            .filter(|profile| profile.id != current && !excluded.contains(&profile.id))
            .filter_map(|profile| profile.candidate(now, reserve_credits))
            .collect::<Vec<_>>();
        let strict = candidates
            .iter()
            .filter(|candidate| candidate.hard.is_none() && candidate.soft.is_none())
            .count();
        if strict > 0 {
            return strict;
        }
        candidates
            .iter()
            .filter(|candidate| candidate.hard.is_none())
            .count()
    }

    /// The earliest known hard-wall expiry, for an honest `Retry-After` on capacity answers.
    fn capacity_retry_after(&self) -> i64 {
        let now = now_unix();
        self.profiles_snapshot()
            .iter()
            .filter_map(|profile| {
                let health = profile.health.lock().expect("Suno profile health lock");
                active_cooling(health.rate_limit_cool_until, now)
            })
            .min()
            .map(|until| (until - now).max(1))
            .unwrap_or(60)
    }

    /// Reserve the conservative hold for one admitted generation. The provider id and the
    /// multiplier pin in the same money transaction, so a concurrent admin edit cannot reprice
    /// an in-flight generation. `None` billing input is the admin (unmetered) path — still
    /// metered into the calibration evidence, never into the customer ledger.
    async fn reserve_customer(
        &self,
        admitted: &AdmittedGeneration,
        request_id: &str,
        priced_ts: i64,
        input: Option<&SunoBillingInput>,
        execution: ExecutionAttempt,
    ) -> Result<Option<Reservation>, GatewayFailure> {
        let Some(input) = input else {
            return Ok(None);
        };
        let billing = self.billing.as_ref().ok_or(GatewayFailure::Unavailable(
            "suno_billing_authority_unavailable",
        ))?;
        let _raw = suno_cost_nanodollars(i128::from(admitted.reserve_credits))
            .map_err(|_| GatewayFailure::Unavailable("suno_price_overflow"))?;
        if input.mult_bp > 0 && input.available_nano <= 0 {
            return Err(GatewayFailure::LowBalance);
        }
        const HOLD_NANO: i64 = 0;
        match billing
            .reserve_priced_request_for_execution(
                request_id,
                &input.account_id,
                &input.key,
                HOLD_NANO,
                execution.clone(),
                registry::PROVIDER_SUNO,
                input.mult_bp,
            )
            .await
            .map_err(|error| {
                elog::error("suno", "Suno reservation failed");
                let _ = error;
                GatewayFailure::Unavailable("suno_reservation_unavailable")
            })? {
            Some(_) => Ok(Some(Reservation {
                request_id: request_id.to_string(),
                account_id: input.account_id.clone(),
                key: input.key.clone(),
                hold: HOLD_NANO,
                mult_bp: input.mult_bp,
                priced_ts,
            })),
            None => Err(GatewayFailure::LowBalance),
        }
    }

    /// One admitted generation request: validate → reserve → pre-check → create → deliver the
    /// generation handle. The response is returned the moment the upstream generation exists;
    /// the poll/download/settle lifecycle then runs detached and never depends on the client
    /// staying connected.
    pub(crate) async fn handle_create(
        self: &Arc<Self>,
        body: Value,
        execution: ExecutionAttempt,
        billing: Option<SunoBillingInput>,
    ) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("suno_shutdown"));
        }
        let parsed: GenerationBody = match serde_json::from_value(body) {
            Ok(parsed) => parsed,
            Err(_) => return error_response(GatewayFailure::BadRequest("suno_invalid_body")),
        };
        let admitted = match admit_generation(parsed) {
            Ok(admitted) => admitted,
            Err(error) => return error_response(error),
        };
        let request_id = crate::upstream::fresh_request_id();
        let priced_ts = now_unix();
        let mut reservation = match self
            .reserve_customer(
                &admitted,
                &request_id,
                priced_ts,
                billing.as_ref(),
                execution,
            )
            .await
        {
            Ok(reservation) => reservation,
            Err(error) => return error_response(error),
        };
        let mut hold_guard = reservation.as_ref().map(|reserved| {
            HoldGuard::new(
                self.billing.clone(),
                reserved.account_id.clone(),
                reserved.key.clone(),
                reserved.hold,
                reserved.request_id.clone(),
            )
        });
        let body_bytes = match serde_json::to_vec(&admitted.upstream_body) {
            Ok(bytes) => Bytes::from(bytes),
            Err(_) => return error_response(GatewayFailure::BadRequest("suno_invalid_body")),
        };

        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();
        loop {
            let Some(profile) = self.select_profile(&excluded, admitted.reserve_credits) else {
                elog::warn("suno", "suno pool exhausted: no profile");
                return error_response_with_retry(
                    GatewayFailure::Capacity,
                    self.capacity_retry_after(),
                );
            };
            let lease = ProfileLease::new(profile.clone());
            // The hCaptcha pre-check runs before every creation on this profile
            // (`oss-hypothesis`, manifest §4). `required: true` soft-cools and rotates — no
            // CAPTCHA is ever solved.
            let (upstream_ids, baseline) = match self
                .precheck_and_create(&profile, &admitted, &body_bytes)
                .await
            {
                CreateOutcome::Created { ids, baseline } => (ids, baseline),
                CreateOutcome::Refused {
                    verdict,
                    retry_after,
                    status,
                } => {
                    let remaining =
                        self.remaining_count(&excluded, &profile.id, admitted.reserve_credits);
                    elog::warn("suno", format!("suno upstream refused: {status}"));
                    let decision = decide(verdict, Phase::BeforeCreate, policy, remaining);
                    profile.apply_effect(decision.effect, now_unix(), retry_after);
                    policy = decision.policy;
                    match decision.next {
                        NextStep::RotateToAnotherProfile => {
                            excluded.insert(profile.id.clone());
                            drop(lease);
                            continue;
                        }
                        NextStep::SurfaceCapacityExhausted => {
                            return error_response_with_retry(
                                GatewayFailure::Capacity,
                                self.capacity_retry_after(),
                            );
                        }
                        NextStep::SurfaceUpstreamError => {
                            return error_response(GatewayFailure::Upstream(status));
                        }
                        // `decide` only yields Created for an Ok verdict, unreachable here.
                        NextStep::Created => {
                            return error_response(GatewayFailure::Protocol);
                        }
                    }
                }
            };
            // THE MONEY BOUNDARY: the reservation becomes delivering, the generation is pinned
            // to this profile forever, and the drain detaches.
            if !self.mark_delivering(reservation.as_ref()).await {
                // The upstream generation may still consume credits. Register the drain so
                // evidence is preserved, but keep the customer hold guard armed.
                if let Some(guard) = self.background.track() {
                    self.track_generation(&request_id, &admitted, billing.as_ref());
                    self.spawn_drain(
                        guard,
                        lease,
                        DrainContext {
                            request_id: request_id.clone(),
                            kind: admitted.kind,
                            requested_model: admitted.requested_model.clone(),
                            reserve_credits: admitted.reserve_credits,
                            priced_ts,
                            profile: profile.clone(),
                            reservation: None,
                            baseline,
                        },
                        upstream_ids,
                    );
                }
                elog::error("suno", "suno delivery marker unavailable");
                return error_response(GatewayFailure::Unavailable(
                    "suno_delivery_marker_unavailable",
                ));
            }
            if let Some(guard) = hold_guard.as_mut() {
                guard.disarm();
            }
            let Some(background) = self.background.track() else {
                // Shutdown raced creation: the generation exists upstream; keep the hold armed
                // and let the reconciler close the reservation after its lease.
                elog::error("suno", "suno generation created during shutdown");
                return error_response(GatewayFailure::Unavailable("suno_shutdown"));
            };
            profile.mark_generation_success();
            self.refresh_live_profile_count();
            self.track_generation(&request_id, &admitted, billing.as_ref());
            self.spawn_drain(
                background,
                lease,
                DrainContext {
                    request_id: request_id.clone(),
                    kind: admitted.kind,
                    requested_model: admitted.requested_model.clone(),
                    reserve_credits: admitted.reserve_credits,
                    priced_ts,
                    profile: profile.clone(),
                    reservation: reservation.take(),
                    baseline,
                },
                upstream_ids,
            );
            return json_response(
                StatusCode::OK,
                json!({
                    "generation_id": request_id,
                    "operation": admitted.kind.as_wire(),
                    "status": "created",
                }),
            );
        }
    }

    /// The pre-creation sequence on one profile: hCaptcha pre-check, then the attribution
    /// baseline (a free billing read inside this lease), then the create call itself. All
    /// failures are pre-money-boundary, so the caller's `decide` governs rotation.
    async fn precheck_and_create(
        &self,
        profile: &Arc<RuntimeProfile>,
        admitted: &AdmittedGeneration,
        body: &Bytes,
    ) -> CreateOutcome {
        let jwt = match profile.session.jwt(&profile.client, &profile.hosts).await {
            Ok(jwt) => jwt,
            Err(error) => {
                return CreateOutcome::Refused {
                    verdict: error.verdict(),
                    retry_after: None,
                    status: 502,
                }
            }
        };
        let (cookie, _) = profile.session.material();

        // hCaptcha pre-check (`oss-hypothesis`, manifest §4): a required gate is a soft verdict,
        // never a solved challenge.
        let required = match self.captcha_required(profile, &jwt, &cookie).await {
            Ok(required) => required,
            Err(verdict) => {
                return CreateOutcome::Refused {
                    verdict,
                    retry_after: None,
                    status: 502,
                }
            }
        };
        if required {
            return CreateOutcome::Refused {
                verdict: UpstreamVerdict::CaptchaRequired,
                retry_after: None,
                status: 200,
            };
        }

        // Attribution baseline: the quota state right before creation, valid only if this lease
        // is the profile's only in-flight work. A failed baseline read does not block the
        // create — settlement then falls to the documented conservative reserve.
        let baseline = match self.read_quota_with(profile, &jwt, &cookie).await {
            Ok(snapshot) if profile.inflight.load(Ordering::SeqCst) == 1 => {
                Some(AttributionBaseline {
                    snapshot,
                    epoch: profile.turn_epoch.load(Ordering::SeqCst),
                })
            }
            _ => None,
        };

        match self
            .send_create(profile, admitted, body, &jwt, &cookie)
            .await
        {
            Ok(ids) => CreateOutcome::Created { ids, baseline },
            Err((verdict, retry_after, status)) => CreateOutcome::Refused {
                verdict,
                retry_after,
                status,
            },
        }
    }

    async fn captcha_required(
        &self,
        profile: &Arc<RuntimeProfile>,
        jwt: &str,
        cookie: &str,
    ) -> Result<bool, UpstreamVerdict> {
        let response = profile
            .client
            .post(profile.hosts.captcha_check_url())
            .bearer_auth(jwt)
            .header(wreq::header::COOKIE, cookie)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("x-requested-with", "com.suno.android")
            .body("{\"ctype\":\"generation\"}")
            .send()
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_bounded(response, ERROR_BODY_LIMIT)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        profile
            .session
            .observe_response(&headers)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        client::parse_captcha_check(status, &body)
    }

    /// A free billing read on an already-minted JWT (the attribution baseline and the post-turn
    /// delta read). Shares the probe parser with the maintenance path.
    async fn read_quota_with(
        &self,
        profile: &Arc<RuntimeProfile>,
        jwt: &str,
        cookie: &str,
    ) -> Result<BillingSnapshot, UpstreamVerdict> {
        let response = profile
            .client
            .get(profile.hosts.billing_info_url())
            .bearer_auth(jwt)
            .header(wreq::header::COOKIE, cookie)
            .header("accept", "application/json")
            .header("x-requested-with", "com.suno.android")
            .send()
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_bounded(response, ERROR_BODY_LIMIT)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        profile
            .session
            .observe_response(&headers)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        match client::parse_billing_probe(status, &body) {
            Ok(BillingProbe::Valid(snapshot)) => Ok(snapshot),
            Ok(BillingProbe::Invalid) => Err(UpstreamVerdict::AuthRefused),
            Err(_) => Err(classify_status(status)),
        }
    }

    /// Native generation creation on the fixed business host. A redirect is never followed — it
    /// must not carry session material to another origin.
    async fn send_create(
        &self,
        profile: &Arc<RuntimeProfile>,
        admitted: &AdmittedGeneration,
        body: &Bytes,
        jwt: &str,
        cookie: &str,
    ) -> Result<Vec<String>, (UpstreamVerdict, Option<i64>, u16)> {
        let url = match admitted.kind {
            OperationKind::Song => profile.hosts.generate_song_url(),
            OperationKind::Extend => profile.hosts.generate_concat_url(),
            OperationKind::Lyrics => profile.hosts.lyrics_create_url(),
            OperationKind::Stems => profile
                .hosts
                .stems_url(&admitted.song_id.clone().unwrap_or_default()),
        };
        let response = profile
            .client
            .post(url)
            .bearer_auth(jwt)
            .header(wreq::header::COOKIE, cookie)
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .header("x-requested-with", "com.suno.android")
            .body(body.clone())
            .send()
            .await
            .map_err(|_| (UpstreamVerdict::Transport, None, 502))?;
        let status = response.status().as_u16();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .and_then(parse_retry_after)
            .map(|seconds| now_unix().saturating_add(seconds));
        let headers = response.headers().clone();
        let body = read_bounded(response, RESPONSE_BODY_LIMIT)
            .await
            .map_err(|_| (UpstreamVerdict::Transport, None, status))?;
        let _ = profile.session.observe_response(&headers).await;
        client::parse_created_ids(status, &body).map_err(|verdict| (verdict, retry_after, status))
    }

    async fn mark_delivering(&self, reservation: Option<&Reservation>) -> bool {
        let Some(reservation) = reservation else {
            return true;
        };
        let Some(billing) = &self.billing else {
            return false;
        };
        matches!(
            billing
                .mark_delivering(&reservation.request_id, RESERVATION_LEASE_SECS)
                .await,
            Ok(true)
        )
    }

    /// Register the read model for one admitted generation. Bounded: when full, the oldest
    /// FINALIZED record is evicted first; a live record is never evicted while its drain owns
    /// money.
    fn track_generation(
        &self,
        request_id: &str,
        admitted: &AdmittedGeneration,
        billing: Option<&SunoBillingInput>,
    ) {
        let now = now_unix();
        let mut generations = self.generations.lock().expect("Suno generations lock");
        if generations.len() >= GENERATION_TRACK_CAPACITY && !generations.contains_key(request_id) {
            let eviction = generations
                .iter()
                .filter(|(_, record)| record.finalized)
                .min_by_key(|(_, record)| record.updated_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = eviction {
                generations.remove(&id);
            }
        }
        generations.insert(
            request_id.to_string(),
            GenerationRecord {
                kind: admitted.kind,
                account_id: billing.map(|input| input.account_id.clone()),
                status: "created",
                artifacts: Vec::new(),
                created_at: now,
                updated_at: now,
                error: None,
                finalized: false,
            },
        );
    }

    fn update_generation(&self, request_id: &str, update: impl FnOnce(&mut GenerationRecord)) {
        let mut generations = self.generations.lock().expect("Suno generations lock");
        if let Some(record) = generations.get_mut(request_id) {
            update(record);
            record.updated_at = now_unix();
        }
    }

    /// The detached lifecycle: poll inside a bounded deadline, download artifacts the moment
    /// the generation completes (upstream media URLs are short-lived, manifest §4), settle
    /// (attributed delta when unambiguous, else the documented conservative reserve), then
    /// deliver the immutable turn event with its paired post-turn quota read. A client
    /// disconnect is invisible to this task by construction.
    fn spawn_drain(
        self: &Arc<Self>,
        background: ActiveTaskGuard,
        lease: ProfileLease,
        context: DrainContext,
        upstream_ids: Vec<String>,
    ) {
        let gateway = Arc::clone(self);
        tokio::spawn(async move {
            let _background = background;
            let _lease = lease;
            gateway.drain_generation(context, upstream_ids).await;
        });
    }

    async fn drain_generation(&self, context: DrainContext, upstream_ids: Vec<String>) {
        let started = std::time::Instant::now();
        loop {
            if self.abort_drains.load(Ordering::Acquire) {
                // Shutdown deadline: the reservation stays with its lease and the reconciler;
                // nothing is settled from a position of ignorance.
                return;
            }
            if started.elapsed() >= POLL_DEADLINE {
                elog::error(
                    "suno",
                    format!(
                        "suno generation drain deadline profile={}",
                        context.profile.id
                    ),
                );
                self.update_generation(&context.request_id, |record| {
                    record.status = "expired";
                    record.error = Some("suno_poll_deadline");
                    record.finalized = true;
                });
                self.settle_conservative_hold(&context, "suno-poll-deadline")
                    .await;
                return;
            }
            match self.poll_upstream(&context, &upstream_ids).await {
                Ok(DrainPoll::Pending { status }) => {
                    // Ongoing states publish immediately; a finalized state is published only by
                    // `finalize_generation`, after the artifacts are in OUR store and the
                    // settlement is queued — `complete` never precedes durable delivery.
                    self.update_generation(&context.request_id, |record| {
                        record.status = status;
                    });
                }
                Ok(DrainPoll::Final(finalized)) => {
                    self.finalize_generation(&context, finalized).await;
                    context.profile.mark_generation_success();
                    self.refresh_live_profile_count();
                    return;
                }
                Err(verdict @ (UpstreamVerdict::ClientError | UpstreamVerdict::Protocol)) => {
                    // A 404 on the pinned generation is provider-side loss evidence; a protocol
                    // anomaly means the wire changed. Neither is retryable.
                    let _ = verdict;
                    elog::error(
                        "suno",
                        format!(
                            "suno generation lost or wire changed profile={}",
                            context.profile.id
                        ),
                    );
                    self.update_generation(&context.request_id, |record| {
                        record.error = Some("suno_generation_lost");
                        record.finalized = true;
                    });
                    self.settle_conservative_hold(&context, "suno-generation-lost")
                        .await;
                    return;
                }
                Err(_) => {
                    // Transport/auth failures keep polling inside the deadline: the generation
                    // is still running upstream and only its own poll can settle it.
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
                _ = self.abort_notify.notified() => {}
            }
        }
    }

    /// One poll of the upstream generation, shaped by the operation: song/extend read the feed
    /// (all created clips must finalize), stems reads the clip endpoint per returned id, lyrics
    /// reads its own status route.
    async fn poll_upstream(
        &self,
        context: &DrainContext,
        upstream_ids: &[String],
    ) -> Result<DrainPoll, UpstreamVerdict> {
        match context.kind {
            OperationKind::Song | OperationKind::Extend => {
                let clips = self.poll_feed(&context.profile, upstream_ids).await?;
                if clips
                    .iter()
                    .any(|clip| clip.lifecycle == Some(ClipLifecycle::Error))
                {
                    return Ok(DrainPoll::Final(Finalized::Clips {
                        clips,
                        succeeded: false,
                    }));
                }
                if !clips.is_empty()
                    && clips
                        .iter()
                        .all(|clip| clip.lifecycle.is_some_and(ClipLifecycle::is_final))
                {
                    return Ok(DrainPoll::Final(Finalized::Clips {
                        clips,
                        succeeded: true,
                    }));
                }
                let status = clips
                    .iter()
                    .filter_map(|clip| clip.lifecycle)
                    .map(|lifecycle| lifecycle.as_str())
                    .next()
                    .unwrap_or("queued");
                Ok(DrainPoll::Pending { status })
            }
            OperationKind::Stems => {
                let Some(id) = upstream_ids.first() else {
                    return Err(UpstreamVerdict::Protocol);
                };
                let clip = self.poll_clip(&context.profile, id).await?;
                match clip.lifecycle {
                    Some(ClipLifecycle::Complete) => Ok(DrainPoll::Final(Finalized::Clips {
                        clips: vec![clip],
                        succeeded: true,
                    })),
                    Some(ClipLifecycle::Error) => Ok(DrainPoll::Final(Finalized::Clips {
                        clips: vec![clip],
                        succeeded: false,
                    })),
                    Some(lifecycle) => Ok(DrainPoll::Pending {
                        status: lifecycle.as_str(),
                    }),
                    None => Err(UpstreamVerdict::Protocol),
                }
            }
            OperationKind::Lyrics => {
                let Some(id) = upstream_ids.first() else {
                    return Err(UpstreamVerdict::Protocol);
                };
                let state = self.poll_lyrics(&context.profile, id).await?;
                // The status vocabulary is an open `unknown` (manifest §6): only the exact
                // reviewed `complete`/`error` are terminal; anything else stays pending inside
                // the bounded deadline.
                match state.status_raw.as_deref() {
                    Some("complete") => Ok(DrainPoll::Final(Finalized::Lyrics(state))),
                    Some("error") => Ok(DrainPoll::Final(Finalized::LyricsFailed)),
                    _ => Ok(DrainPoll::Pending { status: "queued" }),
                }
            }
        }
    }

    async fn poll_feed(
        &self,
        profile: &Arc<RuntimeProfile>,
        ids: &[String],
    ) -> Result<Vec<ClipState>, UpstreamVerdict> {
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let body = self
            .authed_get(profile, profile.hosts.feed_url(&refs))
            .await?;
        client::parse_feed_clips(body.0, &body.1)
    }

    async fn poll_clip(
        &self,
        profile: &Arc<RuntimeProfile>,
        id: &str,
    ) -> Result<ClipState, UpstreamVerdict> {
        let (status, body) = self.authed_get(profile, profile.hosts.clip_url(id)).await?;
        client::parse_clip(status, &body)
    }

    async fn poll_lyrics(
        &self,
        profile: &Arc<RuntimeProfile>,
        id: &str,
    ) -> Result<client::LyricsState, UpstreamVerdict> {
        let (status, body) = self
            .authed_get(profile, profile.hosts.lyrics_status_url(id))
            .await?;
        client::parse_lyrics_state(status, &body)
    }

    /// One authenticated GET on the business host: JWT on demand, cookie re-sent, `set-cookie`
    /// merged back (manifest §2).
    async fn authed_get(
        &self,
        profile: &Arc<RuntimeProfile>,
        url: String,
    ) -> Result<(u16, Bytes), UpstreamVerdict> {
        let jwt = profile
            .session
            .jwt(&profile.client, &profile.hosts)
            .await
            .map_err(|error| error.verdict())?;
        let (cookie, _) = profile.session.material();
        let response = profile
            .client
            .get(url)
            .bearer_auth(&jwt)
            .header(wreq::header::COOKIE, cookie)
            .header("accept", "application/json")
            .header("x-requested-with", "com.suno.android")
            .send()
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let status = response.status().as_u16();
        let headers = response.headers().clone();
        let body = read_bounded(response, RESPONSE_BODY_LIMIT)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let _ = profile.session.observe_response(&headers).await;
        Ok((status, body))
    }

    /// Terminal accounting for one finalized generation. The money authority is the ATTRIBUTED
    /// post-turn credit delta on `/api/billing/info/` (manifest §5.3, owner's admission
    /// directive): an unambiguous delta settles exactly it; ambiguous attribution (concurrent
    /// traffic on the same profile, an unreadable snapshot, or a delta above the reserve bound)
    /// settles at the documented conservative reserve with the movement recorded unattributed;
    /// a finalized-but-failed generation with zero attributed movement refunds the hold
    /// (manifest §4.1).
    async fn finalize_generation(&self, context: &DrainContext, finalized: Finalized) {
        let completed_at = now_unix();
        let (succeeded, mut stored, served_model, clips): (
            bool,
            Vec<String>,
            Option<String>,
            Vec<ClipState>,
        ) = match &finalized {
            Finalized::Clips { clips, succeeded } => (
                *succeeded,
                Vec::new(),
                clips.iter().find_map(|clip| clip.served_model.clone()),
                clips.clone(),
            ),
            Finalized::Lyrics(_) => (true, Vec::new(), None, Vec::new()),
            Finalized::LyricsFailed => (false, Vec::new(), None, Vec::new()),
        };
        if succeeded {
            // Artifacts first: upstream media URLs are short-lived, settlement can wait a
            // moment. Lyrics persist through the same durable path from memory.
            match &finalized {
                Finalized::Clips { clips, .. } => {
                    for clip in clips {
                        for (field, url) in &clip.artifacts {
                            let artifact_field = if clips.len() > 1 {
                                format!("{}.{field}", clip.id)
                            } else {
                                field.clone()
                            };
                            match store_artifact(
                                &context.profile.client,
                                url,
                                &self.config.artifact_dir,
                                &context.request_id,
                                &artifact_field,
                            )
                            .await
                            {
                                Ok(name) => stored.push(name),
                                Err(error) => {
                                    self.artifact_failures.fetch_add(1, Ordering::Relaxed);
                                    elog::error(
                                        "suno",
                                        format!(
                                            "suno artifact download failed profile={} field={field}: {error:#}",
                                            context.profile.id
                                        ),
                                    );
                                }
                            }
                        }
                    }
                }
                Finalized::Lyrics(state) => {
                    if let Some(text) = &state.text {
                        match store_payload(
                            &self.config.artifact_dir,
                            &context.request_id,
                            "lyrics.txt",
                            text.as_bytes(),
                        )
                        .await
                        {
                            Ok(name) => stored.push(name),
                            Err(error) => {
                                self.artifact_failures.fetch_add(1, Ordering::Relaxed);
                                elog::error(
                                    "suno",
                                    format!(
                                        "suno lyrics persist failed profile={}: {error:#}",
                                        context.profile.id
                                    ),
                                );
                            }
                        }
                    }
                }
                Finalized::LyricsFailed => {}
            }
        }
        let _ = clips;

        // The post-turn quota read: the delta against the pre-create baseline attributes this
        // generation's credit movement, iff no other lease overlapped the window.
        let post = self.fetch_quota(&context.profile, false).await.ok();
        let attributed_delta_credits = match (&context.baseline, &post) {
            (Some(baseline), Some(post))
                if context.profile.turn_epoch.load(Ordering::SeqCst) == baseline.epoch =>
            {
                credit_delta(&baseline.snapshot, post)
            }
            _ => None,
        };

        enum Money {
            /// Attributed provider-observed delta (credits).
            Attributed(i64),
            /// Documented zero: failed generation, zero attributed movement — refund the hold.
            Refund,
            /// Ambiguous/anomalous: settle at the reserve, record the movement unattributed.
            Reserve(&'static str),
        }
        let money = if succeeded {
            match attributed_delta_credits {
                Some(delta) if delta > 0 && delta <= context.reserve_credits => {
                    Money::Attributed(delta)
                }
                Some(_) => {
                    // A delta above the reserve bound is a typed anomaly: quarantine, never
                    // silent acceptance.
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    Money::Reserve("suno-tariff-anomaly")
                }
                // Zero movement on a completed generation is not credible consumption
                // evidence; the reserve holds (documented conservative default).
                None => {
                    self.unattributed_settlements
                        .fetch_add(1, Ordering::Relaxed);
                    Money::Reserve("suno-attribution-ambiguous")
                }
            }
        } else {
            match attributed_delta_credits {
                // Finalized-but-failed with zero credit movement: the documented refund
                // (manifest §4.1).
                Some(0) => Money::Refund,
                Some(delta) if delta > 0 && delta <= context.reserve_credits => {
                    Money::Attributed(delta)
                }
                Some(_) => {
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    Money::Reserve("suno-tariff-anomaly")
                }
                None => {
                    self.unattributed_settlements
                        .fetch_add(1, Ordering::Relaxed);
                    Money::Reserve("suno-attribution-ambiguous")
                }
            }
        };

        let (native_milli, api_nano, schedule_derived) = match money {
            Money::Attributed(credits) => {
                let milli = credits.saturating_mul(1_000);
                let api = i128::from(milli)
                    .checked_mul(i128::from(registry::SUNO_NANOUSD_PER_MILLICREDIT))
                    .and_then(|value| i64::try_from(value).ok());
                let Some(api) = api else {
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    self.settle_conservative_hold(context, "suno-money-overflow")
                        .await;
                    self.publish_terminal(context, &finalized, stored, None);
                    return;
                };
                (milli, api, false)
            }
            Money::Refund => (0, 0, true),
            Money::Reserve(reason) => {
                let _ = reason;
                let milli = context.reserve_credits.saturating_mul(1_000);
                let api = i128::from(milli)
                    .checked_mul(i128::from(registry::SUNO_NANOUSD_PER_MILLICREDIT))
                    .and_then(|value| i64::try_from(value).ok());
                let Some(api) = api else {
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    self.settle_conservative_hold(context, "suno-money-overflow")
                        .await;
                    self.publish_terminal(context, &finalized, stored, None);
                    return;
                };
                (milli, api, true)
            }
        };

        // Customer settlement: attributed price x the pinned multiplier through the single
        // writer; a refund settles zero and releases the hold. Admin generations carry no
        // reservation — their evidence still records below.
        if let Some(reservation) = &context.reservation {
            if let Some(billing) = &self.billing {
                let actual = metering::apply_multiplier(i128::from(api_nano), reservation.mult_bp)
                    .clamp(0, i128::from(i64::MAX)) as i64;
                let usage_event = (api_nano > 0).then(|| registry::UsageEventInput {
                    model: context.kind.as_wire().to_string(),
                    provider: registry::PROVIDER_SUNO.to_string(),
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                    cache_write_5m_tokens: 0,
                    cache_write_1h_tokens: 0,
                    web_search_requests: 0,
                    real_nano: api_nano,
                    charge_basis_nano: api_nano,
                    speed: "standard".to_string(),
                    inference_geo: String::new(),
                    input_nano: 0,
                    output_nano: api_nano,
                    cache_read_nano: 0,
                    cache_write_5m_nano: 0,
                    cache_write_1h_nano: 0,
                    web_search_nano: 0,
                    priced_ts: reservation.priced_ts,
                });
                if let Err(error) = billing
                    .settle_request_with_usage(
                        &reservation.request_id,
                        &reservation.account_id,
                        &reservation.key,
                        reservation.hold,
                        actual,
                        None,
                        usage_event,
                    )
                    .await
                {
                    elog::error(
                        "suno",
                        format!("Suno customer settlement deferred: {error:#}"),
                    );
                }
            }
        }

        // The immutable turn event: zero pairs are legal evidence (a documented refund), a paid
        // generation can never carry one (the schema's joint invariant), and a reserve-settled
        // generation is flagged `native_schedule_derived` — never presented as provider truth.
        let event = SunoTurnCalibrationEvent {
            request_id: context.request_id.clone(),
            subject_id: context.profile.subject_id.clone(),
            plan: context.profile.plan.as_str().to_string(),
            requested_model: context
                .requested_model
                .clone()
                .unwrap_or_else(|| context.kind.as_wire().to_string()),
            served_model,
            tariff_schedule_id: SUNO_TARIFF_SCHEDULE_ID.to_string(),
            priced_ts: context.priced_ts,
            completed_at,
            upstream_clip_id: upstream_clip_id_of(&finalized),
            native_total_millicredits: native_milli,
            api_total_nanousd: api_nano,
            native_schedule_derived: schedule_derived,
        };
        if let Err(error) = event.validate() {
            elog::error(
                "suno",
                format!("Suno calibration event rejected before FIFO: {error:#}"),
            );
            self.publish_terminal(context, &finalized, stored, None);
            return;
        }
        // Codex/Gemini pairing: the post-turn quota read taken in the turn's wake rides the
        // same FIFO entry, so the writer persists the observation with cumulative ledgers that
        // already include this turn — and the observation is what attributes the delta.
        let billing = post.map(|snapshot| crate::billing::SunoQuotaSnapshot {
            observed_at: now_unix(),
            total_credits_left: snapshot.total_credits_left,
            monthly_limit: snapshot.monthly_limit,
            monthly_usage: snapshot.monthly_usage,
            period_raw: snapshot.period_raw,
        });
        self.enqueue_turn(PendingTurn { event, billing }).await;
        self.publish_terminal(context, &finalized, stored, None);
    }

    /// Single publication point for the terminal read model: after artifacts are in our store
    /// and the settlement/FIFO work is done, never before.
    fn publish_terminal(
        &self,
        context: &DrainContext,
        finalized: &Finalized,
        stored: Vec<String>,
        error: Option<&'static str>,
    ) {
        let status: &'static str = match finalized {
            Finalized::Clips { succeeded, .. } => {
                if *succeeded {
                    "complete"
                } else {
                    "error"
                }
            }
            Finalized::Lyrics(_) => "complete",
            Finalized::LyricsFailed => "error",
        };
        self.update_generation(&context.request_id, |record| {
            record.artifacts = stored;
            record.finalized = true;
            record.status = status;
            record.error = error;
        });
    }

    /// The documented-conservative settle: delivery occurred (or the generation state moved)
    /// but settlement cannot be exact — the drain deadline expired, or the generation was lost
    /// upstream. Preserve the hold, advance the typed counter (done by the caller), create no
    /// immutable provider event.
    async fn settle_conservative_hold(&self, context: &DrainContext, reason: &'static str) {
        let Some(reservation) = &context.reservation else {
            return;
        };
        let Some(billing) = &self.billing else {
            return;
        };
        if let Err(error) = billing
            .settle_request(
                &reservation.request_id,
                &reservation.account_id,
                &reservation.key,
                reservation.hold,
                reservation.hold,
                Some(reason),
            )
            .await
        {
            elog::error(
                "suno",
                format!("Suno conservative settlement deferred: {error:#}"),
            );
        }
    }

    async fn enqueue_turn(&self, turn: PendingTurn) {
        let _drain = self.turn_drain.lock().await;
        let accepted = self
            .turn_queue
            .lock()
            .expect("Suno turn queue lock")
            .push(turn);
        if !accepted {
            elog::error(
                "suno",
                "Suno calibration event dropped because the bounded FIFO is full",
            );
            return;
        }
        self.drain_turn_queue_locked().await;
    }

    /// Drain under `turn_drain`. A transient head remains in place and keeps quota polling
    /// blocked; a permanent replay conflict quarantines exactly that event and continues.
    async fn drain_turn_queue_locked(&self) -> bool {
        loop {
            let head = self
                .turn_queue
                .lock()
                .expect("Suno turn queue lock")
                .head()
                .cloned();
            let Some(head) = head else { break };
            let outcome = match &self.billing {
                Some(billing) => match billing.record_suno_turn(head).await {
                    Ok(_) => WriteOutcome::Durable,
                    Err(error) if registry::is_suno_turn_replay_conflict(&error) => {
                        WriteOutcome::Conflict
                    }
                    Err(error) => {
                        elog::warn(
                            "suno",
                            format!(
                                "Suno calibration persistence deferred with FIFO head retained: {error:#}"
                            ),
                        );
                        WriteOutcome::Transient
                    }
                },
                None => WriteOutcome::Transient,
            };
            self.turn_queue
                .lock()
                .expect("Suno turn queue lock")
                .resolve_head(outcome);
            if outcome == WriteOutcome::Transient {
                break;
            }
        }
        self.turn_queue
            .lock()
            .expect("Suno turn queue lock")
            .may_poll_quota()
    }

    // ── uploads (customer binary intake; upstream path unknown → fail closed) ──

    /// One customer audio binary (multipart `file` field, ≤96 MiB): persisted durably on OUR
    /// storage and answerable by id. The blueprint documents no upstream upload endpoint
    /// (manifest §4/§6), so admission of a generation carrying the id fails closed with a clear
    /// 400 — the intake exists so the capability is end-to-end on our side the moment an
    /// upstream path is proven, and so a customer binary is never silently dropped.
    pub(crate) async fn handle_audio_upload(self: &Arc<Self>, bytes: Bytes) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("suno_shutdown"));
        }
        let upload_id = format!("suo-{}", crate::upstream::fresh_request_id());
        match store_upload(&self.config.artifact_dir, &upload_id, &bytes).await {
            Ok(stored) => json_response(
                StatusCode::OK,
                json!({
                    "upload_id": stored.upload_id,
                    "format": stored.format.extension(),
                    "bytes": stored.bytes,
                }),
            ),
            Err(_) => error_response(GatewayFailure::BadRequest("suno_upload_rejected")),
        }
    }

    // ── read surface (our generation records; artifacts from our store) ───────

    /// The read model of one generation for its creating account (or an admin). A foreign or
    /// unknown id answers `None` — the plane never reveals whether a generation exists across
    /// accounts.
    pub fn generation_view(
        &self,
        generation_id: &str,
        requester: Option<&str>,
    ) -> Option<SunoGenerationView> {
        let record = self
            .generations
            .lock()
            .expect("Suno generations lock")
            .get(generation_id)
            .cloned()?;
        // Admin (no account scope) reads everything; a metered reader sees only its own
        // account's generations; a foreign id is indistinguishable from an unknown one.
        if let (Some(owner), Some(requester)) = (record.account_id.as_deref(), requester) {
            if owner != requester {
                return None;
            }
        }
        Some(SunoGenerationView {
            generation_id: generation_id.to_string(),
            operation: record.kind.as_wire().to_string(),
            status: record.status.to_string(),
            artifacts: record.artifacts.clone(),
            created_at: record.created_at,
            updated_at: record.updated_at,
            error: record.error,
        })
    }

    /// The on-disk path of one stored artifact for an authorized reader. The name must be one
    /// the generation actually recorded — client input never becomes a path component.
    pub fn generation_artifact_path(
        &self,
        generation_id: &str,
        name: &str,
        requester: Option<&str>,
    ) -> Option<PathBuf> {
        let record = self
            .generations
            .lock()
            .expect("Suno generations lock")
            .get(generation_id)
            .cloned()?;
        if let (Some(owner), Some(requester)) = (record.account_id.as_deref(), requester) {
            if owner != requester {
                return None;
            }
        }
        if !record.artifacts.iter().any(|artifact| artifact == name) {
            return None;
        }
        Some(artifact_path(
            &self.config.artifact_dir,
            generation_id,
            name,
        ))
    }

    /// Read only cached operational state. Metrics collection and the admin projection never
    /// start a network request here.
    pub fn operational_status(&self) -> SunoOperationalStatus {
        let delivery = self
            .turn_queue
            .lock()
            .expect("Suno turn queue lock")
            .health();
        let now = now_unix();
        let profiles = self.profiles.read().expect("Suno profiles lock");
        let mut rate_limited_profiles = 0;
        let mut quota_walled_profiles = 0;
        let mut auth_cooling_profiles = 0;
        let mut captcha_cooling_profiles = 0;
        let mut transport_cooling_profiles = 0;
        let mut inflight_requests = 0u64;
        let mut statuses = Vec::with_capacity(profiles.len());
        for profile in profiles.iter() {
            let health = profile.health.lock().expect("Suno profile health lock");
            let rate_until = active_cooling(health.rate_limit_cool_until, now);
            let auth_until = active_cooling(health.auth_cool_until, now);
            let captcha_until = active_cooling(health.captcha_cool_until, now);
            let transport_until = active_cooling(health.transport_cool_until, now);
            rate_limited_profiles += usize::from(rate_until.is_some());
            quota_walled_profiles += usize::from(health.quota_walled);
            auth_cooling_profiles += usize::from(auth_until.is_some());
            captcha_cooling_profiles += usize::from(captcha_until.is_some());
            transport_cooling_profiles += usize::from(transport_until.is_some());
            let inflight = profile.inflight.load(Ordering::Acquire);
            inflight_requests += u64::from(inflight);
            statuses.push(SunoProfileStatus {
                id: profile.id.clone(),
                plan: profile.plan.as_str().to_string(),
                routable: !health.auth_dead,
                live: health.authenticated,
                rate_limit_cool_until: rate_until,
                quota_walled: health.quota_walled,
                auth_cool_until: auth_until,
                captcha_cool_until: captcha_until,
                transport_cool_until: transport_until,
                inflight,
                quota_observed_at: health.quota_observed_at,
                total_credits_left: health.total_credits_left,
                monthly_limit: health.monthly_limit,
                monthly_usage: health.monthly_usage,
            });
        }
        drop(profiles);
        let available_profiles = {
            let candidates: Vec<Candidate> = self
                .profiles
                .read()
                .expect("Suno profiles lock")
                .iter()
                .filter_map(|profile| profile.candidate(now, 0))
                .collect();
            candidates
                .iter()
                .filter(|candidate| candidate.hard.is_none() && candidate.soft.is_none())
                .count()
        };
        SunoOperationalStatus {
            total_profiles: statuses.len(),
            live_profiles: self.live_profiles.load(Ordering::Acquire),
            available_profiles,
            rate_limited_profiles,
            quota_walled_profiles,
            auth_cooling_profiles,
            captcha_cooling_profiles,
            transport_cooling_profiles,
            inflight_requests,
            inflight_drains: 0,
            tracked_generations: self
                .generations
                .lock()
                .expect("Suno generations lock")
                .len(),
            unattributed_settlements: self.unattributed_settlements.load(Ordering::Acquire),
            tariff_anomaly: self.tariff_anomaly.load(Ordering::Acquire),
            artifact_failures: self.artifact_failures.load(Ordering::Acquire),
            quota_sweep_ms: self.quota_sweep_ms.load(Ordering::Acquire),
            profiles: statuses,
            delivery,
        }
    }

    /// Resolve a durable calibration subject to its opaque roster id. The subject itself never
    /// leaves the gateway; rows whose subject is no longer in the roster resolve to `None` so
    /// the caller drops them instead of serializing an unresolvable identity.
    pub fn profile_id_for_subject(&self, subject_id: &str) -> Option<String> {
        self.profiles
            .read()
            .expect("Suno profiles lock")
            .iter()
            .find(|profile| profile.subject_id == subject_id)
            .map(|profile| profile.id.clone())
    }

    pub fn readiness(&self) -> Result<(), NotReady> {
        let status = self.operational_status();
        readiness(status.live_profiles, status.delivery.persistence_ok)
    }

    /// Close admission, wait for detached drains (poll + download + settle), then run the
    /// final turn-before-quota ordering inside the process deadline. On the deadline the drains
    /// stop mid-poll: the reservation stays with its lease and the reconciler, never a
    /// settlement from ignorance.
    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.shutting_down.store(true, Ordering::Release);
        self.maintenance_abort.notify_waiters();
        self.background.close();
        match deadline {
            Some(deadline) => {
                if tokio::time::timeout_at(deadline, self.background.wait_idle())
                    .await
                    .is_err()
                {
                    self.abort_drains.store(true, Ordering::Release);
                    self.abort_notify.notify_waiters();
                    self.background.wait_idle().await;
                }
            }
            None => self.background.wait_idle().await,
        }
        let final_calibration = async {
            let _sweep = self.quota_sweep.lock().await;
            // Admission is closed and every drain is idle, so each profile is stable: finish
            // the same turn-before-quota ordering used by the steady-state poller.
            self.poll_quota_generation(true).await;
            let _drain = self.turn_drain.lock().await;
            self.drain_turn_queue_locked().await
        };
        let complete = match deadline {
            Some(deadline) => tokio::time::timeout_at(deadline, final_calibration)
                .await
                .unwrap_or(false),
            None => final_calibration.await,
        };
        if !complete {
            elog::error(
                "suno",
                "Suno shutdown calibration drain remained incomplete at deadline",
            );
        }
    }
}

/// The credit delta between two billing snapshots: `total_credits_left` drawdown preferred,
/// else `monthly_usage` advance. Both halves missing → unattributable (`None`). A negative
/// delta (a refill mid-window) is not consumption evidence.
fn credit_delta(baseline: &BillingSnapshot, post: &BillingSnapshot) -> Option<i64> {
    if let (Some(before), Some(after)) = (baseline.total_credits_left, post.total_credits_left) {
        return (after <= before).then_some(before - after);
    }
    if let (Some(before), Some(after)) = (baseline.monthly_usage, post.monthly_usage) {
        return (after >= before).then_some(after - before);
    }
    None
}

/// The audit identity of the upstream generation: the first provider id, never the money
/// identity (that is the internal request id).
fn upstream_clip_id_of(finalized: &Finalized) -> String {
    match finalized {
        Finalized::Clips { clips, .. } => clips
            .first()
            .map(|clip| clip.id.clone())
            .unwrap_or_else(|| "unknown".to_string()),
        Finalized::Lyrics(state) => state.id.clone(),
        Finalized::LyricsFailed => "unknown".to_string(),
    }
}

/// One drain poll answer.
enum DrainPoll {
    Pending { status: &'static str },
    Final(Finalized),
}

/// A finalized upstream generation, with the evidence the settlement needs.
enum Finalized {
    Clips {
        clips: Vec<ClipState>,
        succeeded: bool,
    },
    Lyrics(client::LyricsState),
    LyricsFailed,
}

/// The result of one pre-creation sequence on one profile.
enum CreateOutcome {
    Created {
        ids: Vec<String>,
        baseline: Option<AttributionBaseline>,
    },
    Refused {
        verdict: UpstreamVerdict,
        retry_after: Option<i64>,
        status: u16,
    },
}

fn same_profile_generation(left: &[Arc<RuntimeProfile>], right: &[Arc<RuntimeProfile>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| Arc::ptr_eq(left, right))
}

fn profiles_match_roster(profiles: &[Arc<RuntimeProfile>], roster: &[SunoProfile]) -> bool {
    profiles.len() == roster.len()
        && profiles
            .iter()
            .zip(roster)
            .all(|(profile, loaded)| profile.matches_roster(loaded))
}

// ── admission: request validation, pricing, upstream body ───────────────────

/// The plane's create-generation request body. Strict: an unknown field is rejected rather
/// than silently dropped (the customer is paying per generation; a dropped control would
/// misprice the reserve against what the provider actually runs).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationBody {
    operation: String,
    model: Option<String>,
    prompt: Option<String>,
    tags: Option<String>,
    title: Option<String>,
    make_instrumental: Option<bool>,
    negative_tags: Option<String>,
    continue_at: Option<f64>,
    continue_clip_id: Option<String>,
    song_id: Option<String>,
    /// Customer upload ids from `POST /v1/audio/uploads`. The blueprint documents no upstream
    /// upload endpoint (manifest §4/§6), so any non-empty list fails closed at admission.
    attachments: Option<Vec<String>>,
}

/// The admitted operations of the manifest §4 wire table. MIDI is priced in the tariff but has
/// no wire-table entry (it is a Studio operation), so it is NOT admitted; an unknown operation
/// fails closed naming the admitted set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OperationKind {
    /// `POST /api/generate/v2/` — published 5 credits/song, flat across paid models.
    Song,
    /// `POST /api/generate/concat/v2/` — price unpublished: the documented conservative
    /// reserve (the highest published per-operation price, 50 credits).
    Extend,
    /// `POST /api/generate/lyrics/` + GET — price unpublished: conservative 50-credit reserve.
    Lyrics,
    /// `POST /api/edit/stems/{song_id}` — the wire carries no split-kind selector (none is
    /// documented), so the provider's default applies and the reserve is the highest published
    /// per-operation price (50 credits, Auto Split), settled from the attributed delta.
    Stems,
}

impl OperationKind {
    fn from_wire(raw: &str) -> Option<Self> {
        Some(match raw {
            "song" => Self::Song,
            "extend" => Self::Extend,
            "lyrics" => Self::Lyrics,
            "stems" => Self::Stems,
            _ => return None,
        })
    }

    fn as_wire(self) -> &'static str {
        match self {
            Self::Song => "song",
            Self::Extend => "extend",
            Self::Lyrics => "lyrics",
            Self::Stems => "stems",
        }
    }

    /// The reserve in native credits: the published price where one exists (song: 5), else the
    /// documented conservative reserve — the highest published per-operation price, 50
    /// credits — settled from the attributed post-turn delta (manifest §5.1, owner's directive).
    fn reserve_credits(self) -> i64 {
        match self {
            Self::Song => metering::suno::SUNO_CREDITS_PER_SONG,
            Self::Extend | Self::Lyrics | Self::Stems => 50,
        }
    }
}

/// An admitted, priced generation: the validated upstream body plus its reserve.
#[derive(Debug)]
struct AdmittedGeneration {
    kind: OperationKind,
    requested_model: Option<String>,
    song_id: Option<String>,
    upstream_body: Value,
    /// The reserve in credits — the published song price or the documented conservative bound;
    /// also the settlement anomaly cross-check bound.
    reserve_credits: i64,
}

const MAX_PROMPT_LEN: usize = 8 * 1024;
const MAX_TAGS_LEN: usize = 1024;
const MAX_TITLE_LEN: usize = 256;
const MAX_ID_LEN: usize = 128;

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.bytes().any(|b| b < 0x08)
}

/// Validate and price one create-generation request: the admitted operations only, every other
/// `operation` rejected with the admitted set named. Returns the upstream body over the
/// documented wire fields plus the reserve.
fn admit_generation(body: GenerationBody) -> Result<AdmittedGeneration, GatewayFailure> {
    let Some(kind) = OperationKind::from_wire(&body.operation) else {
        // The refusal names the admitted set: an unknown operation fails closed before reserve.
        return Err(GatewayFailure::BadRequest("suno_operation_unknown"));
    };

    // Attachments: the real API takes audio input for covers/extend-type operations, but the
    // blueprint documents no upstream upload endpoint (manifest §4/§6). Fail closed with the
    // gap named — never invent an upstream path.
    if body
        .attachments
        .as_ref()
        .is_some_and(|list| !list.is_empty())
    {
        return Err(GatewayFailure::Unsupported(
            "suno_attachment_upstream_unknown",
        ));
    }

    let model = body.model.as_deref();
    if let Some(model) = model {
        // Model ids are the reviewed paid catalog in metering (manifest §3); an unknown, free-
        // tier or deprecated id fails closed rather than borrowing the flat song rate.
        if suno_paid_model(model).is_none() {
            return Err(GatewayFailure::BadRequest("suno_model_unknown"));
        }
    }

    let mut upstream = json!({});
    let object = upstream.as_object_mut().expect("generation object");
    let mut song_id = None;

    match kind {
        OperationKind::Song => {
            let Some(model) = model else {
                return Err(GatewayFailure::BadRequest("suno_model_required"));
            };
            let make_instrumental = body.make_instrumental.unwrap_or(false);
            object.insert("mv".into(), json!(model));
            object.insert("make_instrumental".into(), json!(make_instrumental));
            // Custom mode when tags/title are given (`prompt` carries lyrics there); otherwise
            // the description form (`gpt_description_prompt`). Both are the reviewed wire
            // shapes (manifest §4).
            let custom = body.tags.is_some() || body.title.is_some();
            if custom {
                if let Some(prompt) = body.prompt.as_deref() {
                    if !bounded_text(prompt, MAX_PROMPT_LEN) {
                        return Err(GatewayFailure::BadRequest("suno_prompt_invalid"));
                    }
                    object.insert("prompt".into(), json!(prompt));
                }
                if let Some(tags) = body.tags.as_deref() {
                    if !bounded_text(tags, MAX_TAGS_LEN) {
                        return Err(GatewayFailure::BadRequest("suno_tags_invalid"));
                    }
                    object.insert("tags".into(), json!(tags));
                }
                if let Some(title) = body.title.as_deref() {
                    if !bounded_text(title, MAX_TITLE_LEN) {
                        return Err(GatewayFailure::BadRequest("suno_title_invalid"));
                    }
                    object.insert("title".into(), json!(title));
                }
                if let Some(negative) = body.negative_tags.as_deref() {
                    if !bounded_text(negative, MAX_TAGS_LEN) {
                        return Err(GatewayFailure::BadRequest("suno_negative_tags_invalid"));
                    }
                    object.insert("negative_tags".into(), json!(negative));
                }
            } else {
                let Some(prompt) = body
                    .prompt
                    .as_deref()
                    .filter(|p| bounded_text(p, MAX_PROMPT_LEN))
                else {
                    return Err(GatewayFailure::BadRequest("suno_prompt_required"));
                };
                object.insert("gpt_description_prompt".into(), json!(prompt));
            }
        }
        OperationKind::Extend => {
            let Some(clip) = body.continue_clip_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("suno_continue_clip_required"));
            };
            if !bounded_text(clip, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("suno_continue_clip_invalid"));
            }
            object.insert("continue_clip_id".into(), json!(clip));
            if let Some(continue_at) = body.continue_at {
                // A playback position, not money: must be a finite non-negative number.
                if !continue_at.is_finite() || continue_at < 0.0 || continue_at > 86_400.0 {
                    return Err(GatewayFailure::BadRequest("suno_continue_at_invalid"));
                }
                object.insert("continue_at".into(), json!(continue_at));
            }
            if model.is_some()
                || body.prompt.is_some()
                || body.tags.is_some()
                || body.title.is_some()
                || body.negative_tags.is_some()
                || body.make_instrumental.is_some()
            {
                // The concat wire takes only the clip reference (manifest §4); extra controls
                // would desync reserve and wire.
                return Err(GatewayFailure::BadRequest("suno_option_not_applicable"));
            }
        }
        OperationKind::Lyrics => {
            let Some(prompt) = body
                .prompt
                .as_deref()
                .filter(|p| bounded_text(p, MAX_PROMPT_LEN))
            else {
                return Err(GatewayFailure::BadRequest("suno_prompt_required"));
            };
            object.insert("prompt".into(), json!(prompt));
            if model.is_some() || body.tags.is_some() || body.title.is_some() {
                return Err(GatewayFailure::BadRequest("suno_option_not_applicable"));
            }
        }
        OperationKind::Stems => {
            let Some(song) = body.song_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("suno_song_id_required"));
            };
            if !bounded_text(song, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("suno_song_id_invalid"));
            }
            song_id = Some(song.to_string());
            // The split kind is a published price dimension (50/10/10, manifest §5.1) with NO
            // documented wire selector: admitting one would misprice the reserve against what
            // the provider actually runs, so the body carries nothing and the reserve is the
            // documented conservative bound, settled from the attributed delta.
            if model.is_some() || body.prompt.is_some() || body.tags.is_some() {
                return Err(GatewayFailure::BadRequest("suno_option_not_applicable"));
            }
        }
    }

    let reserve_credits = kind.reserve_credits();
    Ok(AdmittedGeneration {
        kind,
        requested_model: model.map(str::to_owned),
        song_id,
        upstream_body: upstream,
        reserve_credits,
    })
}

#[derive(Clone, Copy, Debug)]
enum GatewayFailure {
    /// The provider refused the session (401/403 after a successful mint): SOFT axis —
    /// quarantine + probe, never a fleet-removing verdict on its own.
    Auth,
    Transport,
    Protocol,
    Capacity,
    LowBalance,
    BadRequest(&'static str),
    Unsupported(&'static str),
    Unavailable(&'static str),
    Upstream(u16),
}

impl GatewayFailure {
    fn verdict(self) -> UpstreamVerdict {
        match self {
            Self::Auth => UpstreamVerdict::AuthRefused,
            Self::Transport | Self::Protocol | Self::Unavailable(_) => UpstreamVerdict::Transport,
            Self::Upstream(status) => classify_status(status),
            Self::Capacity => UpstreamVerdict::QuotaExhausted,
            Self::LowBalance | Self::BadRequest(_) | Self::Unsupported(_) => {
                UpstreamVerdict::ClientError
            }
        }
    }

    fn from_verdict(verdict: UpstreamVerdict, status: u16) -> Self {
        match verdict {
            UpstreamVerdict::AuthRefused => Self::Auth,
            UpstreamVerdict::RateLimitedHard
            | UpstreamVerdict::QuotaExhausted
            | UpstreamVerdict::CaptchaRequired => Self::Capacity,
            UpstreamVerdict::Transport => Self::Transport,
            UpstreamVerdict::Protocol => Self::Protocol,
            UpstreamVerdict::ClientError => Self::Upstream(status),
            UpstreamVerdict::Ok => Self::Protocol,
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Transport => "transport",
            Self::Protocol => "protocol",
            Self::Capacity => "capacity",
            Self::LowBalance => "balance",
            Self::BadRequest(_) => "request",
            Self::Unsupported(_) => "unsupported",
            Self::Unavailable(_) => "unavailable",
            Self::Upstream(_) => "upstream",
        }
    }
}

/// The plane's own error envelope: a static type/message pair — the client never learns about
/// the roster, profiles, proxies, sessions or provider bodies. `Retry-After` only on retryable
/// classes, and the terminal reason rides the response extensions for the server audit
/// middleware.
fn error_response(error: GatewayFailure) -> Response {
    error_response_with_retry(error, 60)
}

fn error_response_with_retry(error: GatewayFailure, capacity_retry_after: i64) -> Response {
    let (status, kind, message, reason, retry_after) = match error {
        GatewayFailure::Auth => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "suno_auth_unavailable",
            Some(2),
        ),
        GatewayFailure::Transport | GatewayFailure::Protocol => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "suno_upstream_unavailable",
            Some(2),
        ),
        GatewayFailure::Capacity => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit",
            "Capacity exhausted. Please try again later.",
            "suno_capacity_exhausted",
            Some(capacity_retry_after.max(1)),
        ),
        GatewayFailure::LowBalance => (
            StatusCode::PAYMENT_REQUIRED,
            "invalid_request",
            "insufficient balance or key spending limit reached for this request",
            "billing_limit",
            None,
        ),
        GatewayFailure::BadRequest(code) => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Invalid request.",
            code,
            None,
        ),
        GatewayFailure::Unsupported(code) => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Unsupported request.",
            code,
            None,
        ),
        GatewayFailure::Unavailable(code) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            code,
            Some(2),
        ),
        GatewayFailure::Upstream(404) => (
            StatusCode::NOT_FOUND,
            "not_found",
            "Not Found",
            "suno_upstream_rejected",
            None,
        ),
        GatewayFailure::Upstream(status) if (400..500).contains(&status) => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The provider rejected the request.",
            "suno_upstream_rejected",
            None,
        ),
        GatewayFailure::Upstream(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "suno_upstream_rejected",
            Some(2),
        ),
    };
    let mut response = json_response(status, json!({"error": {"type": kind, "message": message}}));
    if let Some(secs) = retry_after {
        if let Ok(value) = axum::http::HeaderValue::from_str(&secs.to_string()) {
            response.headers_mut().insert("retry-after", value);
        }
    }
    response
        .extensions_mut()
        .insert(crate::proxy::TerminalErrorReason(reason));
    response
}

fn json_response(status: StatusCode, value: Value) -> Response {
    let mut response = Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec()),
        ))
        .expect("Suno response");
    if let Ok(value) = axum::http::HeaderValue::from_str(&crate::fresh_request_id()) {
        response.headers_mut().insert("x-request-id", value);
    }
    response
}

async fn read_bounded(response: wreq::Response, limit: usize) -> Result<Bytes, GatewayFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
        let chunk = chunk.map_err(|_| GatewayFailure::Transport)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(GatewayFailure::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

// ── axum surface (mounted by `server` only on the dedicated Suno plane) ──

use axum::extract::{ConnectInfo, FromRequest, Multipart, Path, State};
use std::net::SocketAddr;

use crate::proxy::{authorize, Authz};
use crate::state::AppState;
use crate::Metrics;

/// The plane's JSON body cap: bounded generation descriptors, never media. Media arrives as
/// multipart on the upload route with its own limit.
const GENERATION_BODY_LIMIT: usize = 256 * 1024;

/// Shared authorization for the plane's customer surface, mirroring the other planes: the same
/// `authorize` the Anthropic path uses, admin first in memory, then the metered key through the
/// billing authority. Auth always precedes body buffering. Returns the billing input (None for
/// admin) and the owning account id (None for admin).
async fn plane_authz(
    app: &AppState,
    headers: &axum::http::HeaderMap,
    peer: &SocketAddr,
) -> Result<(Option<SunoBillingInput>, Option<String>), Response> {
    match authorize(app, headers, peer).await {
        Authz::Admin { .. } => Ok((None, None)),
        metered @ Authz::Metered { .. } => {
            let Authz::Metered {
                account_id,
                key,
                available_nano,
                ..
            } = &metered
            else {
                unreachable!("matched above");
            };
            Ok((
                Some(SunoBillingInput {
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: metered.mult_for(registry::PROVIDER_SUNO),
                    available_nano: *available_nano,
                }),
                Some(account_id.clone()),
            ))
        }
        Authz::Unauthorized => Err(json_response(
            StatusCode::UNAUTHORIZED,
            json!({"error": {"type": "authentication", "message": "invalid x-api-key"}}),
        )),
        Authz::Unavailable => Err(error_response(GatewayFailure::Unavailable(
            "suno_auth_authority_unavailable",
        ))),
    }
}

fn gateway_or_404(app: &AppState) -> Result<Arc<SunoGateway>, Response> {
    app.suno.clone().ok_or_else(|| {
        json_response(
            StatusCode::NOT_FOUND,
            json!({"error": {"type": "not_found", "message": "Not Found"}}),
        )
    })
}

/// `POST /v1/audio/generations` — create one admitted generation. The response names OUR
/// generation id (the internal request id); the lifecycle then completes detached.
pub async fn create_generation(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let (parts, body) = request.into_parts();
    let gateway = match gateway_or_404(&app) {
        Ok(gateway) => gateway,
        Err(response) => return response,
    };
    let (billing, _) = match plane_authz(&app, &parts.headers, &peer).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    Metrics::inc(&app.metrics.suno_requests);
    let execution = match crate::execution::parse_execution_attempt(&parts.headers) {
        Ok(execution) => execution,
        Err(_) => return error_response(GatewayFailure::BadRequest("suno_execution_identity")),
    };
    let raw = match axum::body::to_bytes(body, GENERATION_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => return error_response(GatewayFailure::BadRequest("suno_body_too_large")),
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => return error_response(GatewayFailure::BadRequest("suno_invalid_body")),
    };
    let response = gateway.handle_create(value, execution, billing).await;
    instrument_response(&app, &response);
    response
}

/// One instrumentation point per surface: count the request once it was admitted to the
/// gateway, and classify the failure by its static terminal reason.
fn instrument_response(app: &AppState, response: &Response) {
    if !response.status().is_success() {
        Metrics::inc(&app.metrics.suno_failures);
        if response
            .extensions()
            .get::<crate::proxy::TerminalErrorReason>()
            .is_some_and(|reason| reason.0 == "suno_capacity_exhausted")
        {
            Metrics::inc(&app.metrics.suno_capacity_exhausted);
        }
    }
}

/// `POST /v1/audio/uploads` — one customer audio binary (multipart `file` field, ≤96 MiB)
/// persisted durably on our storage. The id is referenceable in generation `attachments`,
/// which currently fail closed (no documented upstream upload endpoint, manifest §4/§6).
pub async fn upload_audio(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    request: axum::extract::Request,
) -> Response {
    let gateway = match gateway_or_404(&app) {
        Ok(gateway) => gateway,
        Err(response) => return response,
    };
    let (parts, body) = request.into_parts();
    let (_, _) = match plane_authz(&app, &parts.headers, &peer).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    Metrics::inc(&app.metrics.suno_requests);
    let bytes = match read_multipart_file(body, &app, UPLOAD_MAX_BYTES).await {
        Ok(bytes) => bytes,
        Err(response) => return response,
    };
    let response = gateway.handle_audio_upload(bytes).await;
    instrument_response(&app, &response);
    response
}

/// Read the single `file` field of a multipart body, bounded. Any other shape is a 400.
async fn read_multipart_file(
    body: Body,
    app: &AppState,
    max_bytes: usize,
) -> Result<Bytes, Response> {
    let request = axum::http::Request::new(body);
    let mut multipart = Multipart::from_request(request, app)
        .await
        .map_err(|_| error_response(GatewayFailure::BadRequest("suno_multipart_invalid")))?;
    let mut found: Option<Bytes> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() != Some("file") {
            continue;
        }
        if found.is_some() {
            return Err(error_response(GatewayFailure::BadRequest(
                "suno_multipart_invalid",
            )));
        }
        let bytes = field
            .bytes()
            .await
            .map_err(|_| error_response(GatewayFailure::BadRequest("suno_multipart_invalid")))?;
        if bytes.is_empty() || bytes.len() > max_bytes {
            return Err(error_response(GatewayFailure::BadRequest(
                "suno_upload_too_large",
            )));
        }
        found = Some(bytes);
    }
    found.ok_or_else(|| error_response(GatewayFailure::BadRequest("suno_file_field_required")))
}

/// `GET /v1/audio/generations/{id}` — the status of OUR generation record. Per-account
/// isolation: a foreign id is indistinguishable from an unknown one.
pub async fn generation_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(generation_id): Path<String>,
    headers: axum::http::HeaderMap,
) -> Response {
    let gateway = match gateway_or_404(&app) {
        Ok(gateway) => gateway,
        Err(response) => return response,
    };
    let (_, requester) = match plane_authz(&app, &headers, &peer).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    match gateway.generation_view(&generation_id, requester.as_deref()) {
        Some(view) => json_response(
            StatusCode::OK,
            json!({
                "generation_id": view.generation_id,
                "operation": view.operation,
                "status": view.status,
                "artifacts": view.artifacts,
                "created_at": view.created_at,
                "updated_at": view.updated_at,
                "error": view.error,
            }),
        ),
        None => json_response(
            StatusCode::NOT_FOUND,
            json!({"error": {"type": "not_found", "message": "Not Found"}}),
        ),
    }
}

/// `GET /v1/audio/generations/{id}/artifact/{name}` — the stored artifact bytes from OUR
/// artifact store. The upstream media URL is never exposed (manifest §4).
pub async fn generation_artifact(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((generation_id, name)): Path<(String, String)>,
    headers: axum::http::HeaderMap,
) -> Response {
    let gateway = match gateway_or_404(&app) {
        Ok(gateway) => gateway,
        Err(response) => return response,
    };
    let (_, requester) = match plane_authz(&app, &headers, &peer).await {
        Ok(pair) => pair,
        Err(response) => return response,
    };
    let Some(path) = gateway.generation_artifact_path(&generation_id, &name, requester.as_deref())
    else {
        return json_response(
            StatusCode::NOT_FOUND,
            json!({"error": {"type": "not_found", "message": "Not Found"}}),
        );
    };
    let file = match tokio::fs::File::open(&path).await {
        Ok(file) => file,
        Err(_) => {
            // The record named an artifact whose file is gone: a storage incident, not a 404
            // the customer caused.
            return error_response(GatewayFailure::Unavailable("suno_artifact_unavailable"));
        }
    };
    // Stream bounded chunks: the file is ≤ ARTIFACT_MAX_BYTES by construction, and streaming
    // keeps a long audio file from becoming a memory spike.
    let stream = futures_util::stream::try_unfold(file, |mut file| async move {
        let mut buffer = vec![0u8; 64 * 1024];
        match tokio::io::AsyncReadExt::read(&mut file, &mut buffer).await {
            Ok(0) => Ok(None),
            Ok(read) => {
                buffer.truncate(read);
                Ok(Some((Bytes::from(buffer), file)))
            }
            Err(error) => Err(error),
        }
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/octet-stream")
        .body(Body::from_stream(stream))
        .expect("Suno artifact response")
}

/// The stored path of a customer upload, for the (currently fail-closed) attachment path.
pub(crate) fn stored_upload_path(app: &AppState, upload_id: &str) -> Option<PathBuf> {
    let gateway = app.suno.as_ref()?;
    upload_path(&gateway.config.artifact_dir, upload_id)
}

#[cfg(test)]
mod tests;
