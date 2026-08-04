//! Live GLM (Zhipu AI / Z.ai) Coding Plan generation gateway.
//!
//! GLM remains an internal backend of the Anthropic plane: exact reviewed subscription aliases
//! dispatch here, while every other Messages request follows the unchanged Claude path. The
//! gateway owns only provider concerns — sealed profile state, native HTTP, placement, one-byte
//! retry policy, terminal usage evidence and graceful stream drain.
//!
//! Contract: `docs/engine/GLM_PROVIDER.md` §4.1 (dispatch), §5.3 (dual ledger) and §5.4
//! (turn-before-quota ordering), and `docs/engine/PROVIDER_ONBOARDING.md` §8 (runtime). Two
//! provider facts separate this gateway from the KIMI one it mirrors:
//!
//! * **The credential is a static API key.** There is no OAuth refresh family, no
//!   single-flight refresh and no reseal-on-rotation. A 401 is terminal for the profile until
//!   the Auth Bot publishes a replacement key, so the attempt loop rotates away on the first
//!   auth refusal and never retries the same profile with the same key.
//! * **Every turn is priced twice, independently.** The official API replacement cost
//!   (nanoUSD, `docs.z.ai` rate card) and the native credits consumption (microcredits, the
//!   provider's published formula with the off-peak schedule) advance as two disjoint ledgers
//!   per turn. One is never derived from the other; both schedule ids ride on the immutable
//!   turn event.

use std::collections::HashSet;
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
use glm_credential::{GlmCredential, GLM_ANTHROPIC_MESSAGES_PATH};
use metering::glm::{
    cost_nanodollars, glm_credit_cost_micro, glm_credit_rates_for_served_model,
    glm_is_peak_utc, glm_prices_for_served_model, glm_resolve_subscription_model,
    merge_stream_event, GlmPrices, GlmUsage, GLM_CREDIT_SCHEDULE_ID, GLM_TARIFF_SCHEDULE_ID,
};
use registry::{ExecutionAttempt, GlmTurnCalibrationEvent};
use serde_json::{json, Value};
use tokio::sync::{Mutex as AsyncMutex, Notify};

use crate::affinity::{AffinityInput, AffinityResolution, AffinityStore};
use crate::billing::{AsyncBilling, GlmQuotaSnapshot};
use crate::proxy::{local_err_for, HoldGuard, LocalErr};
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};

use super::client::{self, QuotaProbe};
use super::config::{readiness, GlmPlaneConfig, NotReady};
use super::pool::{decide, AttemptPolicy, Delivery, NextStep, ProfileEffect};
use super::queue::{DeliveryHealth, TurnQueue, WriteOutcome, DEFAULT_QUEUE_CAPACITY};
use super::roster::{load_roster, load_roster_for_reload, GlmProfile};
use super::selection::{ineligible_ids, select, Candidate, Ineligible};
use super::transport::{
    classify_status, error_business_code, probe_url, quota_authorization, ProbeRoute,
    UpstreamVerdict,
};

const ERROR_BODY_LIMIT: usize = 64 * 1024;
const RESPONSE_BODY_LIMIT: usize = 32 * 1024 * 1024;
const STREAM_START_MAX_BYTES: usize = 256 * 1024;
const STREAM_START_MAX_CHUNKS: usize = 64;
const STREAM_START_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNSTREAM_SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// Published maximum output of every subscription model (manifest §3); a larger request is
/// capped before pricing so the reserve never inflates past what the provider can emit.
const MAX_REQUESTED_OUTPUT_TOKENS: u64 = 131_072;
const TRANSPORT_COOL_SECS: i64 = 10;
/// Fallback cooling for a quota wall whose body names no parseable reset. Bounded on purpose:
/// the next idle quota poll observes the exact provider reset and replaces this guess.
const QUOTA_WALL_FALLBACK_COOL_SECS: i64 = 300;
const RESERVATION_LEASE_SECS: i64 = 3_600;

/// Customer money context already authenticated by the shared Anthropic handler.
#[derive(Clone)]
pub(crate) struct GlmBillingInput {
    pub account_id: String,
    pub key: String,
    pub mult_bp: i64,
    pub available_nano: i64,
    /// GLM has no policy snapshot/capability in the strict catalogue yet. Such accounts must not
    /// silently fall through to the Anthropic tariff identity.
    pub strict_policy: bool,
}

pub(crate) struct GlmRequest {
    // No client headers on purpose: the upstream request is built from the reviewed fleet
    // persona only (see `send_generation`), so inbound identity headers never enter the gateway.
    pub body: Value,
    pub raw_body_len: usize,
    pub model: String,
    pub execution: ExecutionAttempt,
    pub billing: Option<GlmBillingInput>,
    pub affinity: Option<AffinityInput>,
    pub affinity_store: Arc<AffinityStore>,
}

/// One retained quota-endpoint window exactly as last published by the provider. Raw counters
/// stay optional — their unit semantics are unproven (manifest §6.3), so unknown is absent,
/// never zero-filled or invented.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmQuotaWindowStatus {
    pub duration_secs: i64,
    pub used_units: Option<i64>,
    pub limit_units: Option<i64>,
    pub remaining_units: Option<i64>,
    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
    pub resets_at: Option<i64>,
    pub observed_at: i64,
}

/// Per-profile operational projection for readiness, metrics and the admin endpoint.
///
/// Privacy by construction: the opaque roster id and the bounded plan label are the only
/// identities this struct can carry. The subject, API key, proxy, credential paths and raw
/// provider errors never enter it — `RuntimeProfile.subject_id` stays private to the gateway.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmProfileStatus {
    /// Opaque roster id. Documented safe for logs, metrics and admin projections.
    pub id: String,
    /// Exact declared plan for a reviewed individual plan, otherwise the bounded `"unreviewed"`
    /// placeholder.
    pub plan: &'static str,
    /// Authenticated and serving (a quota probe passed on this runtime generation).
    pub live: bool,
    /// The static key was refused or the plan expired: out of rotation until the Auth Bot
    /// publishes a replacement. Durable — no timer clears it, only fresh auth evidence does.
    pub account_dead: bool,
    /// Risk-control fair-use or account anomaly: out of rotation pending review, recoverable.
    pub account_suspect: bool,
    /// Cooling axes as unix seconds; `None` means the axis is not cooling right now.
    pub transport_cool_until: Option<i64>,
    pub quota_cool_until: Option<i64>,
    pub inflight: u32,
    /// Last successful quota observation, unix seconds. `None` means never observed.
    pub quota_observed_at: Option<i64>,
    pub quota_windows: Vec<GlmQuotaWindowStatus>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmOperationalStatus {
    pub total_profiles: usize,
    pub live_profiles: usize,
    /// Eligible right now: not dead, suspect, transport-cooled or quota-walled.
    pub available_profiles: usize,
    pub account_dead_profiles: usize,
    pub account_suspect_profiles: usize,
    pub transport_cooling_profiles: usize,
    pub quota_cooling_profiles: usize,
    pub inflight_requests: u64,
    /// Delivered turns whose authoritative terminal usage never arrived. Each one settled on
    /// the documented conservative hold; no synthetic usage was created.
    pub missing_terminal_usage: u64,
    /// Delivered turns with terminal usage whose served model fell outside the priced/rated
    /// admission set. Each one settled on the conservative hold with no immutable event.
    pub served_model_rejected: u64,
    pub profiles: Vec<GlmProfileStatus>,
    pub delivery: DeliveryHealth,
}

/// The calibration cohort identity of a declared plan (`docs/engine/GLM_PROVIDER.md` §5.3:
/// cohorts key on the exact declared plan). The sealed credential stores the lowercase serde
/// spelling (`lite`/`pro`/`max`); the calibration authority and the estimator use the
/// capitalized declared form, so both spellings resolve here. Anything else names no cohort
/// and stays `None`.
fn declared_plan(plan_name: &str) -> Option<&'static str> {
    match plan_name {
        "lite" | "Lite" => Some("Lite"),
        "pro" | "Pro" => Some("Pro"),
        "max" | "Max" => Some("Max"),
        _ => None,
    }
}

/// Bounded plan label for logs, metrics and admin projections. The credential parser already
/// constrains the roster to the three reviewed individual plans; anything else collapses to
/// the bounded placeholder rather than becoming a metric label.
pub fn bounded_plan_label(plan: &str) -> &'static str {
    declared_plan(plan).unwrap_or("unreviewed")
}

/// A cooling deadline is only meaningful while it is still in the future; an expired or never-set
/// axis is "not cooling", not a timestamp in the past.
fn active_cooling(until: i64, now: i64) -> Option<i64> {
    (until > now).then_some(until)
}

#[derive(Default)]
struct ProfileHealth {
    authenticated: bool,
    /// Durable until a roster reload replaces the key or a fresh probe authenticates it.
    account_dead: bool,
    /// Out of rotation pending review; a passing quota probe clears it.
    account_suspect: bool,
    transport_cool_until: i64,
    quota_cool_until: i64,
    /// Lowercased requested aliases the provider refused with 1311. Model-scoped, so the same
    /// profile keeps serving the models it does grant; bounded by the reviewed alias set.
    model_ineligible: HashSet<String>,
    quota_used_fraction_units: Option<i64>,
    quota_observed_at: Option<i64>,
    /// Last full quota snapshot, retained for the operational projection. Empty until the first
    /// durable observation; never zero-filled.
    quota_windows: Vec<GlmQuotaWindowStatus>,
}

struct RuntimeProfile {
    id: String,
    subject_id: String,
    /// Declared capitalized plan identity — the calibration cohort key.
    plan: String,
    /// Per-profile console origin from the sealed credential. Keys are only valid against the
    /// console that issued them.
    base_url: String,
    credential_key_id: String,
    /// Sealed material, held in memory only. Plain field on purpose: the static key has no
    /// refresh family, so nothing mutates it between roster generations.
    credential: GlmCredential,
    client: wreq::Client,
    health: Mutex<ProfileHealth>,
    inflight: AtomicU32,
    /// Monotonic start boundary for quota polls. If any customer lease starts while the quota
    /// GET is in flight, that snapshot is discarded even when the turn finishes before it
    /// returns.
    turn_epoch: AtomicU64,
}

impl RuntimeProfile {
    fn from_roster(profile: GlmProfile, config: &GlmPlaneConfig) -> anyhow::Result<Arc<Self>> {
        let client = client::build_client(
            &profile.credential.proxy_url,
            Duration::from_secs(10),
            config.transport.request_timeout,
        )?;
        let plan = declared_plan(&profile.plan_name)
            .ok_or_else(|| anyhow!("GLM profile declares an unreviewed plan"))?;
        Ok(Arc::new(Self {
            id: profile.id,
            subject_id: profile.subject_id,
            plan: plan.to_string(),
            base_url: profile.base_url,
            credential_key_id: profile.credential_key_id,
            credential: profile.credential,
            client,
            health: Mutex::new(ProfileHealth::default()),
            inflight: AtomicU32::new(0),
            turn_epoch: AtomicU64::new(0),
        }))
    }

    fn candidate(&self, model: &str, now: i64) -> Candidate {
        let health = self.health.lock().expect("GLM profile health lock");
        let model_key = model.to_ascii_lowercase();
        // The cooling axes stay deliberately separate (PROVIDER_ONBOARDING §8.4): account,
        // quota, model scope and transport clear independently and never share a timer.
        let ineligible = if health.model_ineligible.contains(&model_key) {
            Some(Ineligible::ModelIneligible)
        } else if health.account_dead {
            Some(Ineligible::AccountDead)
        } else if health.account_suspect {
            Some(Ineligible::AccountSuspect)
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

    fn apply_effect(
        &self,
        effect: ProfileEffect,
        now: i64,
        model: Option<&str>,
        quota_reset: Option<i64>,
    ) {
        let mut health = self.health.lock().expect("GLM profile health lock");
        match effect {
            ProfileEffect::None => {}
            ProfileEffect::CoolUntilReset => {
                // The exact reset is parsed from the wall body when the provider named one
                // (1308/1310). Without it a bounded cool avoids hammering a known wall; the
                // next idle quota poll replaces the guess with the provider's own reset.
                health.quota_cool_until = quota_reset
                    .filter(|reset| *reset > now)
                    .unwrap_or_else(|| now.saturating_add(QUOTA_WALL_FALLBACK_COOL_SECS));
            }
            ProfileEffect::AccountDead => {
                // The static key was refused or the plan expired. No timer clears this: only a
                // passing probe (the key works again) or a roster republication can.
                health.authenticated = false;
                health.account_dead = true;
            }
            ProfileEffect::AccountSuspect => {
                health.account_suspect = true;
            }
            ProfileEffect::ModelIneligible => {
                if let Some(model) = model {
                    health.model_ineligible.insert(model.to_ascii_lowercase());
                }
            }
            ProfileEffect::TransportFault => {
                health.transport_cool_until = now.saturating_add(TRANSPORT_COOL_SECS);
            }
        }
    }

    /// A successful generation for `model` rehabilitates the account/transport axes and lifts
    /// exactly that model's scope block — never another model's (PROVIDER_ONBOARDING §8.4).
    fn mark_healthy(&self, model: &str) {
        let mut health = self.health.lock().expect("GLM profile health lock");
        health.authenticated = true;
        health.account_dead = false;
        health.account_suspect = false;
        health.transport_cool_until = 0;
        health.model_ineligible.remove(&model.to_ascii_lowercase());
    }

    fn publish_quota(&self, snapshots: &[GlmQuotaSnapshot], observed_at: i64) {
        let used = snapshots
            .iter()
            .filter_map(|snapshot| snapshot.used_fraction_units)
            .max();
        let cool_until = snapshots
            .iter()
            .filter(|snapshot| {
                snapshot
                    .used_fraction_units
                    .is_some_and(|fraction| fraction >= registry::GLM_FRACTION_SCALE)
            })
            .filter_map(|snapshot| snapshot.resets_at)
            .max()
            .unwrap_or(0);
        let windows = snapshots
            .iter()
            .map(|snapshot| GlmQuotaWindowStatus {
                duration_secs: snapshot.window_duration_secs,
                used_units: snapshot.native_used_units,
                limit_units: snapshot.native_limit_units,
                remaining_units: snapshot.native_remaining_units,
                used_fraction_units: snapshot.used_fraction_units,
                measurement_resolution_fraction_units: snapshot
                    .measurement_resolution_fraction_units,
                resets_at: snapshot.resets_at,
                observed_at: snapshot.observed_at,
            })
            .collect();
        let mut health = self.health.lock().expect("GLM profile health lock");
        // A passing probe is auth/capacity evidence (manifest §2.1): the account axes clear.
        // The model-ineligible set deliberately survives — a free quota read must not
        // rehabilitate the generation route, which is a different backend path.
        health.authenticated = true;
        health.account_dead = false;
        health.account_suspect = false;
        health.transport_cool_until = 0;
        health.quota_cool_until = cool_until;
        health.quota_used_fraction_units = used;
        health.quota_observed_at = Some(observed_at);
        health.quota_windows = windows;
    }

    fn authenticated(&self) -> bool {
        self.health
            .lock()
            .expect("GLM profile health lock")
            .authenticated
    }

    fn matches_roster(&self, profile: &GlmProfile) -> bool {
        self.id == profile.id
            && self.subject_id == profile.subject_id
            && Some(self.plan.as_str()) == declared_plan(&profile.plan_name)
            && self.credential_key_id == profile.credential_key_id
            && credentials_match(&self.credential, &profile.credential)
    }
}

fn credentials_match(left: &GlmCredential, right: &GlmCredential) -> bool {
    left.version == right.version
        && left.kind == right.kind
        && left.api_key == right.api_key
        && left.plan == right.plan
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
    /// `Some` only for glm-5.2, the one model that takes a reasoning effort (manifest §3).
    reasoning_effort: Option<String>,
    priced_ts: i64,
    profile: Arc<RuntimeProfile>,
}

#[derive(Default)]
struct SseAccounting {
    pending: Vec<u8>,
    usage: GlmUsage,
    usage_seen: bool,
    served_model: Option<String>,
    terminal: bool,
}

impl SseAccounting {
    fn push(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        self.pending.extend_from_slice(bytes);
        if self.pending.len() > STREAM_START_MAX_BYTES {
            anyhow::bail!("GLM SSE accounting frame exceeded bound");
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

/// Default-off live GLM pool. It has no public catalogue or provider mode of its own.
pub struct GlmGateway {
    config: Arc<GlmPlaneConfig>,
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
    abort_streams: AtomicBool,
    abort_notify: Notify,
    live_profiles: AtomicUsize,
    missing_terminal_usage: AtomicU64,
    served_model_rejected: AtomicU64,
}

impl GlmGateway {
    pub fn new_with_calibration(
        config: GlmPlaneConfig,
        billing: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        let roster = load_roster(&config.roster_dir, &config.keyring)?;
        let mut profiles = Vec::with_capacity(roster.len());
        for profile in roster {
            profiles.push(RuntimeProfile::from_roster(profile, &config)?);
        }
        Ok(Self::from_profiles(config, billing, profiles))
    }

    /// Keep exact GLM aliases fail-closed when the initial roster cannot be opened.
    ///
    /// GLM is an optional backend inside the Anthropic service, so a bad roster must not take
    /// Claude readiness down. It also must not remove the GLM dispatcher and let those aliases
    /// fall through to the Claude pool. A degraded gateway has zero capacity until a later
    /// last-good roster reload checkpoint can recover it.
    pub fn new_degraded(config: GlmPlaneConfig, billing: Option<Arc<AsyncBilling>>) -> Self {
        Self::from_profiles(config, billing, Vec::new())
    }

    fn from_profiles(
        config: GlmPlaneConfig,
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
            abort_streams: AtomicBool::new(false),
            abort_notify: Notify::new(),
            live_profiles: AtomicUsize::new(0),
            missing_terminal_usage: AtomicU64::new(0),
            served_model_rejected: AtomicU64::new(0),
        }
    }

    pub fn model_is_glm(model: &str) -> bool {
        glm_resolve_subscription_model(model).is_some()
    }

    fn profiles_snapshot(&self) -> Vec<Arc<RuntimeProfile>> {
        self.profiles.read().expect("GLM profiles lock").clone()
    }

    /// Atomically adopt one fully validated roster generation and retain the last-good snapshot on
    /// every read, decrypt, client-build or quota-probe failure.
    ///
    /// Unchanged profiles keep their exact `Arc`, preserving health, in-flight accounting and HTTP
    /// state. Changed/new profiles authenticate through the free quota probe before publication.
    /// A final roster re-read prevents a snapshot that went stale during the probe from replacing
    /// a credential the Auth Bot republished meanwhile (a peer blue-green generation may already
    /// have adopted it). A removed profile closes to new admission immediately; its in-flight
    /// lease lives on its own `Arc` until natural drop.
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
                eprintln!("GLM encrypted roster refresh skipped; last-good capacity retained");
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

            // Every new or changed credential passes the free quota probe BEFORE it joins the
            // serving generation. A key the provider rejects (the probe's business-code 401)
            // must not start carrying traffic.
            for profile in needs_probe {
                self.probe_profile(&profile)
                    .await
                    .map_err(|error| anyhow!("GLM reload quota-probe class={}", error.class()))?;
                profile.mark_probe_healthy();
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
            *self.profiles.write().expect("GLM profiles lock") = next;
            self.live_profiles.store(live, Ordering::Release);
            return Ok(true);
        }
        anyhow::bail!("GLM roster changed repeatedly during reload")
    }

    async fn load_reload_snapshot(
        &self,
        has_last_good_capacity: bool,
    ) -> anyhow::Result<Vec<GlmProfile>> {
        let root = self.config.roster_dir.clone();
        let keyring = self.config.keyring.clone();
        tokio::task::spawn_blocking(move || {
            load_roster_for_reload(&root, &keyring, has_last_good_capacity)
        })
        .await
        .map_err(|_| anyhow!("GLM roster reader stopped"))?
    }

    /// Startup validation: open the keyring roster and quota-probe every profile. A profile
    /// whose key is rejected (the probe's business-code 401) is quarantined on its own — one
    /// dead key never takes the rest of the fleet, let alone the whole gateway, down with it.
    pub async fn preflight(&self) -> usize {
        let mut live = 0usize;
        for profile in self.profiles_snapshot() {
            match self.probe_profile(&profile).await {
                Ok(()) => {
                    profile.mark_probe_healthy();
                    live += 1;
                }
                Err(error) => {
                    let verdict = error.verdict();
                    if verdict == UpstreamVerdict::AccountDead {
                        profile.apply_effect(
                            ProfileEffect::AccountDead,
                            now_unix(),
                            None,
                            None,
                        );
                    }
                    // Classification only: never print provider bodies, subject, proxy or keys.
                    eprintln!(
                        "GLM quota preflight failed profile={} class={}",
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
    /// Any lease that starts while the quota GET is in flight invalidates that profile's
    /// snapshot; customer traffic is never queued or rejected merely because maintenance is
    /// reading quota.
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

    /// One idle-only quota read with the turn-before-quota ordering of
    /// `docs/engine/GLM_PROVIDER.md` §5.4: drain the bounded turn FIFO, read the provider
    /// snapshot, re-check the generation epoch, drain again under the FIFO barrier, then let
    /// the serial writer pair cumulative dual-ledger spend with the observation/CAS. Quota
    /// steering publishes only after every independent window is durable.
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
                    "GLM quota poll returned no usable windows profile={}",
                    profile.id
                );
                return false;
            }
            Err(error) => {
                let effect = match error.verdict() {
                    UpstreamVerdict::AccountDead => ProfileEffect::AccountDead,
                    UpstreamVerdict::AccountSuspect => ProfileEffect::AccountSuspect,
                    UpstreamVerdict::QuotaExhausted => ProfileEffect::CoolUntilReset,
                    UpstreamVerdict::Transport => ProfileEffect::TransportFault,
                    UpstreamVerdict::ModelIneligible
                    | UpstreamVerdict::Ok
                    | UpstreamVerdict::ClientError => ProfileEffect::None,
                };
                profile.apply_effect(effect, now_unix(), None, None);
                self.refresh_live_profile_count();
                eprintln!(
                    "GLM quota poll failed profile={} class={}",
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
        // Raw per-limit fields ride 1:1 with the mapped windows (`client::map_windows`
        // preserves order), so the provider's own percentage display value joins its window
        // as raw evidence — never as an estimator input.
        let snapshots = snapshot
            .windows
            .iter()
            .zip(snapshot.limits.iter())
            .map(|(window, limit)| GlmQuotaSnapshot {
                window_duration_secs: window.duration_secs,
                resets_at: window.resets_at,
                observed_at,
                native_used_units: window.used_units,
                native_limit_units: window.limit_units,
                native_remaining_units: window.remaining_units,
                percentage_raw: whole_percent(limit.percentage),
                used_fraction_units: window.used_fraction_units,
                measurement_resolution_fraction_units: window
                    .measurement_resolution_fraction_units,
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
            .observe_glm_windows(&profile.subject_id, &profile.plan, snapshots.clone())
            .await
        {
            eprintln!(
                "GLM quota observation persistence deferred profile={}: {error:#}",
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

    /// Free quota read used by preflight and roster reload: the key is valid or it is not.
    async fn probe_profile(&self, profile: &Arc<RuntimeProfile>) -> Result<(), GatewayFailure> {
        self.fetch_quota(profile, false).await.map(|_| ())
    }

    /// GET the provider's free quota endpoint. Authentication is the **raw key without the
    /// `Bearer` prefix** the generation route uses (oss-hypothesis wire contract, manifest §4);
    /// validity is decided on the business code inside the body, never on the HTTP status
    /// (HTTP 200 + `code: 401` is a dead key).
    async fn fetch_quota(
        &self,
        profile: &Arc<RuntimeProfile>,
        during_shutdown: bool,
    ) -> Result<client::QuotaSnapshot, GatewayFailure> {
        let send = profile
            .client
            .get(probe_url(&profile.base_url, ProbeRoute::Quota))
            .header(
                "authorization",
                quota_authorization(&profile.credential.api_key),
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
                    return Err(GatewayFailure::Unavailable("glm_shutdown"));
                }
            }
        }
        .map_err(|_| GatewayFailure::Transport)?;
        let status = response.status().as_u16();
        let body = read_bounded(response, ERROR_BODY_LIMIT).await?;
        match client::parse_quota_probe(status, &body) {
            Ok(QuotaProbe::Valid(snapshot)) => Ok(snapshot),
            Ok(QuotaProbe::Invalid) => Err(GatewayFailure::Auth),
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

    /// Read only cached operational state. Metrics collection and the admin projection never
    /// start a network request here.
    pub fn operational_status(&self) -> GlmOperationalStatus {
        let delivery = self
            .turn_queue
            .lock()
            .expect("GLM turn queue lock")
            .health();
        let now = now_unix();
        let profiles = self.profiles.read().expect("GLM profiles lock");
        let mut account_dead_profiles = 0;
        let mut account_suspect_profiles = 0;
        let mut transport_cooling_profiles = 0;
        let mut quota_cooling_profiles = 0;
        let mut inflight_requests = 0u64;
        let mut statuses = Vec::with_capacity(profiles.len());
        for profile in profiles.iter() {
            let health = profile.health.lock().expect("GLM profile health lock");
            let transport_until = active_cooling(health.transport_cool_until, now);
            let quota_until = active_cooling(health.quota_cool_until, now);
            account_dead_profiles += usize::from(health.account_dead);
            account_suspect_profiles += usize::from(health.account_suspect);
            transport_cooling_profiles += usize::from(transport_until.is_some());
            quota_cooling_profiles += usize::from(quota_until.is_some());
            let inflight = profile.inflight.load(Ordering::Acquire);
            inflight_requests += u64::from(inflight);
            statuses.push(GlmProfileStatus {
                id: profile.id.clone(),
                plan: bounded_plan_label(&profile.plan),
                live: health.authenticated,
                account_dead: health.account_dead,
                account_suspect: health.account_suspect,
                transport_cool_until: transport_until,
                quota_cool_until: quota_until,
                inflight,
                quota_observed_at: health.quota_observed_at,
                quota_windows: health.quota_windows.clone(),
            });
        }
        drop(profiles);
        // Availability reuses the selection ineligibility view. `glm-5.2` is served by every
        // reviewed plan (manifest §3), so only live cooling axes — never a plan gap — can mark
        // a profile ineligible here.
        let candidates: Vec<Candidate> = self
            .profiles
            .read()
            .expect("GLM profiles lock")
            .iter()
            .map(|profile| profile.candidate("glm-5.2", now))
            .collect();
        let available_profiles = candidates.len() - ineligible_ids(&candidates).len();
        GlmOperationalStatus {
            total_profiles: candidates.len(),
            live_profiles: self.live_profiles.load(Ordering::Acquire),
            available_profiles,
            account_dead_profiles,
            account_suspect_profiles,
            transport_cooling_profiles,
            quota_cooling_profiles,
            inflight_requests,
            missing_terminal_usage: self.missing_terminal_usage.load(Ordering::Acquire),
            served_model_rejected: self.served_model_rejected.load(Ordering::Acquire),
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
            .expect("GLM profiles lock")
            .iter()
            .find(|profile| profile.subject_id == subject_id)
            .map(|profile| profile.id.clone())
    }

    pub fn readiness(&self) -> Result<(), NotReady> {
        let status = self.operational_status();
        readiness(status.live_profiles, status.delivery.persistence_ok)
    }

    pub(crate) async fn handle(self: &Arc<Self>, mut request: GlmRequest) -> Response {
        if self.shutting_down.load(Ordering::Acquire) {
            return error_response(GatewayFailure::Unavailable("glm_shutdown"));
        }
        if request
            .billing
            .as_ref()
            .is_some_and(|input| input.strict_policy)
        {
            return error_response(GatewayFailure::Unsupported(
                "glm_strict_pricing_unavailable",
            ));
        }
        if let Err(error) = validate_priced_surface(&request.body) {
            return error_response(error);
        }
        let context_mode = context_mode(&request.model).to_string();
        let reasoning_effort = match reasoning_effort(&request.model, &request.body) {
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
        // The Claude Code identity goes first into `system` (once — a body that already carries
        // a Claude Code marker is left as sent, so a genuine Claude Code customer is never
        // doubled). The fleet persona needs it in the body, not only in headers: risk-control
        // fingerprints the whole request shape.
        let identity = self.config.transport.identity.clone();
        super::transport::inject_identity(&mut request.body, &identity.identity);
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
            // The billing header block is per profile (cch and the .dNN build suffix are keyed
            // on the roster id), so it is written per attempt; on rotation it is replaced in
            // place, never duplicated. With injection off the pre-serialized body is reused.
            let attempt_body = if identity.inject_billing {
                super::transport::set_billing_block(
                    &mut request.body,
                    &identity.billing_header_for(&profile.id),
                );
                match serde_json::to_vec(&request.body) {
                    Ok(body) => Bytes::from(body),
                    Err(_) => return error_response(GatewayFailure::BadRequest("invalid_json")),
                }
            } else {
                body.clone()
            };
            let response = match self
                .send_generation(&profile, attempt_body, stream_requested, &request_id)
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
                    profile.apply_effect(
                        decision.effect,
                        now_unix(),
                        Some(&request.model),
                        None,
                    );
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
            if (200..300).contains(&status) {
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
                            return error_response(GatewayFailure::Unavailable("glm_shutdown"))
                        }
                    };
                    let headers = response_headers(&response);
                    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY);
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
                    // Atomic delivery marker before the first public byte. Everything after this
                    // point is committed to this profile — no retry, no account switch.
                    if !self.mark_delivering(reservation.as_ref()).await {
                        // Upstream may still consume the turn. Drain and preserve provider
                        // evidence, but keep the customer hold guard armed for a refund.
                        let _ = self.spawn_stream(
                            background, lease, accounting, None, sse, initial, upstream, false,
                        );
                        return error_response(GatewayFailure::Unavailable(
                            "glm_delivery_marker_unavailable",
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
                        "glm_delivery_marker_unavailable",
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
                profile.mark_healthy(&request.model);
                self.refresh_live_profile_count();
                return response_with(
                    StatusCode::from_u16(status).unwrap_or(StatusCode::OK),
                    headers,
                    Body::from(body),
                );
            }

            // Pre-byte failure: the two-layer error contract (manifest §4.2) means the business
            // code inside the body wins over the HTTP class, so classification needs the bounded
            // body — including the quota wall's reset evidence when the provider named one.
            let (error_body, read_ok) = match read_bounded(response, ERROR_BODY_LIMIT).await {
                Ok(body) => (body, true),
                Err(_) => (Bytes::new(), false),
            };
            let payload = serde_json::from_slice::<Value>(&error_body).ok();
            let verdict = if read_ok {
                classify_status(status, payload.as_ref().and_then(error_business_code))
            } else {
                UpstreamVerdict::Transport
            };
            let quota_reset = payload.as_ref().and_then(client::quota_wall_reset);
            let remaining = self.eligible_count(&request.model, &excluded, &profile.id);
            let decision = decide(verdict, Delivery::PreByte, policy, remaining);
            profile.apply_effect(
                decision.effect,
                now_unix(),
                Some(&request.model),
                quota_reset,
            );
            policy = decision.policy;
            match decision.next {
                NextStep::RotateToAnotherProfile => {
                    excluded.insert(profile.id.clone());
                    drop(lease);
                    continue;
                }
                NextStep::SurfaceCapacityExhausted => {
                    return error_response(GatewayFailure::Capacity);
                }
                NextStep::SurfaceUpstreamError => {
                    return error_response(GatewayFailure::Upstream(status));
                }
                // `decide` only yields Deliver for an Ok verdict, unreachable on a non-2xx.
                NextStep::Deliver => return error_response(GatewayFailure::Protocol),
            }
        }
    }

    async fn reserve_customer(
        &self,
        body: &mut Value,
        raw_len: usize,
        model: &str,
        request_id: &str,
        priced_ts: i64,
        input: Option<&GlmBillingInput>,
        execution: ExecutionAttempt,
    ) -> Result<Option<Reservation>, GatewayFailure> {
        let Some(input) = input else {
            return Ok(None);
        };
        let billing = self.billing.as_ref().ok_or(GatewayFailure::Unavailable(
            "glm_billing_authority_unavailable",
        ))?;
        let resolved = glm_resolve_subscription_model(model)
            .ok_or(GatewayFailure::Unsupported("glm_model_unavailable"))?;
        let prices = glm_prices_for_served_model(resolved.official_model, priced_ts)
            .ok_or(GatewayFailure::Unsupported("glm_price_unavailable"))?;
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
                .map_err(|_| GatewayFailure::Unavailable("glm_reservation_unavailable"))?
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
                        .map_err(|_| GatewayFailure::Unavailable("glm_balance_unavailable"))?
                        .map(|account| i128::from(account.balance_nano))
                        .unwrap_or(0);
                }
            }
        }
        Err(GatewayFailure::LowBalance)
    }

    /// Native request on the profile's own console origin. The FULL Claude Code identity set
    /// comes from the reviewed transport config, never from the inbound client: Z.ai
    /// risk-control fingerprints SDK-like traffic and bans the subscription over it
    /// (manifest §4), and a foreign value under our persona (a Python SDK's
    /// `x-stainless-lang: python` under a claude-cli UA) is exactly the contradiction it keys
    /// on — the same skip rule as the Claude plane's `skip_req_header`. A redirect is never
    /// followed — it must not carry a subscription key to another origin.
    async fn send_generation(
        &self,
        profile: &RuntimeProfile,
        body: Bytes,
        stream: bool,
        request_id: &str,
    ) -> Result<wreq::Response, GatewayFailure> {
        let url = format!(
            "{}{}",
            profile.base_url.trim_end_matches('/'),
            GLM_ANTHROPIC_MESSAGES_PATH
        );
        let identity = &self.config.transport.identity;
        profile
            .client
            .post(url)
            .header(
                self.config.transport.auth_scheme.header_name(),
                self.config
                    .transport
                    .auth_scheme
                    .header_value(&profile.credential.api_key),
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
            // Per-profile UA pin (fleet pool) — the same anti-cluster lever the Claude plane
            // applies per subscription.
            .header("user-agent", identity.user_agent_for(&profile.id))
            .header("anthropic-version", &identity.anthropic_version)
            .header("anthropic-beta", &identity.anthropic_beta)
            .header("x-app", &identity.x_app)
            .header("x-stainless-lang", &identity.stainless_lang)
            .header("x-stainless-runtime", &identity.stainless_runtime)
            .header(
                "x-stainless-runtime-version",
                &identity.stainless_runtime_version,
            )
            .header(
                "x-stainless-package-version",
                &identity.stainless_package_version,
            )
            .header("x-stainless-os", &identity.stainless_os)
            .header("x-stainless-arch", &identity.stainless_arch)
            // A real Claude Code always sends this.
            .header("anthropic-dangerous-direct-browser-access", "true")
            .header("x-client-request-id", request_id)
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

    /// Incremental SSE passthrough with no translation: the provider speaks the Anthropic
    /// Messages wire natively (manifest §4), so public bytes move unchanged. A stalled or
    /// disconnected downstream stops public delivery but never the upstream drain — the
    /// bounded task keeps reading to terminal usage for exact settlement.
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
                accounting.profile.mark_healthy(&accounting.requested_model);
            } else {
                accounting.profile.apply_effect(
                    ProfileEffect::TransportFault,
                    now_unix(),
                    None,
                    None,
                );
            }
            gateway.refresh_live_profile_count();
        });
        let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
            receiver
                .recv()
                .await
                .map(|chunk| (Ok::<Bytes, Infallible>(chunk), receiver))
        });
        Body::from_stream(stream)
    }

    /// Terminal accounting for one delivered turn. Authoritative terminal usage plus an
    /// in-set served model produces the immutable dual-ledger event and the exact customer
    /// settlement. Missing evidence never synthesizes usage: the reservation settles on the
    /// documented conservative hold and the typed operational counter advances instead.
    async fn finalize_turn(
        &self,
        context: &AccountingContext,
        reservation: Option<Reservation>,
        parsed: ParsedAccounting,
    ) {
        let completed_at = now_unix();
        let priced = parsed
            .terminal
            .then_some(())
            .and_then(|_| parsed.usage.as_ref())
            .zip(parsed.served_model.as_deref())
            .and_then(|(usage, served)| {
                price_turn(usage, served, context.priced_ts, completed_at).ok()
            });

        if priced.is_none() {
            if parsed.terminal
                && parsed.usage.as_ref().is_some_and(|usage| !usage.is_zero())
                && parsed.served_model.is_some()
            {
                // Evidence arrived but the served model is outside the priced/rated admission
                // set (or the usage vector broke its own invariants): billing fails closed.
                self.served_model_rejected.fetch_add(1, Ordering::Relaxed);
            } else {
                self.missing_terminal_usage.fetch_add(1, Ordering::Relaxed);
            }
        }

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
                            eprintln!("GLM customer settlement deferred: {error:#}");
                        }
                    }
                    None => {
                        // Delivery occurred but authoritative terminal usage did not (or its
                        // served model is outside the admission set). Preserve the conservative
                        // hold and create no immutable provider event.
                        let reason = if parsed.terminal
                            && parsed.usage.as_ref().is_some_and(|usage| !usage.is_zero())
                            && parsed.served_model.is_some()
                        {
                            "glm-served-model-rejected"
                        } else {
                            "glm-terminal-usage-missing"
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
                            eprintln!("GLM conservative settlement deferred: {error:#}");
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
        let event = match priced.calibration_event(context, &usage, &served_model, completed_at) {
            Ok(event) => event,
            Err(error) => {
                eprintln!("GLM calibration event rejected before FIFO: {error:#}");
                return;
            }
        };
        self.enqueue_turn(event).await;
    }

    async fn enqueue_turn(&self, event: GlmTurnCalibrationEvent) {
        let _drain = self.turn_drain.lock().await;
        let accepted = self
            .turn_queue
            .lock()
            .expect("GLM turn queue lock")
            .push(event);
        if !accepted {
            eprintln!("GLM calibration event dropped because the bounded FIFO is full");
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
                .expect("GLM turn queue lock")
                .head()
                .cloned();
            let Some(head) = head else { break };
            let outcome = match &self.billing {
                Some(billing) => match billing.record_glm_turn(head).await {
                    Ok(_) => WriteOutcome::Durable,
                    Err(error) if registry::is_glm_turn_replay_conflict(&error) => {
                        WriteOutcome::Conflict
                    }
                    Err(error) => {
                        eprintln!(
                            "GLM calibration persistence deferred with FIFO head retained: {error:#}"
                        );
                        WriteOutcome::Transient
                    }
                },
                None => WriteOutcome::Transient,
            };
            self.turn_queue
                .lock()
                .expect("GLM turn queue lock")
                .resolve_head(outcome);
            if outcome == WriteOutcome::Transient {
                break;
            }
        }
        self.turn_queue
            .lock()
            .expect("GLM turn queue lock")
            .may_poll_quota()
    }

    /// Close admission, wait for stream finalizers and detached drains, then run the final
    /// quota flush with the same turn-before-quota ordering inside the process deadline.
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
            eprintln!("GLM shutdown calibration drain remained incomplete at deadline");
        }
    }
}

impl RuntimeProfile {
    /// A passing free quota probe is auth/capacity evidence: it clears the account and
    /// transport axes. It deliberately does NOT touch the model-ineligible set — the probe is
    /// a monitor surface, not the generation route (PROVIDER_ONBOARDING §8.4).
    fn mark_probe_healthy(&self) {
        let mut health = self.health.lock().expect("GLM profile health lock");
        health.authenticated = true;
        health.account_dead = false;
        health.account_suspect = false;
        health.transport_cool_until = 0;
    }
}

fn same_profile_generation(left: &[Arc<RuntimeProfile>], right: &[Arc<RuntimeProfile>]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| Arc::ptr_eq(left, right))
}

fn profiles_match_roster(profiles: &[Arc<RuntimeProfile>], roster: &[GlmProfile]) -> bool {
    profiles.len() == roster.len()
        && profiles
            .iter()
            .zip(roster)
            .all(|(profile, loaded)| profile.matches_roster(loaded))
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

/// One priced turn: the official API replacement legs (nanoUSD) and the native credit legs
/// (microcredits), computed independently from the same authoritative usage. The cache-write
/// money leg exists only inside this struct — the immutable event folds it into the
/// fresh-input leg, because GLM publishes no separate paid write rate ("Limited-time Free"
/// storage; a write is a miss).
#[derive(Clone, Copy)]
struct PricedTurn {
    prices: GlmPrices,
    input: i128,
    cache_read: i128,
    cache_write: i128,
    output: i128,
    total: i128,
    native_input: i128,
    native_cache_read: i128,
    native_output: i128,
    native_total: i128,
    off_peak: bool,
}

impl PricedTurn {
    fn usage_event(&self, served_model: &str, priced_ts: i64) -> registry::UsageEventInput {
        let clamp = |value: i128| value.clamp(0, i128::from(i64::MAX)) as i64;
        registry::UsageEventInput {
            model: served_model.to_string(),
            provider: registry::PROVIDER_GLM.to_string(),
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
            speed: "standard".to_string(),
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

    /// The immutable dual-ledger evidence for one turn (`docs/engine/GLM_PROVIDER.md` §5.3):
    /// requested and served model kept apart, disjoint usage legs, exact API nanoUSD legs by
    /// the served model's rate card, native microcredit legs by the served model's published
    /// multipliers with the off-peak schedule evaluated at completion, and both schedule ids.
    fn calibration_event(
        &self,
        context: &AccountingContext,
        usage: &GlmUsage,
        served_model: &str,
        completed_at: i64,
    ) -> anyhow::Result<GlmTurnCalibrationEvent> {
        let i64_counter = |value: u64| i64::try_from(value).context("GLM usage overflow");
        let i64_money = |value: i128| i64::try_from(value).context("GLM money overflow");
        // The event has no cache-write money leg: with the write billed at the miss rate, its
        // cost folds into fresh input so the three disjoint legs still sum to the total.
        let api_fresh_input = self
            .input
            .checked_add(self.cache_write)
            .ok_or_else(|| anyhow!("GLM cost overflow"))?;
        let event = GlmTurnCalibrationEvent {
            request_id: context.request_id.clone(),
            subject_id: context.profile.subject_id.clone(),
            plan: context.profile.plan.clone(),
            requested_model: context.requested_model.clone(),
            served_model: served_model.to_string(),
            context_mode: context.context_mode.clone(),
            reasoning_effort: context.reasoning_effort.clone(),
            api_tariff_schedule_id: GLM_TARIFF_SCHEDULE_ID.to_string(),
            credit_schedule_id: GLM_CREDIT_SCHEDULE_ID.to_string(),
            priced_ts: context.priced_ts,
            completed_at,
            fresh_input_tokens: i64_counter(usage.input_tokens)?,
            cached_input_tokens: i64_counter(usage.cache_read_tokens)?,
            cache_write_tokens: i64_counter(usage.cache_write_tokens)?,
            output_tokens: i64_counter(usage.output_tokens)?,
            reasoning_tokens: i64_counter(usage.reasoning_output_tokens)?,
            api_fresh_input_nanousd: i64_money(api_fresh_input)?,
            api_cached_input_nanousd: i64_money(self.cache_read)?,
            api_output_nanousd: i64_money(self.output)?,
            api_total_nanousd: i64_money(self.total)?,
            native_fresh_input_microcredits: i64_money(self.native_input)?,
            native_cached_input_microcredits: i64_money(self.native_cache_read)?,
            native_output_microcredits: i64_money(self.native_output)?,
            native_total_microcredits: i64_money(self.native_total)?,
            off_peak: self.off_peak,
        };
        event.validate()?;
        Ok(event)
    }
}

struct ParsedAccounting {
    usage: Option<GlmUsage>,
    served_model: Option<String>,
    terminal: bool,
}

fn non_stream_accounting(body: &[u8]) -> ParsedAccounting {
    let parsed = serde_json::from_slice::<Value>(body).ok();
    ParsedAccounting {
        usage: parsed
            .as_ref()
            .and_then(metering::glm::usage_from_response_value),
        served_model: parsed
            .as_ref()
            .and_then(|value| value.get("model"))
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .map(str::to_string),
        terminal: parsed.is_some(),
    }
}

/// Price one terminal turn on both ledgers. The served model must sit inside the admission
/// set on BOTH schedules: an id with no reviewed dollar card cannot price the API ledger, and
/// an id without published credit multipliers (an echoed `glm-5`/`glm-5.1`, which the provider
/// silently re-routes) cannot price the native ledger — either way the turn fails closed
/// rather than borrowing a neighbour's rate (manifest §3).
fn price_turn(
    usage: &GlmUsage,
    served_model: &str,
    priced_ts: i64,
    completed_at: i64,
) -> anyhow::Result<PricedTurn> {
    usage
        .validate()
        .map_err(|_| anyhow!("invalid GLM usage"))?;
    if usage.is_zero() {
        anyhow::bail!("empty GLM usage is not terminal evidence");
    }
    let prices = glm_prices_for_served_model(served_model, priced_ts)
        .ok_or_else(|| anyhow!("unpriced GLM served model"))?;
    let credit_rates = glm_credit_rates_for_served_model(served_model)
        .ok_or_else(|| anyhow!("GLM served model has no published credit rates"))?;
    let leg = |tokens: u64, rate: i128| {
        i128::from(tokens)
            .checked_mul(rate)
            .ok_or_else(|| anyhow!("GLM cost overflow"))
    };
    let input = leg(usage.input_tokens, prices.input)?;
    let cache_read = leg(usage.cache_read_tokens, prices.cached_input)?;
    let cache_write = leg(usage.cache_write_tokens, prices.cache_write)?;
    let output = leg(usage.output_tokens, prices.output)?;
    let total = cost_nanodollars(usage, &prices).map_err(|_| anyhow!("GLM cost overflow"))?;
    let off_peak = !glm_is_peak_utc(completed_at);
    // The official formula has no cache-write leg (storage is "Limited-time Free"), so the
    // three native legs below already sum to the total; `glm_credit_cost_micro` recomputes it
    // from the same formula rather than trusting the parts.
    let native_leg = |tokens: u64, tenths: i128| {
        let micro_per_weighted: i128 = if off_peak { 5 } else { 10 };
        i128::from(tokens)
            .checked_mul(tenths)
            .and_then(|weighted| weighted.checked_mul(micro_per_weighted))
            .ok_or_else(|| anyhow!("GLM credit overflow"))
    };
    let native_input = native_leg(usage.input_tokens, credit_rates.input_tenths)?;
    let native_cache_read = native_leg(usage.cache_read_tokens, credit_rates.cached_input_tenths)?;
    let native_output = native_leg(usage.output_tokens, credit_rates.output_tenths)?;
    let native_total = glm_credit_cost_micro(usage, &credit_rates, off_peak)
        .map_err(|_| anyhow!("GLM credit overflow"))?;
    Ok(PricedTurn {
        prices,
        input,
        cache_read,
        cache_write,
        output,
        total,
        native_input,
        native_cache_read,
        native_output,
        native_total,
        off_peak,
    })
}

fn cap_to_balance(
    balance: i128,
    input_upper_bound: i128,
    prices: GlmPrices,
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

/// Unknown tool/media surfaces fail closed (manifest §3 and §5.1: vision, highspeed, web
/// search and MCP tools are `unavailable` in v1 — their per-request ceiling is unproven, so
/// the plane must not spend budget on them).
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
        return Err(GatewayFailure::Unsupported("glm_tools_unavailable"));
    }
    if contains_unpriced_media(body) {
        return Err(GatewayFailure::Unsupported("glm_media_unavailable"));
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

/// `glm-5.2[1m]` is the 1M-window selector spelling, not a distinct model (manifest §3); every
/// other alias runs the default 200k window.
fn context_mode(model: &str) -> &'static str {
    if model.eq_ignore_ascii_case("glm-5.2[1m]") {
        "1m"
    } else {
        "200k"
    }
}

/// Reasoning effort for the calibration event. Only glm-5.2 takes one (manifest §3); the
/// provider maps low/medium→high, xhigh→max and none/minimal→off, so the event stores the
/// mapped value, never the raw request string. Other models carry `None`.
fn reasoning_effort(model: &str, body: &Value) -> Result<Option<String>, GatewayFailure> {
    let canonical = model.to_ascii_lowercase();
    if canonical != "glm-5.2" && canonical != "glm-5.2[1m]" {
        return Ok(None);
    }
    if body
        .pointer("/thinking/type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.eq_ignore_ascii_case("disabled"))
    {
        return Ok(Some("off".to_string()));
    }
    let Some(raw) = body.get("reasoning_effort") else {
        return Ok(Some("high".to_string()));
    };
    if raw.is_null() {
        return Ok(Some("high".to_string()));
    }
    let Some(raw) = raw.as_str() else {
        return Err(GatewayFailure::BadRequest("invalid_reasoning_effort"));
    };
    let normalized = match raw.to_ascii_lowercase().as_str() {
        "max" | "xhigh" => "max",
        "high" | "medium" | "low" => "high",
        "none" | "minimal" | "off" => "off",
        _ => return Err(GatewayFailure::BadRequest("invalid_reasoning_effort")),
    };
    Ok(Some(normalized.to_string()))
}

/// The provider's own percentage display value, preserved as raw evidence at whole-percent
/// granularity. It never feeds the estimator: the fraction derives from the raw used/limit
/// counters with their real measurement resolution.
fn whole_percent(percentage: Option<f64>) -> Option<i64> {
    let value = percentage?;
    if !value.is_finite() || value < 0.0 || value >= i64::MAX as f64 {
        return None;
    }
    Some((value + 0.5) as i64)
}

#[derive(Clone, Copy, Debug)]
enum GatewayFailure {
    /// The provider rejected the static key (business-code 401 or a real 401), or the plan
    /// expired. Terminal for the profile until republication — never retried in place.
    Auth,
    /// Risk-control fair-use or account anomaly: recoverable, out of rotation pending review.
    Suspect,
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
            Self::Auth => UpstreamVerdict::AccountDead,
            Self::Suspect => UpstreamVerdict::AccountSuspect,
            Self::Transport | Self::Protocol | Self::Unavailable(_) => UpstreamVerdict::Transport,
            Self::Upstream(status) => classify_status(status, None),
            Self::Capacity => UpstreamVerdict::QuotaExhausted,
            Self::LowBalance | Self::BadRequest(_) | Self::Unsupported(_) => {
                UpstreamVerdict::ClientError
            }
        }
    }

    fn from_verdict(verdict: UpstreamVerdict, status: u16) -> Self {
        match verdict {
            UpstreamVerdict::AccountDead => Self::Auth,
            UpstreamVerdict::AccountSuspect => Self::Suspect,
            UpstreamVerdict::QuotaExhausted => Self::Capacity,
            UpstreamVerdict::Transport => Self::Transport,
            UpstreamVerdict::ModelIneligible | UpstreamVerdict::ClientError => {
                Self::Upstream(status)
            }
            UpstreamVerdict::Ok => Self::Protocol,
        }
    }

    fn class(self) -> &'static str {
        match self {
            Self::Auth => "auth",
            Self::Suspect => "suspect",
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

/// Synthetic errors go through the shared Anthropic-compatible sanitizer: the client never
/// learns about the backend, the roster, subscriptions or provider bodies — only an
/// Anthropic-authentic status/type/message triplet plus a static terminal reason.
fn error_response(error: GatewayFailure) -> Response {
    let (public, reason, retry_after) = match error {
        GatewayFailure::Auth => (LocalErr::Overloaded, "glm_auth_unavailable", Some(2)),
        GatewayFailure::Suspect => (LocalErr::Overloaded, "glm_account_unavailable", Some(2)),
        GatewayFailure::Transport | GatewayFailure::Protocol => {
            (LocalErr::Overloaded, "glm_upstream_unavailable", Some(2))
        }
        GatewayFailure::Capacity => (LocalErr::RateLimited, "glm_capacity_exhausted", Some(60)),
        GatewayFailure::LowBalance => (LocalErr::LowBalance, "billing_limit", None),
        GatewayFailure::BadRequest(code) => (LocalErr::BadRequest, code, None),
        GatewayFailure::Unsupported(code) => (LocalErr::NotFound, code, None),
        GatewayFailure::Unavailable(code) => (LocalErr::Overloaded, code, Some(2)),
        GatewayFailure::Upstream(status) if status == 429 => {
            (LocalErr::RateLimited, "glm_upstream_rejected", Some(2))
        }
        GatewayFailure::Upstream(404) => (LocalErr::NotFound, "glm_upstream_rejected", None),
        GatewayFailure::Upstream(status) if (400..500).contains(&status) => {
            (LocalErr::BadRequest, "glm_upstream_rejected", None)
        }
        GatewayFailure::Upstream(_) => (LocalErr::Overloaded, "glm_upstream_rejected", Some(2)),
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
        .expect("GLM response");
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
    use glm_credential::{
        encode_envelope, CredentialKeyring, GlmCredentialKind, GlmPlan,
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
            let root = std::env::temp_dir().join(format!("glm-gateway-{suffix}"));
            fs::create_dir_all(root.join("credentials")).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            fs::set_permissions(root.join("credentials"), fs::Permissions::from_mode(0o700))
                .unwrap();
            Self { root }
        }

        fn publish_profile(&self, api_key: &str, base_url: &str) {
            self.publish_profile_as("glm-01", api_key, base_url);
        }

        fn publish_profile_as(&self, id: &str, api_key: &str, base_url: &str) {
            self.publish_profiles(&[(id, api_key)], base_url);
        }

        fn publish_profiles(&self, profiles: &[(&str, &str)], base_url: &str) {
            let ring = keyring();
            let mut entries = Vec::with_capacity(profiles.len());
            for (id, api_key) in profiles {
                let credential = GlmCredential {
                    version: 1,
                    kind: GlmCredentialKind::PlanKey,
                    api_key: (*api_key).into(),
                    plan: GlmPlan::Pro,
                    base_url: base_url.into(),
                    proxy_url: String::new(),
                };
                let envelope = ring.seal("a1", id, &credential).unwrap();
                let credential_path = self.root.join("credentials").join(format!("{id}.json"));
                write_private(&credential_path, &encode_envelope(&envelope).unwrap());
                entries.push(json!({
                    "id": id,
                    "credential_file": credential_path.to_string_lossy(),
                }));
            }
            write_private(
                &self.root.join("profiles.json"),
                &serde_json::to_vec(&json!({"profiles": entries})).unwrap(),
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

    fn config(root: &Path, base_url: &str) -> GlmPlaneConfig {
        let _ = base_url; // GLM has no fleet-wide base url: the console origin is per-profile.
        GlmPlaneConfig {
            roster_dir: root.to_path_buf(),
            keyring: keyring(),
            transport: super::super::transport::GlmTransportConfig {
                auth_scheme: super::super::transport::AuthScheme::Bearer,
                request_timeout: Duration::from_secs(5),
                identity: super::super::transport::GlmIdentityHeaders::default(),
            },
            readiness_probe: ProbeRoute::Quota,
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

    /// SSE server that proves incremental passthrough: the first frame is flushed to the
    /// client and the rest of the stream is held until the test confirms the frame arrived.
    fn gated_sse_server() -> (String, mpsc::Receiver<Vec<u8>>, mpsc::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (request_sender, request_receiver) = mpsc::channel();
        let (gate_sender, gate_receiver) = mpsc::channel::<()>();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            request_sender.send(read_request(&mut stream)).unwrap();
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n",
                )
                .unwrap();
            let write_chunk = |stream: &mut TcpStream, data: &[u8]| {
                stream
                    .write_all(format!("{:x}\r\n", data.len()).as_bytes())
                    .unwrap();
                stream.write_all(data).unwrap();
                stream.write_all(b"\r\n").unwrap();
                stream.flush().unwrap();
            };
            write_chunk(
                &mut stream,
                br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}"#
                    .as_slice(),
            );
            write_chunk(&mut stream, b"\n");
            // Hold the terminal frames until the client proves the first one arrived.
            gate_receiver.recv().unwrap();
            write_chunk(
                &mut stream,
                br#"data: {"type":"message_delta","usage":{"output_tokens":4}}"#
                    .as_slice(),
            );
            write_chunk(&mut stream, b"\n\n");
            write_chunk(&mut stream, br#"data: {"type":"message_stop"}"#.as_slice());
            write_chunk(&mut stream, b"\n\n");
            stream.write_all(b"0\r\n\r\n").unwrap();
            stream.flush().unwrap();
        });
        (format!("http://{address}"), request_receiver, gate_sender)
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

    fn request_body(model: &str) -> Value {
        json!({
            "model": model,
            "max_tokens": 8,
            "messages": [{"role": "user", "content": "hello"}],
        })
    }

    fn glm_request(model: &str, body: Value, billing: Option<GlmBillingInput>) -> GlmRequest {
        GlmRequest {
            raw_body_len: serde_json::to_vec(&body).unwrap().len(),
            body,
            model: model.into(),
            execution: ExecutionAttempt::direct(),
            billing,
            affinity: None,
            affinity_store: affinity(),
        }
    }

    fn turn_event(request_id: &str) -> GlmTurnCalibrationEvent {
        GlmTurnCalibrationEvent {
            request_id: request_id.into(),
            subject_id: "subject-1".into(),
            plan: "Pro".into(),
            requested_model: "glm-5.2".into(),
            served_model: "glm-5.2".into(),
            context_mode: "200k".into(),
            reasoning_effort: Some("high".into()),
            api_tariff_schedule_id: GLM_TARIFF_SCHEDULE_ID.into(),
            credit_schedule_id: GLM_CREDIT_SCHEDULE_ID.into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_001,
            fresh_input_tokens: 1,
            cached_input_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 1,
            reasoning_tokens: 0,
            api_fresh_input_nanousd: 1_400,
            api_cached_input_nanousd: 0,
            api_output_nanousd: 4_400,
            api_total_nanousd: 5_800,
            native_fresh_input_microcredits: 69,
            native_cached_input_microcredits: 0,
            native_output_microcredits: 240,
            native_total_microcredits: 309,
            off_peak: false,
        }
    }

    /// A valid quota envelope: the documented credits form with both independent windows.
    fn quota_body(used_5h: i64, limit_5h: i64, used_week: i64, limit_week: i64) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "code": 200, "msg": "success", "success": true,
            "data": {
                "limits": [
                    {"type":"TIME_LIMIT","unit":5,"number":limit_5h,"usage":used_5h,
                     "currentValue":limit_5h-used_5h,"remaining":limit_5h-used_5h,
                     "percentage":100.0*used_5h as f64/limit_5h as f64,
                     "nextResetTime":4_102_444_800_000i64},
                    {"type":"TIME_LIMIT","unit":7,"number":limit_week,"usage":used_week,
                     "currentValue":limit_week-used_week,"remaining":limit_week-used_week,
                     "percentage":100.0*used_week as f64/limit_week as f64,
                     "nextResetTime":4_102_500_000_000i64}
                ],
                "usageDetails": [{"modelCode":"glm-5.2"}]
            }
        }))
        .unwrap()
    }

    fn quota_snapshot(
        duration_secs: i64,
        used: i64,
        limit: i64,
        resets_at: i64,
        observed_at: i64,
    ) -> GlmQuotaSnapshot {
        let derived = registry::glm_fraction_from_native(used, limit).unwrap();
        GlmQuotaSnapshot {
            window_duration_secs: duration_secs,
            resets_at: Some(resets_at),
            observed_at,
            native_used_units: Some(used),
            native_limit_units: Some(limit),
            native_remaining_units: Some(limit - used),
            percentage_raw: None,
            used_fraction_units: Some(derived.used_fraction_units),
            measurement_resolution_fraction_units: Some(
                derived.measurement_resolution_fraction_units,
            ),
        }
    }

    async fn wait_for_pending(gateway: &GlmGateway, expected: usize) {
        for _ in 0..100 {
            if gateway.operational_status().delivery.pending_events == expected {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("turn FIFO never reached {expected} pending events");
    }

    #[tokio::test]
    async fn a_pending_turn_blocks_the_provider_quota_read() {
        let fixture = Fixture::new();
        let (base_url, mut requests, _responses) = controlled_mock_server(1);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None)
            .unwrap();
        gateway
            .turn_queue
            .lock()
            .unwrap()
            .push(turn_event("pending-before-poll"));

        assert_eq!(gateway.poll_quotas().await, 0);
        assert_eq!(gateway.operational_status().delivery.pending_events, 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), requests.recv())
                .await
                .is_err(),
            "the quota GET must not run past an undelivered spend head"
        );
    }

    #[tokio::test]
    async fn customer_generation_start_invalidates_a_concurrent_quota_snapshot_without_waiting() {
        let fixture = Fixture::new();
        let (base_url, mut requests, responses) = controlled_mock_server(1);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        let profile = gateway.profiles_snapshot()[0].clone();

        let poll = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        let request = requests.recv().await.unwrap();
        assert!(request.starts_with(b"GET /api/monitor/usage/quota/limit "));

        // No semaphore or maintenance wait: a customer lease starts immediately while the GET is
        // outstanding. Its epoch makes the returned snapshot unusable for calibration.
        let lease = ProfileLease::new(profile.clone());
        assert_eq!(profile.inflight.load(Ordering::Acquire), 1);
        responses
            .send(http_response("application/json", &quota_body(10, 12_000, 0, 60_000)))
            .unwrap();
        assert_eq!(poll.await.unwrap(), 0);
        drop(lease);
        assert_eq!(
            profile.candidate("glm-5.2", now_unix()).used_fraction_units,
            None
        );
    }

    #[tokio::test]
    async fn transient_observation_failure_keeps_the_previous_quota_generation() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(10, 12_000, 0, 60_000),
        )]);
        fixture.publish_profile("zai-key-1", &base_url);
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, &base_url),
            Some(billing),
        )
        .unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();

        // SQLite deliberately refuses GLM calibration. A successful provider GET is not enough
        // to publish steering before the durable PostgreSQL observation/CAS succeeds.
        assert_eq!(gateway.poll_quotas().await, 0);
        assert!(requests
            .recv()
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));
        let candidate = profile.candidate("glm-5.2", now_unix());
        assert_eq!(candidate.used_fraction_units, None);
        assert_eq!(candidate.quota_age_secs, None);
    }

    #[test]
    fn a_durable_snapshot_publishes_the_tightest_window_and_exact_full_reset() {
        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:1"),
            None,
        )
        .unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();
        let observed_at = now_unix();
        let snapshots = vec![
            quota_snapshot(
                registry::GLM_5H_WINDOW_SECS,
                250,
                1_000,
                observed_at + 300,
                observed_at,
            ),
            quota_snapshot(
                registry::GLM_WEEKLY_WINDOW_SECS,
                1_000,
                1_000,
                observed_at + 600,
                observed_at,
            ),
        ];
        profile.publish_quota(&snapshots, observed_at);
        let candidate = profile.candidate("glm-5.2", observed_at);
        assert_eq!(
            candidate.used_fraction_units,
            Some(registry::GLM_FRACTION_SCALE)
        );
        assert_eq!(candidate.quota_age_secs, Some(0));
        assert_eq!(candidate.ineligible, Some(Ineligible::QuotaWall));
        assert_eq!(
            profile.health.lock().unwrap().quota_cool_until,
            observed_at + 600
        );
    }

    #[tokio::test]
    async fn quota_poll_failures_stay_profile_local_and_classify_on_the_business_code() {
        let code_401 = br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#
            .as_slice();
        let quota_wall =
            br#"{"error":{"code":"1308","message":"5-hour quota exhausted"}}"#.as_slice();
        let fair_use = br#"{"error":{"code":"1313","message":"fair use violation"}}"#.as_slice();
        let empty = br#"{}"#.as_slice();
        for (status, body, expected) in [
            ("401 Unauthorized", empty, Some(Ineligible::AccountDead)),
            // The trap of this endpoint: HTTP 200 carrying code:401 is a dead key.
            ("200 OK", code_401, Some(Ineligible::AccountDead)),
            ("429 Too Many Requests", quota_wall, Some(Ineligible::QuotaWall)),
            ("429 Too Many Requests", fair_use, Some(Ineligible::AccountSuspect)),
            (
                "503 Service Unavailable",
                empty,
                Some(Ineligible::TransportWedged),
            ),
        ] {
            let fixture = Fixture::new();
            let (base_url, requests) =
                mock_server(vec![http_status_response(status, "application/json", body)]);
            fixture.publish_profile("zai-key-1", &base_url);
            let gateway =
                GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None)
                    .unwrap();
            let profile = gateway.profiles_snapshot()[0].clone();
            profile.mark_probe_healthy();
            gateway.live_profiles.store(1, Ordering::Release);

            assert_eq!(gateway.poll_quotas().await, 0, "status {status}");
            assert_eq!(gateway.profiles_snapshot().len(), 1, "status {status}");
            assert_eq!(
                profile.candidate("glm-5.2", now_unix()).ineligible,
                expected,
                "status {status}"
            );
            assert!(requests
                .recv()
                .unwrap()
                .starts_with(b"GET /api/monitor/usage/quota/limit "));
            // No same-profile retry: a refused static key is terminal, not a refresh prompt.
            assert!(requests.try_recv().is_err(), "status {status}");
        }
    }

    #[tokio::test]
    async fn a_profile_removed_during_quota_io_is_never_reintroduced() {
        let fixture = Fixture::new();
        let (base_url, mut requests, responses) = controlled_mock_server(1);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        let poll = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        assert!(requests
            .recv()
            .await
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));

        fixture.publish_empty_roster();
        assert!(gateway.refresh_profiles().await);
        assert!(gateway.profiles_snapshot().is_empty());
        responses
            .send(http_response("application/json", &quota_body(10, 12_000, 0, 60_000)))
            .unwrap();
        assert_eq!(poll.await.unwrap(), 0);
        assert!(gateway.profiles_snapshot().is_empty());
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));
    }

    #[tokio::test]
    async fn shutdown_cancels_the_steady_poll_and_bounds_its_final_quota_read() {
        let fixture = Fixture::new();
        let (base_url, mut requests, responses) = controlled_mock_server(2);
        fixture.publish_profile("zai-key-1", &base_url);
        let mut test_config = config(&fixture.root, &base_url);
        test_config.transport.request_timeout = Duration::from_secs(30);
        let gateway = Arc::new(GlmGateway::new_with_calibration(test_config, None).unwrap());
        let steady = {
            let gateway = gateway.clone();
            tokio::spawn(async move { gateway.poll_quotas().await })
        };
        assert!(requests
            .recv()
            .await
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));

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
        assert!(requests
            .recv()
            .await
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));
        stopping.await.unwrap();
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "provider I/O must not extend shutdown past the bounded deadline"
        );
    }

    #[tokio::test]
    async fn exact_alias_uses_quota_readiness_and_transparent_messages_bytes() {
        let fixture = Fixture::new();
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
            http_response("application/json", generation),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        assert_eq!(gateway.preflight().await, 1);
        assert_eq!(gateway.readiness(), Ok(()));

        let body = request_body("glm-5.2");
        let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let returned = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
            .await
            .unwrap();
        // Anthropic-compatible upstream: public bytes pass through without a translation layer.
        assert_eq!(returned.as_ref(), generation);

        let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(probe.starts_with("GET /api/monitor/usage/quota/limit "));
        // The quota endpoint takes the raw key WITHOUT the Bearer prefix (wire contract §4).
        assert!(probe
            .to_ascii_lowercase()
            .contains("authorization: zai-key-1"));
        assert!(!probe.to_ascii_lowercase().contains("bearer"));
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(turn.starts_with("POST /api/anthropic/v1/messages "));
        assert!(turn
            .to_ascii_lowercase()
            .contains("authorization: bearer zai-key-1"));
        // Generation traffic carries the full Claude-Code-compatible identity set: Z.ai
        // risk-control bans SDK-like clients, so these headers keep the subscription alive.
        let turn_lower = turn.to_ascii_lowercase();
        assert!(turn_lower.contains("user-agent: claude-cli/"));
        assert!(turn_lower.contains("anthropic-version: 2023-06-01"));
        assert!(turn_lower.contains("x-client-request-id:"));
        assert!(turn.contains("\"model\":\"glm-5.2\""));
        // No calibration authority in this test: evidence remains visible in the bounded FIFO
        // instead of being silently discarded.
        assert_eq!(gateway.operational_status().delivery.pending_events, 1);
    }

    /// The body of a captured raw HTTP/1.1 request as parsed JSON.
    fn captured_request_json(request: &str) -> Value {
        let body = request
            .split("\r\n\r\n")
            .nth(1)
            .expect("a captured request always carries a body");
        serde_json::from_str(body).expect("the generation body is JSON")
    }

    #[tokio::test]
    async fn generation_carries_the_full_fleet_fingerprint_and_nothing_from_the_client() {
        let fixture = Fixture::new();
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
            http_response("application/json", generation),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        assert_eq!(gateway.preflight().await, 1);
        let response = gateway
            .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let _probe = requests.recv().unwrap();
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        let turn = turn.to_ascii_lowercase();
        // The exact reviewed 2.1.195 capture, header by header.
        assert!(turn.contains("user-agent: claude-cli/2.1.195 (external, sdk-cli)"));
        assert!(turn.contains("anthropic-version: 2023-06-01"));
        let beta_line = turn
            .lines()
            .find(|line| line.starts_with("anthropic-beta: "))
            .expect("anthropic-beta is sent");
        for beta in [
            "oauth-2025-04-20",
            "interleaved-thinking-2025-05-14",
            "thinking-token-count-2026-05-13",
            "context-management-2025-06-27",
            "prompt-caching-scope-2026-01-05",
            "claude-code-20250219",
            "advisor-tool-2026-03-01",
            "advanced-tool-use-2025-11-20",
            "extended-cache-ttl-2025-04-11",
            "cache-diagnosis-2026-04-07",
        ] {
            assert!(beta_line.contains(beta), "missing beta {beta}");
        }
        assert!(turn.contains("x-app: cli"));
        assert!(turn.contains("x-stainless-lang: js"));
        assert!(turn.contains("x-stainless-runtime: node"));
        assert!(turn.contains("x-stainless-runtime-version: v26.3.0"));
        assert!(turn.contains("x-stainless-package-version: 0.94.0"));
        assert!(turn.contains("x-stainless-os: linux"));
        assert!(turn.contains("x-stainless-arch: x64"));
        assert!(turn.contains("accept: application/json"));
        assert!(turn.contains("anthropic-dangerous-direct-browser-access: true"));
        // Client identity headers structurally never enter the gateway: the wire shows the
        // synthesized persona and nothing a foreign SDK could have sent (no python, no curl).
        assert!(!turn.contains("python"));
        assert!(!turn.contains("curl"));
    }

    #[tokio::test]
    async fn generation_body_carries_identity_first_and_a_per_profile_billing_block() {
        let fixture = Fixture::new();
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
            http_response("application/json", generation),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        assert_eq!(gateway.preflight().await, 1);
        let response = gateway
            .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let _probe = requests.recv().unwrap();
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        let body = captured_request_json(&turn);
        let system = body["system"].as_array().expect("system is an array");
        // Exactly the real client's order: billing header first, identity second, no
        // cache_control on either (a cache-controlled identity is not what Claude Code sends).
        assert_eq!(system.len(), 2);
        let billing = system[0]["text"].as_str().unwrap();
        assert!(
            billing.starts_with("x-anthropic-billing-header: cc_version=2.1.195.d"),
            "billing block: {billing}"
        );
        assert!(billing.contains("; cc_entrypoint=sdk-cli; cch="));
        assert!(billing.ends_with(';'));
        assert_eq!(system.len(), 2, "billing and identity only");
        // Deterministic per profile: the fixture profile is "glm-01", so the gateway must emit
        // exactly the derivation the persona function produces for that id.
        let expected = super::super::transport::GlmIdentityHeaders::default()
            .billing_header_for("glm-01");
        assert_eq!(billing, expected);
        assert_eq!(
            system[1]["text"].as_str().unwrap(),
            "You are a Claude agent, built on Anthropic's Claude Agent SDK."
        );
        assert!(system[0].get("cache_control").is_none());
        assert!(system[1].get("cache_control").is_none());
    }

    #[tokio::test]
    async fn a_genuine_claude_code_body_is_never_doubled() {
        let fixture = Fixture::new();
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
            http_response("application/json", generation),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        assert_eq!(gateway.preflight().await, 1);
        // The customer IS Claude Code: its own identity block is already first. The gateway
        // must not stack a second identity; the per-profile billing block still goes first
        // (replacing nothing here — the client's billing marker would be replaced in place).
        let mut body = request_body("glm-5.2");
        body["system"] = json!([{
            "type": "text",
            "text": "You are a Claude agent, built on Anthropic's Claude Agent SDK."
        }]);
        let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let _probe = requests.recv().unwrap();
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        let body = captured_request_json(&turn);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 2, "no stacked identity: {system:?}");
        assert!(system[0]["text"]
            .as_str()
            .unwrap()
            .starts_with("x-anthropic-billing-header:"));
        assert_eq!(
            system[1]["text"].as_str().unwrap(),
            "You are a Claude agent, built on Anthropic's Claude Agent SDK."
        );
    }

    #[tokio::test]
    async fn billing_injection_can_be_switched_off_without_losing_the_identity() {
        let fixture = Fixture::new();
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(120, 12_000, 0, 60_000)),
            http_response("application/json", generation),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let mut config = config(&fixture.root, &base_url);
        config.transport.identity.inject_billing = false;
        let gateway = Arc::new(GlmGateway::new_with_calibration(config, None).unwrap());
        assert_eq!(gateway.preflight().await, 1);
        let response = gateway
            .handle(glm_request("glm-5.2", request_body("glm-5.2"), None))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let _probe = requests.recv().unwrap();
        let turn = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(!turn.contains("x-anthropic-billing-header"));
        let body = captured_request_json(&turn);
        let system = body["system"].as_array().unwrap();
        assert_eq!(system.len(), 1);
        assert_eq!(
            system[0]["text"].as_str().unwrap(),
            "You are a Claude agent, built on Anthropic's Claude Agent SDK."
        );
    }

    #[tokio::test]
    async fn the_quota_probe_carries_no_identity_set() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        )]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);

        let probe = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(probe.starts_with("GET /api/monitor/usage/quota/limit "));
        // The quota endpoint is a monitor surface, not generation: none of the Claude Code
        // fingerprint may ride on it.
        let probe = probe.to_ascii_lowercase();
        for absent in [
            "anthropic-version",
            "anthropic-beta",
            "x-stainless",
            "x-app",
            "x-anthropic-billing-header",
            "anthropic-dangerous-direct-browser-access",
            "claude-cli",
        ] {
            assert!(!probe.contains(absent), "probe must not carry {absent}");
        }
    }

    #[tokio::test]
    async fn a_rejected_key_quarantines_its_profile_without_taking_down_the_fleet() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
            http_response(
                "application/json",
                br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#,
            ),
        ]);
        fixture.publish_profiles(
            &[("glm-01", "zai-key-good"), ("glm-02", "zai-key-dead")],
            &base_url,
        );
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();

        // One dead key quarantines exactly its own profile (manifest §4.2): the business code
        // decides, and the rest of the fleet — let alone the whole gateway — stays up.
        assert_eq!(gateway.preflight().await, 1);
        assert_eq!(gateway.readiness(), Ok(()));
        let status = gateway.operational_status();
        assert_eq!(status.live_profiles, 1);
        assert_eq!(status.account_dead_profiles, 1);
        let dead = status
            .profiles
            .iter()
            .find(|profile| profile.id == "glm-02")
            .unwrap();
        assert!(dead.account_dead);
        assert!(!dead.live);
        assert!(status
            .profiles
            .iter()
            .any(|profile| profile.id == "glm-01" && profile.live));
        // Exactly one probe per profile: a static key is never retried in place.
        assert!(requests.recv().is_ok());
        assert!(requests.recv().is_ok());
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_cold_degraded_gateway_adopts_a_new_profile_only_after_a_quota_probe() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        )]);
        let gateway = GlmGateway::new_degraded(config(&fixture.root, &base_url), None);
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

        fixture.publish_profile("zai-key-1", &base_url);
        assert!(gateway.refresh_profiles().await);
        assert_eq!(gateway.readiness(), Ok(()));
        assert_eq!(gateway.operational_status().total_profiles, 1);
        let request = String::from_utf8(requests.recv().unwrap()).unwrap();
        assert!(request.starts_with("GET /api/monitor/usage/quota/limit "));
        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: zai-key-1"));
    }

    #[tokio::test]
    async fn an_unchanged_roster_reuses_the_exact_profile_without_another_probe() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        )]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));
        assert!(requests
            .recv()
            .unwrap()
            .starts_with(b"GET /api/monitor/usage/quota/limit "));
        assert!(requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn broken_or_disappeared_rosters_retain_the_last_good_ready_profile() {
        let fixture = Fixture::new();
        let (base_url, _requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        )]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
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
        let (base_url, _requests) = mock_server(vec![http_response(
            "application/json",
            &quota_body(0, 12_000, 0, 60_000),
        )]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
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
    async fn a_changed_credential_is_published_only_after_a_successful_quota_probe() {
        let fixture = Fixture::new();
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
            http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        fixture.publish_profile("zai-key-2", &base_url);
        assert!(gateway.refresh_profiles().await);
        let replacement = gateway.profiles_snapshot()[0].clone();
        assert!(!Arc::ptr_eq(&original, &replacement));
        assert_eq!(gateway.readiness(), Ok(()));

        let first = String::from_utf8(requests.recv().unwrap()).unwrap();
        let second = String::from_utf8(requests.recv().unwrap()).unwrap();
        // Replacement arrives as an atomic republication with a new static key; the probe
        // authenticates with the raw key, never a Bearer prefix.
        assert!(first
            .to_ascii_lowercase()
            .contains("authorization: zai-key-1"));
        assert!(!first.to_ascii_lowercase().contains("bearer"));
        assert!(second
            .to_ascii_lowercase()
            .contains("authorization: zai-key-2"));
    }

    #[tokio::test]
    async fn a_failed_probe_for_a_changed_credential_keeps_the_old_ready_snapshot() {
        let fixture = Fixture::new();
        let rejected = http_response(
            "application/json",
            br#"{"code":401,"msg":"invalid api key","success":false,"data":null}"#,
        );
        let (base_url, requests) = mock_server(vec![
            http_response("application/json", &quota_body(0, 12_000, 0, 60_000)),
            rejected,
        ]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway =
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap();
        assert_eq!(gateway.preflight().await, 1);
        let original = gateway.profiles_snapshot()[0].clone();

        fixture.publish_profile("zai-key-rejected", &base_url);
        assert!(!gateway.refresh_profiles().await);
        assert!(Arc::ptr_eq(&original, &gateway.profiles_snapshot()[0]));
        assert_eq!(gateway.readiness(), Ok(()));
        for expected in ["zai-key-1", "zai-key-rejected"] {
            let request = String::from_utf8(requests.recv().unwrap()).unwrap();
            assert!(request
                .to_ascii_lowercase()
                .contains(&format!("authorization: {expected}")));
        }
    }

    #[tokio::test]
    async fn final_verification_never_publishes_a_credential_rotated_during_probe() {
        let fixture = Fixture::new();
        let (base_url, mut requests, responses) = controlled_mock_server(2);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        let original = gateway.profiles_snapshot()[0].clone();
        original.mark_probe_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        fixture.publish_profile("candidate-secret", &base_url);
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
            .contains("authorization: candidate-secret"));

        // Simulate the other blue-green generation atomically republishing the shared roster
        // after this generation loaded it but before its candidate probe completed.
        fixture.publish_profile("peer-rotated-secret", &base_url);
        responses
            .send(http_response("application/json", &quota_body(0, 12_000, 0, 60_000)))
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
            .contains("authorization: peer-rotated-secret"));
        responses
            .send(http_response("application/json", &quota_body(0, 12_000, 0, 60_000)))
            .unwrap();

        assert!(reload.await.unwrap());
        let published = gateway.profiles_snapshot()[0].clone();
        assert_eq!(published.credential.api_key, "peer-rotated-secret");
        assert!(!Arc::ptr_eq(&original, &published));
        assert_eq!(gateway.readiness(), Ok(()));
    }

    #[tokio::test]
    async fn degraded_gateway_keeps_exact_aliases_on_a_zero_capacity_glm_path() {
        let fixture = Fixture::new();
        let gateway = Arc::new(GlmGateway::new_degraded(
            config(&fixture.root, "https://example.invalid"),
            None,
        ));
        assert_eq!(gateway.readiness(), Err(NotReady::NoLiveProfile));

        let body = request_body("glm-5.2");
        let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
        assert_eq!(
            response
                .extensions()
                .get::<crate::proxy::TerminalErrorReason>()
                .map(|reason| reason.0),
            Some("glm_capacity_exhausted")
        );
        let body = axum::body::to_bytes(response.into_body(), RESPONSE_BODY_LIMIT)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(value["error"]["type"], "rate_limit_error");
        let lowered = String::from_utf8_lossy(&body).to_ascii_lowercase();
        for private in ["glm", "zhipu", "z.ai"] {
            assert!(!lowered.contains(private), "leaked {private}: {lowered}");
        }
    }

    #[tokio::test]
    async fn every_synthetic_failure_uses_the_shared_anthropic_sanitizer() {
        for failure in [
            GatewayFailure::Auth,
            GatewayFailure::Suspect,
            GatewayFailure::Transport,
            GatewayFailure::Protocol,
            GatewayFailure::Capacity,
            GatewayFailure::LowBalance,
            GatewayFailure::BadRequest("glm_private_request_reason"),
            GatewayFailure::Unsupported("glm_private_capability_reason"),
            GatewayFailure::Unavailable("glm_private_runtime_reason"),
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
            for private in [
                "glm", "zhipu", "z.ai", "subscription", "roster", "upstream", "provider",
            ] {
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
            .push(br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}"#)
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
        assert_eq!(accounting.served_model.as_deref(), Some("glm-5.2"));
    }

    #[tokio::test]
    async fn reserve_deliver_settle_charges_the_exact_served_model_cost() {
        let fixture = Fixture::new();
        // The provider answers on glm-4.7 while glm-5.2 was requested: billing follows the
        // SERVED model's rate card (manifest §3), not the requested alias.
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-4.7","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, _requests) = mock_server(vec![http_response("application/json", generation)]);
        fixture.publish_profile("zai-key-1", &base_url);
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        billing.create_account("acct", None, 10_000).await.unwrap();
        billing
            .topup("acct", 20_000_000, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key("sk-glm-test", "acct", None, None, None)
            .await
            .unwrap();
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(
                config(&fixture.root, &base_url),
                Some(billing.clone()),
            )
            .unwrap(),
        );

        let body = request_body("glm-5.2");
        let billing_input = GlmBillingInput {
            account_id: "acct".into(),
            key: "sk-glm-test".into(),
            mult_bp: 10_000,
            available_nano: 20_000_000,
            strict_policy: false,
        };
        let response = gateway
            .handle(glm_request("glm-5.2", body, Some(billing_input)))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        // glm-4.7 rates: 7 input × 600 + 2 output × 2_200 = 8_600 nanoUSD exactly.
        let account = billing.account("acct").await.unwrap().unwrap();
        assert_eq!(
            (account.balance_nano, account.reserved_nano),
            (20_000_000 - 8_600, 0)
        );

        // The immutable turn event prices the served model on both ledgers.
        let status = gateway.operational_status();
        assert_eq!(status.delivery.pending_events, 1);
        let queue = gateway.turn_queue.lock().unwrap();
        let event = queue.head().unwrap();
        assert_eq!(event.requested_model, "glm-5.2");
        assert_eq!(event.served_model, "glm-4.7");
        assert_eq!(event.plan, "Pro");
        assert_eq!(event.api_fresh_input_nanousd, 7 * 600);
        assert_eq!(event.api_output_nanousd, 2 * 2_200);
        assert_eq!(event.api_total_nanousd, 8_600);
        // Native credits: 7 × 4.6 + 2 × 16 = 48.2 weighted-tenths → 6_420 microcredits at peak,
        // exactly half of it off-peak. Both schedule ids ride along.
        let expected_native = if event.off_peak { 3_210 } else { 6_420 };
        assert_eq!(event.native_total_microcredits, expected_native);
        assert_eq!(
            event.native_fresh_input_microcredits + event.native_cached_input_microcredits
                + event.native_output_microcredits,
            event.native_total_microcredits
        );
        assert_eq!(event.api_tariff_schedule_id, GLM_TARIFF_SCHEDULE_ID);
        assert_eq!(event.credit_schedule_id, GLM_CREDIT_SCHEDULE_ID);
        drop(queue);
        assert_eq!(status.missing_terminal_usage, 0);
        assert_eq!(status.served_model_rejected, 0);
    }

    #[tokio::test]
    async fn an_out_of_set_served_model_fails_billing_closed_on_the_conservative_hold() {
        let fixture = Fixture::new();
        // glm-5 is priceable on the dollar card but has NO published credit multipliers — an
        // echoed id the provider silently re-routes. The native ledger cannot price it, so the
        // whole turn fails closed (manifest §3): conservative hold, typed counter, no event.
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5","content":[{"type":"text","text":"ok"}],"usage":{"input_tokens":7,"output_tokens":2}}"#;
        let (base_url, _requests) = mock_server(vec![http_response("application/json", generation)]);
        fixture.publish_profile("zai-key-1", &base_url);
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        billing.create_account("acct", None, 10_000).await.unwrap();
        billing
            .topup("acct", 20_000_000, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key("sk-glm-test", "acct", None, None, None)
            .await
            .unwrap();
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(
                config(&fixture.root, &base_url),
                Some(billing.clone()),
            )
            .unwrap(),
        );

        let body = request_body("glm-5.2");
        let raw_len = serde_json::to_vec(&body).unwrap().len() as i64;
        let response = gateway
            .handle(glm_request(
                "glm-5.2",
                body,
                Some(GlmBillingInput {
                    account_id: "acct".into(),
                    key: "sk-glm-test".into(),
                    mult_bp: 10_000,
                    available_nano: 20_000_000,
                    strict_policy: false,
                }),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        // The documented conservative policy keeps the full hold; nothing is invented.
        let hold = raw_len * 1_400 + 8 * 4_400;
        let account = billing.account("acct").await.unwrap().unwrap();
        assert_eq!(
            (account.balance_nano, account.reserved_nano),
            (20_000_000 - hold, 0)
        );
        let status = gateway.operational_status();
        assert_eq!(status.served_model_rejected, 1);
        assert_eq!(status.missing_terminal_usage, 0);
        assert_eq!(status.delivery.pending_events, 0);
    }

    #[tokio::test]
    async fn missing_terminal_usage_settles_the_conservative_hold_without_an_event() {
        let fixture = Fixture::new();
        // A parseable 200 without a usage object is not authoritative terminal evidence.
        // Synthetic usage is never created: the reservation settles on the documented hold.
        let generation = br#"{"id":"msg_1","type":"message","model":"glm-5.2","content":[]}"#;
        let (base_url, _requests) = mock_server(vec![http_response("application/json", generation)]);
        fixture.publish_profile("zai-key-1", &base_url);
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        billing.create_account("acct", None, 10_000).await.unwrap();
        billing
            .topup("acct", 20_000_000, Some("seed"))
            .await
            .unwrap();
        billing
            .issue_key("sk-glm-test", "acct", None, None, None)
            .await
            .unwrap();
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(
                config(&fixture.root, &base_url),
                Some(billing.clone()),
            )
            .unwrap(),
        );

        let body = request_body("glm-5.2");
        let raw_len = serde_json::to_vec(&body).unwrap().len() as i64;
        let response = gateway
            .handle(glm_request(
                "glm-5.2",
                body,
                Some(GlmBillingInput {
                    account_id: "acct".into(),
                    key: "sk-glm-test".into(),
                    mult_bp: 10_000,
                    available_nano: 20_000_000,
                    strict_policy: false,
                }),
            ))
            .await;
        assert_eq!(response.status(), StatusCode::OK);

        let hold = raw_len * 1_400 + 8 * 4_400;
        let account = billing.account("acct").await.unwrap().unwrap();
        assert_eq!(
            (account.balance_nano, account.reserved_nano),
            (20_000_000 - hold, 0)
        );
        let status = gateway.operational_status();
        assert_eq!(status.missing_terminal_usage, 1);
        assert_eq!(status.served_model_rejected, 0);
        assert_eq!(status.delivery.pending_events, 0);
    }

    #[tokio::test]
    async fn the_turn_fifo_holds_order_and_a_transient_head_blocks_the_tail() {
        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
        let sqlite = fixture.root.join("billing.sqlite");
        let billing =
            Arc::new(AsyncBilling::start(sqlite.to_string_lossy().into_owned(), 1).unwrap());
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:1"),
            Some(billing),
        )
        .unwrap();
        // Readiness needs a live profile first; only then can degraded delivery block it.
        gateway.profiles_snapshot()[0].mark_probe_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        // SQLite refuses GLM calibration, so every write is transient: the head must stay in
        // place and hold the tail — a later turn may never overtake an undelivered one.
        gateway.enqueue_turn(turn_event("glm-fifo-a")).await;
        gateway.enqueue_turn(turn_event("glm-fifo-b")).await;
        let queue = gateway.turn_queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.head().unwrap().request_id, "glm-fifo-a");
        drop(queue);
        let delivery = gateway.operational_status().delivery;
        assert_eq!(delivery.pending_events, 2);
        assert!(!delivery.persistence_ok);
        assert_eq!(gateway.readiness(), Err(NotReady::DeliveryDegraded));
    }

    #[tokio::test]
    async fn a_stream_passthrough_is_incremental_and_never_replays_after_the_first_byte() {
        let fixture = Fixture::new();
        let (base_url, requests, gate) = gated_sse_server();
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        let profile = gateway.profiles_snapshot()[0].clone();
        profile.mark_probe_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        let mut body = request_body("glm-5.2");
        body["stream"] = json!(true);
        let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let mut stream = response.into_body().into_data_stream();
        let first = futures_util::StreamExt::next(&mut stream)
            .await
            .unwrap()
            .unwrap();
        // The first public byte arrived while the upstream still held the rest of the stream:
        // passthrough is genuinely incremental, not a buffered frame.
        assert!(String::from_utf8_lossy(&first).contains("message_start"));
        gate.send(()).unwrap();

        let mut rest = Vec::new();
        while let Some(chunk) = futures_util::StreamExt::next(&mut stream).await {
            rest.extend_from_slice(&chunk.unwrap());
        }
        let rest = String::from_utf8_lossy(&rest);
        assert!(rest.contains("message_delta"));
        assert!(rest.contains("message_stop"));

        // One upstream request, no replay: retry after the first public byte is forbidden.
        assert!(requests.recv().is_ok());
        assert!(requests.try_recv().is_err());
        wait_for_pending(&gateway, 1).await;
        let queue = gateway.turn_queue.lock().unwrap();
        let event = queue.head().unwrap();
        assert_eq!(event.served_model, "glm-5.2");
        assert_eq!(event.fresh_input_tokens, 10);
        assert_eq!(event.output_tokens, 4);
    }

    #[tokio::test]
    async fn a_client_disconnect_still_drains_upstream_to_terminal_usage() {
        let fixture = Fixture::new();
        let sse = br#"data: {"type":"message_start","message":{"model":"glm-5.2","usage":{"input_tokens":10}}}

data: {"type":"message_delta","usage":{"output_tokens":4}}

data: {"type":"message_stop"}

"#
        .as_slice();
        let (base_url, _requests) = mock_server(vec![http_response("text/event-stream", sse)]);
        fixture.publish_profile("zai-key-1", &base_url);
        let gateway = Arc::new(
            GlmGateway::new_with_calibration(config(&fixture.root, &base_url), None).unwrap(),
        );
        let profile = gateway.profiles_snapshot()[0].clone();
        profile.mark_probe_healthy();
        gateway.live_profiles.store(1, Ordering::Release);

        let mut body = request_body("glm-5.2");
        body["stream"] = json!(true);
        let response = gateway.handle(glm_request("glm-5.2", body, None)).await;
        assert_eq!(response.status(), StatusCode::OK);

        let mut stream = response.into_body().into_data_stream();
        let first = futures_util::StreamExt::next(&mut stream)
            .await
            .unwrap()
            .unwrap();
        assert!(String::from_utf8_lossy(&first).contains("message_start"));
        // The client goes away mid-stream. Delivery stops, but the bounded task keeps draining
        // the provider to terminal usage so settlement stays exact.
        drop(stream);

        wait_for_pending(&gateway, 1).await;
        let queue = gateway.turn_queue.lock().unwrap();
        let event = queue.head().unwrap();
        assert_eq!(event.served_model, "glm-5.2");
        assert_eq!(event.fresh_input_tokens, 10);
        assert_eq!(event.output_tokens, 4);
        assert!(event.native_total_microcredits > 0);
    }

    #[test]
    fn reservation_cap_is_an_integer_upper_bound_and_never_crosses_the_overdraft_floor() {
        let prices = glm_prices_for_served_model("glm-5.2", 1).unwrap();
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
        // Tools, web search and MCP surfaces are `unavailable` in v1 (manifest §3/§5.1): their
        // per-request ceiling is unproven, so the plane must not spend budget on them.
        assert!(matches!(
            validate_priced_surface(&json!({"tools": [{"name": "search"}]})),
            Err(GatewayFailure::Unsupported("glm_tools_unavailable"))
        ));
        for body in [
            json!({"tools": "provider-default"}),
            json!({"tool_choice": {"type": "auto"}}),
            json!({"mcp_servers": [{"name": "zread"}]}),
            json!({"messages": [{"content": [{"type": "tool_result"}]}]}),
            json!({"messages": [{"content": [{"type": "web_search_tool_result"}]}]}),
        ] {
            assert!(matches!(
                validate_priced_surface(&body),
                Err(GatewayFailure::Unsupported("glm_tools_unavailable"))
            ));
        }
        // Vision is outside the reviewed trio and fails closed the same way.
        assert!(matches!(
            validate_priced_surface(&json!({
                "messages": [{"content": [{"type": "image", "source": {}}]}]
            })),
            Err(GatewayFailure::Unsupported("glm_media_unavailable"))
        ));
    }

    #[test]
    fn reasoning_effort_maps_only_for_glm_5_2_and_follows_the_provider_mapping() {
        assert_eq!(reasoning_effort("glm-5.2", &json!({})).unwrap(), Some("high".into()));
        assert_eq!(
            reasoning_effort("glm-5.2[1m]", &json!({})).unwrap(),
            Some("high".into())
        );
        for (raw, mapped) in [
            ("xhigh", "max"),
            ("max", "max"),
            ("medium", "high"),
            ("low", "high"),
            ("minimal", "off"),
            ("none", "off"),
        ] {
            assert_eq!(
                reasoning_effort("glm-5.2", &json!({"reasoning_effort": raw})).unwrap(),
                Some(mapped.into()),
                "{raw}"
            );
        }
        assert_eq!(
            reasoning_effort("glm-5.2", &json!({"thinking": {"type": "disabled"}})).unwrap(),
            Some("off".into())
        );
        assert!(reasoning_effort("glm-5.2", &json!({"reasoning_effort": "invented"})).is_err());
        // Models without a reasoning effort carry none at all (manifest §3).
        assert_eq!(reasoning_effort("glm-4.7", &json!({})).unwrap(), None);
        assert_eq!(reasoning_effort("glm-5-turbo", &json!({})).unwrap(), None);
    }

    #[test]
    fn context_mode_marks_only_the_1m_selector_spelling() {
        assert_eq!(context_mode("glm-5.2[1m]"), "1m");
        assert_eq!(context_mode("glm-5.2"), "200k");
        assert_eq!(context_mode("glm-5-turbo"), "200k");
        assert_eq!(context_mode("glm-4.7"), "200k");
    }

    #[test]
    fn model_is_glm_accepts_only_the_exact_reviewed_aliases() {
        for alias in ["glm-5.2", "glm-5.2[1m]", "glm-5-turbo", "glm-4.7", "GLM-5.2"] {
            assert!(GlmGateway::model_is_glm(alias), "{alias}");
        }
        // Historical/echoed ids are NOT admission aliases: they never dispatch here.
        for other in [
            "glm-5",
            "glm-5.1",
            "glm-4.5",
            "glm-4.6v",
            "glm-5.2-highspeed",
            "claude-sonnet-4-6",
            "kimi-for-coding",
            "",
        ] {
            assert!(!GlmGateway::model_is_glm(other), "{other}");
        }
    }

    #[test]
    fn price_turn_computes_both_ledgers_from_the_served_model() {
        // Monday 15:00 SGT is peak; Monday 12:00 SGT is off-peak (official schedule, UTC+8).
        const PEAK: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
        const OFF_PEAK: i64 = 4 * 86_400 + 12 * 3_600 - 8 * 3_600;
        let usage = GlmUsage {
            input_tokens: 1_000,
            cache_read_tokens: 500,
            cache_write_tokens: 50,
            output_tokens: 100,
            reasoning_output_tokens: 40,
        };
        let peak = price_turn(&usage, "glm-5.2", 1, PEAK).unwrap();
        assert!(!peak.off_peak);
        // API ledger (glm-5.2: 260/1_400/4_400 nanoUSD): the cache write carries the miss rate.
        assert_eq!(peak.input, 1_000 * 1_400);
        assert_eq!(peak.cache_read, 500 * 260);
        assert_eq!(peak.cache_write, 50 * 1_400);
        assert_eq!(peak.output, 100 * 4_400);
        assert_eq!(peak.total, 2_040_000);
        // Native ledger (6.9/1.7/24 tenths): exact, no rounding, no cache-write leg.
        assert_eq!(peak.native_input, 1_000 * 69 * 10);
        assert_eq!(peak.native_cache_read, 500 * 17 * 10);
        assert_eq!(peak.native_output, 100 * 240 * 10);
        assert_eq!(peak.native_total, 1_015_000);

        let off_peak = price_turn(&usage, "glm-5.2", 1, OFF_PEAK).unwrap();
        assert!(off_peak.off_peak);
        // Off-peak is exactly half on the native ledger; the dollar ledger never changes.
        assert_eq!(off_peak.total, peak.total);
        assert_eq!(off_peak.native_total, peak.native_total / 2);

        // The immutable event folds cache-write cost into the fresh-input leg so the three
        // disjoint legs still sum to the total.
        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:1");
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:1"),
            None,
        )
        .unwrap();
        let context = AccountingContext {
            request_id: "req-1".into(),
            requested_model: "glm-5.2".into(),
            context_mode: "200k".into(),
            reasoning_effort: Some("high".into()),
            priced_ts: 1,
            profile: gateway.profiles_snapshot()[0].clone(),
        };
        let event = peak
            .calibration_event(&context, &usage, "glm-5.2", PEAK)
            .unwrap();
        assert_eq!(event.api_fresh_input_nanousd, 1_470_000);
        assert_eq!(event.api_total_nanousd, 2_040_000);
        assert_eq!(event.native_total_microcredits, 1_015_000);
        assert!(!event.off_peak);

        // Fail closed: an echoed id without credit multipliers, an unknown id, a broken subset
        // invariant and an empty usage vector are never priced.
        assert!(price_turn(&usage, "glm-5", 1, PEAK).is_err());
        assert!(price_turn(&usage, "glm-9", 1, PEAK).is_err());
        let mut broken = usage.clone();
        broken.reasoning_output_tokens = broken.output_tokens + 1;
        assert!(price_turn(&broken, "glm-5.2", 1, PEAK).is_err());
        assert!(price_turn(&GlmUsage::default(), "glm-5.2", 1, PEAK).is_err());
    }

    #[tokio::test]
    async fn the_status_projection_reports_cooling_axes_availability_inflight_and_counters() {
        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:9"),
            None,
        )
        .unwrap();
        let now = now_unix();
        let profile = gateway.profiles_snapshot()[0].clone();

        let healthy = gateway.operational_status();
        assert_eq!(healthy.total_profiles, 1);
        assert_eq!(healthy.available_profiles, 1);
        assert_eq!(healthy.account_dead_profiles, 0);
        assert_eq!(healthy.account_suspect_profiles, 0);
        assert_eq!(healthy.transport_cooling_profiles, 0);
        assert_eq!(healthy.quota_cooling_profiles, 0);
        assert_eq!(healthy.inflight_requests, 0);
        assert_eq!(healthy.missing_terminal_usage, 0);
        assert_eq!(healthy.served_model_rejected, 0);
        assert_eq!(healthy.profiles[0].id, "glm-01");
        assert_eq!(healthy.profiles[0].plan, "Pro");
        assert!(!healthy.profiles[0].account_dead);
        assert!(!healthy.profiles[0].account_suspect);
        assert_eq!(healthy.profiles[0].transport_cool_until, None);
        assert_eq!(healthy.profiles[0].quota_cool_until, None);
        assert_eq!(healthy.profiles[0].quota_observed_at, None);
        assert_eq!(healthy.profiles[0].quota_windows, Vec::new());
        assert!(!healthy.profiles[0].live);

        profile.inflight.store(3, Ordering::Release);
        profile.apply_effect(ProfileEffect::AccountDead, now, None, None);
        let dead = gateway.operational_status();
        assert_eq!(dead.available_profiles, 0);
        assert_eq!(dead.account_dead_profiles, 1);
        assert_eq!(dead.inflight_requests, 3);
        assert!(dead.profiles[0].account_dead);
        assert!(!dead.profiles[0].live);

        profile.apply_effect(ProfileEffect::TransportFault, now, None, None);
        let wedged = gateway.operational_status();
        assert_eq!(wedged.transport_cooling_profiles, 1);
        assert_eq!(
            wedged.profiles[0].transport_cool_until,
            Some(now + TRANSPORT_COOL_SECS)
        );

        // Model scope stays model-scoped: 1311 on one alias blocks nothing else.
        profile.mark_probe_healthy();
        profile.apply_effect(ProfileEffect::ModelIneligible, now, Some("glm-5.2"), None);
        assert_eq!(
            profile.candidate("glm-5.2", now).ineligible,
            Some(Ineligible::ModelIneligible)
        );
        assert_eq!(profile.candidate("glm-4.7", now).ineligible, None);
        // A successful generation rehabilitates exactly the served model's block.
        profile.mark_healthy("glm-5.2");
        assert_eq!(profile.candidate("glm-5.2", now).ineligible, None);
        assert!(profile.authenticated());

        // A quota wall with provider reset evidence cools to the exact reset, never a guess.
        profile.apply_effect(ProfileEffect::CoolUntilReset, now, None, Some(now + 3_600));
        assert_eq!(
            profile.health.lock().unwrap().quota_cool_until,
            now + 3_600
        );
        // …and without one it falls back to the bounded cool the next poll will refine.
        profile.apply_effect(ProfileEffect::CoolUntilReset, now, None, None);
        assert_eq!(
            profile.health.lock().unwrap().quota_cool_until,
            now + QUOTA_WALL_FALLBACK_COOL_SECS
        );
        // A probe alone proves auth, not quota state: the wall axis survives it and clears
        // only on a real quota snapshot or at its deadline.
        profile.mark_probe_healthy();
        assert_eq!(
            profile.candidate("glm-4.7", now).ineligible,
            Some(Ineligible::QuotaWall)
        );

        // Typed operational counters surface without any private identity.
        gateway.missing_terminal_usage.store(2, Ordering::Release);
        gateway.served_model_rejected.store(1, Ordering::Release);
        let counted = gateway.operational_status();
        assert_eq!(counted.missing_terminal_usage, 2);
        assert_eq!(counted.served_model_rejected, 1);
    }

    #[tokio::test]
    async fn publish_quota_retains_the_exact_per_window_snapshot_with_unknowns_absent() {
        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:9"),
            None,
        )
        .unwrap();
        let profile = gateway.profiles_snapshot()[0].clone();
        let observed_at = now_unix();
        let five_hour = quota_snapshot(18_000, 250, 1_000, 4_102_444_800, observed_at);
        // A window whose units are unproven keeps every raw field absent — never zero.
        let unproven = GlmQuotaSnapshot {
            window_duration_secs: 604_800,
            resets_at: None,
            observed_at,
            native_used_units: None,
            native_limit_units: None,
            native_remaining_units: None,
            percentage_raw: None,
            used_fraction_units: None,
            measurement_resolution_fraction_units: None,
        };
        profile.publish_quota(&[five_hour, unproven], observed_at);

        let status = gateway.operational_status();
        let projected = &status.profiles[0];
        assert_eq!(projected.quota_observed_at, Some(observed_at));
        assert_eq!(projected.quota_windows.len(), 2);
        let window = &projected.quota_windows[0];
        assert_eq!(window.duration_secs, 18_000);
        assert_eq!(window.used_units, Some(250));
        assert_eq!(window.limit_units, Some(1_000));
        assert_eq!(window.remaining_units, Some(750));
        assert_eq!(window.used_fraction_units, Some(25_000_000));
        assert_eq!(window.measurement_resolution_fraction_units, Some(100_000));
        assert_eq!(window.resets_at, Some(4_102_444_800));
        assert_eq!(window.observed_at, observed_at);
        let unknown = &projected.quota_windows[1];
        assert_eq!(unknown.used_units, None);
        assert_eq!(unknown.used_fraction_units, None);
        assert_eq!(unknown.resets_at, None);
        // No window is full: no quota cooling, and a successful poll authenticates the profile.
        assert_eq!(status.quota_cooling_profiles, 0);
        assert!(projected.live);
    }

    #[tokio::test]
    async fn the_projection_bounds_plan_labels_and_cannot_carry_the_subject() {
        assert_eq!(bounded_plan_label("lite"), "Lite");
        assert_eq!(bounded_plan_label("pro"), "Pro");
        assert_eq!(bounded_plan_label("max"), "Max");
        // The runtime stores the capitalized declared form; the label mapping accepts both.
        assert_eq!(bounded_plan_label("Pro"), "Pro");
        assert_eq!(bounded_plan_label("Max"), "Max");
        assert_eq!(bounded_plan_label("enterprise"), "unreviewed");

        let fixture = Fixture::new();
        fixture.publish_profile("zai-key-1", "http://127.0.0.1:9");
        let gateway = GlmGateway::new_with_calibration(
            config(&fixture.root, "http://127.0.0.1:9"),
            None,
        )
        .unwrap();
        let status = gateway.operational_status();
        assert_eq!(status.profiles[0].plan, "Pro");
        // The durable-calibration join resolves only through the opaque roster id; an unknown
        // subject resolves to nothing and its rows are dropped rather than serialized.
        let profile = gateway.profiles_snapshot()[0].clone();
        assert_eq!(
            gateway
                .profile_id_for_subject(&profile.subject_id)
                .as_deref(),
            Some("glm-01")
        );
        assert_eq!(gateway.profile_id_for_subject("subject-unknown"), None);
        let rendered = format!("{status:?}");
        assert!(!rendered.contains(&profile.subject_id));
        assert!(!rendered.contains("zai-key-1"));
    }
}
