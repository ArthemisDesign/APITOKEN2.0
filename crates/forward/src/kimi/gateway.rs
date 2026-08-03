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
use metering::kimi::{
    cost_nanodollars, kimi_prices_for_served_model, kimi_resolve_subscription_model,
    merge_stream_event, KimiPrices, KimiUsage, KIMI_TARIFF_SCHEDULE_ID,
};
use registry::{ExecutionAttempt, KimiTurnCalibrationEvent};
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use crate::billing::{AsyncBilling, KimiQuotaSnapshot};
use crate::proxy::{local_err_for, HoldGuard, LocalErr};
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};

use super::client::{self, RefreshedTokens};
use super::config::{readiness, KimiPlaneConfig, NotReady};
use super::pool::{decide, AttemptPolicy, Delivery, NextStep, ProfileEffect};
use super::queue::{DeliveryHealth, TurnQueue, WriteOutcome, DEFAULT_QUEUE_CAPACITY};
use super::roster::{load_roster, load_roster_for_reload, reseal_credential, KimiProfile};
use super::selection::{ineligible_ids, select, Candidate, Ineligible};
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
    /// KIMI has no policy snapshot/capability in the strict catalogue yet. Such accounts must not
    /// silently fall through to the Anthropic tariff identity.
    pub strict_policy: bool,
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
    kimi_credential::KIMI_REVIEWED_PLANS
        .iter()
        .find(|entry| entry.plan_name == plan)
        .map(|entry| entry.plan_name)
        .unwrap_or("unreviewed")
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
}

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
        } else {
            None
        };
        Candidate {
            profile_id: self.id.clone(),
            ineligible,
            used_fraction_units: health.quota_used_fraction_units,
            quota_age_secs: health
                .quota_observed_at
                .map(|observed| now.saturating_sub(observed).max(0)),
            inflight: self.inflight.load(Ordering::Acquire),
        }
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

    fn mark_healthy(&self) {
        let mut health = self.health.lock().expect("KIMI profile health lock");
        health.authenticated = true;
        health.auth_quarantined_until = 0;
        health.transport_cool_until = 0;
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

    pub fn model_is_kimi(model: &str) -> bool {
        kimi_resolve_subscription_model(model).is_some()
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
                eprintln!("KIMI encrypted roster refresh skipped; last-good capacity retained");
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
                    eprintln!(
                        "KIMI identity preflight failed profile={} class={}",
                        profile.id,
                        error.class()
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
                eprintln!(
                    "KIMI quota poll returned no usable windows profile={}",
                    profile.id
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
                eprintln!(
                    "KIMI quota poll failed profile={} class={}",
                    profile.id,
                    error.class()
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
            eprintln!(
                "KIMI quota observation persistence deferred profile={}: {error:#}",
                profile.id
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
                self.access_token(profile, rejected_token.as_deref()).await?
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
        if request
            .billing
            .as_ref()
            .is_some_and(|input| input.strict_policy)
        {
            return error_response(GatewayFailure::Unsupported(
                "kimi_strict_pricing_unavailable",
            ));
        }
        if let Err(error) = validate_priced_surface(&request.body) {
            return error_response(error);
        }
        let context_mode = context_mode(&request.model).to_string();
        let reasoning_effort = match reasoning_effort(&request.body) {
            Ok(value) => value,
            Err(error) => return error_response(error),
        };
        let request_id = crate::upstream::fresh_request_id();
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
        let mut excluded = HashSet::new();
        let mut policy = AttemptPolicy::default();

        loop {
            let Some(profile) = self.select_profile(&request.model, &excluded, sticky.as_deref())
            else {
                return error_response(GatewayFailure::Capacity);
            };
            let lease = ProfileLease::new(profile.clone());
            let mut rejected_token: Option<String> = None;
            loop {
                let token = match self.access_token(&profile, rejected_token.as_deref()).await {
                    Ok(token) => token,
                    Err(error) => {
                        let verdict = error.verdict();
                        let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
                        let decision = decide(verdict, Delivery::PreByte, policy, remaining);
                        profile.apply_effect(decision.effect, now_unix());
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
                let verdict = classify_status(status);
                let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
                let decision = decide(verdict, Delivery::PreByte, policy, remaining);
                profile.apply_effect(decision.effect, now_unix());
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
                                Ok(Err(error)) => return error_response(error),
                                Err(_) => return error_response(GatewayFailure::Transport),
                            };
                            let mut sse = SseAccounting::default();
                            if sse.push(&initial).is_err() {
                                return error_response(GatewayFailure::Protocol);
                            }
                            if !self.mark_delivering(reservation.as_ref()).await {
                                // Upstream may still consume the turn. Drain and preserve provider
                                // evidence, but keep the customer hold guard armed for a refund.
                                let _ = self.spawn_stream(
                                    background, lease, accounting, None, sse, initial, upstream,
                                    false,
                                );
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
                            Err(error) => return error_response(error),
                        };
                        if !self.mark_delivering(reservation.as_ref()).await {
                            let parsed = non_stream_accounting(&body);
                            self.finalize_turn(&accounting, None, parsed).await;
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
        let prices = kimi_prices_for_served_model(resolved.official_model, priced_ts)
            .ok_or(GatewayFailure::Unsupported("kimi_price_unavailable"))?;
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
                .reserve_request_for_execution(
                    request_id,
                    &input.account_id,
                    &input.key,
                    hold,
                    execution.clone(),
                )
                .await
                .map_err(|_| GatewayFailure::Unavailable("kimi_reservation_unavailable"))?
            {
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
                    }));
                }
                None => {
                    balance = billing
                        .account(&input.account_id)
                        .await
                        .map_err(|_| GatewayFailure::Unavailable("kimi_balance_unavailable"))?
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
        let selected = select(
            &candidates,
            sticky,
            self.cursor.fetch_add(1, Ordering::Relaxed),
        )?;
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
        let priced = parsed
            .terminal
            .then_some(())
            .and_then(|_| parsed.usage.as_ref())
            .zip(parsed.served_model.as_deref())
            .and_then(|(usage, served)| price_turn(usage, served, context.priced_ts).ok());

        if let Some(reservation) = reservation {
            if let Some(billing) = &self.billing {
                match &priced {
                    Some(priced) => {
                        let actual = metering::apply_multiplier(priced.total, reservation.mult_bp)
                            .clamp(
                                0,
                                i128::from(reservation.hold.max(0)) + metering::OVERDRAFT_NANO,
                            )
                            .min(i128::from(i64::MAX)) as i64;
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
                            eprintln!("KIMI customer settlement deferred: {error:#}");
                        }
                    }
                    None => {
                        // Delivery occurred but authoritative terminal usage did not. Preserve the
                        // conservative hold and create no immutable provider event.
                        if let Err(error) = billing
                            .settle_request(
                                &reservation.request_id,
                                &reservation.account_id,
                                &reservation.key,
                                reservation.hold,
                                reservation.hold,
                                Some("kimi-terminal-usage-missing"),
                            )
                            .await
                        {
                            eprintln!("KIMI conservative settlement deferred: {error:#}");
                        }
                    }
                }
            }
        }

        let (Some(priced), Some(usage), Some(served_model)) =
            (priced, parsed.usage, parsed.served_model)
        else {
            return;
        };
        let event = match priced.calibration_event(context, &usage, &served_model) {
            Ok(event) => event,
            Err(error) => {
                eprintln!("KIMI calibration event rejected before FIFO: {error:#}");
                return;
            }
        };
        self.enqueue_turn(event).await;
    }

    async fn enqueue_turn(&self, event: KimiTurnCalibrationEvent) {
        let _drain = self.turn_drain.lock().await;
        let accepted = self
            .turn_queue
            .lock()
            .expect("KIMI turn queue lock")
            .push(event);
        if !accepted {
            eprintln!("KIMI calibration event dropped because the bounded FIFO is full");
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
                        eprintln!("KIMI calibration persistence deferred with FIFO head retained: {error:#}");
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
            eprintln!("KIMI shutdown calibration drain remained incomplete at deadline");
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

#[derive(Clone, Copy)]
struct PricedTurn {
    prices: KimiPrices,
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
            tariff_schedule_id: KIMI_TARIFF_SCHEDULE_ID.to_string(),
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

fn price_turn(usage: &KimiUsage, served_model: &str, priced_ts: i64) -> anyhow::Result<PricedTurn> {
    usage
        .validate()
        .map_err(|_| anyhow!("invalid KIMI usage"))?;
    if usage.is_zero() {
        anyhow::bail!("empty KIMI usage is not terminal evidence");
    }
    let prices = kimi_prices_for_served_model(served_model, priced_ts)
        .ok_or_else(|| anyhow!("unpriced KIMI served model"))?;
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
        input,
        cache_read,
        cache_write,
        output,
        total,
    })
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

fn validate_priced_surface(body: &Value) -> Result<(), GatewayFailure> {
    let enabled = |value: &Value| match value {
        Value::Null => false,
        Value::Array(values) => !values.is_empty(),
        Value::Object(values) => !values.is_empty(),
        _ => true,
    };
    if ["tools", "tool_choice", "mcp_servers"]
        .into_iter()
        .filter_map(|field| body.get(field))
        .any(enabled)
        || contains_unpriced_tool_content(body)
    {
        return Err(GatewayFailure::Unsupported("kimi_tools_unpriced"));
    }
    if contains_unpriced_media(body) {
        return Err(GatewayFailure::Unsupported("kimi_media_unpriced"));
    }
    Ok(())
}

fn contains_unpriced_tool_content(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_unpriced_tool_content),
        Value::Object(object) => {
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| {
                    let kind = kind.to_ascii_lowercase();
                    kind.contains("tool")
                        || kind.contains("search")
                        || kind.contains("computer")
                        || kind.contains("code_execution")
                })
            {
                return true;
            }
            object.values().any(contains_unpriced_tool_content)
        }
        _ => false,
    }
}

fn contains_unpriced_media(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_unpriced_media),
        Value::Object(object) => {
            if object
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| matches!(kind, "image" | "document" | "audio" | "video"))
            {
                return true;
            }
            object.values().any(contains_unpriced_media)
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
mod tests {
    use super::*;
    use kimi_credential::{
        encode_envelope, CredentialKeyring, KimiCredentialKind, KIMI_STATUS_NORMAL,
    };
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::sync::mpsc;
    use tokio::sync::mpsc as async_mpsc;

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let mut random = [0u8; 8];
            getrandom::fill(&mut random).unwrap();
            let suffix = random
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let root = std::env::temp_dir().join(format!("kimi-gateway-{suffix}"));
            fs::create_dir_all(root.join("credentials")).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            fs::set_permissions(root.join("credentials"), fs::Permissions::from_mode(0o700))
                .unwrap();
            Self { root }
        }

        fn publish_console_profile(&self) {
            self.publish_console_profile_with("console-secret");
        }

        fn publish_console_profile_with(&self, access_token: &str) {
            let credential = KimiCredential {
                version: 1,
                kind: KimiCredentialKind::ConsoleKey,
                access_token: access_token.into(),
                refresh_token: String::new(),
                expires_at: 0,
                scope: "coding".into(),
                subject_id: "subject-1".into(),
                plan_name: "unreviewed-base-plan".into(),
                plan_level: 1,
                status: KIMI_STATUS_NORMAL.into(),
                region: "REGION_CN".into(),
                proxy_url: String::new(),
            };
            let ring = keyring();
            let envelope = ring.seal("a1", "kimi-01", &credential).unwrap();
            let credential_path = self.root.join("credentials/kimi-01.json");
            write_private(&credential_path, &encode_envelope(&envelope).unwrap());
            let roster = json!({
                "profiles": [{
                    "id": "kimi-01",
                    "credential_file": credential_path.to_string_lossy(),
                }]
            });
            write_private(
                &self.root.join("profiles.json"),
                &serde_json::to_vec(&roster).unwrap(),
            );
        }

        fn publish_empty_roster(&self) {
            write_private(
                &self.root.join("profiles.json"),
                &serde_json::to_vec(&json!({"profiles": []})).unwrap(),
            );
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn write_private(path: &Path, bytes: &[u8]) {
        fs::write(path, bytes).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn keyring() -> CredentialKeyring {
        CredentialKeyring::parse(&format!("a1:{}", "11".repeat(32))).unwrap()
    }

    fn config(root: &Path, base_url: String) -> KimiPlaneConfig {
        KimiPlaneConfig {
            roster_dir: root.to_path_buf(),
            keyring: keyring(),
            transport: super::super::transport::KimiTransportConfig {
                base_url,
                auth_scheme: super::super::transport::AuthScheme::Bearer,
                request_timeout: Duration::from_secs(5),
                refresh_lead: Duration::from_secs(120),
            },
            readiness_probe: ProbeRoute::Identity,
            quota_poll_interval: Duration::from_secs(300),
        }
    }

    fn http_status_response(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
        format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
            body.len()
        )
        .into_bytes()
        .into_iter()
            .chain(body.iter().copied())
            .collect()
    }

    fn http_response(content_type: &str, body: &[u8]) -> Vec<u8> {
        http_status_response("200 OK", content_type, body)
    }

    fn mock_server(responses: Vec<Vec<u8>>) -> (String, mpsc::Receiver<Vec<u8>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let request = read_request(&mut stream);
                sender.send(request).unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        (format!("http://{address}"), receiver)
    }

    fn controlled_mock_server(
        requests: usize,
    ) -> (
        String,
        async_mpsc::UnboundedReceiver<Vec<u8>>,
        mpsc::Sender<Vec<u8>>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_sender, request_receiver) = async_mpsc::unbounded_channel();
        let (response_sender, response_receiver) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for _ in 0..requests {
                let (mut stream, _) = listener.accept().unwrap();
                request_sender.send(read_request(&mut stream)).unwrap();
                let Ok(response) = response_receiver.recv() else {
                    return;
                };
                stream.write_all(&response).unwrap();
            }
        });
        (
            format!("http://{address}"),
            request_receiver,
            response_sender,
        )
    }

    fn read_request(stream: &mut TcpStream) -> Vec<u8> {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut request = Vec::new();
        let mut scratch = [0u8; 4096];
        let mut expected = None;
        loop {
            let read = stream.read(&mut scratch).unwrap();
            if read == 0 {
                break;
            }
            request.extend_from_slice(&scratch[..read]);
            if expected.is_none() {
                if let Some(end) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]);
                    let length = headers
                        .lines()
                        .find_map(|line| {
                            line.split_once(':').and_then(|(name, value)| {
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse::<usize>().ok())
                                    .flatten()
                            })
                        })
                        .unwrap_or(0);
                    expected = Some(end + 4 + length);
                }
            }
            if expected.is_some_and(|expected| request.len() >= expected) {
                break;
            }
        }
        request
    }

    fn affinity() -> Arc<AffinityStore> {
        Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap())
    }

    fn calibration_event(request_id: &str) -> KimiTurnCalibrationEvent {
        KimiTurnCalibrationEvent {
            request_id: request_id.into(),
            subject_id: "subject-1".into(),
            plan: "unreviewed-base-plan".into(),
            requested_model: "kimi-for-coding".into(),
            served_model: "kimi-k2.7-code".into(),
            context_mode: "256k".into(),
            reasoning_effort: "high".into(),
            tariff_schedule_id: KIMI_TARIFF_SCHEDULE_ID.into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_001,
            input_tokens: 10,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 2,
            reasoning_output_tokens: 0,
            api_input_nanousd: 600_000,
            api_cache_read_nanousd: 0,
            api_cache_write_nanousd: 0,
            api_output_nanousd: 600_000,
            api_total_nanousd: 1_200_000,
        }
    }

    fn quota_body(used: i64) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "usage": {
                "used": used.to_string(),
                "limit": "1000",
                "resetTime": "2099-01-07T00:00:00Z"
            },
            "limits": [{
                "name": "rate",
                "window": {"duration": 300, "timeUnit": "TIME_UNIT_MINUTE"},
                "detail": {
                    "used": used.to_string(),
                    "limit": "100",
                    "resetTime": "2099-01-01T00:00:00Z"
                }
            }]
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn a_pending_turn_blocks_the_provider_quota_read() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let (base_url, mut requests, _responses) = controlled_mock_server(1);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        gateway
            .turn_queue
            .lock()
            .unwrap()
            .push(calibration_event("pending-before-poll"));

        assert_eq!(gateway.poll_quotas().await, 0);
        assert_eq!(gateway.operational_status().delivery.pending_events, 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), requests.recv())
                .await
                .is_err(),
            "/usages must not run past an undelivered spend head"
        );
    }

    #[tokio::test]
    async fn customer_generation_start_invalidates_a_concurrent_quota_snapshot_without_waiting() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let (base_url, mut requests, responses) = controlled_mock_server(1);
        let gateway = Arc::new(
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap(),
        );
        let profile = gateway.profiles_snapshot()[0].clone();

        let poll = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        let request = requests.recv().await.unwrap();
        assert!(request.starts_with(b"GET /usages "));

        // No semaphore or maintenance wait: a customer lease starts immediately while the GET is
        // outstanding. Its epoch makes the returned snapshot unusable for calibration.
        let lease = ProfileLease::new(profile.clone());
        assert_eq!(profile.inflight.load(Ordering::Acquire), 1);
        responses
            .send(http_response("application/json", &quota_body(10)))
            .unwrap();
        assert_eq!(poll.await.unwrap(), 0);
        drop(lease);
        assert_eq!(
            profile
                .candidate("kimi-for-coding", now_unix())
                .used_fraction_units,
            None
        );
    }

    #[tokio::test]
    async fn transient_observation_failure_keeps_the_previous_quota_generation() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let (base_url, requests) =
            mock_server(vec![http_response("application/json", &quota_body(10))]);
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), Some(billing))
                .unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();

        // SQLite deliberately refuses KIMI calibration. A successful provider GET is not enough
        // to publish steering before the durable PostgreSQL observation/CAS succeeds.
        assert_eq!(gateway.poll_quotas().await, 0);
        assert!(requests.recv().unwrap().starts_with(b"GET /usages "));
        let candidate = profile.candidate("kimi-for-coding", now_unix());
        assert_eq!(candidate.used_fraction_units, None);
        assert_eq!(candidate.quota_age_secs, None);
    }

    #[test]
    fn a_durable_snapshot_publishes_the_tightest_window_and_exact_full_reset() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let gateway = KimiGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:1".into()),
            None,
        )
        .unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();
        let observed_at = now_unix();
        let snapshots = vec![
            KimiQuotaSnapshot {
                window_duration_secs: registry::KIMI_ROLLING_WINDOW_SECS,
                window_name: Some("rate".into()),
                resets_at: observed_at + 300,
                observed_at,
                native_used_units: 60,
                native_limit_units: 100,
                used_fraction_units: 60_000_000,
                measurement_resolution_fraction_units: 1_000_000,
            },
            KimiQuotaSnapshot {
                window_duration_secs: registry::KIMI_WEEKLY_WINDOW_SECS,
                window_name: None,
                resets_at: observed_at + 600,
                observed_at,
                native_used_units: 1_000,
                native_limit_units: 1_000,
                used_fraction_units: registry::KIMI_FRACTION_SCALE,
                measurement_resolution_fraction_units: 100_000,
            },
        ];
        profile.publish_quota(&snapshots, observed_at);
        let candidate = profile.candidate("kimi-for-coding", observed_at);
        assert_eq!(
            candidate.used_fraction_units,
            Some(registry::KIMI_FRACTION_SCALE)
        );
        assert_eq!(candidate.quota_age_secs, Some(0));
        assert_eq!(candidate.ineligible, Some(Ineligible::QuotaWall));
        assert_eq!(
            profile.health.lock().unwrap().quota_cool_until,
            observed_at + 600
        );
    }

    #[tokio::test]
    async fn quota_auth_capacity_and_transport_failures_stay_profile_local() {
        for (status, responses, expected) in [
            ("401 Unauthorized", 2, Some(Ineligible::AuthQuarantined)),
            ("403 Forbidden", 1, Some(Ineligible::QuotaWall)),
            (
                "429 Too Many Requests",
                1,
                Some(Ineligible::TransportWedged),
            ),
            (
                "503 Service Unavailable",
                1,
                Some(Ineligible::TransportWedged),
            ),
        ] {
            let fixture = Fixture::new();
            fixture.publish_console_profile();
            let response = http_status_response(status, "application/json", br#"{}"#);
            let (base_url, requests) = mock_server(vec![response; responses]);
            let gateway =
                KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
            let profile = gateway.profiles_snapshot()[0].clone();
            profile.mark_healthy();
            gateway.live_profiles.store(1, Ordering::Release);

            assert_eq!(gateway.poll_quotas().await, 0, "status {status}");
            assert_eq!(gateway.profiles_snapshot().len(), 1, "status {status}");
            assert_eq!(
                profile.candidate("kimi-for-coding", now_unix()).ineligible,
                expected,
                "status {status}"
            );
            for _ in 0..responses {
                assert!(requests.recv().unwrap().starts_with(b"GET /usages "));
            }
        }
    }

    #[tokio::test]
    async fn a_profile_removed_during_quota_io_is_never_reintroduced() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let (base_url, mut requests, responses) = controlled_mock_server(1);
        let gateway = Arc::new(
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap(),
        );
        let poll = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));

        fixture.publish_empty_roster();
        assert!(gateway.refresh_profiles().await);
        assert!(gateway.profiles_snapshot().is_empty());
        responses
            .send(http_response("application/json", &quota_body(10)))
            .unwrap();
        assert_eq!(poll.await.unwrap(), 0);
        assert!(gateway.profiles_snapshot().is_empty());
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
    }

    #[tokio::test]
    async fn shutdown_cancels_the_steady_poll_and_bounds_its_final_quota_read() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let (base_url, mut requests, responses) = controlled_mock_server(2);
        let mut test_config = config(&fixture.root, base_url);
        test_config.transport.request_timeout = Duration::from_secs(30);
        let gateway = Arc::new(KimiGateway::new_with_calibration(test_config, None).unwrap());
        let steady = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));

        let started = tokio::time::Instant::now();
        let stopping = {
            let gateway = gateway.clone();
            tokio::spawn(async move {
                gateway
                    .shutdown_until(Some(started + Duration::from_millis(300)))
                    .await
            })
        };
        // The test server handles connections serially. Let it retire the cancelled steady-state
        // socket so the final bounded shutdown attempt can be observed on the next connection.
        tokio::time::sleep(Duration::from_millis(20)).await;
        responses
            .send(http_status_response(
                "503 Service Unavailable",
                "application/json",
                br#"{}"#,
            ))
            .unwrap();
        assert_eq!(steady.await.unwrap(), 0);
        // The regular request was cancelled by shutdown; one final ordered attempt was permitted
        // inside the existing process deadline and cancelled at that boundary.
        assert!(requests.recv().await.unwrap().starts_with(b"GET /usages "));
        stopping.await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "provider I/O must not extend shutdown past the bounded deadline"
        );
    }

    #[tokio::test]
    async fn exact_base_alias_uses_identity_readiness_and_transparent_messages_bytes() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let generation = br#"{"id":"msg_1","type":"message","model":"kimi-k2.7-code","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", identity),
            http_response("application/json", generation),
        ]);
        let gateway = Arc::new(
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap(),
        );
        assert_eq!(gateway.preflight().await, 1);
        assert_eq!(gateway.readiness(), Ok(()));

        let body = json!({
            "model": "kimi-for-coding",
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "hello"}],
        });
        let response = gateway
            .handle(KimiRequest {
                headers: HeaderMap::new(),
                raw_body_len: serde_json::to_vec(&body).unwrap().len(),
                body,
                model: "kimi-for-coding".into(),
                execution: ExecutionAttempt::direct(),
                billing: None,
                affinity: None,
                affinity_store: affinity(),
            })
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let returned = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
            .await
            .unwrap();
        assert_eq!(returned.as_ref(), generation);

        let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(probe.starts_with("GET /me "));
        assert!(probe
            .to_ascii_lowercase()
            .contains("authorization: bearer console-secret"));
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(turn.starts_with("POST /messages "));
        assert!(turn.contains("\"model\":\"kimi-for-coding\""));
        // No calibration authority in this test: evidence remains visible in the bounded FIFO
        // instead of being silently discarded.
        assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    }

    #[tokio::test]
    async fn identity_auth_rejection_forces_exactly_one_refresh_retry() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, requests) = mock_server(vec![
            http_status_response("401 Unauthorized", "application/json", br#"{}"#),
            http_response("application/json", identity),
        ]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();

        assert_eq!(gateway.preflight().await, 1);
        assert_eq!(gateway.readiness(), Ok(()));
        for _ in 0..2 {
            let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
            assert!(probe.starts_with("GET /me "));
            assert!(probe
                .to_ascii_lowercase()
                .contains("authorization: bearer console-secret"));
        }
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_cold_degraded_gateway_adopts_a_new_profile_only_after_identity_probe() {
        let fixture = Fixture::new();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
        let gateway = KimiGateway::new_degraded(config(&fixture.root, base_url), None);
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

        fixture.publish_console_profile();
        assert!(gateway.refresh_profiles().await);
        assert_eq!(gateway.readiness(), Ok(()));
        assert_eq!(gateway.operational_status().total_profiles, 1);
        let request = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(request.starts_with("GET /me "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer console-secret"));
    }

    #[tokio::test]
    async fn an_unchanged_roster_reuses_the_exact_profile_without_another_probe() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, requests) = mock_server(vec![http_response("application/json", identity)]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));
        assert!(requests.recv().unwrap().starts_with(b"GET /me "));
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn broken_or_disappeared_rosters_retain_the_last_good_ready_profile() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, _requests) = mock_server(vec![http_response("application/json", identity)]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        write_private(&fixture.root.join("profiles.json"), br#"{"profiles":["#);
        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));

        fs::remove_file(fixture.root.join("profiles.json")).unwrap();
        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));
    }

    #[tokio::test]
    async fn an_explicit_empty_roster_removes_new_admission_but_not_an_existing_lease() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, _requests) = mock_server(vec![http_response("application/json", identity)]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();
        let lease = ProfileLease::new(original.clone());
        assert_eq!(original.inflight.load(Ordering::Acquire), 1);

        fixture.publish_empty_roster();
        assert!(gateway.refresh_profiles().await);
        assert!(gateway.profiles_snapshot().is_empty());
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
        assert_eq!(original.inflight.load(Ordering::Acquire), 1);
        drop(lease);
        assert_eq!(original.inflight.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn a_changed_credential_is_published_only_after_a_successful_identity_probe() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", identity),
            http_response("application/json", identity),
        ]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        fixture.publish_console_profile_with("replacement-secret");
        assert!(gateway.refresh_profiles().await);
        let replacement = gateway.profiles_snapshot()[0].clone();
        assert!(!Arc::ptr_eq(&original, &replacement));
        assert_eq!(gateway.readiness(), Ok(()));

        let first = String::from_utf8(requests.recv().unwrap()).unwrap();
        let second = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(first
            .to_ascii_lowercase()
            .contains("authorization: bearer console-secret"));
        assert!(second
            .to_ascii_lowercase()
            .contains("authorization: bearer replacement-secret"));
    }

    #[tokio::test]
    async fn a_failed_probe_for_a_changed_credential_keeps_the_old_ready_snapshot() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let rejected = http_status_response("403 Forbidden", "application/json", br#"{}"#);
        let (base_url, requests) =
            mock_server(vec![http_response("application/json", identity), rejected]);
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        fixture.publish_console_profile_with("rejected-secret");
        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));
        for expected in ["console-secret", "rejected-secret"] {
            let request = String::from_utf8(requests.recv().unwrap()).unwrap();
            assert!(request
                .to_ascii_lowercase()
                .contains(&format!("authorization: bearer {expected}")));
        }
    }

    #[tokio::test]
    async fn final_verification_never_publishes_a_credential_rotated_during_probe() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let identity = br#"{"user_id":"subject-1","user_level_name":"unreviewed-base-plan","status":"USER_STATUS_NORMAL"}"#;
        let (base_url, mut requests, responses) = controlled_mock_server(2);
        let gateway = Arc::new(
            KimiGateway::new_with_calibration(config(&fixture.root, base_url), None).unwrap(),
        );
        let original = gateway.profiles_snapshot()[0].clone();
        original.mark_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        fixture.publish_console_profile_with("candidate-secret");
        let reload = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.refresh_profiles().await })
        };
        let first = String::from_utf8(
            tokio::time::timeout(Duration::from_secs(5), requests.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(first
            .to_ascii_lowercase()
            .contains("authorization: bearer candidate-secret"));

        // Simulate the other blue-green generation atomically rotating the shared envelope after
        // this generation loaded it but before its candidate probe completed.
        fixture.publish_console_profile_with("peer-rotated-secret");
        responses
            .send(http_response("application/json", identity))
            .unwrap();

        let second = String::from_utf8(
            tokio::time::timeout(Duration::from_secs(5), requests.recv())
                .await
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(second
            .to_ascii_lowercase()
            .contains("authorization: bearer peer-rotated-secret"));
        responses
            .send(http_response("application/json", identity))
            .unwrap();

        assert!(reload.await.unwrap());
        let published = gateway.profiles_snapshot()[0].clone();
        let state = published.credential.lock().await;
        assert_eq!(state.credential.access_token, "peer-rotated-secret");
        assert!(!Arc::ptr_eq(&original, &published));
        assert_eq!(gateway.readiness(), Ok(()));
    }

    #[tokio::test]
    async fn degraded_gateway_keeps_exact_aliases_on_a_zero_capacity_kimi_path() {
        let fixture = Fixture::new();
        let gateway = Arc::new(KimiGateway::new_degraded(
            config(&fixture.root, "https://example.invalid".into()),
            None,
        ));
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

        let body = json!({
            "model": "kimi-for-coding",
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "hello"}],
        });
        let response = gateway
            .handle(KimiRequest {
                headers: HeaderMap::new(),
                raw_body_len: serde_json::to_vec(&body).unwrap().len(),
                body,
                model: "kimi-for-coding".into(),
                execution: ExecutionAttempt::direct(),
                billing: None,
                affinity: None,
                affinity_store: affinity(),
            })
            .await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .extensions()
                .get::<crate::proxy::TerminalErrorReason>()
                .map(|reason| reason.0),
            Some("kimi_capacity_exhausted")
        );
        let body = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["type"], "rate_limit_error");
        assert!(!String::from_utf8_lossy(&body)
            .to_ascii_lowercase()
            .contains("kimi"));
    }

    #[tokio::test]
    async fn every_synthetic_failure_uses_the_shared_anthropic_sanitizer() {
        for failure in [
            GatewayFailure::Auth,
            GatewayFailure::Transport,
            GatewayFailure::Protocol,
            GatewayFailure::Capacity,
            GatewayFailure::LowBalance,
            GatewayFailure::BadRequest("kimi_private_request_reason"),
            GatewayFailure::Unsupported("kimi_private_capability_reason"),
            GatewayFailure::Unavailable("kimi_private_runtime_reason"),
            GatewayFailure::Upstream(400),
            GatewayFailure::Upstream(404),
            GatewayFailure::Upstream(429),
            GatewayFailure::Upstream(503),
        ] {
            let response = error_response(failure);
            assert!(!response.status().is_success());
            assert!(crate::proxy::is_exact_not_started_response(&response));
            let body = axum::body::to_bytes(response.into_body(), ERROR_BODY_LIMIT)
                .await
                .unwrap();
            let body = String::from_utf8_lossy(&body).to_ascii_lowercase();
            for private in ["kimi", "subscription", "roster", "upstream", "provider"] {
                assert!(
                    !body.contains(private),
                    "{failure:?} leaked {private}: {body}"
                );
            }
        }
    }

    #[test]
    fn sse_accounting_survives_split_frames_and_requires_a_terminal_event() {
        let mut accounting = SseAccounting::default();
        accounting
            .push(br#"data: {"type":"message_start","message":{"model":"kimi-k2.7-code","usage":{"input_tokens":10}}}"#)
            .unwrap();
        assert!(!accounting.terminal);
        accounting
            .push(b"\n\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":4}}\n")
            .unwrap();
        accounting
            .push(b"\ndata: {\"type\":\"message_stop\"}\n\n")
            .unwrap();
        assert!(accounting.terminal);
        assert!(accounting.usage_seen);
        assert_eq!(accounting.usage.input_tokens, 10);
        assert_eq!(accounting.usage.output_tokens, 4);
        assert_eq!(accounting.served_model.as_deref(), Some("kimi-k2.7-code"));
    }

    #[test]
    fn reservation_cap_is_an_integer_upper_bound_and_never_crosses_the_overdraft_floor() {
        let prices = kimi_prices_for_served_model("kimi-k2.7-code", 1).unwrap();
        let balance = 1_000_000i128;
        let (tokens, hold) = cap_to_balance(balance, 100, prices, 10_000, 1_000_000).unwrap();
        assert!(tokens > 0);
        assert!(i128::from(hold) <= balance + metering::OVERDRAFT_NANO);
        let raw = 100 * prices.input + i128::from(tokens) * prices.output;
        assert_eq!(i128::from(hold), metering::apply_multiplier(raw, 10_000));
        assert!(cap_to_balance(-metering::OVERDRAFT_NANO, 1, prices, 10_000, 1).is_none());
    }

    #[test]
    fn output_reserve_bound_is_also_enforced_on_the_forwarded_body() {
        let mut body = json!({"max_tokens": u64::MAX});
        assert_eq!(
            bounded_requested_output(&mut body),
            MAX_REQUESTED_OUTPUT_TOKENS
        );
        assert_eq!(body["max_tokens"], MAX_REQUESTED_OUTPUT_TOKENS);

        let mut defaulted = json!({});
        assert_eq!(bounded_requested_output(&mut defaulted), 4_096);
        assert!(defaulted.get("max_tokens").is_none());
    }

    #[test]
    fn unknown_money_surfaces_fail_closed_before_transport() {
        assert!(matches!(
            validate_priced_surface(&json!({"tools": [{"name": "search"}]})),
            Err(GatewayFailure::Unsupported("kimi_tools_unpriced"))
        ));
        for body in [
            json!({"tools": "provider-default"}),
            json!({"tool_choice": {"type": "auto"}}),
            json!({"messages": [{"content": [{"type": "tool_result"}]}]}),
            json!({"messages": [{"content": [{"type": "web_search_tool_result"}]}]}),
        ] {
            assert!(matches!(
                validate_priced_surface(&body),
                Err(GatewayFailure::Unsupported("kimi_tools_unpriced"))
            ));
        }
        assert!(matches!(
            validate_priced_surface(&json!({
                "messages": [{"content": [{"type": "image", "source": {}}]}]
            })),
            Err(GatewayFailure::Unsupported("kimi_media_unpriced"))
        ));
        assert_eq!(reasoning_effort(&json!({})).unwrap(), "high");
        assert_eq!(
            reasoning_effort(&json!({"reasoning_effort": "xhigh"})).unwrap(),
            "max"
        );
        assert!(reasoning_effort(&json!({"reasoning_effort": "invented"})).is_err());
    }

    fn quota_snapshot(used: i64, limit: i64, resets_at: i64, observed_at: i64) -> KimiQuotaSnapshot {
        let derived = registry::kimi_fraction_from_native(used, limit).unwrap();
        KimiQuotaSnapshot {
            window_duration_secs: 18_000,
            window_name: None,
            resets_at,
            observed_at,
            native_used_units: used,
            native_limit_units: limit,
            used_fraction_units: derived.used_fraction_units,
            measurement_resolution_fraction_units: derived.measurement_resolution_fraction_units,
        }
    }

    #[tokio::test]
    async fn the_status_projection_reports_cooling_axes_availability_and_inflight() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
                .unwrap();
        let now = now_unix();
        let profile = gateway.profiles.read().unwrap()[0].clone();

        let healthy = gateway.operational_status();
        assert_eq!(healthy.total_profiles, 1);
        assert_eq!(healthy.available_profiles, 1);
        assert_eq!(healthy.auth_quarantined_profiles, 0);
        assert_eq!(healthy.transport_cooling_profiles, 0);
        assert_eq!(healthy.quota_cooling_profiles, 0);
        assert_eq!(healthy.inflight_requests, 0);
        assert_eq!(healthy.profiles[0].id, "kimi-01");
        assert_eq!(healthy.profiles[0].auth_quarantined_until, None);
        assert_eq!(healthy.profiles[0].transport_cool_until, None);
        assert_eq!(healthy.profiles[0].quota_cool_until, None);
        assert_eq!(healthy.profiles[0].quota_observed_at, None);
        assert_eq!(healthy.profiles[0].quota_windows, Vec::new());
        assert!(!healthy.profiles[0].live);

        profile.inflight.store(3, Ordering::Release);
        profile.apply_effect(ProfileEffect::AuthQuarantine, now);
        let quarantined = gateway.operational_status();
        assert_eq!(quarantined.available_profiles, 0);
        assert_eq!(quarantined.auth_quarantined_profiles, 1);
        assert_eq!(quarantined.inflight_requests, 3);
        assert_eq!(quarantined.profiles[0].inflight, 3);
        assert_eq!(
            quarantined.profiles[0].auth_quarantined_until,
            Some(now + AUTH_QUARANTINE_SECS)
        );
        assert!(!quarantined.profiles[0].live);

        profile.apply_effect(ProfileEffect::TransportFault, now);
        let wedged = gateway.operational_status();
        assert_eq!(wedged.transport_cooling_profiles, 1);
        assert_eq!(
            wedged.profiles[0].transport_cool_until,
            Some(now + TRANSPORT_COOL_SECS)
        );

        // An expired or cleared deadline is "not cooling", never a timestamp in the past.
        profile.mark_healthy();
        let recovered = gateway.operational_status();
        assert_eq!(recovered.available_profiles, 1);
        assert_eq!(recovered.auth_quarantined_profiles, 0);
        assert_eq!(recovered.transport_cooling_profiles, 0);
        assert_eq!(recovered.profiles[0].auth_quarantined_until, None);
        assert_eq!(recovered.profiles[0].transport_cool_until, None);
        assert!(recovered.profiles[0].live);
    }

    #[tokio::test]
    async fn publish_quota_retains_the_exact_per_window_snapshot() {
        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
                .unwrap();
        let profile = gateway.profiles.read().unwrap()[0].clone();
        let observed_at = now_unix();
        let five_hour = quota_snapshot(250, 1000, 4_102_444_800, observed_at);
        let mut weekly = quota_snapshot(100, 100, 4_102_500_000, observed_at);
        weekly.window_duration_secs = 604_800;
        profile.publish_quota(&[five_hour, weekly], observed_at);

        let status = gateway.operational_status();
        let projected = &status.profiles[0];
        assert_eq!(projected.quota_observed_at, Some(observed_at));
        assert_eq!(projected.quota_windows.len(), 2);
        let window = &projected.quota_windows[0];
        assert_eq!(window.duration_secs, 18_000);
        assert_eq!(window.used_units, 250);
        assert_eq!(window.limit_units, 1000);
        // Exact fraction semantics: 250/1000 is 25% in 10^-8 units, and the real measurement
        // resolution of a limit-1000 counter is one 0.1% step, not one fixed-point unit.
        assert_eq!(window.used_fraction_units, 25_000_000);
        assert_eq!(window.measurement_resolution_fraction_units, 100_000);
        assert_eq!(window.resets_at, 4_102_444_800);
        assert_eq!(window.observed_at, observed_at);
        assert_eq!(projected.quota_windows[1].duration_secs, 604_800);
        // The full weekly window walls the profile until the exact provider reset instant.
        assert_eq!(status.quota_cooling_profiles, 1);
        assert_eq!(projected.quota_cool_until, Some(4_102_500_000));
        assert_eq!(status.available_profiles, 0);
        // A successful poll authenticates the profile on this runtime generation.
        assert!(projected.live);
    }

    #[tokio::test]
    async fn the_projection_bounds_plan_labels_and_cannot_carry_the_subject() {
        // The reviewed list is empty today, so every provider-controlled string collapses to the
        // bounded placeholder; a raw plan name must never reach logs, metrics or admin output.
        assert_eq!(bounded_plan_label("unreviewed-base-plan"), "unreviewed");
        assert_eq!(bounded_plan_label("Moderato"), "unreviewed");
        for entry in kimi_credential::KIMI_REVIEWED_PLANS {
            assert_eq!(bounded_plan_label(entry.plan_name), entry.plan_name);
        }

        let fixture = Fixture::new();
        fixture.publish_console_profile();
        let gateway =
            KimiGateway::new_with_calibration(config(&fixture.root, "http://127.0.0.1:9".into()), None)
                .unwrap();
        let status = gateway.operational_status();
        assert_eq!(status.profiles[0].plan, "unreviewed");
        // The durable-calibration join resolves only through the opaque roster id; an unknown
        // subject resolves to nothing and its rows are dropped rather than serialized.
        assert_eq!(
            gateway.profile_id_for_subject("subject-1").as_deref(),
            Some("kimi-01")
        );
        assert_eq!(gateway.profile_id_for_subject("subject-unknown"), None);
        let rendered = format!("{status:?}");
        assert!(!rendered.contains("subject-1"));
        assert!(!rendered.contains("unreviewed-base-plan"));
    }
}
