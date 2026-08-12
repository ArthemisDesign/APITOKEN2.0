//! Live Tripo3D (VAST / Holymolly) task-lifecycle gateway.
//!
//! Tripo3D is a task-based media API, not a chat protocol (manifest §4), so this gateway owns
//! the whole upstream task lifecycle: reserve → select profile → create task → detached
//! poll → immediate artifact download (result URLs live ≤60 s) → exact settlement from the
//! authoritative `consumed_credit` → immutable turn event through the bounded FIFO → paired
//! post-turn balance observation. The money boundary is a successful upstream task creation
//! (`code: 0 + task_id`): rotation across profiles is legal only before it, and after it the
//! task is owned by the creating profile (per-key isolation, manifest §2) — a client disconnect
//! never cancels the drain.
//!
//! Calibration follows the Codex/Gemini pairing discipline (immutable per-turn evidence plus a
//! quota read taken in the turn's wake, persisted by one writer command whose cumulative
//! ledgers already include that turn), NOT the split KIMI/GLM ordering. The free periodic
//! balance poll still runs the turn-before-quota gate: it never reads with a pending FIFO head.
//!
//! Contract: `docs/engine/TRIPO3D_PROVIDER.md` §4/§5 and `docs/engine/PROVIDER_ONBOARDING.md`
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
use metering::tripo3d::{
    tripo3d_animate_retarget_credits, tripo3d_convert_model_credits, tripo3d_cost_nanodollars,
    tripo3d_edit_multiview_image_credits, tripo3d_reserve_credits, Tripo3dConvertMode,
    Tripo3dGeometryQuality, Tripo3dOptions, Tripo3dTaskKind, Tripo3dTextureQuality,
    TRIPO3D_TARIFF_SCHEDULE_ID,
};
use registry::{ExecutionAttempt, Tripo3dTurnCalibrationEvent};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};
use tripo3d_credential::{Tripo3dCredential, TRIPO3D_UPLOAD_STS_PATH};

use crate::billing::{AsyncBilling, Tripo3dBalanceSnapshot};
use crate::proxy::HoldGuard;
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};

use super::artifacts::{artifact_path, store_artifact};
use super::client::{self, BalanceProbe, TaskLifecycle, TaskState};
use super::config::{readiness, NotReady, Tripo3dPlaneConfig};
use super::pool::{decide, AttemptPolicy, NextStep, Phase, ProfileEffect};
use super::queue::{DeliveryHealth, PendingTurn, TurnQueue, WriteOutcome, DEFAULT_QUEUE_CAPACITY};
use super::roster::{load_roster, load_roster_for_reload, Tripo3dProfile};
use super::selection::{select, select_ignoring_soft, Candidate, Hard, Soft};
use super::transport::{
    classify_status, error_business_code, parse_retry_after, probe_url, task_create_url,
    task_poll_url, ProbeRoute, UpstreamVerdict,
};
use super::upload::{
    build_multipart_body, fresh_boundary, s3_put_object, sniff_image_format, ModelFormat,
    IMAGE_UPLOAD_MAX_BYTES, MODEL_UPLOAD_MAX_BYTES,
};

const ERROR_BODY_LIMIT: usize = 64 * 1024;
/// Create/poll/balance JSON answers are small; anything larger is a contract anomaly.
const RESPONSE_BODY_LIMIT: usize = 1024 * 1024;
/// The documented poll cadence (manifest §4 example): 2 s.
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// A task's total drain is bounded: on expiry the reservation stays with the reconciler under
/// its lease rather than polling forever.
const POLL_DEADLINE: Duration = Duration::from_secs(1_800);
/// The reservation lease covers the whole bounded drain with headroom.
const RESERVATION_LEASE_SECS: i64 = 3_600;
const TRANSPORT_COOL_SECS: i64 = 10;
/// Soft auth axis: exponential backoff from a small base, reset on proven success
/// (PROVIDER_ONBOARDING §8.4). Flat, long cooling is what turns one bad wave into an outage.
const AUTH_SOFT_BASE_SECS: i64 = 15;
const AUTH_SOFT_MAX_SECS: i64 = 900;
/// Fallback cooling for a 429+`2000` wall that named no parseable `Retry-After`. Bounded on
/// purpose: the next balance probe re-proves the profile.
const RATE_LIMIT_FALLBACK_COOL_SECS: i64 = 30;
/// In-memory pin map of uploaded tokens to their owning profile (uploads are account-scoped,
/// manifest §2). Bounded; entries expire after one hour — a task created long after its upload
/// may resolve unpinned, which surfaces the provider's own per-key refusal honestly.
const TOKEN_PIN_CAPACITY: usize = 4_096;
const TOKEN_PIN_TTL_SECS: i64 = 3_600;
/// The tracked-task projection is a read model; money never depends on it. The bound evicts
/// the oldest FINALIZED record first (live tasks are never evicted while draining).
const TASK_TRACK_CAPACITY: usize = 65_536;

/// Customer money context already authenticated by the plane's shared authorization.
#[derive(Clone)]
pub(crate) struct Tripo3dBillingInput {
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub available_nano: i64,
}

/// Customer-facing read model of one task record. Only our own bounded fields; the upstream
/// task id, profile id and signed URLs never appear (the upstream task id is audit metadata in
/// the durable calibration event, not a serving identity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dTaskView {
    /// Our internal request id — the public task identity of this plane.
    pub task_id: String,
    pub task_type: String,
    /// `queued`/`running`/`success`/`failed`/`expired` or the provider's other finalized
    /// states; `created` until the first poll answer.
    pub status: String,
    pub progress: Option<i64>,
    /// Artifact names fetchable through `GET /v1/3d/tasks/{task_id}/artifact/{name}`.
    pub artifacts: Vec<String>,
    pub created_at: i64,
    pub updated_at: i64,
    /// Bounded terminal class, never provider text.
    pub error: Option<&'static str>,
}

/// Per-profile operational projection for readiness, metrics and the admin endpoint.
///
/// Privacy by construction: the opaque roster id and the cohort are the only identities this
/// struct can carry. The subject, API key, proxy, credential paths and raw provider errors
/// never enter it — `RuntimeProfile.subject_id` stays private to the gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dProfileStatus {
    /// Opaque roster id. Documented safe for logs, metrics and admin projections.
    pub id: String,
    /// Declared top-up cohort (bounded, lowercase-normalized by the credential).
    pub cohort: String,
    /// Authenticated and serving (a balance probe passed on this runtime generation).
    pub live: bool,
    /// Cooling axes as unix seconds; `None` means the axis is not cooling right now.
    pub rate_limit_cool_until: Option<i64>,
    /// HARD balance verdict (403 + code 2010): resting until a balance probe shows funds.
    pub balance_walled: bool,
    /// SOFT axes with their deadlines.
    pub auth_cool_until: Option<i64>,
    pub transport_cool_until: Option<i64>,
    pub inflight: u32,
    /// Latest balance evidence: raw halves verbatim; the parsed halves stay `None` while the
    /// unit is unproven (manifest §5.2) — unknown is `None`, never `0`.
    pub balance_observed_at: Option<i64>,
    pub balance_raw: Option<String>,
    pub frozen_raw: Option<String>,
    pub balance_micro_units: Option<i64>,
    pub frozen_micro_units: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dOperationalStatus {
    pub total_profiles: usize,
    pub live_profiles: usize,
    /// Eligible right now under the strict pass (no hard and no soft axis active).
    pub available_profiles: usize,
    pub rate_limited_profiles: usize,
    pub balance_walled_profiles: usize,
    pub auth_cooling_profiles: usize,
    pub transport_cooling_profiles: usize,
    pub inflight_requests: u64,
    /// Detached drains currently polling/downloading/settling.
    pub inflight_drains: u64,
    pub tracked_tasks: usize,
    /// Finalized successful tasks whose authoritative `consumed_credit` never arrived. Each one
    /// settled on the documented conservative hold; no synthetic consumption was created.
    pub missing_consumed_credit: u64,
    /// Settlements whose `consumed_credit` exceeded the admitted shape's reserve bound — a
    /// typed anomaly (quarantine), never silent acceptance.
    pub tariff_anomaly: u64,
    /// Finalized tasks in the undocumented `banned`/`cancelled`/`unknown` states: refund
    /// semantics are unpublished (manifest §6.5), so each settled on the conservative hold.
    pub undocumented_final: u64,
    /// Artifact downloads that failed inside the ≤60 s URL TTL window.
    pub artifact_failures: u64,
    /// Wall time of the last completed per-profile balance sweep, milliseconds.
    pub balance_sweep_ms: u64,
    pub profiles: Vec<Tripo3dProfileStatus>,
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
    balance_walled: bool,
    /// SOFT axes (our own inferences; never deny admission on their own).
    auth_cool_until: i64,
    auth_soft_streak: u32,
    transport_cool_until: i64,
    /// Latest balance evidence; parsed halves stay `None` while the unit is unproven.
    balance_raw: Option<String>,
    frozen_raw: Option<String>,
    balance_micro_units: Option<i64>,
    frozen_micro_units: Option<i64>,
    balance_observed_at: Option<i64>,
}

struct RuntimeProfile {
    id: String,
    subject_id: String,
    /// Declared top-up cohort — the calibration cohort key.
    cohort: String,
    /// Per-profile platform origin from the sealed credential. Keys are only valid against the
    /// platform that issued them.
    base_url: String,
    credential_key_id: String,
    /// Sealed material, held in memory only. Plain field on purpose: the static key has no
    /// refresh family, so nothing mutates it between roster generations.
    credential: Tripo3dCredential,
    client: wreq::Client,
    health: Mutex<ProfileHealth>,
    inflight: AtomicU32,
    /// Monotonic start boundary for balance polls. If any customer lease starts while the
    /// balance GET is in flight, that snapshot is discarded even when the turn finishes before
    /// it returns.
    turn_epoch: AtomicU64,
}

impl RuntimeProfile {
    fn from_roster(profile: Tripo3dProfile, config: &Tripo3dPlaneConfig) -> anyhow::Result<Arc<Self>> {
        let client = client::build_client(
            &profile.credential.proxy_url,
            Duration::from_secs(10),
            config.transport.request_timeout,
        )?;
        Ok(Arc::new(Self {
            id: profile.id,
            subject_id: profile.subject_id,
            cohort: profile.cohort,
            base_url: profile.base_url,
            credential_key_id: profile.credential_key_id,
            credential: profile.credential,
            client,
            health: Mutex::new(ProfileHealth::default()),
            inflight: AtomicU32::new(0),
            turn_epoch: AtomicU64::new(0),
        }))
    }

    fn candidate(&self, now: i64, reserve_credits: i64) -> Candidate {
        let health = self.health.lock().expect("Tripo3D profile health lock");
        // The axes stay deliberately separate (PROVIDER_ONBOARDING §8.4): hard provider
        // verdicts, soft inferences and proven balance shortfall clear independently.
        let hard = if health.rate_limit_cool_until > now {
            Some(Hard::RateLimited)
        } else if health.balance_walled {
            Some(Hard::BalanceWall)
        } else if balance_shortfall(&health, reserve_credits) {
            Some(Hard::BalanceShortfall)
        } else {
            None
        };
        let soft = if health.auth_cool_until > now {
            Some(Soft::AuthCooling)
        } else if health.transport_cool_until > now {
            Some(Soft::TransportWedged)
        } else {
            None
        };
        Candidate {
            profile_id: self.id.clone(),
            hard,
            soft,
            inflight: self.inflight.load(Ordering::Acquire),
        }
    }

    fn apply_effect(&self, effect: ProfileEffect, now: i64, retry_after: Option<i64>) {
        let mut health = self.health.lock().expect("Tripo3D profile health lock");
        match effect {
            ProfileEffect::None => {}
            ProfileEffect::CoolRateLimited => {
                // The exact `Retry-After` the provider named, else a bounded guess the next
                // probe replaces.
                health.rate_limit_cool_until = retry_after
                    .filter(|until| *until > now)
                    .unwrap_or_else(|| now.saturating_add(RATE_LIMIT_FALLBACK_COOL_SECS));
            }
            ProfileEffect::RestForBalance => {
                // A money verdict: no timer clears it, only a balance probe showing funds.
                health.balance_walled = true;
            }
            ProfileEffect::SoftAuthFault => {
                health.auth_soft_streak = health.auth_soft_streak.saturating_add(1);
                let shift = health.auth_soft_streak.min(6);
                let backoff = AUTH_SOFT_BASE_SECS
                    .saturating_mul(1i64 << shift)
                    .min(AUTH_SOFT_MAX_SECS);
                health.auth_cool_until = now.saturating_add(backoff);
            }
            ProfileEffect::TransportFault => {
                health.transport_cool_until = now.saturating_add(TRANSPORT_COOL_SECS);
            }
        }
    }

    /// A successful task creation is proven success: it rehabilitates the soft axes. The hard
    /// axes deliberately survive — only their own evidence clears them.
    fn mark_task_success(&self) {
        let mut health = self.health.lock().expect("Tripo3D profile health lock");
        health.authenticated = true;
        health.auth_cool_until = 0;
        health.auth_soft_streak = 0;
        health.transport_cool_until = 0;
    }

    /// A passing free balance probe is auth/capacity evidence. It clears the soft axes and the
    /// balance wall when the reading shows funds; the rate-limit axis clears only on its own
    /// clock (the wall was time-boxed by the provider, not by balance).
    fn publish_balance(&self, snapshot: &BalanceSnapshotView, observed_at: i64) {
        let mut health = self.health.lock().expect("Tripo3D profile health lock");
        health.authenticated = true;
        health.auth_cool_until = 0;
        health.auth_soft_streak = 0;
        health.transport_cool_until = 0;
        if balance_shows_funds(&snapshot.balance_raw) {
            health.balance_walled = false;
        }
        health.balance_raw = Some(snapshot.balance_raw.clone());
        health.frozen_raw = Some(snapshot.frozen_raw.clone());
        health.balance_micro_units = snapshot.balance_micro_units;
        health.frozen_micro_units = snapshot.frozen_micro_units;
        health.balance_observed_at = Some(observed_at);
    }

    fn authenticated(&self) -> bool {
        self.health
            .lock()
            .expect("Tripo3D profile health lock")
            .authenticated
    }

    fn matches_roster(&self, profile: &Tripo3dProfile) -> bool {
        self.id == profile.id
            && self.subject_id == profile.subject_id
            && self.cohort == profile.cohort
            && self.credential_key_id == profile.credential_key_id
            && credentials_match(&self.credential, &profile.credential)
    }
}

/// Whether the latest PROVEN balance observation cannot cover the reserve — `balance − frozen`
/// below the reserve in micro-units (1 credit = 1e6 micro-units of the proven unit — a credit).
/// Both halves unknown → inert (never a shortfall from unproven units).
fn balance_shortfall(health: &ProfileHealth, reserve_credits: i64) -> bool {
    let (Some(balance), Some(frozen)) = (health.balance_micro_units, health.frozen_micro_units)
    else {
        return false;
    };
    let remaining = balance.saturating_sub(frozen);
    remaining < reserve_credits.saturating_mul(1_000_000)
}

/// A balance reading shows funds when its raw decimal is strictly positive. Strict, float-free
/// parsing (digits with at most one `.`); anything unparseable proves nothing. Used to lift the
/// 2010 balance wall: the probe is the provider's own evidence that the account answers with a
/// positive balance.
fn balance_shows_funds(raw: &str) -> bool {
    let (integer, fraction) = match raw.split_once('.') {
        Some((i, f)) => (i, f),
        None => (raw, ""),
    };
    if integer.is_empty()
        || !integer.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return false;
    }
    integer.bytes().any(|b| b != b'0') || fraction.bytes().any(|b| b != b'0')
}

/// The gateway-side view of one balance snapshot: raw halves plus parsed halves (the latter
/// stay `None` while the endpoint's unit is unproven, manifest §5.2).
struct BalanceSnapshotView {
    balance_raw: String,
    frozen_raw: String,
    balance_micro_units: Option<i64>,
    frozen_micro_units: Option<i64>,
}

fn credentials_match(left: &Tripo3dCredential, right: &Tripo3dCredential) -> bool {
    left.version == right.version
        && left.kind == right.kind
        && left.api_key == right.api_key
        && left.cohort == right.cohort
        && left.base_url == right.base_url
        && left.proxy_url == right.proxy_url
}

struct ProfileLease {
    profile: Arc<RuntimeProfile>,
}

impl ProfileLease {
    fn new(profile: Arc<RuntimeProfile>) -> Self {
        // Publish the live lease before its epoch. Pollers read in the opposite order, so every
        // interleaving observes either in-flight work or an epoch change, never a false idle gap.
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

/// One reservation: the conservative hold pinned at admission, settled exactly at task end.
struct Reservation {
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
}

/// What the detached drain needs to finish the task without the client.
struct DrainContext {
    request_id: String,
    task_type: Tripo3dTaskKind,
    requested_model_version: Option<String>,
    reserve_credits: i64,
    priced_ts: i64,
    profile: Arc<RuntimeProfile>,
    reservation: Option<Reservation>,
}

/// The tracked read model of one admitted task. Money never lives here — the reservation and
/// the calibration FIFO are the money state; this is only what `GET /v1/3d/tasks/*` serves.
#[derive(Clone)]
struct TaskRecord {
    task_type: Tripo3dTaskKind,
    /// The creating account; `None` for an admin (unmetered) task. Read isolation keys on it.
    account_id: Option<String>,
    status: &'static str,
    progress: Option<i64>,
    artifacts: Vec<String>,
    created_at: i64,
    updated_at: i64,
    error: Option<&'static str>,
    finalized: bool,
}

/// An uploaded token's owning profile (uploads are account-scoped, manifest §2). For model
/// files the pin also carries the S3 object the task body references.
#[derive(Clone)]
struct TokenPin {
    profile_id: String,
    object: Option<(String, String)>,
    created: i64,
}

/// Default-off live Tripo3D pool. Dedicated plane only (`ProviderMode::Tripo3d`); it has no
/// public catalogue and never rides the Anthropic Messages surface.
pub struct Tripo3dGateway {
    config: Arc<Tripo3dPlaneConfig>,
    profiles: RwLock<Vec<Arc<RuntimeProfile>>>,
    cursor: AtomicU64,
    reload_lock: AsyncMutex<()>,
    billing: Option<Arc<AsyncBilling>>,
    turn_queue: Mutex<TurnQueue>,
    turn_drain: AsyncMutex<()>,
    balance_sweep: AsyncMutex<()>,
    maintenance_abort: Notify,
    background: Arc<ActiveTaskTracker>,
    shutting_down: AtomicBool,
    abort_drains: AtomicBool,
    abort_notify: Notify,
    live_profiles: AtomicUsize,
    tasks: Mutex<HashMap<String, TaskRecord>>,
    token_pins: Mutex<HashMap<String, TokenPin>>,
    missing_consumed_credit: AtomicU64,
    tariff_anomaly: AtomicU64,
    undocumented_final: AtomicU64,
    artifact_failures: AtomicU64,
    balance_sweep_ms: AtomicU64,
}

impl Tripo3dGateway {
    pub fn new_with_calibration(
        config: Tripo3dPlaneConfig,
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
    pub fn new_degraded(config: Tripo3dPlaneConfig, billing: Option<Arc<AsyncBilling>>) -> Self {
        Self::from_profiles(config, billing, Vec::new())
    }

    fn from_profiles(
        config: Tripo3dPlaneConfig,
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
            balance_sweep: AsyncMutex::new(()),
            maintenance_abort: Notify::new(),
            background: Arc::new(ActiveTaskTracker::default()),
            shutting_down: AtomicBool::new(false),
            abort_drains: AtomicBool::new(false),
            abort_notify: Notify::new(),
            live_profiles: AtomicUsize::new(0),
            tasks: Mutex::new(HashMap::new()),
            token_pins: Mutex::new(HashMap::new()),
            missing_consumed_credit: AtomicU64::new(0),
            tariff_anomaly: AtomicU64::new(0),
            undocumented_final: AtomicU64::new(0),
            artifact_failures: AtomicU64::new(0),
            balance_sweep_ms: AtomicU64::new(0),
        }
    }

    fn profiles_snapshot(&self) -> Vec<Arc<RuntimeProfile>> {
        self.profiles.read().expect("Tripo3D profiles lock").clone()
    }

    /// Atomically adopt one fully validated roster generation and retain the last-good snapshot
    /// on every read, decrypt, client-build or balance-probe failure.
    ///
    /// Unchanged profiles keep their exact `Arc`, preserving health, in-flight accounting and
    /// HTTP state. Changed/new profiles authenticate through the free balance probe before
    /// publication. A final roster re-read prevents a snapshot that went stale during the probe
    /// from replacing a credential the Auth Bot republished meanwhile. A removed profile closes
    /// to new admission immediately; its in-flight tasks drain on their own `Arc`.
    pub async fn refresh_profiles(&self) -> bool {
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        let _reload = self.reload_lock.lock().await;
        match self.reload_profiles().await {
            Ok(changed) => changed,
            Err(_) => {
                // Do not render the error: malformed proxy URLs and credential envelopes may
                // contain private egress or key material.
                elog::warn(
                    "tripo3d",
                    "Tripo3D encrypted roster refresh skipped; last-good capacity retained",
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

            // Every new or changed credential passes the free balance probe BEFORE it joins the
            // serving generation. A key the provider rejects (401) must not start carrying
            // traffic.
            for profile in needs_probe {
                self.probe_profile(&profile)
                    .await
                    .map_err(|error| anyhow!("Tripo3D reload balance-probe class={}", error.class()))?;
            }

            let verified = self.load_reload_snapshot(!current.is_empty()).await?;
            if !profiles_match_roster(&next, &verified) {
                continue;
            }
            if self.shutting_down.load(Ordering::Acquire) {
                return Ok(false);
            }
            let live = next.iter().filter(|profile| profile.authenticated()).count();
            *self.profiles.write().expect("Tripo3D profiles lock") = next;
            self.live_profiles.store(live, Ordering::Release);
            return Ok(true);
        }
        anyhow::bail!("Tripo3D roster changed repeatedly during reload")
    }

    async fn load_reload_snapshot(
        &self,
        has_last_good_capacity: bool,
    ) -> anyhow::Result<Vec<Tripo3dProfile>> {
        let root = self.config.roster_dir.clone();
        let keyring = self.config.keyring.clone();
        tokio::task::spawn_blocking(move || {
            load_roster_for_reload(&root, &keyring, has_last_good_capacity)
        })
        .await
        .map_err(|_| anyhow!("Tripo3D roster reader stopped"))?
    }

    /// Startup validation: open the keyring roster and balance-probe every profile. A profile
    /// whose key is rejected is soft-quarantined on its own — one bad key never takes the rest
    /// of the fleet, let alone the whole gateway, down with it.
    pub async fn preflight(&self) -> usize {
        let mut live = 0usize;
        for profile in self.profiles_snapshot() {
            match self.probe_profile(&profile).await {
                Ok(()) => live += 1,
                Err(error) => {
                    let effect = match error.verdict() {
                        UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
                        UpstreamVerdict::Transport | UpstreamVerdict::Protocol => {
                            ProfileEffect::TransportFault
                        }
                        UpstreamVerdict::RateLimitedHard => ProfileEffect::CoolRateLimited,
                        UpstreamVerdict::InsufficientBalance => ProfileEffect::RestForBalance,
                        UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
                    };
                    profile.apply_effect(effect, now_unix(), None);
                    // Classification only: never print provider bodies, subject, proxy or keys.
                    elog::warn(
                        "tripo3d",
                        format!(
                            "Tripo3D balance preflight failed profile={} class={}",
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

    pub fn balance_poll_interval(&self) -> Duration {
        self.config.balance_poll_interval
    }

    /// Poll every currently published idle profile. Any lease that starts while the balance GET
    /// is in flight invalidates that profile's snapshot; customer traffic is never queued or
    /// rejected merely because maintenance is reading balance.
    pub async fn poll_balances(&self) -> usize {
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let started = std::time::Instant::now();
        let _sweep = self.balance_sweep.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let published = self.poll_balance_generation(false).await;
        self.balance_sweep_ms.store(
            started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            Ordering::Release,
        );
        published
    }

    async fn poll_balance_generation(&self, during_shutdown: bool) -> usize {
        let mut published = 0usize;
        for profile in self.profiles_snapshot() {
            if !during_shutdown && self.shutting_down.load(Ordering::Acquire) {
                break;
            }
            if self.poll_profile_balance(&profile, during_shutdown).await {
                published += 1;
            }
        }
        published
    }

    /// One idle-only balance read with the turn-before-quota ordering: drain the bounded turn
    /// FIFO, read the provider snapshot, re-check the generation epoch, drain again under the
    /// FIFO barrier, then let the serial writer pair cumulative dual-ledger spend with the
    /// observation/CAS. Balance steering publishes only after the observation is durable.
    async fn poll_profile_balance(
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

        let snapshot = match self.fetch_balance(profile, during_shutdown).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let effect = match error.verdict() {
                    UpstreamVerdict::AuthRefused => ProfileEffect::SoftAuthFault,
                    UpstreamVerdict::RateLimitedHard => ProfileEffect::CoolRateLimited,
                    UpstreamVerdict::InsufficientBalance => ProfileEffect::RestForBalance,
                    UpstreamVerdict::Transport | UpstreamVerdict::Protocol => {
                        ProfileEffect::TransportFault
                    }
                    UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
                };
                profile.apply_effect(effect, now_unix(), None);
                self.refresh_live_profile_count();
                elog::warn(
                    "tripo3d",
                    format!(
                        "Tripo3D balance poll failed profile={} class={}",
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
            // The provider snapshot can already include that task while its durable spend is not
            // yet paired. Discard the whole read; the next idle poll will observe both.
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
            .observe_tripo3d_balance(
                &profile.subject_id,
                &profile.cohort,
                Tripo3dBalanceSnapshot {
                    observed_at,
                    balance_raw: snapshot.balance_raw.clone(),
                    frozen_raw: snapshot.frozen_raw.clone(),
                    balance_micro_units: snapshot.balance_micro_units,
                    frozen_micro_units: snapshot.frozen_micro_units,
                },
            )
            .await
        {
            elog::error(
                "tripo3d",
                format!(
                    "Tripo3D balance observation persistence deferred profile={}: {error:#}",
                    profile.id
                ),
            );
            return false;
        }

        // Steering sees a snapshot only after the observation is durable. A transient
        // turn/observation/CAS failure therefore retains the exact previous balance generation.
        profile.publish_balance(&snapshot, observed_at);
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

    /// Free balance read used by preflight and roster reload: the key is valid or it is not.
    async fn probe_profile(&self, profile: &Arc<RuntimeProfile>) -> Result<(), GatewayFailure> {
        let snapshot = self.fetch_balance(profile, false).await?;
        let observed_at = now_unix();
        // A probe during reload/preflight publishes steering directly: it carries no durable
        // observation (those come only from the FIFO-gated poll path), exactly so a probe can
        // never pair balance with a stale spend total.
        profile.publish_balance(&snapshot, observed_at);
        Ok(())
    }

    /// GET the provider's free balance endpoint. Authentication is the documented
    /// `Bearer tsk_…`; an invalid key is a documented 401 (a `tcli_` Client ID lands here).
    async fn fetch_balance(
        &self,
        profile: &Arc<RuntimeProfile>,
        during_shutdown: bool,
    ) -> Result<BalanceSnapshotView, GatewayFailure> {
        let send = profile
            .client
            .get(probe_url(&profile.base_url, ProbeRoute::Balance))
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
            )
            .header("accept", "application/json")
            .send();
        tokio::pin!(send);
        let response = if during_shutdown {
            send.await
        } else {
            tokio::select! {
                response = &mut send => response,
                _ = self.maintenance_shutdown_requested() => {
                    return Err(GatewayFailure::Unavailable("tripo3d_shutdown"));
                }
            }
        }
        .map_err(|_| GatewayFailure::Transport)?;
        let status = response.status().as_u16();
        let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
        match client::parse_balance_probe(status, &body) {
            Ok(BalanceProbe::Valid(snapshot)) => Ok(BalanceSnapshotView {
                balance_raw: snapshot.balance_raw,
                frozen_raw: snapshot.frozen_raw,
                // The endpoint's unit is unproven (manifest §5.2/§6.1): the parsed halves stay
                // None until a live run proves it — unknown is None, never 0.
                balance_micro_units: None,
                frozen_micro_units: None,
            }),
            Ok(BalanceProbe::Invalid) => Err(GatewayFailure::Auth),
            Err(_) => {
                // A parser refusal is a contract change or an HTTP-level failure: classify from
                // the business code when one is readable and fail closed, never trusting data.
                let code = serde_json::from_slice::<Value>(&body)
                    .ok()
                    .as_ref()
                    .and_then(error_business_code);
                Err(GatewayFailure::from_verdict(
                    classify_status(status, code),
                    status,
                ))
            }
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

// ── serving: admission → create → detached drain → exact settlement ─────────

impl Tripo3dGateway {
    /// The profile an uploaded token belongs to (uploads are account-scoped). An unknown or
    /// expired token resolves to no pin — the task then runs unpinned and the provider's own
    /// per-key refusal answers honestly.
    fn profile_for_token(&self, token: &str) -> Option<String> {
        let now = now_unix();
        self.token_pins
            .lock()
            .expect("Tripo3D token pins lock")
            .get(token)
            .filter(|pin| now - pin.created <= TOKEN_PIN_TTL_SECS)
            .map(|pin| pin.profile_id.clone())
    }

    /// The S3 object behind a plane-issued model token, with its owning profile.
    fn model_object_for_token(&self, token: &str) -> Option<(String, String, String)> {
        let now = now_unix();
        let pin = self
            .token_pins
            .lock()
            .expect("Tripo3D token pins lock")
            .get(token)
            .filter(|pin| now - pin.created <= TOKEN_PIN_TTL_SECS)
            .cloned()?;
        let (bucket, key) = pin.object?;
        Some((pin.profile_id, bucket, key))
    }

    fn pin_token(&self, token: String, pin: TokenPin) {
        let mut pins = self.token_pins.lock().expect("Tripo3D token pins lock");
        if pins.len() >= TOKEN_PIN_CAPACITY && !pins.contains_key(&token) {
            // Evict the oldest entry: pins are an affinity hint, never money state.
            if let Some(oldest) = pins
                .iter()
                .min_by_key(|(_, pin)| pin.created)
                .map(|(token, _)| token.clone())
            {
                pins.remove(&oldest);
            }
        }
        pins.insert(token, pin);
    }

    /// Strict-then-relaxed selection (PROVIDER_ONBOARDING §8.4): the strict pass honors both
    /// cooling axes; when it is empty the relaxed pass ignores only the SOFT axis, so a
    /// full-soft fleet still serves while a full-hard fleet honestly selects nothing.
    fn select_profile(
        &self,
        excluded: &HashSet<String>,
        sticky: Option<&str>,
        reserve_credits: i64,
    ) -> Option<Arc<RuntimeProfile>> {
        let now = now_unix();
        let profiles = self.profiles_snapshot();
        let candidates = profiles
            .iter()
            .filter(|profile| !excluded.contains(&profile.id))
            .map(|profile| profile.candidate(now, reserve_credits))
            .collect::<Vec<_>>();
        // Upload affinity wins outright while the pinned profile is hard-eligible: the token
        // exists only on that account, so any other profile would fail the task outright.
        if let Some(pinned_id) = sticky {
            if let Some(pinned) = candidates
                .iter()
                .find(|candidate| candidate.profile_id == pinned_id && candidate.hard.is_none())
            {
                let id = pinned.profile_id.clone();
                return profiles.into_iter().find(|profile| profile.id == id);
            }
            return None;
        }
        let cursor = self.cursor.fetch_add(1, Ordering::Relaxed);
        let selected = select(&candidates, cursor)
            .or_else(|| select_ignoring_soft(&candidates, cursor))?;
        profiles
            .into_iter()
            .find(|profile| profile.id == selected.profile_id)
    }

    /// Profiles still tryable after `current` under the same pass selection would use next:
    /// strict-eligible first, then merely hard-eligible. Zero means real provider limits — the
    /// honest 429, never an invented 503.
    fn remaining_count(&self, excluded: &HashSet<String>, current: &str, reserve_credits: i64) -> usize {
        let now = now_unix();
        let candidates = self
            .profiles_snapshot()
            .iter()
            .filter(|profile| profile.id != current && !excluded.contains(&profile.id))
            .map(|profile| profile.candidate(now, reserve_credits))
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
                let health = profile.health.lock().expect("Tripo3D profile health lock");
                active_cooling(health.rate_limit_cool_until, now)
            })
            .min()
            .map(|until| (until - now).max(1))
            .unwrap_or(60)
    }

    /// Reserve the conservative hold for one admitted task. The provider id and the multiplier
    /// pin in the same money transaction, so a concurrent admin edit cannot reprice an
    /// in-flight task. `None` billing input is the admin (unmetered) path — still metered into
    /// the calibration evidence, never into the customer ledger.
    async fn reserve_customer(
        &self,
        admitted: &AdmittedTask,
        request_id: &str,
        priced_ts: i64,
        input: Option<&Tripo3dBillingInput>,
        execution: ExecutionAttempt,
    ) -> Result<Option<Reservation>, GatewayFailure> {
        let Some(input) = input else {
            return Ok(None);
        };
        let billing = self.billing.as_ref().ok_or(GatewayFailure::Unavailable(
            "tripo3d_billing_authority_unavailable",
        ))?;
        let raw = tripo3d_cost_nanodollars(i128::from(admitted.reserve_credits))
            .map_err(|_| GatewayFailure::Unavailable("tripo3d_price_overflow"))?;
        // The hold is the reserve priced with the customer's multiplier, clamped to the ledger
        // width; a zero multiplier is free but still metered.
        let hold = metering::apply_multiplier(raw, input.mult_bp).clamp(0, i128::from(i64::MAX)) as i64;
        let mut balance = i128::from(input.available_nano);
        for _ in 0..4 {
            if i128::from(hold) > balance + metering::OVERDRAFT_NANO {
                return Err(GatewayFailure::LowBalance);
            }
            match billing
                .reserve_priced_request_for_execution(
                    request_id,
                    &input.account_id,
                    &input.key,
                    hold,
                    execution.clone(),
                    registry::PROVIDER_TRIPO3D,
                    input.mult_bp,
                )
                .await
                .map_err(|error| {
                    elog::error("tripo3d", "Tripo3D reservation failed");
                    let _ = error;
                    GatewayFailure::Unavailable("tripo3d_reservation_unavailable")
                })? {
                Some(_) => {
                    return Ok(Some(Reservation {
                        request_id: request_id.to_string(),
                        account_id: input.account_id.clone(),
                        key: input.key.clone(),
                        hold,
                        mult_bp: input.mult_bp,
                        priced_ts,
                    }));
                }
                None => {
                    balance = billing
                        .account(&input.account_id)
                        .await
                        .map_err(|error| {
                            elog::error("tripo3d", "Tripo3D balance read failed");
                            let _ = error;
                            GatewayFailure::Unavailable("tripo3d_balance_unavailable")
                        })?
                        .map(|account| i128::from(account.balance_nano))
                        .unwrap_or(0);
                }
            }
        }
        Err(GatewayFailure::LowBalance)
    }

    /// One admitted generation request: validate → reserve → create → deliver the task handle.
    /// The response is returned the moment the upstream task exists; the poll/download/settle
    /// lifecycle then runs detached and never depends on the client staying connected.
    pub(crate) async fn handle_create(
        self: &Arc<Self>,
        body: Value,
        execution: ExecutionAttempt,
        billing: Option<Tripo3dBillingInput>,
    ) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("tripo3d_shutdown"));
        }
        let parsed: GenerationBody = match serde_json::from_value(body) {
            Ok(parsed) => parsed,
            Err(_) => return error_response(GatewayFailure::BadRequest("tripo3d_invalid_body")),
        };
        let admitted = match admit_task(self, parsed) {
            Ok(admitted) => admitted,
            Err(error) => return error_response(error),
        };
        let request_id = crate::upstream::fresh_request_id();
        let priced_ts = now_unix();
        let mut reservation = match self
            .reserve_customer(&admitted, &request_id, priced_ts, billing.as_ref(), execution)
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
            Err(_) => return error_response(GatewayFailure::BadRequest("tripo3d_invalid_body")),
        };

        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();
        loop {
            let Some(profile) = self.select_profile(
                &excluded,
                admitted.sticky_profile.as_deref(),
                admitted.reserve_credits,
            ) else {
                elog::warn("tripo3d", "tripo3d pool exhausted: no profile");
                return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
            };
            let lease = ProfileLease::new(profile.clone());
            let response = match self.send_create(&profile, body_bytes.clone()).await {
                Ok(response) => response,
                Err(error) => {
                    elog::error("tripo3d", format!("tripo3d upstream transport failed: {error:?}"));
                    let remaining =
                        self.remaining_count(&excluded, &profile.id, admitted.reserve_credits);
                    let decision = decide(UpstreamVerdict::Transport, Phase::BeforeCreate, policy, remaining);
                    profile.apply_effect(decision.effect, now_unix(), None);
                    policy = decision.policy;
                    if decision.next == NextStep::RotateToAnotherProfile {
                        excluded.insert(profile.id.clone());
                        drop(lease);
                        continue;
                    }
                    return error_response(error);
                }
            };
            let status = response.status().as_u16();
            let retry_after = response
                .headers()
                .get("retry-after")
                .and_then(|value| value.to_str().ok())
                .and_then(parse_retry_after)
                .map(|seconds| now_unix().saturating_add(seconds));
            let body = match read_bounded(response, RESPONSE_BODY_LIMIT).await {
                Ok(body) => body,
                Err(error) => {
                    elog::error("tripo3d", format!("tripo3d create body read failed: {error:?}"));
                    return error_response(error);
                }
            };
            match client::parse_create_task(status, &body) {
                Ok(upstream_task_id) => {
                    // THE MONEY BOUNDARY: the reservation becomes delivering, the task is
                    // pinned to this profile forever, and the drain detaches.
                    if !self.mark_delivering(reservation.as_ref()).await {
                        // The upstream task may still consume credits. Register the drain so
                        // evidence is preserved, but keep the customer hold guard armed.
                        if let Some(guard) = self.background.track() {
                            self.track_task(&request_id, &admitted, billing.as_ref());
                            self.spawn_drain(
                                guard,
                                lease,
                                DrainContext {
                                    request_id: request_id.clone(),
                                    task_type: admitted.kind,
                                    requested_model_version: admitted.requested_model_version.clone(),
                                    reserve_credits: admitted.reserve_credits,
                                    priced_ts,
                                    profile: profile.clone(),
                                    reservation: None,
                                },
                                upstream_task_id,
                            );
                        }
                        elog::error("tripo3d", "tripo3d delivery marker unavailable");
                        return error_response(GatewayFailure::Unavailable(
                            "tripo3d_delivery_marker_unavailable",
                        ));
                    }
                    if let Some(guard) = hold_guard.as_mut() {
                        guard.disarm();
                    }
                    let Some(background) = self.background.track() else {
                        // Shutdown raced creation: the task exists upstream; keep the hold armed
                        // and let the reconciler close the reservation after its lease.
                        elog::error("tripo3d", "tripo3d task created during shutdown");
                        return error_response(GatewayFailure::Unavailable("tripo3d_shutdown"));
                    };
                    profile.mark_task_success();
                    self.refresh_live_profile_count();
                    self.track_task(&request_id, &admitted, billing.as_ref());
                    self.spawn_drain(
                        background,
                        lease,
                        DrainContext {
                            request_id: request_id.clone(),
                            task_type: admitted.kind,
                            requested_model_version: admitted.requested_model_version.clone(),
                            reserve_credits: admitted.reserve_credits,
                            priced_ts,
                            profile: profile.clone(),
                            reservation: reservation.take(),
                        },
                        upstream_task_id,
                    );
                    return json_response(
                        StatusCode::OK,
                        json!({
                            "task_id": request_id,
                            "type": admitted.kind.as_wire(),
                            "status": "created",
                        }),
                    );
                }
                Err(verdict) => {
                    let remaining =
                        self.remaining_count(&excluded, &profile.id, admitted.reserve_credits);
                    elog::warn("tripo3d", format!("tripo3d upstream refused: {status}"));
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
                            return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
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
            }
        }
    }

    /// Native task creation on the profile's own platform origin. A redirect is never followed —
    /// it must not carry a subscription key to another origin.
    async fn send_create(
        &self,
        profile: &RuntimeProfile,
        body: Bytes,
    ) -> Result<wreq::Response, GatewayFailure> {
        profile
            .client
            .post(task_create_url(&profile.base_url))
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
            )
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|_| GatewayFailure::Transport)
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

    /// Register the read model for one admitted task. Bounded: when full, the oldest FINALIZED
    /// record is evicted first; a live record is never evicted while its drain owns money.
    fn track_task(
        &self,
        request_id: &str,
        admitted: &AdmittedTask,
        billing: Option<&Tripo3dBillingInput>,
    ) {
        let now = now_unix();
        let mut tasks = self.tasks.lock().expect("Tripo3D tasks lock");
        if tasks.len() >= TASK_TRACK_CAPACITY && !tasks.contains_key(request_id) {
            let eviction = tasks
                .iter()
                .filter(|(_, record)| record.finalized)
                .min_by_key(|(_, record)| record.updated_at)
                .map(|(id, _)| id.clone());
            if let Some(id) = eviction {
                tasks.remove(&id);
            }
        }
        tasks.insert(
            request_id.to_string(),
            TaskRecord {
                task_type: admitted.kind,
                account_id: billing.map(|input| input.account_id.clone()),
                status: "created",
                progress: None,
                artifacts: Vec::new(),
                created_at: now,
                updated_at: now,
                error: None,
                finalized: false,
            },
        );
    }

    fn update_task(&self, request_id: &str, update: impl FnOnce(&mut TaskRecord)) {
        let mut tasks = self.tasks.lock().expect("Tripo3D tasks lock");
        if let Some(record) = tasks.get_mut(request_id) {
            update(record);
            record.updated_at = now_unix();
        }
    }

    /// The detached lifecycle: poll at the documented 2 s cadence inside a bounded deadline,
    /// download artifacts the moment the task succeeds (result URLs live ≤60 s), settle exactly
    /// from `consumed_credit`, then deliver the immutable turn event with its paired post-turn
    /// balance read. A client disconnect is invisible to this task by construction.
    fn spawn_drain(
        self: &Arc<Self>,
        background: ActiveTaskGuard,
        lease: ProfileLease,
        context: DrainContext,
        upstream_task_id: String,
    ) {
        let gateway = Arc::clone(self);
        tokio::spawn(async move {
            let _background = background;
            let _lease = lease;
            gateway.drain_task(context, upstream_task_id).await;
        });
    }

    async fn drain_task(&self, context: DrainContext, upstream_task_id: String) {
        let started = std::time::Instant::now();
        loop {
            if self.abort_drains.load(Ordering::Acquire) {
                // Shutdown deadline: the reservation stays with its lease and the reconciler;
                // nothing is settled from a position of ignorance.
                return;
            }
            if started.elapsed() >= POLL_DEADLINE {
                elog::error(
                    "tripo3d",
                    format!("tripo3d task drain deadline profile={}", context.profile.id),
                );
                self.update_task(&context.request_id, |record| {
                    record.status = "expired";
                    record.error = Some("tripo3d_poll_deadline");
                    record.finalized = true;
                });
                self.settle_conservative_hold(&context, "tripo3d-poll-deadline")
                    .await;
                return;
            }
            match self.poll_upstream_task(&context.profile, &upstream_task_id).await {
                Ok(state) => {
                    let lifecycle = state.lifecycle.expect("a parsed poll names a lifecycle");
                    if lifecycle.is_final() {
                        self.finalize_task(&context, &upstream_task_id, lifecycle, state)
                            .await;
                        context.profile.mark_task_success();
                        self.refresh_live_profile_count();
                        return;
                    }
                    // Ongoing states publish immediately; a finalized state is published only by
                    // `finalize_task`, after the artifacts are in OUR store and the settlement
                    // is queued — `success` never precedes durable delivery.
                    self.update_task(&context.request_id, |record| {
                        record.status = lifecycle.as_str();
                        record.progress = state.progress;
                    });
                }
                Err(verdict @ (UpstreamVerdict::ClientError | UpstreamVerdict::Protocol)) => {
                    // A 404 on the pinned task is per-key isolation evidence of a provider-side
                    // loss; a protocol anomaly means the wire changed. Neither is retryable.
                    let _ = verdict;
                    elog::error(
                        "tripo3d",
                        format!(
                            "tripo3d task lost or wire changed profile={}",
                            context.profile.id
                        ),
                    );
                    self.update_task(&context.request_id, |record| {
                        record.error = Some("tripo3d_task_lost");
                        record.finalized = true;
                    });
                    self.settle_conservative_hold(&context, "tripo3d-task-lost").await;
                    return;
                }
                Err(_) => {
                    // Transport/auth failures keep polling inside the deadline: the task is
                    // still running upstream and only its own poll can settle it.
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
                _ = self.abort_notify.notified() => {}
            }
        }
    }

    async fn poll_upstream_task(
        &self,
        profile: &Arc<RuntimeProfile>,
        upstream_task_id: &str,
    ) -> Result<TaskState, UpstreamVerdict> {
        let response = profile
            .client
            .get(task_poll_url(&profile.base_url, upstream_task_id))
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
            )
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let status = response.status().as_u16();
        let body = read_bounded(response, RESPONSE_BODY_LIMIT)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        client::parse_task_poll(status, &body)
    }

    /// Terminal accounting for one finalized task. The authority is the provider-reported
    /// `consumed_credit`: success settles exactly it; `failed`/`expired` are documented refunds
    /// (zero settle + refund of the hold); the undocumented `banned`/`cancelled`/`unknown`
    /// states (manifest §6.5) keep the conservative hold with a typed counter — no silent
    /// acceptance, no fabricated consumption.
    async fn finalize_task(
        &self,
        context: &DrainContext,
        upstream_task_id: &str,
        lifecycle: TaskLifecycle,
        state: TaskState,
    ) {
        let completed_at = now_unix();
        if lifecycle == TaskLifecycle::Success {
            // Artifacts first: the signed URLs live ≤60 s, settlement can wait a moment.
            let mut stored = Vec::with_capacity(state.artifacts.len());
            for (field, url) in &state.artifacts {
                match store_artifact(
                    &context.profile.client,
                    url,
                    &self.config.artifact_dir,
                    &context.request_id,
                    field,
                )
                .await
                {
                    Ok(name) => stored.push(name),
                    Err(error) => {
                        self.artifact_failures.fetch_add(1, Ordering::Relaxed);
                        elog::error(
                            "tripo3d",
                            format!(
                                "tripo3d artifact download failed profile={} field={field}: {error:#}",
                                context.profile.id
                            ),
                        );
                    }
                }
            }
            self.update_task(&context.request_id, |record| {
                record.artifacts = stored;
                record.finalized = true;
                record.status = lifecycle.as_str();
            });
        } else {
            self.update_task(&context.request_id, |record| {
                record.finalized = true;
                record.status = lifecycle.as_str();
                if lifecycle != TaskLifecycle::Failed && lifecycle != TaskLifecycle::Expired {
                    record.error = Some("tripo3d_task_undocumented_final");
                }
            });
        }

        let consumed_milli = state
            .consumed_credit_raw
            .as_deref()
            .and_then(client::millicredits_from_raw);
        let bound_milli = context.reserve_credits.saturating_mul(1_000);
        enum Money {
            Exact(i64),       // millicredits
            Refund,           // documented zero
            Conservative(&'static str),
        }
        let money = match lifecycle {
            TaskLifecycle::Success => match consumed_milli {
                Some(milli) if milli <= bound_milli => Money::Exact(milli),
                Some(_) => {
                    // consumed_credit above the admitted shape's reserve bound: typed anomaly,
                    // quarantine — never silent acceptance.
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    Money::Conservative("tripo3d-tariff-anomaly")
                }
                None => {
                    self.missing_consumed_credit.fetch_add(1, Ordering::Relaxed);
                    Money::Conservative("tripo3d-consumed-credit-missing")
                }
            },
            TaskLifecycle::Failed | TaskLifecycle::Expired => {
                // Documented: credits are refunded, consumed_credit is 0 (manifest §4.1). If the
                // field is absent the documented refund still applies — the counter records it.
                if consumed_milli.is_none() {
                    self.missing_consumed_credit.fetch_add(1, Ordering::Relaxed);
                }
                Money::Refund
            }
            TaskLifecycle::Banned | TaskLifecycle::Cancelled | TaskLifecycle::Unknown => {
                self.undocumented_final.fetch_add(1, Ordering::Relaxed);
                Money::Conservative("tripo3d-task-undocumented-final")
            }
            TaskLifecycle::Queued | TaskLifecycle::Running => return,
        };

        let (native_milli, api_nano) = match money {
            Money::Exact(milli) => {
                let api = i128::from(milli)
                    .checked_mul(i128::from(registry::TRIPO3D_NANOUSD_PER_MILLICREDIT))
                    .and_then(|value| i64::try_from(value).ok());
                let Some(api) = api else {
                    self.tariff_anomaly.fetch_add(1, Ordering::Relaxed);
                    self.settle_conservative_hold(context, "tripo3d-money-overflow")
                        .await;
                    return;
                };
                (milli, api)
            }
            Money::Refund => (0, 0),
            Money::Conservative(reason) => {
                self.settle_conservative_hold(context, reason).await;
                return;
            }
        };

        // Exact customer settlement: official price x the pinned multiplier, through the single
        // writer. Admin tasks carry no reservation — their evidence still records below.
        if let Some(reservation) = &context.reservation {
            if let Some(billing) = &self.billing {
                let actual = metering::apply_multiplier(i128::from(api_nano), reservation.mult_bp)
                    .clamp(0, i128::from(i64::MAX)) as i64;
                let usage_event = (api_nano > 0).then(|| registry::UsageEventInput {
                    model: context.task_type.as_wire().to_string(),
                    provider: registry::PROVIDER_TRIPO3D.to_string(),
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
                        "tripo3d",
                        format!("Tripo3D customer settlement deferred: {error:#}"),
                    );
                }
            }
        }

        // The immutable turn event: zero pairs are legal evidence (documented free tasks and
        // documented refunds), a paid task can never carry one (the schema's joint invariant).
        let event = Tripo3dTurnCalibrationEvent {
            request_id: context.request_id.clone(),
            subject_id: context.profile.subject_id.clone(),
            cohort: context.profile.cohort.clone(),
            task_type: context.task_type.as_wire().to_string(),
            requested_model_version: context.requested_model_version.clone(),
            resolved_model_version: state.resolved_model_version.clone(),
            tariff_schedule_id: TRIPO3D_TARIFF_SCHEDULE_ID.to_string(),
            priced_ts: context.priced_ts,
            completed_at,
            upstream_task_id: upstream_task_id.to_string(),
            native_total_millicredits: native_milli,
            api_total_nanousd: api_nano,
        };
        if let Err(error) = event.validate() {
            elog::error(
                "tripo3d",
                format!("Tripo3D calibration event rejected before FIFO: {error:#}"),
            );
            return;
        }
        // Codex/Gemini pairing: the free balance read taken in the turn's wake rides the same
        // FIFO entry, so the writer persists the observation with cumulative ledgers that
        // already include this turn.
        let balance = self
            .fetch_balance(&context.profile, false)
            .await
            .map(|snapshot| Tripo3dBalanceSnapshot {
                observed_at: now_unix(),
                balance_raw: snapshot.balance_raw,
                frozen_raw: snapshot.frozen_raw,
                balance_micro_units: snapshot.balance_micro_units,
                frozen_micro_units: snapshot.frozen_micro_units,
            })
            .ok();
        self.enqueue_turn(PendingTurn { event, balance }).await;
    }

    /// The documented-conservative settle: delivery occurred (or the task state moved) but the
    /// authoritative consumption is missing, anomalous or undocumented — preserve the hold,
    /// advance the typed counter (done by the caller), create no immutable provider event.
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
                "tripo3d",
                format!("Tripo3D conservative settlement deferred: {error:#}"),
            );
        }
    }

    async fn enqueue_turn(&self, turn: PendingTurn) {
        let _drain = self.turn_drain.lock().await;
        let accepted = self
            .turn_queue
            .lock()
            .expect("Tripo3D turn queue lock")
            .push(turn);
        if !accepted {
            elog::error(
                "tripo3d",
                "Tripo3D calibration event dropped because the bounded FIFO is full",
            );
            return;
        }
        self.drain_turn_queue_locked().await;
    }

    /// Drain under `turn_drain`. A transient head remains in place and keeps balance polling
    /// blocked; a permanent replay conflict quarantines exactly that event and continues.
    async fn drain_turn_queue_locked(&self) -> bool {
        loop {
            let head = self
                .turn_queue
                .lock()
                .expect("Tripo3D turn queue lock")
                .head()
                .cloned();
            let Some(head) = head else { break };
            let outcome = match &self.billing {
                Some(billing) => match billing.record_tripo3d_turn(head).await {
                    Ok(_) => WriteOutcome::Durable,
                    Err(error) if registry::is_tripo3d_turn_replay_conflict(&error) => {
                        WriteOutcome::Conflict
                    }
                    Err(error) => {
                        elog::warn(
                            "tripo3d",
                            format!(
                                "Tripo3D calibration persistence deferred with FIFO head retained: {error:#}"
                            ),
                        );
                        WriteOutcome::Transient
                    }
                },
                None => WriteOutcome::Transient,
            };
            self.turn_queue
                .lock()
                .expect("Tripo3D turn queue lock")
                .resolve_head(outcome);
            if outcome == WriteOutcome::Transient {
                break;
            }
        }
        self.turn_queue
            .lock()
            .expect("Tripo3D turn queue lock")
            .may_poll_balance()
    }

    // ── uploads (attachments end to end) ────────────────────────────────────

    /// One customer image through the profile's own platform origin: multipart passthrough to
    /// `upload/sts` (≤20 MB, manifest §4), then pin the returned `image_token` to the uploading
    /// profile — uploads are account-scoped. Free per the rate card; rotation is unconstrained
    /// by money because nothing upstream is created.
    pub(crate) async fn handle_image_upload(
        self: &Arc<Self>,
        filename: &str,
        bytes: Bytes,
    ) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("tripo3d_shutdown"));
        }
        let Some(format) = sniff_image_format(&bytes) else {
            return error_response(GatewayFailure::BadRequest("tripo3d_image_format_unknown"));
        };
        let boundary = fresh_boundary();
        let body = build_multipart_body("file", filename, format.content_type(), &bytes, &boundary);
        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();
        loop {
            let Some(profile) = self.select_profile(&excluded, None, 0) else {
                return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
            };
            let lease = ProfileLease::new(profile.clone());
            let result = self
                .send_image_upload(&profile, body.clone(), &boundary)
                .await;
            let verdict = match result {
                Ok((status, body)) => client::parse_image_upload(status, &body).map(|t| (profile.clone(), t)),
                Err(error) => Err(error.verdict()),
            };
            match verdict {
                Ok((profile, token)) => {
                    drop(lease);
                    profile.mark_task_success();
                    self.refresh_live_profile_count();
                    self.pin_token(
                        token.clone(),
                        TokenPin {
                            profile_id: profile.id.clone(),
                            object: None,
                            created: now_unix(),
                        },
                    );
                    return json_response(StatusCode::OK, json!({"image_token": token}));
                }
                Err(verdict) => {
                    let remaining = self.remaining_count(&excluded, &profile.id, 0);
                    let decision = decide(verdict, Phase::BeforeCreate, policy, remaining);
                    profile.apply_effect(decision.effect, now_unix(), None);
                    policy = decision.policy;
                    drop(lease);
                    match decision.next {
                        NextStep::RotateToAnotherProfile => {
                            excluded.insert(profile.id.clone());
                            continue;
                        }
                        NextStep::SurfaceCapacityExhausted => {
                            return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
                        }
                        _ => return error_response(GatewayFailure::from_verdict(verdict, 502)),
                    }
                }
            }
        }
    }

    async fn send_image_upload(
        &self,
        profile: &RuntimeProfile,
        body: Vec<u8>,
        boundary: &str,
    ) -> Result<(u16, Bytes), GatewayFailure> {
        let response = profile
            .client
            .post(format!(
                "{}{}",
                profile.base_url.trim_end_matches('/'),
                TRIPO3D_UPLOAD_STS_PATH
            ))
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
            )
            .header(
                "content-type",
                format!("multipart/form-data; boundary={boundary}"),
            )
            .body(body)
            .send()
            .await
            .map_err(|_| GatewayFailure::Transport)?;
        let status = response.status().as_u16();
        let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
        Ok((status, body))
    }

    /// One customer model file through the STS flow (manifest §4): fetch temporary S3
    /// credentials on the profile, PUT the bytes there with SigV4, and mint a plane token that
    /// `import_model` resolves to `file: {"object": {bucket, key}}` on the same profile.
    pub(crate) async fn handle_model_upload(
        self: &Arc<Self>,
        filename: &str,
        bytes: Bytes,
    ) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("tripo3d_shutdown"));
        }
        let extension = filename
            .rsplit_once('.')
            .map(|(_, extension)| extension.to_string())
            .unwrap_or_default();
        let Some(format) = ModelFormat::from_extension(&extension) else {
            return error_response(GatewayFailure::BadRequest("tripo3d_model_format_unknown"));
        };
        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();
        loop {
            let Some(profile) = self.select_profile(&excluded, None, 0) else {
                return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
            };
            let lease = ProfileLease::new(profile.clone());
            let result = self.upload_model_via(&profile, format, &bytes).await;
            match result {
                Ok((bucket, key)) => {
                    drop(lease);
                    profile.mark_task_success();
                    self.refresh_live_profile_count();
                    let token = format!("t3m-{}", crate::upstream::fresh_request_id());
                    self.pin_token(
                        token.clone(),
                        TokenPin {
                            profile_id: profile.id.clone(),
                            object: Some((bucket, key)),
                            created: now_unix(),
                        },
                    );
                    return json_response(StatusCode::OK, json!({"model_token": token}));
                }
                Err(verdict) => {
                    let remaining = self.remaining_count(&excluded, &profile.id, 0);
                    let decision = decide(verdict, Phase::BeforeCreate, policy, remaining);
                    profile.apply_effect(decision.effect, now_unix(), None);
                    policy = decision.policy;
                    drop(lease);
                    match decision.next {
                        NextStep::RotateToAnotherProfile => {
                            excluded.insert(profile.id.clone());
                            continue;
                        }
                        NextStep::SurfaceCapacityExhausted => {
                            return error_response_with_retry(GatewayFailure::Capacity, self.capacity_retry_after());
                        }
                        _ => return error_response(GatewayFailure::from_verdict(verdict, 502)),
                    }
                }
            }
        }
    }

    async fn upload_model_via(
        &self,
        profile: &Arc<RuntimeProfile>,
        format: ModelFormat,
        bytes: &[u8],
    ) -> Result<(String, String), UpstreamVerdict> {
        let response = profile
            .client
            .post(format!(
                "{}{}/token",
                profile.base_url.trim_end_matches('/'),
                TRIPO3D_UPLOAD_STS_PATH
            ))
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
            )
            .header("content-type", "application/json")
            .body(format!("{{\"format\":\"{}\"}}", format.as_sts_format()))
            .send()
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let status = response.status().as_u16();
        let body = read_bounded(response, ERROR_BODY_LIMIT)
            .await
            .map_err(|_| UpstreamVerdict::Transport)?;
        let session = client::parse_sts_token(status, &body)?;
        s3_put_object(
            &profile.client,
            &session,
            bytes.to_vec(),
            format.content_type(),
            std::time::SystemTime::now(),
        )
        .await
        .map_err(|_| UpstreamVerdict::Transport)?;
        Ok((session.bucket, session.object_key))
    }

    // ── read surface (our task records; artifacts from our store) ───────────

    /// The read model of one task for its creating account (or an admin). A foreign or unknown
    /// id answers `None` — the plane never reveals whether a task exists across accounts.
    pub fn task_view(&self, task_id: &str, requester: Option<&str>) -> Option<Tripo3dTaskView> {
        let record = self.tasks.lock().expect("Tripo3D tasks lock").get(task_id).cloned()?;
        // Admin (no account scope) reads everything; a metered reader sees only its own
        // account's tasks; a foreign id is indistinguishable from an unknown one.
        if let (Some(owner), Some(requester)) = (record.account_id.as_deref(), requester) {
            if owner != requester {
                return None;
            }
        }
        Some(Tripo3dTaskView {
            task_id: task_id.to_string(),
            task_type: record.task_type.as_wire().to_string(),
            status: record.status.to_string(),
            progress: record.progress,
            artifacts: record.artifacts.clone(),
            created_at: record.created_at,
            updated_at: record.updated_at,
            error: record.error,
        })
    }

    /// The on-disk path of one stored artifact for an authorized reader. The name must be one
    /// the task actually recorded — client input never becomes a path component.
    pub fn task_artifact_path(
        &self,
        task_id: &str,
        name: &str,
        requester: Option<&str>,
    ) -> Option<PathBuf> {
        let record = self.tasks.lock().expect("Tripo3D tasks lock").get(task_id).cloned()?;
        if let (Some(owner), Some(requester)) = (record.account_id.as_deref(), requester) {
            if owner != requester {
                return None;
            }
        }
        if !record.artifacts.iter().any(|artifact| artifact == name) {
            return None;
        }
        Some(artifact_path(&self.config.artifact_dir, task_id, name))
    }

    /// Read only cached operational state. Metrics collection and the admin projection never
    /// start a network request here.
    pub fn operational_status(&self) -> Tripo3dOperationalStatus {
        let delivery = self
            .turn_queue
            .lock()
            .expect("Tripo3D turn queue lock")
            .health();
        let now = now_unix();
        let profiles = self.profiles.read().expect("Tripo3D profiles lock");
        let mut rate_limited_profiles = 0;
        let mut balance_walled_profiles = 0;
        let mut auth_cooling_profiles = 0;
        let mut transport_cooling_profiles = 0;
        let mut inflight_requests = 0u64;
        let mut statuses = Vec::with_capacity(profiles.len());
        for profile in profiles.iter() {
            let health = profile.health.lock().expect("Tripo3D profile health lock");
            let rate_until = active_cooling(health.rate_limit_cool_until, now);
            let auth_until = active_cooling(health.auth_cool_until, now);
            let transport_until = active_cooling(health.transport_cool_until, now);
            rate_limited_profiles += usize::from(rate_until.is_some());
            balance_walled_profiles += usize::from(health.balance_walled);
            auth_cooling_profiles += usize::from(auth_until.is_some());
            transport_cooling_profiles += usize::from(transport_until.is_some());
            let inflight = profile.inflight.load(Ordering::Acquire);
            inflight_requests += u64::from(inflight);
            statuses.push(Tripo3dProfileStatus {
                id: profile.id.clone(),
                cohort: profile.cohort.clone(),
                live: health.authenticated,
                rate_limit_cool_until: rate_until,
                balance_walled: health.balance_walled,
                auth_cool_until: auth_until,
                transport_cool_until: transport_until,
                inflight,
                balance_observed_at: health.balance_observed_at,
                balance_raw: health.balance_raw.clone(),
                frozen_raw: health.frozen_raw.clone(),
                balance_micro_units: health.balance_micro_units,
                frozen_micro_units: health.frozen_micro_units,
            });
        }
        drop(profiles);
        let available_profiles = {
            let candidates: Vec<Candidate> = self
                .profiles
                .read()
                .expect("Tripo3D profiles lock")
                .iter()
                .map(|profile| profile.candidate(now, 0))
                .collect();
            candidates
                .iter()
                .filter(|candidate| candidate.hard.is_none() && candidate.soft.is_none())
                .count()
        };
        Tripo3dOperationalStatus {
            total_profiles: statuses.len(),
            live_profiles: self.live_profiles.load(Ordering::Acquire),
            available_profiles,
            rate_limited_profiles,
            balance_walled_profiles,
            auth_cooling_profiles,
            transport_cooling_profiles,
            inflight_requests,
            inflight_drains: 0,
            tracked_tasks: self.tasks.lock().expect("Tripo3D tasks lock").len(),
            missing_consumed_credit: self.missing_consumed_credit.load(Ordering::Acquire),
            tariff_anomaly: self.tariff_anomaly.load(Ordering::Acquire),
            undocumented_final: self.undocumented_final.load(Ordering::Acquire),
            artifact_failures: self.artifact_failures.load(Ordering::Acquire),
            balance_sweep_ms: self.balance_sweep_ms.load(Ordering::Acquire),
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
            .expect("Tripo3D profiles lock")
            .iter()
            .find(|profile| profile.subject_id == subject_id)
            .map(|profile| profile.id.clone())
    }

    pub fn readiness(&self) -> Result<(), NotReady> {
        let status = self.operational_status();
        readiness(status.live_profiles, status.delivery.persistence_ok)
    }

    /// Close admission, wait for detached drains (poll + download + settle), then run the
    /// final turn-before-balance ordering inside the process deadline. On the deadline the
    /// drains stop mid-poll: the reservation stays with its lease and the reconciler, never a
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
            let _sweep = self.balance_sweep.lock().await;
            // Admission is closed and every drain is idle, so each profile is stable: finish
            // the same turn-before-balance ordering used by the steady-state poller.
            self.poll_balance_generation(true).await;
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
                "tripo3d",
                "Tripo3D shutdown calibration drain remained incomplete at deadline",
            );
        }
    }
}

fn same_profile_generation(left: &[Arc<RuntimeProfile>], right: &[Arc<RuntimeProfile>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| Arc::ptr_eq(left, right))
}

fn profiles_match_roster(profiles: &[Arc<RuntimeProfile>], roster: &[Tripo3dProfile]) -> bool {
    profiles.len() == roster.len()
        && profiles
            .iter()
            .zip(roster)
            .all(|(profile, loaded)| profile.matches_roster(loaded))
}

// ── admission: request validation, pricing, upstream body ───────────────────

/// The plane's create-task request body. Strict: an unknown field is rejected rather than
/// silently dropped (the customer is paying per task; a dropped control would misprice the
/// reserve against what the provider actually runs).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerationBody {
    #[serde(rename = "type")]
    task_type: String,
    model_version: Option<String>,
    prompt: Option<String>,
    negative_prompt: Option<String>,
    // Image inputs: a token from our upload endpoint, or a URL the provider fetches itself.
    image_token: Option<String>,
    image_url: Option<String>,
    image_type: Option<String>,
    /// `multiview_to_model`: exactly 4 ordered slots, the front (index 0) mandatory.
    files: Option<Vec<Option<ImageRef>>>,
    /// A model file uploaded through our model-upload endpoint (import_model).
    model_token: Option<String>,
    original_model_task_id: Option<String>,
    draft_model_task_id: Option<String>,
    original_task_id: Option<String>,
    format: Option<String>,
    mode: Option<String>,
    animations: Option<Vec<String>>,
    out_format: Option<String>,
    rig_type: Option<String>,
    spec: Option<String>,
    texture_prompt_text: Option<String>,
    style_image_token: Option<String>,
    style_image_url: Option<String>,
    part_names: Option<Vec<String>>,
    prompts: Option<Vec<EditPrompt>>,
    template: Option<String>,
    // Price-relevant generation options (§5.1 surcharges).
    texture: Option<bool>,
    pbr: Option<bool>,
    smart_low_poly: Option<bool>,
    generate_parts: Option<bool>,
    quad: Option<bool>,
    style: Option<bool>,
    texture_quality: Option<String>,
    geometry_quality: Option<String>,
    face_limit: Option<u64>,
    model_seed: Option<u64>,
    texture_seed: Option<u64>,
}

/// One image reference: a token from our upload endpoint or a provider-fetched URL.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ImageRef {
    image_token: Option<String>,
    image_url: Option<String>,
    image_type: Option<String>,
}

/// One view edit instruction of `edit_multiview_image`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct EditPrompt {
    view: String,
    prompt: String,
}

/// An admitted, priced task: the validated upstream body plus its reserve.
#[derive(Debug)]
struct AdmittedTask {
    kind: Tripo3dTaskKind,
    requested_model_version: Option<String>,
    upstream_body: Value,
    /// The reserve in credits — exact published price or the documented family-max
    /// conservative bound; also the settlement anomaly cross-check bound.
    reserve_credits: i64,
    conservative: bool,
    /// Upload-affinity pin: the task must be created on the profile that owns its tokens.
    sticky_profile: Option<String>,
}

const MAX_TEXT_LEN: usize = 8 * 1024;
const MAX_ID_LEN: usize = 128;
const MAX_TOKEN_LEN: usize = 512;
/// The 16 documented animation presets (manifest §3).
const MAX_ANIMATIONS: usize = 16;
/// The 4 fixed multiview slots.
const MULTIVIEW_SLOTS: usize = 4;

fn bounded_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.bytes().any(|b| b < 0x08)
}

fn image_file_object(image_type: Option<&str>, token: Option<&str>, url: Option<&str>) -> Result<Value, GatewayFailure> {
    let kind = match image_type.unwrap_or("jpg") {
        "jpg" | "jpeg" => "jpeg",
        "png" => "png",
        "webp" => "webp",
        _ => return Err(GatewayFailure::BadRequest("tripo3d_image_type_unknown")),
    };
    match (token, url) {
        (Some(token), None) if bounded_text(token, MAX_TOKEN_LEN) => {
            Ok(json!({"type": kind, "file_token": token}))
        }
        (None, Some(url))
            if bounded_text(url, 2048)
                && (url.starts_with("https://") || url.starts_with("http://")) =>
        {
            Ok(json!({"type": kind, "url": url}))
        }
        _ => Err(GatewayFailure::BadRequest("tripo3d_image_input_invalid")),
    }
}

fn bool_flag(value: Option<bool>) -> bool {
    value.unwrap_or(false)
}

/// Insert the price-relevant generation options into the upstream body, only when non-default
/// (the SDK's own convention), and build the pricing view.
fn generation_options(body: &GenerationBody) -> Result<(Tripo3dOptions, Vec<(&'static str, Value)>), GatewayFailure> {
    let texture_quality = match body.texture_quality.as_deref() {
        None | Some("standard") => Tripo3dTextureQuality::Standard,
        Some("detailed") => Tripo3dTextureQuality::Detailed,
        // `extreme` is on the billing changelog only (manifest §6.8): priced on the card, so
        // admitted — an upstream refusal is the provider's own honest 4xx.
        Some("extreme") => Tripo3dTextureQuality::Extreme,
        Some(_) => return Err(GatewayFailure::BadRequest("tripo3d_texture_quality_unknown")),
    };
    let geometry_quality = match body.geometry_quality.as_deref() {
        None | Some("standard") => Tripo3dGeometryQuality::Standard,
        Some("detailed") => Tripo3dGeometryQuality::Detailed,
        Some(_) => return Err(GatewayFailure::BadRequest("tripo3d_geometry_quality_unknown")),
    };
    let options = Tripo3dOptions {
        texture: bool_flag(body.texture),
        pbr: bool_flag(body.pbr),
        smart_low_poly: bool_flag(body.smart_low_poly),
        generate_parts: bool_flag(body.generate_parts),
        quad: bool_flag(body.quad),
        style: bool_flag(body.style),
        texture_quality,
        geometry_quality,
    };
    let mut wire = Vec::new();
    if options.texture {
        wire.push(("texture", json!(true)));
    }
    if options.pbr {
        wire.push(("pbr", json!(true)));
    }
    if options.smart_low_poly {
        wire.push(("smart_low_poly", json!(true)));
    }
    if options.generate_parts {
        wire.push(("generate_parts", json!(true)));
    }
    if options.quad {
        wire.push(("quad", json!(true)));
    }
    if options.style {
        wire.push(("style", json!(true)));
    }
    if options.texture && texture_quality != Tripo3dTextureQuality::Standard {
        wire.push(("texture_quality", json!(body.texture_quality.as_deref().unwrap_or("standard"))));
    }
    if geometry_quality == Tripo3dGeometryQuality::Detailed {
        wire.push(("geometry_quality", json!("detailed")));
    }
    Ok((options, wire))
}

/// Validate and price one create-task request: the metering-admitted shapes only, every other
/// `type` rejected with the admitted set named. Returns the upstream body 1:1 over the
/// documented task fields plus the reserve.
fn admit_task(gateway: &Tripo3dGateway, body: GenerationBody) -> Result<AdmittedTask, GatewayFailure> {
    let Some(kind) = Tripo3dTaskKind::from_wire(&body.task_type) else {
        return Err(GatewayFailure::BadRequest("tripo3d_task_type_unknown"));
    };
    if kind == Tripo3dTaskKind::HighpolyToLowpoly {
        // The documented docs-vs-SDK version conflict (manifest §6.6): do not guess.
        return Err(GatewayFailure::BadRequest("tripo3d_highpoly_version_conflict"));
    }
    if let Some(version) = body.model_version.as_deref() {
        if version.is_empty() || version.len() > 64 {
            return Err(GatewayFailure::BadRequest("tripo3d_model_version_invalid"));
        }
    }
    let requested_model_version = body.model_version.clone();

    // `style` on generation tasks: the card prices +5 but no SDK-proven wire field exists for
    // `*_to_model` — fail closed with the limitation named rather than guess the wire.
    let style_on_model_task = bool_flag(body.style)
        && matches!(
            kind,
            Tripo3dTaskKind::TextToModel
                | Tripo3dTaskKind::ImageToModel
                | Tripo3dTaskKind::MultiviewToModel
        );
    if style_on_model_task {
        return Err(GatewayFailure::Unsupported("tripo3d_style_wire_unproven"));
    }

    let mut upstream = json!({"type": kind.as_wire()});
    let object = upstream.as_object_mut().expect("task object");
    if let Some(version) = body.model_version.as_deref() {
        object.insert("model_version".into(), json!(version));
    }

    // Non-applicable price flags must not silently ride along: reserve and wire would desync.
    let options_applicable = matches!(
        kind,
        Tripo3dTaskKind::TextToModel | Tripo3dTaskKind::ImageToModel | Tripo3dTaskKind::MultiviewToModel
    );
    if !options_applicable
        && (bool_flag(body.texture)
            || bool_flag(body.pbr)
            || bool_flag(body.smart_low_poly)
            || bool_flag(body.generate_parts)
            || bool_flag(body.quad)
            || body.geometry_quality.is_some())
    {
        return Err(GatewayFailure::BadRequest("tripo3d_option_not_applicable"));
    }

    let mut sticky: Option<String> = None;
    // Upload affinity: a token from our upload endpoint is account-scoped, so the task must be
    // created on the profile that uploaded it. Two tokens from different profiles can never
    // form one task.
    let mut pin_for = |token: Option<&str>| -> Result<Option<()>, GatewayFailure> {
        let Some(token) = token else { return Ok(None) };
        let Some(profile) = gateway.profile_for_token(token) else {
            return Ok(None);
        };
        match &sticky {
            Some(existing) if *existing != profile => Err(GatewayFailure::BadRequest(
                "tripo3d_image_inputs_split_profiles",
            )),
            _ => {
                sticky = Some(profile);
                Ok(None)
            }
        }
    };

    let (options, wire_options) = generation_options(&body)?;

    let reserve_credits: i64;
    let mut conservative = false;
    match kind {
        Tripo3dTaskKind::TextToModel => {
            let prompt = body.prompt.as_deref().filter(|p| bounded_text(p, MAX_TEXT_LEN));
            let Some(prompt) = prompt else {
                return Err(GatewayFailure::BadRequest("tripo3d_prompt_required"));
            };
            object.insert("prompt".into(), json!(prompt));
            if let Some(negative) = body.negative_prompt.as_deref() {
                if !bounded_text(negative, MAX_TEXT_LEN) {
                    return Err(GatewayFailure::BadRequest("tripo3d_negative_prompt_invalid"));
                }
                object.insert("negative_prompt".into(), json!(negative));
            }
            for (key, value) in wire_options {
                object.insert(key.into(), value);
            }
        }
        Tripo3dTaskKind::ImageToModel => {
            pin_for(body.image_token.as_deref())?;
            let file = image_file_object(
                body.image_type.as_deref(),
                body.image_token.as_deref(),
                body.image_url.as_deref(),
            )?;
            object.insert("file".into(), file);
            for (key, value) in wire_options {
                object.insert(key.into(), value);
            }
        }
        Tripo3dTaskKind::MultiviewToModel => {
            let Some(files) = body.files.as_ref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_multiview_files_required"));
            };
            // Manifest §3: exactly 4 ordered views, the front mandatory.
            if files.len() != MULTIVIEW_SLOTS || files.first().is_none_or(|slot| slot.is_none()) {
                return Err(GatewayFailure::BadRequest("tripo3d_multiview_slots_invalid"));
            }
            let mut wire_files = Vec::with_capacity(MULTIVIEW_SLOTS);
            for slot in files {
                match slot {
                    Some(reference) => {
                        pin_for(reference.image_token.as_deref())?;
                        wire_files.push(image_file_object(
                            reference.image_type.as_deref(),
                            reference.image_token.as_deref(),
                            reference.image_url.as_deref(),
                        )?);
                    }
                    // The SDK's sparse form: an empty object marks a skipped slot.
                    None => wire_files.push(json!({})),
                }
            }
            object.insert("files".into(), Value::Array(wire_files));
            for (key, value) in wire_options {
                object.insert(key.into(), value);
            }
        }
        Tripo3dTaskKind::TextureModel => {
            let Some(original) = body.original_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_invalid"));
            }
            object.insert("original_model_task_id".into(), json!(original));
            // The wire always textures (that IS this task); the price selector is
            // texture_quality, and the +5 style reference rides texture_prompt.style_image.
            object.insert("texture".into(), json!(true));
            object.insert("pbr".into(), json!(true));
            if let Some(quality) = body.texture_quality.as_deref() {
                object.insert("texture_quality".into(), json!(quality));
            }
            let has_style_image = body.style_image_token.is_some() || body.style_image_url.is_some();
            if bool_flag(body.style) && !has_style_image {
                return Err(GatewayFailure::BadRequest("tripo3d_style_image_required"));
            }
            let mut texture_prompt = json!({});
            if let Some(text) = body.texture_prompt_text.as_deref() {
                if !bounded_text(text, MAX_TEXT_LEN) {
                    return Err(GatewayFailure::BadRequest("tripo3d_texture_prompt_invalid"));
                }
                texture_prompt["text"] = json!(text);
            }
            if has_style_image {
                pin_for(body.style_image_token.as_deref())?;
                texture_prompt["style_image"] = image_file_object(
                    None,
                    body.style_image_token.as_deref(),
                    body.style_image_url.as_deref(),
                )?;
            }
            if !texture_prompt.as_object().is_none_or(|o| o.is_empty()) {
                object.insert("texture_prompt".into(), texture_prompt);
            }
            if let Some(part_names) = body.part_names.as_ref() {
                if part_names.len() > 64
                    || !part_names.iter().all(|name| bounded_text(name, MAX_ID_LEN))
                {
                    return Err(GatewayFailure::BadRequest("tripo3d_part_names_invalid"));
                }
                object.insert("part_names".into(), json!(part_names));
            }
        }
        Tripo3dTaskKind::MeshSegmentation | Tripo3dTaskKind::MeshCompletion => {
            let Some(original) = body.original_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_invalid"));
            }
            object.insert("original_model_task_id".into(), json!(original));
            if kind == Tripo3dTaskKind::MeshCompletion {
                if let Some(part_names) = body.part_names.as_ref() {
                    if part_names.len() > 64
                        || !part_names.iter().all(|name| bounded_text(name, MAX_ID_LEN))
                    {
                        return Err(GatewayFailure::BadRequest("tripo3d_part_names_invalid"));
                    }
                    object.insert("part_names".into(), json!(part_names));
                }
            }
        }
        Tripo3dTaskKind::AnimatePrerigcheck | Tripo3dTaskKind::AnimateRig => {
            let Some(original) = body.original_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_invalid"));
            }
            object.insert("original_model_task_id".into(), json!(original));
            if kind == Tripo3dTaskKind::AnimateRig {
                if let Some(out_format) = body.out_format.as_deref() {
                    if !matches!(out_format, "glb" | "fbx") {
                        return Err(GatewayFailure::BadRequest("tripo3d_out_format_unknown"));
                    }
                    object.insert("out_format".into(), json!(out_format));
                }
                if let Some(rig_type) = body.rig_type.as_deref() {
                    if !matches!(
                        rig_type,
                        "biped" | "quadruped" | "hexapod" | "octopod" | "avian" | "serpentine"
                            | "aquatic" | "others"
                    ) {
                        return Err(GatewayFailure::BadRequest("tripo3d_rig_type_unknown"));
                    }
                    object.insert("rig_type".into(), json!(rig_type));
                }
                if let Some(spec) = body.spec.as_deref() {
                    if !matches!(spec, "mixamo" | "tripo") {
                        return Err(GatewayFailure::BadRequest("tripo3d_rig_spec_unknown"));
                    }
                    object.insert("spec".into(), json!(spec));
                }
            }
        }
        Tripo3dTaskKind::AnimateRetarget => {
            let Some(original) = body.original_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_invalid"));
            }
            let Some(animations) = body.animations.as_ref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_animations_required"));
            };
            if animations.is_empty()
                || animations.len() > MAX_ANIMATIONS
                || !animations.iter().all(|name| bounded_text(name, MAX_ID_LEN))
            {
                return Err(GatewayFailure::BadRequest("tripo3d_animations_invalid"));
            }
            object.insert("original_model_task_id".into(), json!(original));
            object.insert("animations".into(), json!(animations));
            // 10 credits per animation (§5.1), count-derived exact price.
            reserve_credits = tripo3d_animate_retarget_credits(animations.len() as u64)
                .ok_or(GatewayFailure::BadRequest("tripo3d_animations_invalid"))?;
            return Ok(AdmittedTask {
                kind,
                requested_model_version,
                upstream_body: upstream,
                reserve_credits,
                conservative,
                sticky_profile: sticky,
            });
        }
        Tripo3dTaskKind::ConvertModel => {
            let Some(original) = body.original_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_model_task_invalid"));
            }
            let Some(format) = body.format.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_format_required"));
            };
            if !matches!(format, "GLTF" | "USDZ" | "FBX" | "OBJ" | "STL" | "3MF") {
                return Err(GatewayFailure::BadRequest("tripo3d_format_unknown"));
            }
            object.insert("original_model_task_id".into(), json!(original));
            object.insert("format".into(), json!(format));
            // The basic/advanced selector's wire field is unproven (manifest §6): the wire runs
            // the plain convert, and `advanced` reserves the published advanced price (10)
            // conservatively — settlement is the exact `consumed_credit` either way.
            let mode = match body.mode.as_deref() {
                None | Some("basic") => Tripo3dConvertMode::Basic,
                Some("advanced") => Tripo3dConvertMode::Advanced,
                Some(_) => return Err(GatewayFailure::BadRequest("tripo3d_convert_mode_unknown")),
            };
            reserve_credits = tripo3d_convert_model_credits(mode);
            conservative = mode == Tripo3dConvertMode::Advanced;
            return Ok(AdmittedTask {
                kind,
                requested_model_version,
                upstream_body: upstream,
                reserve_credits,
                conservative,
                sticky_profile: sticky,
            });
        }
        Tripo3dTaskKind::ImportModel => {
            let Some(token) = body.model_token.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_model_token_required"));
            };
            let Some((profile, bucket, key)) = gateway.model_object_for_token(token) else {
                // A token we did not issue (or already evicted) names nothing.
                return Err(GatewayFailure::BadRequest("tripo3d_model_token_unknown"));
            };
            sticky = Some(profile);
            object.insert("file".into(), json!({"object": {"bucket": bucket, "key": key}}));
        }
        Tripo3dTaskKind::TextToImage => {
            let Some(prompt) = body.prompt.as_deref().filter(|p| bounded_text(p, 1_024)) else {
                return Err(GatewayFailure::BadRequest("tripo3d_prompt_required"));
            };
            object.insert("prompt".into(), json!(prompt));
            if let Some(negative) = body.negative_prompt.as_deref() {
                if !bounded_text(negative, 255) {
                    return Err(GatewayFailure::BadRequest("tripo3d_negative_prompt_invalid"));
                }
                object.insert("negative_prompt".into(), json!(negative));
            }
        }
        Tripo3dTaskKind::GenerateImage => {
            if let Some(prompt) = body.prompt.as_deref() {
                if !bounded_text(prompt, 1_024) {
                    return Err(GatewayFailure::BadRequest("tripo3d_prompt_invalid"));
                }
                object.insert("prompt".into(), json!(prompt));
            }
            if let Some(template) = body.template.as_deref() {
                if !bounded_text(template, MAX_ID_LEN) {
                    return Err(GatewayFailure::BadRequest("tripo3d_template_invalid"));
                }
                object.insert("template".into(), json!(template));
            }
            let has_single = body.image_token.is_some() || body.image_url.is_some();
            let files = body.files.as_ref();
            match (has_single, files) {
                (true, None) => {
                    pin_for(body.image_token.as_deref())?;
                    object.insert(
                        "file".into(),
                        image_file_object(
                            body.image_type.as_deref(),
                            body.image_token.as_deref(),
                            body.image_url.as_deref(),
                        )?,
                    );
                }
                (false, Some(references)) => {
                    if references.is_empty() || references.len() > MULTIVIEW_SLOTS {
                        return Err(GatewayFailure::BadRequest("tripo3d_files_invalid"));
                    }
                    let mut wire_files = Vec::with_capacity(references.len());
                    for slot in references {
                        let Some(reference) = slot else {
                            return Err(GatewayFailure::BadRequest("tripo3d_files_invalid"));
                        };
                        pin_for(reference.image_token.as_deref())?;
                        wire_files.push(image_file_object(
                            reference.image_type.as_deref(),
                            reference.image_token.as_deref(),
                            reference.image_url.as_deref(),
                        )?);
                    }
                    object.insert("files".into(), Value::Array(wire_files));
                }
                (true, Some(_)) => {
                    return Err(GatewayFailure::BadRequest("tripo3d_image_input_ambiguous"));
                }
                (false, None) => {}
            }
            if object.get("prompt").is_none() && object.get("file").is_none() && object.get("template").is_none() {
                return Err(GatewayFailure::BadRequest("tripo3d_generate_image_input_required"));
            }
        }
        Tripo3dTaskKind::GenerateMultiviewImage => {
            pin_for(body.image_token.as_deref())?;
            let file = image_file_object(
                body.image_type.as_deref(),
                body.image_token.as_deref(),
                body.image_url.as_deref(),
            )?;
            object.insert("file".into(), file);
        }
        Tripo3dTaskKind::EditMultiviewImage => {
            let Some(original) = body.original_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_original_task_required"));
            };
            if !bounded_text(original, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_original_task_invalid"));
            }
            let Some(prompts) = body.prompts.as_ref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_edit_prompts_required"));
            };
            if prompts.is_empty() || prompts.len() > MULTIVIEW_SLOTS {
                return Err(GatewayFailure::BadRequest("tripo3d_edit_prompts_invalid"));
            }
            for edit in prompts {
                if !matches!(edit.view.as_str(), "front" | "left" | "back" | "right")
                    || !bounded_text(&edit.prompt, MAX_TEXT_LEN)
                {
                    return Err(GatewayFailure::BadRequest("tripo3d_edit_prompts_invalid"));
                }
            }
            object.insert("original_task_id".into(), json!(original));
            object.insert("prompts".into(), json!(prompts.iter().map(|edit| json!({"view": edit.view, "prompt": edit.prompt})).collect::<Vec<_>>()));
            // 5 credits per edited image (§5.1), count-derived exact price.
            reserve_credits = tripo3d_edit_multiview_image_credits(prompts.len() as u64)
                .ok_or(GatewayFailure::BadRequest("tripo3d_edit_prompts_invalid"))?;
            return Ok(AdmittedTask {
                kind,
                requested_model_version,
                upstream_body: upstream,
                reserve_credits,
                conservative,
                sticky_profile: sticky,
            });
        }
        Tripo3dTaskKind::RefineModel => {
            let Some(draft) = body.draft_model_task_id.as_deref() else {
                return Err(GatewayFailure::BadRequest("tripo3d_draft_model_task_required"));
            };
            if !bounded_text(draft, MAX_ID_LEN) {
                return Err(GatewayFailure::BadRequest("tripo3d_draft_model_task_invalid"));
            }
            object.insert("draft_model_task_id".into(), json!(draft));
        }
        Tripo3dTaskKind::HighpolyToLowpoly => unreachable!("refused at admission above"),
    }

    if let Some(face_limit) = body.face_limit {
        if face_limit == 0 || face_limit > 10_000_000 {
            return Err(GatewayFailure::BadRequest("tripo3d_face_limit_invalid"));
        }
        if options_applicable {
            object.insert("face_limit".into(), json!(face_limit));
        }
    }
    for (seed, name) in [(body.model_seed, "model_seed"), (body.texture_seed, "texture_seed")] {
        if let Some(seed) = seed {
            if options_applicable {
                object.insert(name.into(), json!(seed));
            }
        }
    }

    let reserve = tripo3d_reserve_credits(kind, body.model_version.as_deref(), &options)
        .ok_or(GatewayFailure::Unsupported("tripo3d_task_unpriced"))?;
    reserve_credits = reserve.credits;
    conservative = reserve.conservative;
    Ok(AdmittedTask {
        kind,
        requested_model_version,
        upstream_body: upstream,
        reserve_credits,
        conservative,
        sticky_profile: sticky,
    })
}

#[derive(Clone, Copy, Debug)]
enum GatewayFailure {
    /// The provider refused the static key (401): SOFT axis — quarantine + probe, never a
    /// fleet-removing verdict on its own.
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
            Self::Upstream(status) => classify_status(status, None),
            Self::Capacity => UpstreamVerdict::InsufficientBalance,
            Self::LowBalance | Self::BadRequest(_) | Self::Unsupported(_) => {
                UpstreamVerdict::ClientError
            }
        }
    }

    fn from_verdict(verdict: UpstreamVerdict, status: u16) -> Self {
        match verdict {
            UpstreamVerdict::AuthRefused => Self::Auth,
            UpstreamVerdict::RateLimitedHard | UpstreamVerdict::InsufficientBalance => {
                Self::Capacity
            }
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
/// the roster, profiles, proxies or provider bodies. `Retry-After` only on retryable classes,
/// and the terminal reason rides the response extensions for the server audit middleware.
fn error_response(error: GatewayFailure) -> Response {
    error_response_with_retry(error, 60)
}

fn error_response_with_retry(error: GatewayFailure, capacity_retry_after: i64) -> Response {
    let (status, kind, message, reason, retry_after) = match error {
        GatewayFailure::Auth => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "tripo3d_auth_unavailable",
            Some(2),
        ),
        GatewayFailure::Transport | GatewayFailure::Protocol => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "tripo3d_upstream_unavailable",
            Some(2),
        ),
        GatewayFailure::Capacity => (
            StatusCode::TOO_MANY_REQUESTS,
            "rate_limit",
            "Capacity exhausted. Please try again later.",
            "tripo3d_capacity_exhausted",
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
            "tripo3d_upstream_rejected",
            None,
        ),
        GatewayFailure::Upstream(status) if (400..500).contains(&status) => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The provider rejected the request.",
            "tripo3d_upstream_rejected",
            None,
        ),
        GatewayFailure::Upstream(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            "overloaded",
            "Overloaded",
            "tripo3d_upstream_rejected",
            Some(2),
        ),
    };
    let mut response = json_response(
        status,
        json!({"error": {"type": kind, "message": message}}),
    );
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
        .body(Body::from(serde_json::to_vec(&value).unwrap_or_else(|_| b"{}".to_vec())))
        .expect("Tripo3D response");
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

// ── axum surface (mounted by `server` only on the dedicated Tripo3D plane) ──

use axum::extract::{ConnectInfo, FromRequest, Multipart, Path, State};
use std::net::SocketAddr;

use crate::proxy::{authorize, Authz};
use crate::state::AppState;
use crate::Metrics;

/// The plane's JSON body cap: bounded task descriptors, never media. Media arrives as
/// multipart on the upload routes with their own limits.
const GENERATION_BODY_LIMIT: usize = 256 * 1024;

/// Shared authorization for the plane's customer surface, mirroring the KIMI plane's
/// `/v1/messages` arm: the same `authorize` the Anthropic path uses, admin first in memory,
/// then the metered key through the billing authority. Auth always precedes body buffering.
/// Returns the billing input (None for admin) and the owning account id (None for admin).
async fn plane_authz(
    app: &AppState,
    headers: &axum::http::HeaderMap,
    peer: &SocketAddr,
) -> Result<(Option<Tripo3dBillingInput>, Option<String>), Response> {
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
                Some(Tripo3dBillingInput {
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: metered.mult_for(registry::PROVIDER_TRIPO3D),
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
            "tripo3d_auth_authority_unavailable",
        ))),
    }
}

fn gateway_or_404(app: &AppState) -> Result<Arc<Tripo3dGateway>, Response> {
    app.tripo3d
        .clone()
        .ok_or_else(|| json_response(StatusCode::NOT_FOUND, json!({"error": {"type": "not_found", "message": "Not Found"}})))
}

/// `POST /v1/3d/generations` — create one admitted task. The response names OUR task id (the
/// internal request id); the lifecycle then completes detached.
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
    Metrics::inc(&app.metrics.tripo3d_requests);
    let execution = match crate::execution::parse_execution_attempt(&parts.headers) {
        Ok(execution) => execution,
        Err(_) => return error_response(GatewayFailure::BadRequest("tripo3d_execution_identity")),
    };
    let raw = match axum::body::to_bytes(body, GENERATION_BODY_LIMIT).await {
        Ok(raw) => raw,
        Err(_) => return error_response(GatewayFailure::BadRequest("tripo3d_body_too_large")),
    };
    let value: Value = match serde_json::from_slice(&raw) {
        Ok(value) => value,
        Err(_) => return error_response(GatewayFailure::BadRequest("tripo3d_invalid_body")),
    };
    let response = gateway.handle_create(value, execution, billing).await;
    instrument_response(&app, &response);
    response
}

/// One instrumentation point per surface: count the request once it was admitted to the
/// gateway, and classify the failure by its static terminal reason.
fn instrument_response(app: &AppState, response: &Response) {
    if !response.status().is_success() {
        Metrics::inc(&app.metrics.tripo3d_failures);
        if response
            .extensions()
            .get::<crate::proxy::TerminalErrorReason>()
            .is_some_and(|reason| reason.0 == "tripo3d_capacity_exhausted")
        {
            Metrics::inc(&app.metrics.tripo3d_capacity_exhausted);
        }
    }
}

/// `POST /v1/3d/uploads/image` — one image (multipart `file` field, ≤20 MB) through the real
/// `upload/sts` mechanism. Returns the provider `image_token` for task bodies.
pub async fn upload_image(
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
    Metrics::inc(&app.metrics.tripo3d_requests);
    let (filename, bytes) = match read_multipart_file(body, &app, IMAGE_UPLOAD_MAX_BYTES).await {
        Ok(file) => file,
        Err(response) => return response,
    };
    let response = gateway.handle_image_upload(&filename, bytes).await;
    instrument_response(&app, &response);
    response
}

/// `POST /v1/3d/uploads/model` — one model file (multipart `file` field, ≤64 MiB) through the
/// real STS token + S3 flow. Returns a plane `model_token` for `import_model`.
pub async fn upload_model(
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
    Metrics::inc(&app.metrics.tripo3d_requests);
    let (filename, bytes) = match read_multipart_file(body, &app, MODEL_UPLOAD_MAX_BYTES).await {
        Ok(file) => file,
        Err(response) => return response,
    };
    let response = gateway.handle_model_upload(&filename, bytes).await;
    instrument_response(&app, &response);
    response
}

/// Read the single `file` field of a multipart body, bounded. Any other shape is a 400.
async fn read_multipart_file(
    body: Body,
    app: &AppState,
    max_bytes: usize,
) -> Result<(String, Bytes), Response> {
    let request = axum::http::Request::new(body);
    let mut multipart = Multipart::from_request(request, app)
        .await
        .map_err(|_| error_response(GatewayFailure::BadRequest("tripo3d_multipart_invalid")))?;
    let mut found: Option<(String, Bytes)> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name() != Some("file") {
            continue;
        }
        if found.is_some() {
            return Err(error_response(GatewayFailure::BadRequest("tripo3d_multipart_invalid")));
        }
        let filename = field
            .file_name()
            .map(str::to_owned)
            .unwrap_or_else(|| "upload".into());
        let bytes = field
            .bytes()
            .await
            .map_err(|_| error_response(GatewayFailure::BadRequest("tripo3d_multipart_invalid")))?;
        if bytes.is_empty() || bytes.len() > max_bytes {
            return Err(error_response(GatewayFailure::BadRequest("tripo3d_upload_too_large")));
        }
        found = Some((filename, bytes));
    }
    found.ok_or_else(|| error_response(GatewayFailure::BadRequest("tripo3d_file_field_required")))
}

/// `GET /v1/3d/tasks/{task_id}` — the status of OUR task record. Per-account isolation: a
/// foreign id is indistinguishable from an unknown one.
pub async fn task_status(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path(task_id): Path<String>,
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
    match gateway.task_view(&task_id, requester.as_deref()) {
        Some(view) => json_response(
            StatusCode::OK,
            json!({
                "task_id": view.task_id,
                "type": view.task_type,
                "status": view.status,
                "progress": view.progress,
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

/// `GET /v1/3d/tasks/{task_id}/artifact/{name}` — the stored artifact bytes from OUR artifact
/// store. The upstream signed URL is never exposed (manifest §5.4).
pub async fn task_artifact(
    State(app): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Path((task_id, name)): Path<(String, String)>,
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
    let Some(path) = gateway.task_artifact_path(&task_id, &name, requester.as_deref()) else {
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
            return error_response(GatewayFailure::Unavailable("tripo3d_artifact_unavailable"));
        }
    };
    // Stream bounded chunks: the file is ≤ ARTIFACT_MAX_BYTES by construction, and streaming
    // keeps a large model from becoming a memory spike.
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
        .expect("Tripo3D artifact response")
}

#[cfg(test)]
mod tests;