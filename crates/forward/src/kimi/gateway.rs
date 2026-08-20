//! Live KIMI (Kimi Code) generation gateway.
//!
//! KIMI remains an internal backend of the Anthropic plane: exact reviewed subscription aliases
//! dispatch here, while every other Messages request follows the unchanged Claude path. The
//! gateway owns only provider concerns — sealed profile state, native HTTP, placement, one-byte
//! retry policy, terminal usage evidence and graceful stream drain.

use std::collections::{BTreeSet, HashSet};
use std::convert::Infallible;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use anyhow::{anyhow, Context};
use axum::body::Body;
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use bytes::Bytes;
use futures_util::StreamExt;
use kimi_credential::{capabilities_or_base, KimiCredential};
#[cfg(test)]
use metering::kimi::kimi_prices_for_served_model;
use metering::kimi::{
    cost_nanodollars, kimi_matched_tariff_at, kimi_resolve_subscription_model, merge_stream_event,
    KimiPrices, KimiUsage, KIMI_TARIFF_SCHEDULE_ID,
};
use registry::{
    ExecutionAttempt, KimiTurnCalibrationEvent, KIMI_ROLLING_WINDOW_SECS, KIMI_WEEKLY_WINDOW_SECS,
};
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use crate::billing::{AsyncBilling, KimiQuotaSnapshot};
use crate::pricing::tariff_book::{self, PinnedTariff};
use crate::proxy::{local_err_for, HoldGuard, LocalErr};
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};

use super::client::{self, RefreshedTokens};
use super::config::{readiness, KimiPlaneConfig, NotReady};
use super::pool::{decide, AttemptPolicy, Delivery, NextStep, ProfileEffect};
use super::queue::{DeliveryHealth, TurnQueue, WriteOutcome, DEFAULT_QUEUE_CAPACITY};
use super::roster::{load_roster, load_roster_for_reload, reseal_credential, KimiProfile};
use super::selection::{ineligible_ids, select, Candidate, Ineligible, WindowEvidence};
use super::transport::{
    classify_status, needs_refresh, probe_url, ProbeRoute, RefreshLocks, UpstreamVerdict,
};

const GENERATION_PATH: &str = "/messages";
const ERROR_BODY_LIMIT: usize = 64 * 1024;
const RESPONSE_BODY_LIMIT: usize = 32 * 1024 * 1024;
const STREAM_START_MAX_BYTES: usize = 256 * 1024;
const STREAM_START_MAX_CHUNKS: usize = 64;
const STREAM_START_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNSTREAM_SEND_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUESTED_OUTPUT_TOKENS: u64 = 2_000_000;
const AUTH_QUARANTINE_SECS: i64 = 300;
const TRANSPORT_COOL_SECS: i64 = 10;
const RESERVATION_LEASE_SECS: i64 = 3_600;

/// Customer money context already authenticated by the shared Anthropic handler.
#[derive(Clone)]
pub(crate) struct KimiBillingInput {
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub available_nano: i64,
}

pub(crate) struct KimiRequest {
    pub headers: HeaderMap,
    pub body: Value,
    pub raw_body_len: usize,
    pub model: String,
    pub execution: ExecutionAttempt,
    pub billing: Option<KimiBillingInput>,
    pub affinity: Option<AffinityInput>,
    pub affinity_store: Arc<AffinityStore>,
    /// Admin-only calibration target: the exact profile and immutable request id the turn must
    /// use. `None` for ordinary traffic. A pinned turn never rebinds to another profile.
    pub calibration: Option<KimiCalibrationTarget>,
}

/// Exact calibration targeting for the admin-only live runner, validated at dispatch.
pub(crate) struct KimiCalibrationTarget {
    /// Opaque roster id the turn must run on; cooling/walls stay walls.
    pub profile_id: String,
    /// UUIDv4 the durable turn event must carry for exact attribution.
    pub request_id: String,
}

/// Parse the admin-only calibration headers, mirroring the Gemini admission contract.
///
/// Both headers must arrive together and validate; a non-admin caller carrying either is refused,
/// and a malformed or half-present pair fails closed rather than silently running untargeted.
/// `Ok(None)` means no calibration headers were present at all.
pub(crate) fn parse_kimi_calibration_headers(
    headers: &HeaderMap,
    is_admin: bool,
) -> Result<Option<KimiCalibrationTarget>, ()> {
    const PROFILE_HEADER: &str = "x-apitoken-calibration-profile";
    const REQUEST_ID_HEADER: &str = "x-apitoken-calibration-request-id";
    let profile = headers
        .get(PROFILE_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    let request_id = headers
        .get(REQUEST_ID_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(str::trim);
    let (Some(profile), Some(request_id)) = (profile, request_id) else {
        // One header without the other is never meaningful.
        return if profile.is_some() || request_id.is_some() {
            Err(())
        } else {
            Ok(None)
        };
    };
    if !is_admin {
        return Err(());
    }
    if kimi_credential::validate_profile_id(profile).is_err() {
        return Err(());
    }
    let Some(typed) = crate::pricing::EnginePricingRequestId::from_engine_uuid_v4(request_id)
    else {
        return Err(());
    };
    Ok(Some(KimiCalibrationTarget {
        profile_id: profile.to_string(),
        request_id: typed.as_str().to_string(),
    }))
}

/// One retained `/usages` window exactly as last published by the provider. A window that was
/// never observed is absent from the vector entirely; unknown is never zero-filled or invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiQuotaWindowStatus {
    pub duration_secs: i64,
    pub used_units: i64,
    pub limit_units: i64,
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub resets_at: i64,
    pub observed_at: i64,
}

/// Per-profile operational projection for readiness, metrics and the admin endpoint.
///
/// Privacy by construction: the opaque roster id and the bounded plan label are the only
/// identities this struct can carry. The subject, email/phone (KIMI has neither), tokens, proxy,
/// credential paths and raw provider errors never enter it — `RuntimeProfile.subject_id` stays
/// private to the gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiProfileStatus {
    /// Opaque roster id. Documented safe for logs, metrics and admin projections.
    pub id: String,
    /// Exact static provider plan name when the plan is reviewed, otherwise the bounded
    /// `"unreviewed"` placeholder. The raw provider-controlled string is never published.
    pub plan: &'static str,
    /// Authenticated and serving (`/me` passed on this runtime generation).
    pub live: bool,
    /// Cooling axes as unix seconds; `None` means the axis is not cooling right now.
    pub auth_quarantined_until: Option<i64>,
    pub transport_cool_until: Option<i64>,
    pub quota_cool_until: Option<i64>,
    pub inflight: u32,
    /// Last successful `/usages` observation, unix seconds. `None` means never observed.
    pub quota_observed_at: Option<i64>,
    pub quota_windows: Vec<KimiQuotaWindowStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiOperationalStatus {
    pub total_profiles: usize,
    pub live_profiles: usize,
    /// Eligible right now: not auth-quarantined, transport-cooled or quota-walled.
    pub available_profiles: usize,
    pub auth_quarantined_profiles: usize,
    pub transport_cooling_profiles: usize,
    pub quota_cooling_profiles: usize,
    /// Live profiles whose `/me` plan is outside the documented ladder, so they serve base
    /// capabilities only. This must never be inferred from a 429: an unreviewed plan silently
    /// degrading to `kimi-for-coding` is exactly how a whole tier stayed invisible for months.
    /// A non-zero value means a subscription is being under-served until its tier is added.
    pub unreviewed_plan_profiles: usize,
    pub inflight_requests: u64,
    pub profiles: Vec<KimiProfileStatus>,
    pub delivery: DeliveryHealth,
}

/// Bounded plan label for logs, metrics and admin projections.
///
/// Plan names are provider-controlled strings from `/me`. The exact static name is published only
/// for a reviewed plan; anything else collapses to the bounded placeholder so an unreviewed
/// provider string can never become a metric label or an admin-facing value.
pub fn bounded_plan_label(plan: &str) -> &'static str {
    kimi_credential::reviewed_plan_name(plan).unwrap_or("unreviewed")
}

/// A cooling deadline is only meaningful while it is still in the future; an expired or never-set
/// axis is "not cooling", not a timestamp in the past.
fn active_cooling(until: i64, now: i64) -> Option<i64> {
    (until > now).then_some(until)
}

struct CredentialState {
    key_id: String,
    credential: KimiCredential,
}

#[derive(Default)]
struct ProfileHealth {
    authenticated: bool,
    auth_quarantined_until: i64,
    transport_cool_until: i64,
    quota_cool_until: i64,
    quota_used_fraction_units: Option<i64>,
    quota_observed_at: Option<i64>,
    /// Last full `/usages` snapshot, retained for the operational projection. Empty until the
    /// first durable observation; never zero-filled.
    quota_windows: Vec<KimiQuotaWindowStatus>,
    /// Per-model failure axis: a single-model wedge cools only that model on this profile.
    model_failures: std::collections::HashMap<String, ModelFailure>,
}

/// One model's recent failure streak and its cooling deadline on this profile.
#[derive(Default)]
struct ModelFailure {
    streak: u32,
    cool_until: i64,
}

/// Two consecutive failures of the same model cool it on this profile for a bounded minute.
/// The first failure alone only records the streak: a single blip must not blind a model on a
/// single-profile fleet.
const MODEL_FAILURE_COOL_SECS: i64 = 60;

struct RuntimeProfile {
    id: String,
    subject_id: String,
    plan: String,
    credential: AsyncMutex<CredentialState>,
    client: wreq::Client,
    health: Mutex<ProfileHealth>,
    inflight: AtomicU32,
    /// Monotonic start boundary for quota polls. If any customer lease starts while `/usages` is
    /// in flight, that snapshot is discarded even when the turn finishes before the GET returns.
    turn_epoch: AtomicU64,
}

impl RuntimeProfile {
    fn from_roster(profile: KimiProfile, config: &KimiPlaneConfig) -> anyhow::Result<Arc<Self>> {
        let client = client::build_client(
            &profile.credential.proxy_url,
            Duration::from_secs(10),
            config.transport.request_timeout,
        )?;
        Ok(Arc::new(Self {
            id: profile.id,
            subject_id: profile.subject_id,
            plan: profile.plan_name,
            credential: AsyncMutex::new(CredentialState {
                key_id: profile.credential_key_id,
                credential: profile.credential,
            }),
            client,
            health: Mutex::new(ProfileHealth::default()),
            inflight: AtomicU32::new(0),
            turn_epoch: AtomicU64::new(0),
        }))
    }

    fn supports(&self, model: &str) -> bool {
        let capabilities = capabilities_or_base(&self.plan);
        match model.to_ascii_lowercase().as_str() {
            "kimi-for-coding" => true,
            "kimi-for-coding-highspeed" => capabilities.allows_highspeed,
            "k3-256k" => capabilities.allows_k3,
            "k3" | "k3[1m]" => capabilities.allows_k3 && capabilities.allows_1m_context,
            _ => false,
        }
    }

    fn candidate(&self, model: &str, now: i64) -> Candidate {
        let health = self.health.lock().expect("KIMI profile health lock");
        let ineligible = if !self.supports(model) {
            Some(Ineligible::CapabilityNotInPlan)
        } else if health.auth_quarantined_until > now {
            Some(Ineligible::AuthQuarantined)
        } else if health.quota_cool_until > now {
            Some(Ineligible::QuotaWall)
        } else if health.transport_cool_until > now {
            Some(Ineligible::TransportWedged)
        } else if health
            .model_failures
            .get(model)
            .is_some_and(|failure| failure.cool_until > now)
        {
            Some(Ineligible::ModelCooling)
        } else {
            None
        };
        Candidate {
            profile_id: self.id.clone(),
            ineligible,
            window_5h: Self::window_evidence(&health, KIMI_ROLLING_WINDOW_SECS, now),
            window_weekly: Self::window_evidence(&health, KIMI_WEEKLY_WINDOW_SECS, now),
            quota_age_secs: health
                .quota_observed_at
                .map(|observed| now.saturating_sub(observed).max(0)),
            inflight: self.inflight.load(Ordering::Acquire),
        }
    }

    /// R7: оба окна доходят до селектора раздельно. Идентичность окна — его длительность
    /// (±10% допуск, как в наблюдательном срезе server): провайдер изредка сдвигает её на
    /// минуты у границы reset. Reset передаём как секунды до него по часам движка; прошедший
    /// reset клампится в 0 (окно у самого сброса почти бесплатно), отсутствующий — `None`,
    /// и тогда селектор держит полный вес: «неизвестно» ≠ «скоро сбросится».
    fn window_evidence(health: &ProfileHealth, duration_secs: i64, now: i64) -> Option<WindowEvidence> {
        let tolerance = duration_secs / 10;
        let window = health
            .quota_windows
            .iter()
            .filter(|w| (w.duration_secs - duration_secs).abs() <= tolerance)
            .min_by_key(|w| (w.duration_secs - duration_secs).abs())?;
        Some(WindowEvidence {
            used_fraction_units: Some(window.used_fraction_units),
            reset_in_secs: Some(window.resets_at.saturating_sub(now).max(0)),
        })
    }

    fn apply_effect(&self, effect: ProfileEffect, now: i64) {
        let mut health = self.health.lock().expect("KIMI profile health lock");
        match effect {
            ProfileEffect::None => {}
            ProfileEffect::CoolUntilReset => {
                // The exact reset arrives from `/usages` in the next checkpoint. Until then a
                // bounded cool avoids hammering a known wall without permanently losing capacity.
                health.quota_cool_until = now.saturating_add(AUTH_QUARANTINE_SECS);
            }
            ProfileEffect::AuthQuarantine => {
                health.authenticated = false;
                health.auth_quarantined_until = now.saturating_add(AUTH_QUARANTINE_SECS);
            }
            ProfileEffect::TransportFault => {
                health.transport_cool_until = now.saturating_add(TRANSPORT_COOL_SECS);
            }
        }
    }

    /// Apply the decision effect, honoring a provider `Retry-After` hint for a quota wall.
    ///
    /// A wall with an explicit hint cools the quota axis until exactly then instead of the flat
    /// fallback constant; an absent or unparsable hint keeps the fallback. The hint is bounded so
    /// a hostile or broken value cannot park a profile forever.
    fn apply_effect_with_hint(
        &self,
        effect: ProfileEffect,
        now: i64,
        retry_after_secs: Option<i64>,
    ) {
        match (effect, retry_after_secs) {
            (ProfileEffect::CoolUntilReset, Some(seconds)) => {
                let mut health = self.health.lock().expect("KIMI profile health lock");
                health.quota_cool_until = now.saturating_add(seconds);
            }
            (ProfileEffect::TransportFault, Some(seconds)) => {
                let mut health = self.health.lock().expect("KIMI profile health lock");
                health.transport_cool_until = now.saturating_add(seconds);
            }
            _ => self.apply_effect(effect, now),
        }
    }

    fn mark_healthy(&self) {
        let mut health = self.health.lock().expect("KIMI profile health lock");
        health.authenticated = true;
        health.auth_quarantined_until = 0;
        health.transport_cool_until = 0;
    }

    /// A generation failure scoped to one model: two in a row cool that model on this profile.
    /// The profile itself stays eligible for its other models — a broken model path is not a
    /// broken egress.
    fn mark_model_failure(&self, model: &str, now: i64) {
        let mut health = self.health.lock().expect("KIMI profile health lock");
        let failure = health.model_failures.entry(model.to_string()).or_default();
        failure.streak = failure.streak.saturating_add(1);
        if failure.streak >= 2 {
            failure.cool_until = now.saturating_add(MODEL_FAILURE_COOL_SECS);
            failure.streak = 0;
        }
    }

    /// A successful turn on a model clears exactly that model's failure axis.
    fn mark_model_healthy(&self, model: &str) {
        let mut health = self.health.lock().expect("KIMI profile health lock");
        health.model_failures.remove(model);
    }

    fn publish_quota(&self, snapshots: &[KimiQuotaSnapshot], observed_at: i64) {
        let used = snapshots
            .iter()
            .map(|snapshot| snapshot.used_fraction_units)
            .max();
        let cool_until = snapshots
            .iter()
            .filter(|snapshot| snapshot.used_fraction_units >= registry::KIMI_FRACTION_SCALE)
            .map(|snapshot| snapshot.resets_at)
            .max()
            .unwrap_or(0);
        let windows = snapshots
            .iter()
            .map(|snapshot| KimiQuotaWindowStatus {
                duration_secs: snapshot.window_duration_secs,
                used_units: snapshot.native_used_units,
                limit_units: snapshot.native_limit_units,
                used_fraction_units: snapshot.used_fraction_units,
                measurement_resolution_fraction_units: snapshot
                    .measurement_resolution_fraction_units,
                resets_at: snapshot.resets_at,
                observed_at: snapshot.observed_at,
            })
            .collect();
        let mut health = self.health.lock().expect("KIMI profile health lock");
        health.authenticated = true;
        health.auth_quarantined_until = 0;
        health.transport_cool_until = 0;
        health.quota_cool_until = cool_until;
        health.quota_used_fraction_units = used;
        health.quota_observed_at = Some(observed_at);
        health.quota_windows = windows;
    }

    fn authenticated(&self) -> bool {
        self.health
            .lock()
            .expect("KIMI profile health lock")
            .authenticated
    }

    async fn matches_roster(&self, profile: &KimiProfile) -> bool {
        if self.id != profile.id
            || self.subject_id != profile.subject_id
            || self.plan != profile.plan_name
        {
            return false;
        }
        let state = self.credential.lock().await;
        state.key_id == profile.credential_key_id
            && credentials_match(&state.credential, &profile.credential)
    }
}

fn credentials_match(left: &KimiCredential, right: &KimiCredential) -> bool {
    left.version == right.version
        && left.kind == right.kind
        && left.access_token == right.access_token
        && left.refresh_token == right.refresh_token
        && left.expires_at == right.expires_at
        && left.scope == right.scope
        && left.subject_id == right.subject_id
        && left.plan_name == right.plan_name
        && left.plan_level == right.plan_level
        && left.status == right.status
        && left.region == right.region
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

struct Reservation {
    request_id: String,
    account_id: String,
    key: String,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
    /// The hot tariff override version admission priced the hold with; settlement replays exactly
    /// this version. `None` is the compiled constants, byte-identical to before.
    pinned_tariff: Option<PinnedTariff>,
}

#[derive(Clone)]
struct AccountingContext {
    request_id: String,
    requested_model: String,
    context_mode: String,
    reasoning_effort: String,
    priced_ts: i64,
    profile: Arc<RuntimeProfile>,
}

#[derive(Default)]
struct SseAccounting {
    pending: Vec<u8>,
    usage: KimiUsage,
    usage_seen: bool,
    served_model: Option<String>,
    terminal: bool,
}

impl SseAccounting {
    fn push(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > STREAM_START_MAX_BYTES {
            anyhow::bail!("KIMI SSE accounting frame exceeded bound");
        }
        while let Some((at, width)) = event_boundary(&self.pending) {
            let frame = self.pending[..at].to_vec();
            self.pending.drain(..at + width);
            self.consume_frame(&frame);
        }
        Ok(())
    }

    fn finish(&mut self) {
        if !self.pending.is_empty() {
            let frame = std::mem::take(&mut self.pending);
            self.consume_frame(&frame);
        }
    }

    fn consume_frame(&mut self, frame: &[u8]) {
        for line in frame.split(|byte| *byte == b'\n') {
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            let Some(data) = line.strip_prefix(b"data:") else {
                continue;
            };
            let data = trim_ascii(data);
            if data == b"[DONE]" {
                self.terminal = true;
                continue;
            }
            let Ok(value) = serde_json::from_slice::<Value>(data) else {
                continue;
            };
            if value.get("usage").is_some() || value.pointer("/message/usage").is_some() {
                self.usage_seen = true;
            }
            merge_stream_event(&mut self.usage, &value);
            if self.served_model.is_none() {
                self.served_model = value
                    .pointer("/message/model")
                    .or_else(|| value.get("model"))
                    .and_then(Value::as_str)
                    .filter(|model| !model.is_empty())
                    .map(str::to_string);
            }
            if matches!(
                value.get("type").and_then(Value::as_str),
                Some("message_stop" | "message_completed")
            ) {
                self.terminal = true;
            }
        }
    }
}

/// Default-off live KIMI pool. It has no public catalogue or provider mode of its own.
pub struct KimiGateway {
    config: Arc<KimiPlaneConfig>,
    profiles: RwLock<Vec<Arc<RuntimeProfile>>>,
    cursor: AtomicU64,
    refresh_locks: RefreshLocks,
    reload_lock: AsyncMutex<()>,
    billing: Option<Arc<AsyncBilling>>,
    turn_queue: Mutex<TurnQueue>,
    turn_drain: AsyncMutex<()>,
    quota_sweep: AsyncMutex<()>,
    maintenance_abort: Notify,
    background: Arc<ActiveTaskTracker>,
    shutting_down: AtomicBool,
    abort_streams: AtomicBool,
    abort_notify: Notify,
    live_profiles: AtomicUsize,
}

impl KimiGateway {
    pub fn new_with_calibration(
        config: KimiPlaneConfig,
        billing: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        let roster = load_roster(&config.roster_dir, &config.keyring)?;
        let mut profiles = Vec::with_capacity(roster.len());
        for profile in roster {
            profiles.push(RuntimeProfile::from_roster(profile, &config)?);
        }
        Ok(Self::from_profiles(config, billing, profiles))
    }

    /// Keep exact KIMI aliases fail-closed when the initial roster cannot be opened.
    ///
    /// KIMI is an optional backend inside the Anthropic service, so a bad roster must not take
    /// Claude readiness down. It also must not remove the KIMI dispatcher and let those aliases
    /// fall through to the Claude pool. A degraded gateway has zero capacity until a later
    /// last-good roster reload checkpoint can recover it.
    pub fn new_degraded(config: KimiPlaneConfig, billing: Option<Arc<AsyncBilling>>) -> Self {
        Self::from_profiles(config, billing, Vec::new())
    }

    fn from_profiles(
        config: KimiPlaneConfig,
        billing: Option<Arc<AsyncBilling>>,
        profiles: Vec<Arc<RuntimeProfile>>,
    ) -> Self {
        Self {
            config: Arc::new(config),
            profiles: RwLock::new(profiles),
            cursor: AtomicU64::new(0),
            refresh_locks: RefreshLocks::default(),
            reload_lock: AsyncMutex::new(()),
            billing,
            turn_queue: Mutex::new(TurnQueue::new(DEFAULT_QUEUE_CAPACITY)),
            turn_drain: AsyncMutex::new(()),
            quota_sweep: AsyncMutex::new(()),
            maintenance_abort: Notify::new(),
            background: Arc::new(ActiveTaskTracker::default()),
            shutting_down: AtomicBool::new(false),
            abort_streams: AtomicBool::new(false),
            abort_notify: Notify::new(),
            live_profiles: AtomicUsize::new(0),
        }
    }

    /// Resolve a client-supplied model id to the exact subscription alias, accepting the unified
    /// router's namespaced spelling.
    ///
    /// The router advertises `kimi/<alias>`, but this plane only ever stripped its own
    /// `anthropic/` prefix — so a namespaced KIMI id matched no alias, fell through the dispatch
    /// in `proxy.rs`, and was forwarded verbatim to the Claude upstream, which does not know it.
    /// Publishing the namespace without teaching admission to read it made every catalogue-driven
    /// client fail on its first request.
    ///
    /// Returns the bare alias so pricing, attribution and the durable turn event key on the same
    /// identity no matter which spelling the client used.
    pub fn resolve_public_model(model: &str) -> Option<&'static str> {
        let bare = model.strip_prefix("kimi/").unwrap_or(model);
        kimi_resolve_subscription_model(bare).map(|resolved| resolved.alias)
    }

    fn profiles_snapshot(&self) -> Vec<Arc<RuntimeProfile>> {
        self.profiles.read().expect("KIMI profiles lock").clone()
    }

    /// Atomically adopt one fully validated roster generation and retain the last-good snapshot on
    /// every read, decrypt, client-build or identity-probe failure.
    ///
    /// Unchanged profiles keep their exact `Arc`, preserving health, in-flight accounting and HTTP
    /// state. Changed/new profiles are authenticated through `/me` before publication. A final
    /// roster read under every affected per-profile refresh lock prevents a blue-green rotating
    /// refresh from being overwritten by an older credential snapshot.
    pub async fn refresh_profiles(&self) -> bool {
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        let _reload = self.reload_lock.lock().await;
        match self.reload_profiles().await {
            Ok(changed) => changed,
            Err(_) => {
                // Do not render the error: malformed proxy URLs and credential envelopes may
                // contain private egress or token material.
                elog::warn(
                    "kimi",
                    "KIMI encrypted roster refresh skipped; last-good capacity retained",
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
                    if existing.matches_roster(&loaded_profile).await {
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

            for profile in needs_probe {
                self.probe_identity(&profile)
                    .await
                    .map_err(|error| anyhow!("KIMI reload identity class={}", error.class()))?;
                profile.mark_healthy();
            }

            let ids = current
                .iter()
                .chain(&next)
                .map(|profile| profile.id.clone())
                .collect::<BTreeSet<_>>();
            let mut locks = Vec::with_capacity(ids.len());
            for id in ids {
                locks.push(self.refresh_locks.for_profile(&id).await);
            }
            let mut guards = Vec::with_capacity(locks.len());
            for lock in locks {
                guards.push(lock.lock_owned().await);
            }

            let verified = self.load_reload_snapshot(!current.is_empty()).await?;
            if !profiles_match_roster(&next, &verified).await {
                drop(guards);
                continue;
            }
            if self.shutting_down.load(Ordering::Acquire) {
                return Ok(false);
            }
            let live = next
                .iter()
                .filter(|profile| profile.authenticated())
                .count();
            *self.profiles.write().expect("KIMI profiles lock") = next;
            self.live_profiles.store(live, Ordering::Release);
            return Ok(true);
        }
        anyhow::bail!("KIMI roster changed repeatedly during reload")
    }

    async fn load_reload_snapshot(
        &self,
        has_last_good_capacity: bool,
    ) -> anyhow::Result<Vec<KimiProfile>> {
        let root = self.config.roster_dir.clone();
        let keyring = self.config.keyring.clone();
        tokio::task::spawn_blocking(move || {
            load_roster_for_reload(&root, &keyring, has_last_good_capacity)
        })
        .await
        .map_err(|_| anyhow!("KIMI roster reader stopped"))?
    }

    pub async fn preflight(&self) -> usize {
        let mut live = 0usize;
        for profile in self.profiles_snapshot() {
            match self.probe_identity(&profile).await {
                Ok(()) => {
                    profile.mark_healthy();
                    live += 1;
                }
                Err(error) => {
                    // Classification only: never print provider bodies, subject, proxy or tokens.
                    elog::warn(
                        "kimi",
                        format!(
                            "KIMI identity preflight failed profile={} class={}",
                            profile.id,
                            error.class()
                        ),
                    );
                }
            }
        }
        self.live_profiles.store(live, Ordering::Release);
        live
    }

    pub fn quota_poll_interval(&self) -> Duration {
        self.config.quota_poll_interval
    }

    /// Poll every currently published idle profile without imposing a customer concurrency cap.
    /// Any lease that starts while `/usages` is in flight invalidates that profile's snapshot;
    /// customer traffic is never queued or rejected merely because maintenance is reading quota.
    pub async fn poll_quotas(&self) -> usize {
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        let _sweep = self.quota_sweep.lock().await;
        if self.shutting_down.load(Ordering::Acquire) {
            return 0;
        }
        self.poll_quota_generation(false).await
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
            Ok(snapshot) if !snapshot.windows.is_empty() => snapshot,
            Ok(_) => {
                elog::warn(
                    "kimi",
                    format!(
                        "KIMI quota poll returned no usable windows profile={}",
                        profile.id
                    ),
                );
                return false;
            }
            Err(error) => {
                let effect = match error.verdict() {
                    UpstreamVerdict::Auth => ProfileEffect::AuthQuarantine,
                    UpstreamVerdict::QuotaExhausted => ProfileEffect::CoolUntilReset,
                    UpstreamVerdict::Transport | UpstreamVerdict::MembershipTemporary => {
                        ProfileEffect::TransportFault
                    }
                    UpstreamVerdict::Ok | UpstreamVerdict::ClientError => ProfileEffect::None,
                };
                profile.apply_effect(effect, now_unix());
                self.refresh_live_profile_count();
                elog::warn(
                    "kimi",
                    format!(
                        "KIMI quota poll failed profile={} class={}",
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
            // The provider snapshot can already include that turn while its durable spend is not
            // yet paired. Discard the whole generation; the next idle poll will observe both.
            return false;
        }
        let observed_at = now_unix();
        let snapshots = snapshot
            .windows
            .into_iter()
            .map(|window| KimiQuotaSnapshot {
                window_duration_secs: window.duration_secs,
                window_name: window.name,
                resets_at: window.resets_at,
                observed_at,
                native_used_units: window.used_units,
                native_limit_units: window.limit_units,
                used_fraction_units: window.used_fraction_units,
                measurement_resolution_fraction_units: window.measurement_resolution_fraction_units,
            })
            .collect::<Vec<_>>();

        // Enqueue takes this same barrier before pushing a turn. Once acquired, a full drain stays
        // full until every observation has crossed the serial writer and its CAS.
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
            .observe_kimi_windows(&profile.subject_id, &profile.plan, snapshots.clone())
            .await
        {
            elog::error(
                "kimi",
                format!(
                    "KIMI quota observation persistence deferred profile={}: {error:#}",
                    profile.id
                ),
            );
            return false;
        }

        // Steering sees a snapshot only after every independent window is durable. A transient
        // turn/observation/CAS failure therefore retains the exact previous quota generation.
        profile.publish_quota(&snapshots, observed_at);
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

    async fn fetch_quota(
        &self,
        profile: &Arc<RuntimeProfile>,
        during_shutdown: bool,
    ) -> Result<client::QuotaSnapshot, GatewayFailure> {
        let mut rejected_token = None;
        loop {
            let token = if during_shutdown {
                // Never begin or cancel a rotating refresh family under the process deadline. A
                // final free poll may use a still-valid token; steady maintenance owns refresh.
                if rejected_token.is_some() {
                    return Err(GatewayFailure::Auth);
                }
                let state = profile.credential.lock().await;
                if needs_refresh(&state.credential, now_unix(), &self.config.transport) {
                    return Err(GatewayFailure::Unavailable("kimi_refresh_deferred"));
                }
                state.credential.access_token.clone()
            } else {
                self.access_token(profile, rejected_token.as_deref())
                    .await?
            };
            let send = profile
                .client
                .get(probe_url(&self.config.transport, ProbeRoute::Usage))
                .header(
                    self.config.transport.auth_scheme.header_name(),
                    self.config.transport.auth_scheme.header_value(&token),
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
                        return Err(GatewayFailure::Unavailable("kimi_shutdown"));
                    }
                }
            }
            .map_err(|_| GatewayFailure::Transport)?;
            let status = response.status().as_u16();
            let verdict = classify_status(status);
            if verdict == UpstreamVerdict::Auth && rejected_token.is_none() {
                drop_bounded(response, ERROR_BODY_LIMIT).await;
                rejected_token = Some(token);
                continue;
            }
            if verdict != UpstreamVerdict::Ok {
                drop_bounded(response, ERROR_BODY_LIMIT).await;
                return Err(GatewayFailure::from_verdict(verdict, status));
            }
            let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
            let payload: Value =
                serde_json::from_slice(&body).map_err(|_| GatewayFailure::Protocol)?;
            return client::parse_usage(&payload).map_err(|_| GatewayFailure::Protocol);
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

    /// Read only cached operational state. Metrics collection and the admin projection never
    /// start a network request here.
    pub fn operational_status(&self) -> KimiOperationalStatus {
        let delivery = self
            .turn_queue
            .lock()
            .expect("KIMI turn queue lock")
            .health();
        let now = now_unix();
        let profiles = self.profiles.read().expect("KIMI profiles lock");
        let mut auth_quarantined_profiles = 0;
        let mut transport_cooling_profiles = 0;
        let mut quota_cooling_profiles = 0;
        let mut unreviewed_plan_profiles = 0;
        let mut inflight_requests = 0u64;
        let mut statuses = Vec::with_capacity(profiles.len());
        for profile in profiles.iter() {
            let health = profile.health.lock().expect("KIMI profile health lock");
            let auth_until = active_cooling(health.auth_quarantined_until, now);
            let transport_until = active_cooling(health.transport_cool_until, now);
            let quota_until = active_cooling(health.quota_cool_until, now);
            auth_quarantined_profiles += usize::from(auth_until.is_some());
            transport_cooling_profiles += usize::from(transport_until.is_some());
            quota_cooling_profiles += usize::from(quota_until.is_some());
            let inflight = profile.inflight.load(Ordering::Acquire);
            inflight_requests += u64::from(inflight);
            // Count only live profiles: a dead credential is already reported on its own axis, and
            // folding it in here would hide the tier problem behind an auth problem.
            if health.authenticated && kimi_credential::reviewed_plan_name(&profile.plan).is_none()
            {
                unreviewed_plan_profiles += 1;
            }
            statuses.push(KimiProfileStatus {
                id: profile.id.clone(),
                plan: bounded_plan_label(&profile.plan),
                live: health.authenticated,
                auth_quarantined_until: auth_until,
                transport_cool_until: transport_until,
                quota_cool_until: quota_until,
                inflight,
                quota_observed_at: health.quota_observed_at,
                quota_windows: health.quota_windows.clone(),
            });
        }
        drop(profiles);
        // Availability reuses the selection ineligibility view. `kimi-for-coding` is served by
        // every plan, so only the three cooling axes — never a capability gap — can mark a
        // profile ineligible here.
        let candidates: Vec<Candidate> = self
            .profiles
            .read()
            .expect("KIMI profiles lock")
            .iter()
            .map(|profile| profile.candidate("kimi-for-coding", now))
            .collect();
        let available_profiles = candidates.len() - ineligible_ids(&candidates).len();
        KimiOperationalStatus {
            total_profiles: candidates.len(),
            live_profiles: self.live_profiles.load(Ordering::Acquire),
            available_profiles,
            auth_quarantined_profiles,
            transport_cooling_profiles,
            quota_cooling_profiles,
            unreviewed_plan_profiles,
            inflight_requests,
            profiles: statuses,
            delivery,
        }
    }

    /// Resolve a durable calibration subject to its opaque roster id. The subject itself never
    /// leaves the gateway; rows whose subject is no longer in the roster resolve to `None` so the
    /// caller drops them instead of serializing an unresolvable identity.
    pub fn profile_id_for_subject(&self, subject_id: &str) -> Option<String> {
        self.profiles
            .read()
            .expect("KIMI profiles lock")
            .iter()
            .find(|profile| profile.subject_id == subject_id)
            .map(|profile| profile.id.clone())
    }

    /// Resolve a durable calibration subject to its opaque roster id and the raw provider plan
    /// the profile currently carries. Calibration rows are keyed subject+plan+duration, so after
    /// a plan change the subject holds rows of both cohorts and only the current one is this
    /// profile's money. The raw plan is used only for that selection — like the subject, it is
    /// never serialized.
    pub fn profile_id_and_plan_for_subject(&self, subject_id: &str) -> Option<(String, String)> {
        self.profiles
            .read()
            .expect("KIMI profiles lock")
            .iter()
            .find(|profile| profile.subject_id == subject_id)
            .map(|profile| (profile.id.clone(), profile.plan.clone()))
    }

    pub fn readiness(&self) -> Result<(), NotReady> {
        let status = self.operational_status();
        readiness(status.live_profiles, status.delivery.persistence_ok)
    }

    async fn probe_identity(&self, profile: &Arc<RuntimeProfile>) -> Result<(), GatewayFailure> {
        let mut rejected_token = None;
        loop {
            let token = self
                .access_token(profile, rejected_token.as_deref())
                .await?;
            let response = profile
                .client
                .get(probe_url(&self.config.transport, ProbeRoute::Identity))
                .header(
                    self.config.transport.auth_scheme.header_name(),
                    self.config.transport.auth_scheme.header_value(&token),
                )
                .header("accept", "application/json")
                .send()
                .await
                .map_err(|_| GatewayFailure::Transport)?;
            let status = response.status().as_u16();
            let verdict = classify_status(status);
            if verdict == UpstreamVerdict::Auth && rejected_token.is_none() {
                // Local expiry is only a hint. A freshly-looking access token can be revoked by
                // the provider; force the same rotating refresh path used by generation and retry
                // identity exactly once before declaring the profile dead.
                drop_bounded(response, ERROR_BODY_LIMIT).await;
                rejected_token = Some(token);
                continue;
            }
            if verdict != UpstreamVerdict::Ok {
                return Err(GatewayFailure::from_verdict(verdict, status));
            }
            let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
            let value: Value =
                serde_json::from_slice(&body).map_err(|_| GatewayFailure::Protocol)?;
            let subject = value
                .get("user_id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let plan = value
                .get("user_level_name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let status = value
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if subject != profile.subject_id
                || plan != profile.plan
                || status != kimi_credential::KIMI_STATUS_NORMAL
            {
                return Err(GatewayFailure::Auth);
            }
            return Ok(());
        }
    }

    async fn access_token(
        &self,
        profile: &Arc<RuntimeProfile>,
        rejected_token: Option<&str>,
    ) -> Result<String, GatewayFailure> {
        let profile_lock = self.refresh_locks.for_profile(&profile.id).await;
        let _refresh = profile_lock.lock().await;
        let mut state = profile.credential.lock().await;
        let now = now_unix();
        let can_reuse =
            rejected_token.is_none_or(|rejected| state.credential.access_token != rejected);
        if can_reuse && !needs_refresh(&state.credential, now, &self.config.transport) {
            return Ok(state.credential.access_token.clone());
        }
        match self.refresh_once(profile, &mut state, now).await {
            Ok(token) => Ok(token),
            Err(GatewayFailure::Auth) => {
                // Another blue-green generation may have won the rotating refresh family. Disk is
                // the shared authority: reuse its newer token, or retry once with its newer family.
                let roster_dir = self.config.roster_dir.clone();
                let keyring = self.config.keyring.clone();
                let profile_id = profile.id.clone();
                let fresh = tokio::task::spawn_blocking(move || {
                    load_roster(&roster_dir, &keyring).and_then(|profiles| {
                        profiles
                            .into_iter()
                            .find(|candidate| candidate.id == profile_id)
                            .ok_or_else(|| anyhow!("KIMI profile disappeared during refresh"))
                    })
                })
                .await
                .map_err(|_| GatewayFailure::Transport)?
                .map_err(|_| GatewayFailure::Auth)?;
                if fresh.credential.refresh_token == state.credential.refresh_token {
                    return Err(GatewayFailure::Auth);
                }
                state.key_id = fresh.credential_key_id;
                state.credential = fresh.credential;
                let now = now_unix();
                if !needs_refresh(&state.credential, now, &self.config.transport) {
                    return Ok(state.credential.access_token.clone());
                }
                self.refresh_once(profile, &mut state, now).await
            }
            Err(error) => Err(error),
        }
    }

    async fn refresh_once(
        &self,
        profile: &Arc<RuntimeProfile>,
        state: &mut CredentialState,
        now: i64,
    ) -> Result<String, GatewayFailure> {
        if !matches!(
            state.credential.kind,
            kimi_credential::KimiCredentialKind::Oauth
        ) {
            return Ok(state.credential.access_token.clone());
        }
        let form =
            serde_urlencoded::to_string(client::refresh_form(&state.credential.refresh_token))
                .map_err(|_| GatewayFailure::Protocol)?;
        let response = profile
            .client
            .post(client::refresh_url())
            .header(
                "content-type",
                "application/x-www-form-urlencoded;charset=UTF-8",
            )
            .body(form)
            .send()
            .await
            .map_err(|_| GatewayFailure::Transport)?;
        if !response.status().is_success() {
            return Err(if matches!(response.status().as_u16(), 400 | 401) {
                GatewayFailure::Auth
            } else {
                GatewayFailure::Transport
            });
        }
        let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
        let value: Value = serde_json::from_slice(&body).map_err(|_| GatewayFailure::Protocol)?;
        let RefreshedTokens {
            access_token,
            refresh_token,
            expires_at,
            scope,
        } = client::parse_refresh(&value, now).map_err(|_| GatewayFailure::Protocol)?;
        let mut rotated = state.credential.clone();
        rotated
            .rotate(access_token, refresh_token, expires_at, scope)
            .map_err(|_| GatewayFailure::Protocol)?;
        let roster_dir = self.config.roster_dir.clone();
        let keyring = self.config.keyring.clone();
        let key_id = state.key_id.clone();
        let profile_id = profile.id.clone();
        let to_seal = rotated.clone();
        tokio::task::spawn_blocking(move || {
            reseal_credential(&roster_dir, &keyring, &key_id, &profile_id, &to_seal)
        })
        .await
        .map_err(|_| GatewayFailure::Transport)?
        .map_err(|_| GatewayFailure::Auth)?;
        let token = rotated.access_token.clone();
        state.credential = rotated;
        Ok(token)
    }

    pub(crate) async fn handle(self: &Arc<Self>, mut request: KimiRequest) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("kimi_shutdown"));
        }
        // A strict-policy key is refused here, before any pricing is attempted, and the refusal is
        // terminal rather than retryable.
        //
        // Removing this on 2026-08-09 looked justified — after the release-v2 retirement a model
        // outside the pinned catalog bills on the account's legacy multiplier — and it was wrong.
        // There is a second gate behind the one I checked: the strict reserve writer itself
        // demands a `policy_v1` admission snapshot, which only the providers that resolve a
        // catalog-backed policy can build. KIMI reserves on the plain legacy writer, so the
        // request reached PostgreSQL and died there with `strict reservation lacks a policy_v1
        // admission snapshot`, surfacing to the customer as a 529 with `Retry-After` — a
        // retryable error for a permanently deterministic condition, which is exactly the shape
        // this repository fixed everywhere else.
        //
        // Serving a strict key therefore needs catalog membership for `kimi`, not a smaller
        // change. Until that exists the honest answer is this one refusal.
        if let Err(error) = validate_priced_surface(&request.body) {
            return error_response(error);
        }
        let context_mode = context_mode(&request.model).to_string();
        // Our catalogue alias is not always what the provider accepts. `k3[1m]` is the bracket
        // convention borrowed from the Claude catalogue; the subscription endpoint has no such id
        // and rejects it, so forwarding the alias verbatim turned every such request into an
        // upstream failure that surfaced as a capacity 429. Rewrite to the wire id once, here,
        // before anything reads the body — selection, pricing and the durable event keep using the
        // requested alias, so attribution and the rate card are unchanged.
        if let Some(resolved) = metering::kimi_resolve_subscription_model(&request.model) {
            if resolved.wire_model != resolved.alias {
                request.body["model"] = json!(resolved.wire_model);
            }
        }
        let reasoning_effort = match reasoning_effort(&request.body) {
            Ok(value) => value,
            Err(error) => return error_response(error),
        };
        // The admin calibration runner preselects its immutable request id; ordinary traffic
        // always mints a fresh CSPRNG one.
        let request_id = request
            .calibration
            .as_ref()
            .map(|target| target.request_id.clone())
            .unwrap_or_else(crate::upstream::fresh_request_id);
        let priced_ts = now_unix();
        let mut reservation = match self
            .reserve_customer(
                &mut request.body,
                request.raw_body_len,
                &request.model,
                &request_id,
                priced_ts,
                request.billing.as_ref(),
                request.execution.clone(),
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
        let body = match serde_json::to_vec(&request.body) {
            Ok(body) => Bytes::from(body),
            Err(_) => return error_response(GatewayFailure::BadRequest("invalid_json")),
        };
        let stream_requested = request
            .body
            .get("stream")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let mut affinity_resolution = match request.affinity.as_ref() {
            Some(input) => request.affinity_store.resolve(input).await,
            None => None,
        };
        let sticky = affinity_resolution.as_ref().and_then(|resolution| {
            self.profile_id_for_home(&request.affinity_store, &resolution.home)
        });
        // A brand-new conversation gets a soft warm-home preference for attempt 0: reusing the
        // profile already holding this tenant's cache root keeps provider cache pricing warm.
        // The hint never outranks eligibility — it only orders otherwise-equal candidates.
        let mut warm_preference: Option<String> = None;
        if sticky.is_none() {
            if let Some(input) = request.affinity.as_ref() {
                warm_preference = request
                    .affinity_store
                    .warm_homes(input)
                    .await
                    .iter()
                    .find_map(|home| self.profile_id_for_home(&request.affinity_store, home));
                request
                    .affinity_store
                    .record_cache_root_placement(input, warm_preference.is_some());
            }
        }
        let placement = sticky.as_deref().or(warm_preference.as_deref());
        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();

        let pinned = request
            .calibration
            .as_ref()
            .map(|target| target.profile_id.clone());
        // Smooth-wait deadline, mirroring the Claude/Gemini/Codex planes. It is spent only on a
        // round that never reached the provider: every profile ineligible means the caller would
        // otherwise get an immediate synthetic 429 for a condition that routinely clears in
        // milliseconds — a transport cool, a single-flight refresh, a blue-green generation swap.
        // A real provider verdict is never waited on.
        let capacity_deadline = std::time::Instant::now() + self.config.smooth_wait;
        loop {
            let Some(profile) = (match pinned.as_deref() {
                // Exact calibration target: the pinned profile is the only candidate, and only
                // while it has not been attempted yet — a calibration turn never rebinds.
                Some(id) if excluded.is_empty() => self
                    .profiles_snapshot()
                    .into_iter()
                    .find(|profile| profile.id == id)
                    .filter(|profile| {
                        profile
                            .candidate(&request.model, now_unix())
                            .ineligible
                            .is_none()
                    }),
                Some(_) => None,
                None => self.select_profile(&request.model, &excluded, placement),
            }) else {
                // Only a first, un-attempted round may wait: once a candidate has been excluded we
                // already carry a provider verdict, and the honest answer is the wall itself.
                let remaining = excluded
                    .is_empty()
                    .then(|| capacity_deadline.saturating_duration_since(std::time::Instant::now()))
                    .unwrap_or_default();
                if let Some(step) = crate::proxy::smooth_step(0, remaining.as_millis())
                    .filter(|_| !self.shutting_down.load(Ordering::Acquire))
                {
                    tokio::time::sleep(step).await;
                    continue;
                }
                elog::warn("kimi", "kimi pool exhausted: no profile");
                return error_response(GatewayFailure::Capacity);
            };
            // Early claim: a new conversation's home is registered before the first attempt, so
            // concurrent first turns of the same conversation cannot double-home. An existing
            // resolution is untouched until a successful first byte rebinds it at commit time.
            if affinity_resolution.is_none() {
                if let Some(input) = request.affinity.as_ref() {
                    let home = request.affinity_store.home_id(&profile.id);
                    affinity_resolution = Some(request.affinity_store.claim(input, &home).await);
                }
            }
            let lease = ProfileLease::new(profile.clone());
            let mut rejected_token: Option<String> = None;
            loop {
                let token = match self.access_token(&profile, rejected_token.as_deref()).await {
                    Ok(token) => token,
                    Err(error) => {
                        elog::error("kimi", format!("kimi upstream transport failed: {error:?}"));
                        let verdict = error.verdict();
                        let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
                        let decision = decide(verdict, Delivery::PreByte, policy, remaining);
                        self.apply_effect_and_hint(
                            &request.affinity_store,
                            &profile,
                            decision.effect,
                            None,
                        );
                        policy = decision.policy;
                        if decision.next == NextStep::RotateToAnotherProfile {
                            excluded.insert(profile.id.clone());
                            break;
                        }
                        return error_response(error);
                    }
                };
                let response = match self
                    .send_generation(
                        &profile,
                        &request.headers,
                        body.clone(),
                        stream_requested,
                        &request_id,
                        &token,
                    )
                    .await
                {
                    Ok(response) => response,
                    Err(error) => {
                        elog::error("kimi", format!("kimi upstream transport failed: {error:?}"));
                        let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
                        let decision = decide(
                            UpstreamVerdict::Transport,
                            Delivery::PreByte,
                            policy,
                            remaining,
                        );
                        profile.apply_effect(decision.effect, now_unix());
                        policy = decision.policy;
                        if decision.next == NextStep::RotateToAnotherProfile {
                            excluded.insert(profile.id.clone());
                            break;
                        }
                        return error_response(error);
                    }
                };
                let status = response.status().as_u16();
                // A provider throttle/wall hint is honored before the response is dropped: the
                // quota axis then cools until exactly the hinted instant, not a flat fallback.
                let retry_after = retry_after_seconds(response.headers());
                let verdict = classify_status(status);
                let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
                elog::warn("kimi", format!("kimi upstream refused: {status}"));
                let decision = decide(verdict, Delivery::PreByte, policy, remaining);
                self.apply_effect_and_hint(
                    &request.affinity_store,
                    &profile,
                    decision.effect,
                    retry_after,
                );
                policy = decision.policy;
                match decision.next {
                    NextStep::RefreshAndRetrySameProfile => {
                        rejected_token = Some(token);
                        drop_bounded(response, ERROR_BODY_LIMIT).await;
                        continue;
                    }
                    NextStep::RotateToAnotherProfile => {
                        excluded.insert(profile.id.clone());
                        drop_bounded(response, ERROR_BODY_LIMIT).await;
                        break;
                    }
                    NextStep::SurfaceCapacityExhausted => {
                        drop_bounded(response, ERROR_BODY_LIMIT).await;
                        return error_response(GatewayFailure::Capacity);
                    }
                    NextStep::SurfaceUpstreamError => {
                        drop_bounded(response, ERROR_BODY_LIMIT).await;
                        return error_response(GatewayFailure::Upstream(status));
                    }
                    NextStep::Deliver => {
                        let accounting = AccountingContext {
                            request_id: request_id.clone(),
                            requested_model: request.model.clone(),
                            context_mode: context_mode.clone(),
                            reasoning_effort: reasoning_effort.clone(),
                            priced_ts,
                            profile: profile.clone(),
                        };
                        if stream_requested || response_is_sse(&response) {
                            let background = match self.background.track() {
                                Some(guard) => guard,
                                None => {
                                    return error_response(GatewayFailure::Unavailable(
                                        "kimi_shutdown",
                                    ))
                                }
                            };
                            let headers = response_headers(&response);
                            let status =
                                StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
                            let mut upstream = response.bytes_stream();
                            let startup = tokio::time::timeout(STREAM_START_TIMEOUT, async {
                                let mut bytes_seen = 0usize;
                                for _ in 0..STREAM_START_MAX_CHUNKS {
                                    let chunk = upstream
                                        .next()
                                        .await
                                        .ok_or(GatewayFailure::Protocol)?
                                        .map_err(|_| GatewayFailure::Transport)?;
                                    bytes_seen = bytes_seen.saturating_add(chunk.len());
                                    if bytes_seen > STREAM_START_MAX_BYTES {
                                        return Err(GatewayFailure::Protocol);
                                    }
                                    if !chunk.is_empty() {
                                        return Ok(chunk);
                                    }
                                }
                                Err(GatewayFailure::Protocol)
                            })
                            .await;
                            let initial = match startup {
                                Ok(Ok(initial)) => initial,
                                // A 2xx that never yields a usable first byte is a pre-byte
                                // transport fault: cool the profile, spend the bounded transport
                                // budget and rotate when another profile is eligible — a wedged
                                // profile must not stay instantly re-selectable.
                                Ok(Err(error)) => {
                                    elog::error(
                                        "kimi",
                                        format!("kimi stream start failed: {error:?}"),
                                    );
                                    let remaining =
                                        self.eligible_count(&request.model, &excluded, &profile.id);
                                    let decision = decide(
                                        UpstreamVerdict::Transport,
                                        Delivery::PreByte,
                                        policy,
                                        remaining,
                                    );
                                    // A 2xx that never becomes a usable first byte is the model
                                    // path wedging, not the egress: cool this model on this
                                    // profile and leave its other models eligible.
                                    profile.mark_model_failure(&request.model, now_unix());
                                    self.publish_cooling_deadline(
                                        &request.affinity_store,
                                        &profile,
                                    );
                                    policy = decision.policy;
                                    match decision.next {
                                        NextStep::RotateToAnotherProfile => {
                                            excluded.insert(profile.id.clone());
                                            break;
                                        }
                                        NextStep::SurfaceCapacityExhausted => {
                                            return error_response(GatewayFailure::Capacity);
                                        }
                                        _ => return error_response(error),
                                    }
                                }
                                Err(_) => {
                                    elog::error("kimi", "kimi stream start failed: timeout");
                                    let remaining =
                                        self.eligible_count(&request.model, &excluded, &profile.id);
                                    let decision = decide(
                                        UpstreamVerdict::Transport,
                                        Delivery::PreByte,
                                        policy,
                                        remaining,
                                    );
                                    // A 2xx that never becomes a usable first byte is the model
                                    // path wedging, not the egress: cool this model on this
                                    // profile and leave its other models eligible.
                                    profile.mark_model_failure(&request.model, now_unix());
                                    self.publish_cooling_deadline(
                                        &request.affinity_store,
                                        &profile,
                                    );
                                    policy = decision.policy;
                                    match decision.next {
                                        NextStep::RotateToAnotherProfile => {
                                            excluded.insert(profile.id.clone());
                                            break;
                                        }
                                        NextStep::SurfaceCapacityExhausted => {
                                            return error_response(GatewayFailure::Capacity);
                                        }
                                        _ => return error_response(GatewayFailure::Transport),
                                    }
                                }
                            };
                            let mut sse = SseAccounting::default();
                            if sse.push(&initial).is_err() {
                                elog::error("kimi", "kimi stream start failed: protocol");
                                return error_response(GatewayFailure::Protocol);
                            }
                            if !self.mark_delivering(reservation.as_ref()).await {
                                // Upstream may still consume the turn. Drain and preserve provider
                                // evidence, but keep the customer hold guard armed for a refund.
                                let _ = self.spawn_stream(
                                    background, lease, accounting, None, sse, initial, upstream,
                                    false,
                                );
                                elog::error("kimi", "kimi delivery marker unavailable");
                                return error_response(GatewayFailure::Unavailable(
                                    "kimi_delivery_marker_unavailable",
                                ));
                            }
                            if let Some(guard) = hold_guard.as_mut() {
                                guard.disarm();
                            }
                            self.commit_affinity(
                                &request.affinity_store,
                                request.affinity.as_ref(),
                                &mut affinity_resolution,
                                &profile,
                            )
                            .await;
                            let response = self.spawn_stream(
                                background,
                                lease,
                                accounting,
                                reservation.take(),
                                sse,
                                initial,
                                upstream,
                                true,
                            );
                            return response_with(status, headers, response);
                        }

                        let headers = response_headers(&response);
                        let body = match read_bounded(response, RESPONSE_BODY_LIMIT).await {
                            Ok(body) => body,
                            // Nothing reached the client yet, so a failed body read is a pre-byte
                            // fault with the same rotation contract as a connect error. A 2xx
                            // whose body breaks is the model path, not the egress: cool this
                            // model on this profile and leave its other models eligible.
                            Err(error) => {
                                elog::error(
                                    "kimi",
                                    format!("kimi response body read failed: {error:?}"),
                                );
                                let remaining =
                                    self.eligible_count(&request.model, &excluded, &profile.id);
                                let decision = decide(
                                    UpstreamVerdict::Transport,
                                    Delivery::PreByte,
                                    policy,
                                    remaining,
                                );
                                profile.mark_model_failure(&request.model, now_unix());
                                self.publish_cooling_deadline(&request.affinity_store, &profile);
                                policy = decision.policy;
                                match decision.next {
                                    NextStep::RotateToAnotherProfile => {
                                        excluded.insert(profile.id.clone());
                                        break;
                                    }
                                    NextStep::SurfaceCapacityExhausted => {
                                        return error_response(GatewayFailure::Capacity);
                                    }
                                    _ => return error_response(error),
                                }
                            }
                        };
                        if !self.mark_delivering(reservation.as_ref()).await {
                            let parsed = non_stream_accounting(&body);
                            self.finalize_turn(&accounting, None, parsed).await;
                            elog::error("kimi", "kimi delivery marker unavailable");
                            return error_response(GatewayFailure::Unavailable(
                                "kimi_delivery_marker_unavailable",
                            ));
                        }
                        if let Some(guard) = hold_guard.as_mut() {
                            guard.disarm();
                        }
                        self.commit_affinity(
                            &request.affinity_store,
                            request.affinity.as_ref(),
                            &mut affinity_resolution,
                            &profile,
                        )
                        .await;
                        let parsed = non_stream_accounting(&body);
                        self.finalize_turn(&accounting, reservation.take(), parsed)
                            .await;
                        profile.mark_healthy();
                        profile.mark_model_healthy(&request.model);
                        self.live_profiles.store(
                            self.profiles_snapshot()
                                .iter()
                                .filter(|candidate| candidate.authenticated())
                                .count(),
                            Ordering::Release,
                        );
                        return response_with(
                            StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                            headers,
                            Body::from(body),
                        );
                    }
                }
            }
            drop(lease);
        }
    }

    async fn reserve_customer(
        &self,
        body: &mut Value,
        raw_len: usize,
        model: &str,
        request_id: &str,
        priced_ts: i64,
        input: Option<&KimiBillingInput>,
        execution: ExecutionAttempt,
    ) -> Result<Option<Reservation>, GatewayFailure> {
        let Some(input) = input else {
            return Ok(None);
        };
        let billing = self.billing.as_ref().ok_or(GatewayFailure::Unavailable(
            "kimi_billing_authority_unavailable",
        ))?;
        let resolved = kimi_resolve_subscription_model(model)
            .ok_or(GatewayFailure::Unsupported("kimi_model_unavailable"))?;
        // Hot tariff override: the matched family of the official served model resolves against
        // the process-wide book; an override replaces only the base vector and pins
        // `<family>/v<version>` for settlement.
        let (family, compiled) = kimi_matched_tariff_at(resolved.official_model, priced_ts)
            .ok_or(GatewayFailure::Unsupported("kimi_price_unavailable"))?;
        let resolved_base = tariff_book::reserve_base(
            &tariff_book::snapshot(),
            family,
            priced_ts,
            compiled,
            tariff_book::as_kimi,
        );
        let prices = resolved_base.prices;
        let requested_max = bounded_requested_output(body);
        let mut balance = i128::from(input.available_nano);
        for _ in 0..4 {
            let Some((effective_max, hold)) = cap_to_balance(
                balance,
                raw_len.max(1) as i128,
                prices,
                input.mult_bp,
                requested_max,
            ) else {
                return Err(GatewayFailure::LowBalance);
            };
            match billing
                .reserve_priced_request_for_execution(
                    request_id,
                    &input.account_id,
                    &input.key,
                    hold,
                    execution.clone(),
                    registry::PROVIDER_KIMI,
                    input.mult_bp,
                )
                .await
                .map_err(|error| {
                    elog::error("kimi", "kimi reservation failed");
                    let _ = error;
                    GatewayFailure::Unavailable("kimi_reservation_unavailable")
                })? {
                Some(_) => {
                    if effective_max < requested_max {
                        body["max_tokens"] = json!(effective_max);
                    }
                    return Ok(Some(Reservation {
                        request_id: request_id.to_string(),
                        account_id: input.account_id.clone(),
                        key: input.key.clone(),
                        hold,
                        mult_bp: input.mult_bp,
                        priced_ts,
                        pinned_tariff: resolved_base.pin.clone(),
                    }));
                }
                None => {
                    balance = billing
                        .account(&input.account_id)
                        .await
                        .map_err(|error| {
                            elog::error("kimi", "kimi balance read failed");
                            let _ = error;
                            GatewayFailure::Unavailable("kimi_balance_unavailable")
                        })?
                        .map(|account| i128::from(account.balance_nano))
                        .unwrap_or(0);
                }
            }
        }
        Err(GatewayFailure::LowBalance)
    }

    async fn send_generation(
        &self,
        profile: &RuntimeProfile,
        client_headers: &HeaderMap,
        body: Bytes,
        stream: bool,
        request_id: &str,
        token: &str,
    ) -> Result<wreq::Response, GatewayFailure> {
        let url = format!(
            "{}{}",
            self.config.transport.base_url.trim_end_matches('/'),
            GENERATION_PATH
        );
        let mut request = profile
            .client
            .post(url)
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config.transport.auth_scheme.header_value(token),
            )
            .header("content-type", "application/json")
            .header(
                "accept",
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .header("x-client-request-id", request_id);
        for name in ["anthropic-version", "anthropic-beta"] {
            if let Some(value) = client_headers
                .get(name)
                .and_then(|value| value.to_str().ok())
            {
                request = request.header(name, value);
            }
        }
        request
            .body(body)
            .send()
            .await
            .map_err(|_| GatewayFailure::Transport)
    }

    fn select_profile(
        &self,
        model: &str,
        excluded: &HashSet<String>,
        sticky: Option<&str>,
    ) -> Option<Arc<RuntimeProfile>> {
        let now = now_unix();
        let profiles = self.profiles_snapshot();
        let candidates = profiles
            .iter()
            .filter(|profile| !excluded.contains(&profile.id))
            .map(|profile| profile.candidate(model, now))
            .collect::<Vec<_>>();
        let cursor = self.cursor.fetch_add(1, Ordering::Relaxed);
        // Escape hatch, mirroring Gemini's `select_routed_ignoring_env_cooling` and Codex's
        // `admission_ignoring_soft_cooling`. When nothing is eligible, a profile held only by an
        // environment-derived reason is still real capacity: an auth refusal here arrives after a
        // successful refresh, and a wedged transport is ours to rebuild. Refusing the request
        // instead turns one refusal wave into a fleet-wide outage — exactly what took Gemini's
        // pool to zero nine times in August 2026. Provider verdicts stay walls: a quota wall and a
        // capability the plan does not grant are never relaxed here.
        let relaxed: Vec<Candidate>;
        let selected = match select(&candidates, sticky, cursor) {
            Some(candidate) => candidate,
            None => {
                relaxed = candidates
                    .iter()
                    .cloned()
                    .map(|mut candidate| {
                        if candidate
                            .ineligible
                            .is_some_and(crate::kimi::selection::Ineligible::is_environmental)
                        {
                            candidate.ineligible = None;
                        }
                        candidate
                    })
                    .collect();
                select(&relaxed, sticky, cursor)?
            }
        };
        profiles
            .into_iter()
            .find(|profile| profile.id == selected.profile_id)
    }

    fn eligible_count(&self, model: &str, excluded: &HashSet<String>, current: &str) -> usize {
        let now = now_unix();
        self.profiles_snapshot()
            .iter()
            .filter(|profile| profile.id != current && !excluded.contains(&profile.id))
            .filter(|profile| profile.candidate(model, now).ineligible.is_none())
            .count()
    }

    /// Share the profile's current cooling deadline with sibling processes. The hint is one
    /// fire-and-forget write that saves every sibling a doomed attempt; losing it costs a sibling
    /// one extra refusal its own handling converts into local cooling anyway.
    fn publish_cooling_deadline(&self, store: &Arc<AffinityStore>, profile: &RuntimeProfile) {
        let now = now_unix();
        let until = {
            let health = profile.health.lock().expect("KIMI profile health lock");
            let profile_axes = health
                .auth_quarantined_until
                .max(health.transport_cool_until)
                .max(health.quota_cool_until);
            let model_axes = health
                .model_failures
                .values()
                .map(|failure| failure.cool_until)
                .max()
                .unwrap_or(0);
            profile_axes.max(model_axes)
        };
        if until > now {
            store.publish_cooling_hint(&profile.id, until);
        }
    }

    /// Apply the decision's profile effect and share the cooling deadline with siblings.
    fn apply_effect_and_hint(
        &self,
        store: &Arc<AffinityStore>,
        profile: &RuntimeProfile,
        effect: ProfileEffect,
        retry_after_secs: Option<i64>,
    ) {
        profile.apply_effect_with_hint(effect, now_unix(), retry_after_secs);
        if effect != ProfileEffect::None {
            self.publish_cooling_deadline(store, profile);
        }
    }

    fn profile_id_for_home(&self, affinity: &AffinityStore, home: &str) -> Option<String> {
        self.profiles_snapshot()
            .into_iter()
            .find(|profile| affinity.home_id(&profile.id) == home)
            .map(|profile| profile.id.clone())
    }

    async fn commit_affinity(
        &self,
        store: &Arc<AffinityStore>,
        input: Option<&AffinityInput>,
        resolution: &mut Option<AffinityResolution>,
        profile: &RuntimeProfile,
    ) {
        let Some(input) = input else { return };
        let home = store.home_id(&profile.id);
        match resolution {
            Some(resolution) => {
                if resolution.home != home {
                    store.rebind(resolution, &home).await;
                }
                store.remember(input, resolution).await;
            }
            None => {
                *resolution = Some(store.claim(input, &home).await);
            }
        }
        store.mark_cache_warm(input, &home);
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

    async fn stream_abort_requested(&self) {
        loop {
            let notified = self.abort_notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.abort_streams.load(Ordering::Acquire) {
                return;
            }
            notified.await;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_stream(
        self: &Arc<Self>,
        background: ActiveTaskGuard,
        lease: ProfileLease,
        accounting: AccountingContext,
        reservation: Option<Reservation>,
        mut parsed: SseAccounting,
        initial: Bytes,
        mut upstream: impl futures_util::Stream<Item = Result<Bytes, wreq::Error>>
            + Send
            + Unpin
            + 'static,
        deliver: bool,
    ) -> Body {
        let (sender, receiver) = tokio::sync::mpsc::channel::<Bytes>(8);
        let initial_len = initial.len();
        if deliver {
            let _ = sender.try_send(initial);
        }
        let gateway = Arc::clone(self);
        tokio::spawn(async move {
            let _background = background;
            let _lease = lease;
            let mut checkpoint = reservation.as_ref().map(MeasuredCheckpoint::from_reservation);
            let mut deliver = deliver;
            let mut clean = true;
            let mut total = initial_len;
            loop {
                if gateway.abort_streams.load(Ordering::Acquire) {
                    clean = false;
                    break;
                }
                let next = tokio::select! {
                    _ = gateway.abort_notify.notified() => continue,
                    next = upstream.next() => next,
                };
                let Some(next) = next else { break };
                match next {
                    Ok(chunk) => {
                        total = total.saturating_add(chunk.len());
                        if total > RESPONSE_BODY_LIMIT || parsed.push(&chunk).is_err() {
                            clean = false;
                            break;
                        }
                        if let Some(checkpoint) = checkpoint.as_mut() {
                            checkpoint.maybe_publish(&gateway, &parsed);
                        }
                        if deliver {
                            let sent = tokio::select! {
                                _ = gateway.stream_abort_requested() => {
                                    clean = false;
                                    break;
                                }
                                sent = tokio::time::timeout(
                                    DOWNSTREAM_SEND_TIMEOUT,
                                    sender.send(chunk),
                                ) => sent,
                            };
                            if !matches!(sent, Ok(Ok(()))) {
                                // A disconnected or stalled downstream must not block the
                                // provider drain. Stop public delivery and keep reading terminal
                                // usage for exact settlement.
                                deliver = false;
                            }
                        }
                    }
                    Err(_) => {
                        clean = false;
                        elog::warn("kimi", "kimi mid-stream upstream error");
                        if deliver {
                            let _ = sender
                                .send(Bytes::from_static(
                                    b"event: error\ndata: {\"type\":\"error\",\"error\":{\"type\":\"api_error\",\"message\":\"upstream stream interrupted\"}}\n\n",
                                ))
                                .await;
                        }
                        break;
                    }
                }
            }
            drop(sender);
            parsed.finish();
            if !clean || !parsed.terminal {
                parsed.terminal = false;
            }
            gateway
                .finalize_turn(
                    &accounting,
                    reservation,
                    ParsedAccounting {
                        usage: parsed.usage_seen.then_some(parsed.usage),
                        served_model: parsed.served_model,
                        terminal: parsed.terminal,
                    },
                )
                .await;
            if clean && parsed.terminal {
                accounting.profile.mark_healthy();
                accounting
                    .profile
                    .mark_model_healthy(&accounting.requested_model);
            } else {
                accounting
                    .profile
                    .apply_effect(ProfileEffect::TransportFault, now_unix());
            }
            gateway.live_profiles.store(
                gateway
                    .profiles_snapshot()
                    .iter()
                    .filter(|profile| profile.authenticated())
                    .count(),
                Ordering::Release,
            );
        });
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver
                .recv()
                .await
                .map(|chunk| (Ok::<Bytes, Infallible>(chunk), receiver))
        });
        Body::from_stream(stream)
    }

    async fn finalize_turn(
        &self,
        context: &AccountingContext,
        reservation: Option<Reservation>,
        parsed: ParsedAccounting,
    ) {
        let class = settlement_class(&parsed);
        let pinned_tariff = reservation
            .as_ref()
            .and_then(|reservation| reservation.pinned_tariff.clone());
        let evidence = match class {
            SettlementClass::Unknown => None,
            SettlementClass::Exact | SettlementClass::Measured => {
                match (parsed.usage.as_ref(), parsed.served_model.as_deref()) {
                    (Some(usage), Some(served)) => price_turn_settlement(
                        self.billing.as_deref(),
                        usage,
                        served,
                        context.priced_ts,
                        pinned_tariff.as_ref(),
                    )
                    .await
                    .ok(),
                    _ => None,
                }
            }
        };

        if let Some(reservation) = reservation {
            if let Some(billing) = &self.billing {
                match (class, &evidence) {
                    (SettlementClass::Exact, Some(priced)) => {
                        let actual = customer_actual(priced.total, reservation.mult_bp);
                        let usage_event = Some(priced.usage_event(
                            parsed.served_model.as_deref().unwrap_or_default(),
                            reservation.priced_ts,
                        ));
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
                                "kimi",
                                format!("KIMI customer settlement deferred: {error:#}"),
                            );
                        }
                    }
                    (SettlementClass::Measured, Some(measured)) => {
                        // Delivery occurred; the terminal frame did not. What the provider did
                        // report is fact: those cumulative counters price at the served card and
                        // nothing is invented beyond them.
                        let actual = customer_actual(measured.total, reservation.mult_bp);
                        let usage_event = Some(measured.usage_event(
                            parsed.served_model.as_deref().unwrap_or_default(),
                            reservation.priced_ts,
                        ));
                        if let Err(error) = billing
                            .settle_request_with_usage(
                                &reservation.request_id,
                                &reservation.account_id,
                                &reservation.key,
                                reservation.hold,
                                actual,
                                Some("kimi-measured-partial"),
                                usage_event,
                            )
                            .await
                        {
                            elog::error(
                                "kimi",
                                format!("KIMI measured settlement deferred: {error:#}"),
                            );
                        }
                    }
                    _ => {
                        // Nothing measured — or the evidence failed pricing. The fleet policy
                        // decides (default: no charge): the hold is an admission device, not a
                        // price, and settling it bills 19x-250x the measured cost of a turn.
                        let charge =
                            crate::settlement_policy::unknown_usage_charge(reservation.hold);
                        if let Err(error) = billing
                            .settle_request(
                                &reservation.request_id,
                                &reservation.account_id,
                                &reservation.key,
                                reservation.hold,
                                charge,
                                Some("kimi-terminal-usage-missing"),
                            )
                            .await
                        {
                            elog::error(
                                "kimi",
                                format!("KIMI unmeasured settlement deferred: {error:#}"),
                            );
                        }
                    }
                }
            }
        }

        if class != SettlementClass::Exact {
            // The immutable calibration event stays terminal-only: a partial turn is customer
            // settlement fact, never provider-window evidence.
            return;
        }
        let (Some(priced), Some(usage), Some(served_model)) =
            (evidence, parsed.usage, parsed.served_model)
        else {
            return;
        };
        let event = match priced.calibration_event(context, &usage, &served_model) {
            Ok(event) => event,
            Err(error) => {
                elog::error(
                    "kimi",
                    format!("KIMI calibration event rejected before FIFO: {error:#}"),
                );
                return;
            }
        };
        self.enqueue_turn(event).await;
    }

    async fn enqueue_turn(&self, event: KimiTurnCalibrationEvent) {
        let _drain = self.turn_drain.lock().await;
        // Durable-first: a completed turn writes its immutable event inline, so a restart loses
        // at most the turn still streaming, never a finished one. The direct write is attempted
        // only when the FIFO is empty — while an older head is undelivered the new event joins
        // the queue instead, because the quota barrier and the replay-conflict quarantine rely on
        // the drain's total order. The bounded FIFO remains the fallback for transient
        // persistence failures and for a missing billing authority.
        let idle = self.turn_queue.lock().expect("KIMI turn queue lock").is_empty();
        if idle {
            if let Some(billing) = &self.billing {
                match billing.record_kimi_turn(event.clone()).await {
                    Ok(_) => return,
                    Err(error) if registry::is_kimi_turn_replay_conflict(&error) => {
                        elog::error(
                            "kimi",
                            format!(
                                "KIMI calibration event quarantined on replay conflict: {error:#}"
                            ),
                        );
                        return;
                    }
                    Err(error) => {
                        elog::warn(
                            "kimi",
                            format!(
                                "KIMI calibration direct persistence deferred to the FIFO: {error:#}"
                            ),
                        );
                    }
                }
            }
        }
        let accepted = self
            .turn_queue
            .lock()
            .expect("KIMI turn queue lock")
            .push(event);
        if !accepted {
            elog::error(
                "kimi",
                "KIMI calibration event dropped because the bounded FIFO is full",
            );
            return;
        }
        self.drain_turn_queue_locked().await;
    }

    /// Drain under `turn_drain`. A transient head remains in place and keeps quota publication
    /// blocked; a permanent replay conflict quarantines exactly that event and continues.
    async fn drain_turn_queue_locked(&self) -> bool {
        loop {
            let head = self
                .turn_queue
                .lock()
                .expect("KIMI turn queue lock")
                .head()
                .cloned();
            let Some(head) = head else { break };
            let outcome = match &self.billing {
                Some(billing) => match billing.record_kimi_turn(head).await {
                    Ok(_) => WriteOutcome::Durable,
                    Err(error) if registry::is_kimi_turn_replay_conflict(&error) => {
                        WriteOutcome::Conflict
                    }
                    Err(error) => {
                        elog::warn(
                            "kimi",
                            format!(
                                "KIMI calibration persistence deferred with FIFO head retained: {error:#}"
                            ),
                        );
                        WriteOutcome::Transient
                    }
                },
                None => WriteOutcome::Transient,
            };
            self.turn_queue
                .lock()
                .expect("KIMI turn queue lock")
                .resolve_head(outcome);
            if outcome == WriteOutcome::Transient {
                break;
            }
        }
        self.turn_queue
            .lock()
            .expect("KIMI turn queue lock")
            .may_poll_quota()
    }

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
                    self.abort_streams.store(true, Ordering::Release);
                    self.abort_notify.notify_waiters();
                    self.background.wait_idle().await;
                }
            }
            None => self.background.wait_idle().await,
        }
        let final_calibration = async {
            let _sweep = self.quota_sweep.lock().await;
            // Admission is closed and every stream finalizer is idle, so each profile is stable:
            // finish the same turn-before-quota ordering used by the steady-state poller.
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
                "kimi",
                "KIMI shutdown calibration drain remained incomplete at deadline",
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

async fn profiles_match_roster(profiles: &[Arc<RuntimeProfile>], roster: &[KimiProfile]) -> bool {
    if profiles.len() != roster.len() {
        return false;
    }
    for (profile, loaded) in profiles.iter().zip(roster) {
        if !profile.matches_roster(loaded).await {
            return false;
        }
    }
    true
}

fn bounded_requested_output(body: &mut Value) -> u64 {
    let supplied = body
        .get("max_tokens")
        .and_then(Value::as_u64)
        .unwrap_or(4_096);
    let bounded = supplied.min(MAX_REQUESTED_OUTPUT_TOKENS);
    if bounded < supplied {
        body["max_tokens"] = json!(bounded);
    }
    bounded
}

#[derive(Clone)]
struct PricedTurn {
    prices: KimiPrices,
    /// The tariff identity of the rate card this turn priced with: the compiled schedule id, or
    /// `<family>/v<version>` when a hot override priced the turn.
    tariff_schedule_id: String,
    input: i128,
    cache_read: i128,
    cache_write: i128,
    output: i128,
    total: i128,
}

impl PricedTurn {
    fn usage_event(&self, served_model: &str, priced_ts: i64) -> registry::UsageEventInput {
        let clamp = |value: i128| value.clamp(0, i128::from(i64::MAX)) as i64;
        registry::UsageEventInput {
            model: served_model.to_string(),
            provider: registry::PROVIDER_KIMI.to_string(),
            input_tokens: if self.prices.input == 0 {
                0
            } else {
                clamp(self.input / self.prices.input)
            },
            output_tokens: if self.prices.output == 0 {
                0
            } else {
                clamp(self.output / self.prices.output)
            },
            cache_read_tokens: if self.prices.cached_input == 0 {
                0
            } else {
                clamp(self.cache_read / self.prices.cached_input)
            },
            cache_write_5m_tokens: 0,
            cache_write_1h_tokens: if self.prices.cache_write == 0 {
                0
            } else {
                clamp(self.cache_write / self.prices.cache_write)
            },
            web_search_requests: 0,
            real_nano: clamp(self.total),
            charge_basis_nano: clamp(self.total),
            speed: if served_model.eq_ignore_ascii_case("kimi-k2.7-code-highspeed") {
                "highspeed".to_string()
            } else {
                "standard".to_string()
            },
            inference_geo: String::new(),
            input_nano: clamp(self.input),
            output_nano: clamp(self.output),
            cache_read_nano: clamp(self.cache_read),
            cache_write_5m_nano: 0,
            cache_write_1h_nano: clamp(self.cache_write),
            web_search_nano: 0,
            priced_ts,
        }
    }

    fn calibration_event(
        &self,
        context: &AccountingContext,
        usage: &KimiUsage,
        served_model: &str,
    ) -> anyhow::Result<KimiTurnCalibrationEvent> {
        let i64_counter = |value: u64| i64::try_from(value).context("KIMI usage overflow");
        let i64_money = |value: i128| i64::try_from(value).context("KIMI money overflow");
        let event = KimiTurnCalibrationEvent {
            request_id: context.request_id.clone(),
            subject_id: context.profile.subject_id.clone(),
            plan: context.profile.plan.clone(),
            requested_model: context.requested_model.clone(),
            served_model: served_model.to_string(),
            context_mode: context.context_mode.clone(),
            reasoning_effort: context.reasoning_effort.clone(),
            tariff_schedule_id: self.tariff_schedule_id.clone(),
            priced_ts: context.priced_ts,
            completed_at: now_unix(),
            input_tokens: i64_counter(usage.input_tokens)?,
            cache_read_tokens: i64_counter(usage.cache_read_tokens)?,
            cache_write_tokens: i64_counter(usage.cache_write_tokens)?,
            output_tokens: i64_counter(usage.output_tokens)?,
            reasoning_output_tokens: i64_counter(usage.reasoning_output_tokens)?,
            api_input_nanousd: i64_money(self.input)?,
            api_cache_read_nanousd: i64_money(self.cache_read)?,
            api_cache_write_nanousd: i64_money(self.cache_write)?,
            api_output_nanousd: i64_money(self.output)?,
            api_total_nanousd: i64_money(self.total)?,
        };
        event.validate()?;
        Ok(event)
    }
}

struct ParsedAccounting {
    usage: Option<KimiUsage>,
    served_model: Option<String>,
    terminal: bool,
}

/// Which settlement evidence a finished turn carries. `Exact` — the terminal frame arrived with
/// authoritative usage. `Measured` — delivery happened and the terminal frame did not, but the
/// provider did report cumulative counters that price by fact at the served card. `Unknown` —
/// nothing was measured, so any charge would be a hardcode rather than a fact; the fleet policy
/// (`settlement_policy::unknown_usage_charge`) decides, and its default is no charge.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SettlementClass {
    Exact,
    Measured,
    Unknown,
}

fn settlement_class(parsed: &ParsedAccounting) -> SettlementClass {
    match (
        parsed.terminal,
        parsed.usage.as_ref(),
        parsed.served_model.as_deref(),
    ) {
        (true, Some(_), Some(_)) => SettlementClass::Exact,
        (_, Some(_), Some(_)) => SettlementClass::Measured,
        _ => SettlementClass::Unknown,
    }
}

/// Hot-path measured pricing for mid-stream checkpoints: the pinned payload from the local book
/// or the compiled reviewed card, never the async refresh. A checkpoint is a lower bound on a
/// turn that must also die before it is used, so it must never stall the answer on bookkeeping.
fn price_measured_sync(
    usage: &KimiUsage,
    served_model: &str,
    priced_ts: i64,
    pinned: Option<&PinnedTariff>,
) -> Option<i128> {
    if usage.is_zero() {
        return None;
    }
    let (family, compiled) = kimi_matched_tariff_at(served_model, priced_ts)?;
    let book = tariff_book::snapshot();
    let prices = match pinned.filter(|pin| pin.family == family) {
        Some(pin) => book
            .version_payload(&pin.family, pin.version)
            .and_then(|payload| tariff_book::as_kimi(&payload))
            .unwrap_or(compiled),
        None => match book.resolve(family, priced_ts) {
            Some((_, payload)) => tariff_book::as_kimi(&payload).unwrap_or(compiled),
            None => compiled,
        },
    };
    cost_nanodollars(usage, &prices).ok()
}

/// How often a streaming turn republishes its measured cost. Writes are detached and monotonic
/// (GREATEST in PostgreSQL), so the interval bounds write amplification, never correctness.
const MEASURED_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(2);

/// Durable "this turn has cost at least X so far" for a live stream: if the owning process dies
/// mid-answer, the reconciler settles the checkpoint instead of writing the turn off to zero.
struct MeasuredCheckpoint {
    request_id: String,
    mult_bp: i64,
    priced_ts: i64,
    pinned_tariff: Option<PinnedTariff>,
    last_value: i64,
    last_at: std::time::Instant,
}

impl MeasuredCheckpoint {
    fn from_reservation(reservation: &Reservation) -> Self {
        Self {
            request_id: reservation.request_id.clone(),
            mult_bp: reservation.mult_bp,
            priced_ts: reservation.priced_ts,
            pinned_tariff: reservation.pinned_tariff.clone(),
            last_value: 0,
            // Backdated so the first provider usage snapshot publishes immediately.
            last_at: std::time::Instant::now() - MEASURED_CHECKPOINT_INTERVAL,
        }
    }

    fn maybe_publish(&mut self, gateway: &KimiGateway, parsed: &SseAccounting) {
        if !parsed.usage_seen || self.last_at.elapsed() < MEASURED_CHECKPOINT_INTERVAL {
            return;
        }
        let Some(served) = parsed.served_model.as_deref() else {
            return;
        };
        let Some(total) =
            price_measured_sync(&parsed.usage, served, self.priced_ts, self.pinned_tariff.as_ref())
        else {
            return;
        };
        let measured = customer_actual(total, self.mult_bp);
        if measured <= self.last_value {
            return;
        }
        let Some(billing) = &gateway.billing else {
            return;
        };
        billing.checkpoint_measured_detached(&self.request_id, measured);
        self.last_value = measured;
        self.last_at = std::time::Instant::now();
    }
}

fn non_stream_accounting(body: &[u8]) -> ParsedAccounting {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    ParsedAccounting {
        usage: parsed
            .as_ref()
            .and_then(metering::kimi::usage_from_response_value),
        served_model: parsed
            .as_ref()
            .and_then(|value| value.get("model"))
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .map(str::to_string),
        terminal: parsed.is_some(),
    }
}

/// The terminal-turn pricing core under one explicit rate card and tariff identity: the hot override vector
/// admission pinned (or the served family's resolution at the pinned priced timestamp after a
/// cross-family serve), compiled constants otherwise.
fn price_turn_with_prices(
    usage: &KimiUsage,
    prices: KimiPrices,
    tariff_schedule_id: String,
) -> anyhow::Result<PricedTurn> {
    usage
        .validate()
        .map_err(|_| anyhow!("invalid KIMI usage"))?;
    if usage.is_zero() {
        anyhow::bail!("empty KIMI usage is not terminal evidence");
    }
    let leg = |tokens: u64, rate: i128| {
        i128::from(tokens)
            .checked_mul(rate)
            .ok_or_else(|| anyhow!("KIMI cost overflow"))
    };
    let input = leg(usage.input_tokens, prices.input)?;
    let cache_read = leg(usage.cache_read_tokens, prices.cached_input)?;
    let cache_write = leg(usage.cache_write_tokens, prices.cache_write)?;
    let output = leg(usage.output_tokens, prices.output)?;
    let total = cost_nanodollars(usage, &prices).map_err(|_| anyhow!("KIMI cost overflow"))?;
    Ok(PricedTurn {
        prices,
        tariff_schedule_id,
        input,
        cache_read,
        cache_write,
        output,
        total,
    })
}

/// Settlement-time pricing through the hot tariff book: the exact version pinned at admission
/// (with one bounded refresh retry on a cache miss — this path is async), or the served family's
/// override at the pinned priced timestamp after a cross-family serve, or the compiled constants.
/// A pinned version that even the refresh cannot produce is an integrity error: the caller then
/// has no trustworthy evidence and falls to the fleet unknown-usage policy, never to a silent
/// compiled reprice.
async fn price_turn_settlement(
    billing: Option<&AsyncBilling>,
    usage: &KimiUsage,
    served_model: &str,
    priced_ts: i64,
    pinned: Option<&PinnedTariff>,
) -> anyhow::Result<PricedTurn> {
    usage
        .validate()
        .map_err(|_| anyhow!("invalid KIMI usage"))?;
    if usage.is_zero() {
        anyhow::bail!("empty KIMI usage is not terminal evidence");
    }
    let (family, compiled) = kimi_matched_tariff_at(served_model, priced_ts)
        .ok_or_else(|| anyhow!("unpriced KIMI served model"))?;
    let book = tariff_book::snapshot();
    let (prices, tariff_schedule_id) = match pinned.filter(|pin| pin.family == family) {
        Some(pin) => {
            let payload = match billing {
                Some(billing) => {
                    tariff_book::version_payload_refreshed(billing, &pin.family, pin.version).await
                }
                None => book.version_payload(&pin.family, pin.version),
            }
            .ok_or_else(|| {
                elog::error(
                    "kimi",
                    format!(
                        "pinned tariff {} is absent from the override book; settlement left to the conservative hold",
                        pin.schedule_id
                    ),
                );
                anyhow!("pinned KIMI tariff version is absent from the override book")
            })?;
            let prices = tariff_book::as_kimi(&payload).ok_or_else(|| {
                elog::error("kimi", "pinned KIMI tariff payload kind mismatch");
                anyhow!("pinned KIMI tariff payload kind mismatch")
            })?;
            (prices, pin.schedule_id.clone())
        }
        None => match book.resolve(family, priced_ts) {
            Some((pin, payload)) => match tariff_book::as_kimi(&payload) {
                Some(prices) => (prices, pin.schedule_id),
                None => (compiled, KIMI_TARIFF_SCHEDULE_ID.to_string()),
            },
            None => (compiled, KIMI_TARIFF_SCHEDULE_ID.to_string()),
        },
    };
    price_turn_with_prices(usage, prices, tariff_schedule_id)
}

fn customer_actual(charge_basis_nano: i128, mult_bp: i64) -> i64 {
    metering::apply_multiplier(charge_basis_nano, mult_bp).clamp(0, i128::from(i64::MAX)) as i64
}

fn cap_to_balance(
    balance: i128,
    input_upper_bound: i128,
    prices: KimiPrices,
    mult_bp: i64,
    requested_max: u64,
) -> Option<(u64, i64)> {
    if mult_bp <= 0 {
        return Some((requested_max, 0));
    }
    let ceiling = balance.checked_add(metering::OVERDRAFT_NANO)?;
    if ceiling <= 0 {
        return None;
    }
    let raw_ceiling = ceiling
        .checked_mul(10_000)?
        .checked_div(i128::from(mult_bp))?;
    let fixed = input_upper_bound.checked_mul(prices.input)?;
    if fixed > raw_ceiling || prices.output <= 0 {
        return None;
    }
    let affordable = u64::try_from((raw_ceiling - fixed) / prices.output).ok()?;
    if affordable == 0 {
        return None;
    }
    let effective = requested_max.min(affordable);
    let raw_hold = fixed.checked_add(i128::from(effective).checked_mul(prices.output)?)?;
    let hold = metering::apply_multiplier(raw_hold, mult_bp).clamp(0, i128::from(i64::MAX)) as i64;
    Some((effective, hold))
}

/// Refuse only what the customer hold cannot bound.
///
/// The hold reserves one input-token price per byte of the request body (`raw_body_len` in
/// `reserve_customer`), and a token never occupies less than a byte of JSON — so anything carried
/// *inside* the body is already covered, conservatively, before dispatch. That includes tool
/// declarations, `tool_use`/`tool_result` blocks and inline base64 media: they are text in the
/// body, priced as tokens, and bounded by its length.
///
/// Client-side function calling also cannot fan out within one request. The model emits a
/// `tool_use` block — output tokens, capped by `max_tokens` — and the *client* executes the tool
/// and returns the result as a new request carrying its own hold.
///
/// What remains genuinely unbounded is provider-executed work: an MCP server or a server-side
/// search/computer/code-execution tool may bill a unit per invocation that appears nowhere in the
/// request body and is not proportional to it. Manifest unknown 8 is about exactly that unit, and
/// it stays refused until a live run proves a finite per-request ceiling.
///
/// The previous rule conflated the two, refusing anything tool- or media-shaped. That was not a
/// tighter guard, it was a wider one: it blocked capabilities whose cost the hold already covered,
/// and it made the refusal self-perpetuating — nothing could spend on them, so nothing could price
/// them, so they could never be allowed.
fn validate_priced_surface(body: &Value) -> Result<(), GatewayFailure> {
    let enabled = |value: &Value| match value {
        Value::Null => false,
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    };
    if body.get("mcp_servers").is_some_and(enabled) || contains_provider_executed_tool(body) {
        return Err(GatewayFailure::Unsupported("kimi_tools_unpriced"));
    }
    Ok(())
}

/// True when the request asks the *provider* to run something on the caller's behalf.
///
/// A declared function the client executes itself is ordinary text in the body. A server-side
/// search, computer-use or code-execution tool, or an MCP server, is work the provider performs and
/// may bill per invocation — a unit that is invisible in the request and not proportional to it.
/// Only the second kind is refused.
fn contains_provider_executed_tool(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_provider_executed_tool),
        Value::Object(object) => {
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    let kind = kind.to_ascii_lowercase();
                    kind.contains("search")
                        || kind.contains("computer")
                        || kind.contains("code_execution")
                        || kind.contains("mcp")
                })
            {
                return true;
            }
            object.values().any(contains_provider_executed_tool)
        }
        _ => false,
    }
}

fn context_mode(model: &str) -> &'static str {
    match model.to_ascii_lowercase().as_str() {
        "k3" | "k3[1m]" => "1m",
        _ => "256k",
    }
}

fn reasoning_effort(body: &Value) -> Result<String, GatewayFailure> {
    if body
        .pointer("/thinking/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("disabled"))
    {
        return Ok("off".to_string());
    }
    let Some(raw) = body.get("reasoning_effort") else {
        return Ok("high".to_string());
    };
    if raw.is_null() {
        return Ok("high".to_string());
    }
    let Some(raw) = raw.as_str() else {
        return Err(GatewayFailure::BadRequest("invalid_reasoning_effort"));
    };
    let normalized = match raw.to_ascii_lowercase().as_str() {
        "ultra" | "max" | "xhigh" => "max",
        "high" | "medium" => "high",
        "low" | "minimum" | "light" => "low",
        "none" | "off" => "off",
        _ => return Err(GatewayFailure::BadRequest("invalid_reasoning_effort")),
    };
    Ok(normalized.to_string())
}

#[derive(Clone, Copy, Debug)]
enum GatewayFailure {
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
            Self::Auth => UpstreamVerdict::Auth,
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
            UpstreamVerdict::Auth => Self::Auth,
            UpstreamVerdict::QuotaExhausted => Self::Capacity,
            UpstreamVerdict::Transport | UpstreamVerdict::MembershipTemporary => Self::Transport,
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

fn error_response(error: GatewayFailure) -> Response {
    let (public, reason, retry_after) = match error {
        GatewayFailure::Auth => (LocalErr::Overloaded, "kimi_auth_unavailable", Some(2)),
        GatewayFailure::Transport | GatewayFailure::Protocol => {
            (LocalErr::Overloaded, "kimi_upstream_unavailable", Some(2))
        }
        GatewayFailure::Capacity => (LocalErr::RateLimited, "kimi_capacity_exhausted", Some(60)),
        GatewayFailure::LowBalance => (LocalErr::LowBalance, "billing_limit", None),
        GatewayFailure::BadRequest(code) => (LocalErr::BadRequest, code, None),
        GatewayFailure::Unsupported(code) => (LocalErr::NotFound, code, None),
        GatewayFailure::Unavailable(code) => (LocalErr::Overloaded, code, Some(2)),
        GatewayFailure::Upstream(status) if status == 429 => {
            (LocalErr::RateLimited, "kimi_upstream_rejected", Some(2))
        }
        GatewayFailure::Upstream(404) => (LocalErr::NotFound, "kimi_upstream_rejected", None),
        GatewayFailure::Upstream(status) if (400..500).contains(&status) => {
            (LocalErr::BadRequest, "kimi_upstream_rejected", None)
        }
        GatewayFailure::Upstream(_) => (LocalErr::Overloaded, "kimi_upstream_rejected", Some(2)),
    };
    local_err_for(public, reason, retry_after)
}

fn response_headers(response: &wreq::Response) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for name in ["content-type", "cache-control", "retry-after", "request-id"] {
        let Some(value) = response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
        else {
            continue;
        };
        if let (Ok(name), Ok(value)) = (
            axum::http::HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.insert(name, value);
        }
    }
    headers
}

fn response_with(status: StatusCode, headers: HeaderMap, body: Body) -> Response {
    let mut response = Response::builder()
        .status(status)
        .body(body)
        .expect("KIMI response");
    *response.headers_mut() = headers;
    response
}

fn response_is_sse(response: &wreq::Response) -> bool {
    response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/event-stream"))
}

async fn read_bounded(response: wreq::Response, limit: usize) -> Result<Bytes, GatewayFailure> {
    let mut stream = response.bytes_stream();
    let mut body = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| GatewayFailure::Transport)?;
        if body.len().saturating_add(chunk.len()) > limit {
            return Err(GatewayFailure::Protocol);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(Bytes::from(body))
}

async fn drop_bounded(response: wreq::Response, limit: usize) {
    let _ = read_bounded(response, limit).await;
}

/// Parse a `Retry-After` delay-seconds hint, bounded to one hour.
///
/// Only the integer-seconds form is honored (the HTTP-date form is ignored): the value drives a
/// cooling deadline, so an unbounded or hostile hint must never park a profile for longer than a
/// plain wall would. Absent, malformed or out-of-range values return `None` and the caller falls
/// back to the default cooldown.
fn retry_after_seconds(headers: &wreq::header::HeaderMap) -> Option<i64> {
    let raw = headers.get("retry-after")?.to_str().ok()?.trim();
    let seconds: i64 = raw.parse().ok()?;
    (seconds > 0 && seconds <= 3_600).then_some(seconds)
}

fn event_boundary(bytes: &[u8]) -> Option<(usize, usize)> {
    let lf = bytes
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|at| (at, 2));
    let crlf = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|at| (at, 4));
    match (lf, crlf) {
        (Some(left), Some(right)) => Some(if left.0 <= right.0 { left } else { right }),
        (Some(found), None) | (None, Some(found)) => Some(found),
        (None, None) => None,
    }
}

fn trim_ascii(mut bytes: &[u8]) -> &[u8] {
    while bytes.first().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[1..];
    }
    while bytes.last().is_some_and(u8::is_ascii_whitespace) {
        bytes = &bytes[..bytes.len() - 1];
    }
    bytes
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests;
