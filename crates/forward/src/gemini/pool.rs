//! Encrypted multi-account Gemini OAuth pool with single-flight refresh and bounded health probes.

use super::calibration::{self, WindowCalibration, FRACTION_SCALE};
use super::config::{GeminiConfig, GeminiProfileSpec, GeminiProfilesFile};
use super::rate_limit::RateLimitDiagnostic;
use super::transport::{attest_node_binary, ProfileTransport, TransportRequest, TransportResponse};
use crate::billing::AsyncBilling;
use crate::state::{ActiveTaskGuard, ActiveTaskTracker};
use anyhow::{bail, Context};
use futures_util::StreamExt;
use gemini_credential::{decode_envelope, GeminiCredential, OAuthKind, SecretString};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use zeroize::{Zeroize, ZeroizeOnDrop};

const HEALTH_PROBE_CONCURRENCY: usize = 16;
const ACCESS_TOKEN_SKEW_SECS: i64 = 120;
/// Match the Claude/Codex shared-root discipline: independent sessions with the same large
/// system/tools prefix seed two competitive profiles before warmth becomes the preferred hint.
const CACHE_ROOT_MIN_WARM_PROFILES: usize = 2;
/// Quota steering is intentionally dormant while a profile has at least half of its model window.
/// Exact fractions below this boundary would otherwise herd a burst onto the mathematically
/// emptiest subscription instead of spreading it across the available profile set.
const QUOTA_STEER_FLOOR_USED_PERCENT: i64 = 50;
const QUOTA_STEER_BUCKET_PERCENT: i64 = 10;

/// Operator-facing account hint for the protected control plane. The full Google identity remains
/// inside the sealed credential and never reaches status JSON, logs or metrics.
fn mask_gemini_email(email: &str) -> String {
    let local = email.split_once('@').map_or(email, |(local, _)| local);
    let head: String = local.chars().take(4).collect();
    format!("{head}…")
}

/// Coarse near-wall quota rank. Lower is preferred; zero deliberately keeps healthy profiles tied
/// so in-flight load and the rotating cursor can spread ordinary traffic.
fn quota_rank(remaining_fraction: Option<f64>) -> i64 {
    let used_percent = remaining_fraction
        .map(|remaining| ((1.0 - remaining.clamp(0.0, 1.0)) * 100.0).floor() as i64)
        .unwrap_or(0);
    used_percent
        .saturating_sub(QUOTA_STEER_FLOOR_USED_PERCENT)
        .max(0)
        / QUOTA_STEER_BUCKET_PERCENT
}

#[derive(Clone, Debug)]
pub struct GeminiProfileStatus {
    pub id: String,
    /// Privacy-safe operator hint: at most four characters from the email local-part.
    pub masked_email: String,
    /// Reviewed paid-plan identity from the sealed credential; contains no Google identity.
    pub plan: String,
    /// Immutable credential issue time and the plan-specific subscription horizon. Invalid source
    /// values stay absent instead of being exposed as sentinel timestamps.
    pub acquired_at: Option<i64>,
    pub subscription_expires_at: Option<i64>,
    pub subscription_days_left: Option<f64>,
    pub authenticated: bool,
    /// Operator pulled this profile out of rotation (`pool_member_disables`). Reported so the
    /// panel can show it and offer to put it back; a disabled profile is still listed, it just
    /// never routes and is never probed.
    pub disabled: bool,
    /// Operator also took the (already disabled) profile out of the board's default view. Purely
    /// presentational: it never affects routing, and the engine keeps reporting the row so the
    /// panel can reveal and restore it.
    pub hidden: bool,
    pub cooling_until: i64,
    pub inflight: usize,
    pub last_probe_at: i64,
    pub quota_updated_at: i64,
    pub quotas: Vec<GeminiQuotaBucketStatus>,
    pub model_cooling: Vec<GeminiModelCoolingStatus>,
    /// Cumulative official-price spend served by this opaque profile through the gateway.
    pub spend_usd_total: f64,
    pub calibration_persistence_ok: bool,
    pub capacities: Vec<GeminiWindowCapacityReport>,
}

#[derive(Clone, Debug)]
pub struct GeminiWindowCapacityReport {
    pub bucket_id: &'static str,
    pub window_kind: &'static str,
    pub window_minutes: i64,
    pub resets_at: i64,
    pub observed_at: i64,
    pub data_age_seconds: i64,
    pub remaining_fraction_units: i64,
    pub used_fraction_units: i64,
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
    pub observed_spend_nano: i64,
    pub observed_fraction_units: i64,
    pub source: &'static str,
    pub confidence: f64,
    pub samples: i64,
}

#[derive(Clone, Debug)]
pub struct GeminiQuotaBucketStatus {
    pub model_id: String,
    pub remaining_amount: Option<u64>,
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
    pub token_type: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GeminiModelCoolingStatus {
    pub model_id: String,
    pub cooling_until: i64,
    pub failure_streak: u32,
    pub last_success_at: i64,
    pub last_failure_at: i64,
    pub last_failure_class: Option<String>,
}

#[derive(Clone, Debug)]
pub struct GeminiOperationalStatus {
    pub profiles: Vec<GeminiProfileStatus>,
    pub models: Vec<GeminiModelStatus>,
    pub available: usize,
    pub authenticated: usize,
    pub soonest_ready: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct GeminiModelStatus {
    pub id: String,
    pub available: usize,
    pub healthy: usize,
    pub degraded: usize,
    pub unknown: usize,
    pub soonest_ready: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenError {
    /// Google отозвал refresh-токен (`400 invalid_grant`) — credential действительно мёртв.
    Invalid,
    /// Запрос отклонён окружением (401/403): репутация IP прокси, блок клиента. Сам grant цел,
    /// поэтому профиль нельзя брендировать auth-ошибкой — иначе живая оплаченная подписка уходит
    /// из ротации навсегда по причине, которой в токене нет.
    Blocked,
    Temporary,
}

struct LoadedProfile {
    source: GeminiProfileSpec,
    credential: GeminiCredential,
    fingerprint: [u8; 32],
}

pub(crate) struct GeminiProfile {
    source: GeminiProfileSpec,
    fingerprint: [u8; 32],
    id: String,
    masked_email: String,
    plan: String,
    /// Immutable metadata copied from the validated credential. Profile reload replaces this whole
    /// object when the sealed credential fingerprint changes.
    issued_at: i64,
    oauth_kind: OAuthKind,
    credential: tokio::sync::Mutex<GeminiCredential>,
    transport: ProfileTransport,
    google_api_client: String,
    refresh_user_agent: String,
    inflight: AtomicUsize,
    /// Soft, environment-derived cooling: auth 401/403, transport faults, blocked probes. These are
    /// our inference about the environment, never a statement by Google that capacity is gone, so
    /// this axis steers routing but is never allowed to be the reason a request finds no profile.
    cooling_until: AtomicI64,
    /// Hard, quota-derived cooling at profile scope: Google answered 429 on a quota-free call. Only
    /// this axis, the per-model axis and the official quota catalogue may deny a request outright.
    quota_cooling_until: AtomicI64,
    /// Consecutive environment-derived auth rejections, driving the exponential backoff below.
    auth_failure_streak: AtomicU32,
    authenticated: AtomicBool,
    last_probe_at: AtomicI64,
    quota: RwLock<GeminiQuotaSnapshot>,
    quota_summary: RwLock<GeminiQuotaSummarySnapshot>,
    model_health: Mutex<HashMap<String, GeminiModelHealthState>>,
    spend_nano_total: AtomicI64,
    calibration_persistence_ok: AtomicBool,
    billing: Option<Arc<AsyncBilling>>,
    calibrations: Mutex<BTreeMap<String, WindowCalibration>>,
    /// Silence allowance for customer generation, or `None` for no deadline — the production
    /// default. Kept here so the send path does not have to thread the config through.
    generation_idle: Option<Duration>,
    /// Silence allowance for token refresh, quota and catalogue calls. Short on purpose: a wedged
    /// auxiliary call must rotate the profile out quickly rather than stall behind a backstop
    /// sized for generation.
    auxiliary_idle: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GeminiRateLimitQuotaEvidence {
    snapshot_state: &'static str,
    age_secs: i64,
    matching_buckets: usize,
    zero_buckets: usize,
    positive_buckets: usize,
    unknown_buckets: usize,
    min_remaining_bp: Option<i64>,
    latest_reset_in_secs: Option<i64>,
}

impl std::fmt::Display for GeminiRateLimitQuotaEvidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "catalog_state={} catalog_age_secs={} catalog_matching_buckets={} catalog_zero_buckets={} catalog_positive_buckets={} catalog_unknown_buckets={} catalog_min_remaining_bp={} catalog_latest_reset_in_secs={}",
            self.snapshot_state,
            self.age_secs,
            self.matching_buckets,
            self.zero_buckets,
            self.positive_buckets,
            self.unknown_buckets,
            self.min_remaining_bp
                .map_or_else(|| "none".to_string(), |value| value.to_string()),
            self.latest_reset_in_secs
                .map_or_else(|| "none".to_string(), |value| value.to_string()),
        )
    }
}

#[derive(Clone, Default)]
struct GeminiModelHealthState {
    cooling_until: i64,
    failure_streak: u32,
    last_success_at: i64,
    last_failure_at: i64,
    last_failure_class: Option<&'static str>,
}

#[derive(Default)]
struct GeminiQuotaSnapshot {
    updated_at: i64,
    buckets: Vec<GeminiQuotaBucketStatus>,
}

#[derive(Clone)]
struct GeminiSummaryBucket {
    contract: calibration::BucketContract,
    remaining_fraction_units: i64,
    measurement_resolution_fraction_units: i64,
    resets_at: i64,
}

#[derive(Default)]
struct GeminiQuotaSummarySnapshot {
    updated_at: i64,
    buckets: Vec<GeminiSummaryBucket>,
}

fn emit_probe_rate_limit_diagnostic(
    profile_id: &str,
    oauth_kind: OAuthKind,
    applied_cool_secs: i64,
    headers: &axum::http::HeaderMap,
    body: &[u8],
) {
    let oauth_kind = match oauth_kind {
        OAuthKind::Antigravity => "antigravity",
        OAuthKind::LegacyGeminiCli => "legacy_gemini_cli",
    };
    let diagnostic = RateLimitDiagnostic::from_body(Some(headers), body);
    elog::warn(
        "gemini-rate-limit",
        format!(
            "gemini probe 429: operation=health_probe phase=load_code_assist profile={profile_id} oauth_kind={oauth_kind} {}",
            diagnostic.fields(applied_cool_secs),
        ),
    );
}

impl GeminiProfile {
    fn new(
        mut loaded: LoadedProfile,
        cfg: &GeminiConfig,
        billing: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        gemini_credential::validate_profile_id(&loaded.source.id)?;
        let oauth_kind = loaded.credential.oauth_kind()?;
        let masked_email = mask_gemini_email(&loaded.credential.email);
        let plan = loaded.credential.plan.clone();
        let issued_at = loaded.credential.issued_at;
        if loaded.credential.proxy.trim().is_empty() && cfg.upstream.starts_with("https://") {
            bail!("Gemini production profile requires a dedicated proxy");
        }
        // Keep the proxy secret in exactly one long-lived owner. The credential mutex only needs
        // OAuth/project fields after publication; helper respawns reuse the zeroizing copy.
        let proxy = SecretString::new(std::mem::take(&mut loaded.credential.proxy));
        let transport =
            ProfileTransport::new(cfg, proxy).context("build Gemini OAuth HTTP transport")?;
        Ok(Self {
            id: loaded.source.id.clone(),
            masked_email,
            plan,
            issued_at,
            source: loaded.source,
            fingerprint: loaded.fingerprint,
            oauth_kind,
            credential: tokio::sync::Mutex::new(loaded.credential),
            transport,
            google_api_client: cfg.google_api_client(),
            refresh_user_agent: cfg.refresh_user_agent(oauth_kind),
            inflight: AtomicUsize::new(0),
            cooling_until: AtomicI64::new(0),
            quota_cooling_until: AtomicI64::new(0),
            auth_failure_streak: AtomicU32::new(0),
            authenticated: AtomicBool::new(true),
            last_probe_at: AtomicI64::new(0),
            quota: RwLock::new(GeminiQuotaSnapshot::default()),
            quota_summary: RwLock::new(GeminiQuotaSummarySnapshot::default()),
            model_health: Mutex::new(HashMap::new()),
            spend_nano_total: AtomicI64::new(0),
            calibration_persistence_ok: AtomicBool::new(billing.is_some()),
            billing,
            calibrations: Mutex::new(BTreeMap::new()),
            generation_idle: (cfg.generation_idle_timeout_secs > 0)
                .then(|| Duration::from_secs(cfg.generation_idle_timeout_secs)),
            auxiliary_idle: Duration::from_secs(cfg.read_timeout_secs),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn oauth_kind(&self) -> OAuthKind {
        self.oauth_kind
    }

    /// Snapshot the already-sanitized model catalogue at the exact time a generation 429 arrives.
    /// This is read-only evidence: it cannot change quota classification, selection or cooling.
    pub(crate) fn rate_limit_quota_evidence(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
    ) -> GeminiRateLimitQuotaEvidence {
        let quota = self
            .quota
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let stale_secs = cfg
            .health_probe_interval_secs
            .saturating_mul(2)
            .clamp(60, 3_600) as i64;
        let snapshot_state = if quota.updated_at <= 0 {
            "missing"
        } else if quota.updated_at.saturating_add(stale_secs) <= now {
            "stale"
        } else {
            "fresh"
        };
        let matching = quota
            .buckets
            .iter()
            .filter(|bucket| {
                cfg.quota_model_id_matches_wire(self.oauth_kind, model_id, &bucket.model_id)
            })
            .collect::<Vec<_>>();
        let mut zero_buckets = 0usize;
        let mut positive_buckets = 0usize;
        let mut unknown_buckets = 0usize;
        for bucket in &matching {
            if bucket.remaining_amount == Some(0) || bucket.remaining_fraction == Some(0.0) {
                zero_buckets = zero_buckets.saturating_add(1);
            } else if bucket.remaining_amount.is_some_and(|value| value > 0)
                || bucket.remaining_fraction.is_some_and(|value| value > 0.0)
            {
                positive_buckets = positive_buckets.saturating_add(1);
            } else {
                unknown_buckets = unknown_buckets.saturating_add(1);
            }
        }
        let min_remaining_bp = matching
            .iter()
            .filter_map(|bucket| bucket.remaining_fraction)
            .filter(|fraction| fraction.is_finite())
            .map(|fraction| (fraction.clamp(0.0, 1.0) * 10_000.0).floor() as i64)
            .min();
        let latest_reset_in_secs = matching
            .iter()
            .filter_map(|bucket| bucket.reset_time.as_deref())
            .filter_map(parse_rfc3339_seconds)
            .map(|reset| reset.saturating_sub(now).max(0))
            .max();
        GeminiRateLimitQuotaEvidence {
            snapshot_state,
            age_secs: if quota.updated_at > 0 {
                now.saturating_sub(quota.updated_at).max(0)
            } else {
                -1
            },
            matching_buckets: matching.len(),
            zero_buckets,
            positive_buckets,
            unknown_buckets,
            min_remaining_bp,
            latest_reset_in_secs,
        }
    }

    fn matches(&self, loaded: &LoadedProfile) -> bool {
        self.source == loaded.source && self.fingerprint == loaded.fingerprint
    }

    pub(crate) async fn request(
        &self,
        url: &str,
        access_token: &str,
        user_agent: &str,
        include_antigravity_metadata: bool,
        accept: Option<&'static str>,
        content_type: &'static str,
        body: bytes::Bytes,
        idle_timeout: Option<Duration>,
    ) -> Result<TransportResponse, super::transport::TransportError> {
        let mut headers = vec![
            (
                "authorization",
                SecretString::new(format!("Bearer {access_token}")),
            ),
            ("content-type", SecretString::new(content_type.to_string())),
            ("user-agent", SecretString::new(user_agent.to_string())),
        ];
        if self.oauth_kind == OAuthKind::LegacyGeminiCli {
            headers.push((
                "x-goog-api-client",
                SecretString::new(self.google_api_client.clone()),
            ));
        }
        if self.oauth_kind == OAuthKind::Antigravity && include_antigravity_metadata {
            // Match the reviewed Antigravity client identity for both generation methods. Live A/B
            // established that missing content roles and the private 65,536 output boundary were
            // the decisive INVALID_ARGUMENT causes; these headers are retained for wire fidelity,
            // not treated as a substitute for adapting the public body. The Node helper sorts
            // headers, so insertion order does not affect the wire fingerprint.
            headers.push((
                "x-goog-api-client",
                SecretString::new("google-cloud-sdk vscode_cloudshelleditor/0.1".to_string()),
            ));
            headers.push((
                "client-metadata",
                SecretString::new(
                    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#
                        .to_string(),
                ),
            ));
        }
        if let Some(accept) = accept {
            headers.push(("accept", SecretString::new(accept.to_string())));
        }
        self.transport
            .send(TransportRequest {
                url,
                headers,
                body,
                idle_timeout,
            })
            .await
    }

    /// Silence allowance for customer generation on this profile.
    pub(crate) fn generation_idle(&self) -> Option<Duration> {
        self.generation_idle
    }
    pub(crate) async fn access_token(
        &self,
        force_refresh: bool,
    ) -> Result<SecretString, TokenError> {
        // One mutex owns both the expiry check and refresh. A burst after expiry produces exactly
        // one Google token request; all other turns reuse its result.
        let mut credential = self.credential.lock().await;
        let now = pool::now();
        if !force_refresh
            && credential.expires_at > now.saturating_add(ACCESS_TOKEN_SKEW_SECS)
            && !credential.access_token.is_empty()
        {
            return Ok(SecretString::new(credential.access_token.clone()));
        }
        self.refresh_locked(&mut credential).await
    }

    /// Refresh after a concrete bearer token was rejected. If another concurrent request already
    /// replaced that token, reuse the winner instead of serially refreshing once per rejected
    /// request. This preserves single-flight behaviour for both expiry and 401 bursts.
    pub(crate) async fn access_token_after_rejection(
        &self,
        rejected_token: &str,
    ) -> Result<SecretString, TokenError> {
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
        credential: &mut GeminiCredential,
    ) -> Result<SecretString, TokenError> {
        let now = pool::now();
        let form = SecretString::new(
            serde_urlencoded::to_string([
                ("refresh_token", credential.refresh_token.as_str()),
                ("client_id", credential.oauth_client_id.as_str()),
                ("client_secret", credential.oauth_client_secret.as_str()),
                ("grant_type", "refresh_token"),
            ])
            .map_err(|_| TokenError::Invalid)?,
        );
        let mut headers = vec![
            (
                "content-type",
                SecretString::new("application/x-www-form-urlencoded;charset=UTF-8".to_string()),
            ),
            (
                "user-agent",
                SecretString::new(self.refresh_user_agent.clone()),
            ),
        ];
        if self.oauth_kind == OAuthKind::LegacyGeminiCli {
            headers.push((
                "x-goog-api-client",
                SecretString::new(self.google_api_client.clone()),
            ));
        }
        let response = self
            .transport
            .send(TransportRequest {
                url: &credential.token_uri,
                headers,
                body: bytes::Bytes::copy_from_slice(form.as_bytes()),
                idle_timeout: Some(self.auxiliary_idle),
            })
            .await
            .map_err(|_| TokenError::Temporary)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // Ровно один статус означает мёртвый credential: `400 invalid_grant`. Остальное —
            // окружение. Раньше 400/401/403 схлопывались в «Invalid», и Google, отклонивший
            // refresh по репутации IP прокси, выглядел неотличимо от отозванного токена.
            let body = response
                .bytes_limited_zeroizing(64 * 1024)
                .await
                .unwrap_or_default();
            let google_error = serde_json::from_slice::<serde_json::Value>(&body)
                .ok()
                .and_then(|value| {
                    value
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
            let verdict = classify_refresh_failure(status, google_error.as_deref());
            let revoked = matches!(verdict, TokenError::Invalid);
            // Только класс ошибки Google — ни токена, ни прокси, ни описания.
            elog::warn(
                "gemini-pool",
                format!(
                    "[gemini] profile={} token refresh rejected: http={} error={} verdict={}",
                    self.id(),
                    status,
                    google_error.as_deref().unwrap_or("-"),
                    if revoked {
                        "revoked"
                    } else if matches!(verdict, TokenError::Blocked) {
                        "blocked"
                    } else {
                        "temporary"
                    }
                ),
            );
            return Err(verdict);
        }
        let bytes = response
            .bytes_limited_zeroizing(1024 * 1024)
            .await
            .map_err(|_| TokenError::Temporary)?;
        let mut token: RefreshResponse =
            serde_json::from_slice(&bytes).map_err(|_| TokenError::Temporary)?;
        if token.access_token.len() < 8
            || token.access_token.len() > 16_384
            || !token
                .access_token
                .bytes()
                .all(|byte| byte.is_ascii_graphic())
            || !(60..=86_400).contains(&token.expires_in)
        {
            return Err(TokenError::Invalid);
        }
        credential.access_token = std::mem::take(&mut token.access_token);
        credential.expires_at = now.saturating_add(token.expires_in).saturating_sub(60);
        if let Some(refresh) = token
            .refresh_token
            .take()
            .filter(|value| !value.is_empty() && value.len() <= 16_384)
        {
            // Google normally keeps the refresh token stable. Retain an unexpected rotation in
            // memory for the process lifetime; the encrypted producer remains the disk authority.
            credential.refresh_token = refresh;
        }
        Ok(SecretString::new(credential.access_token.clone()))
    }

    pub(crate) async fn project_id(&self) -> String {
        self.credential.lock().await.project_id.clone()
    }

    pub(crate) fn mark_model_success(&self, model_id: &str) {
        self.mark_authenticated();
        let mut health = self
            .model_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = health.entry(model_id.to_string()).or_default();
        state.cooling_until = 0;
        state.failure_streak = 0;
        state.last_success_at = pool::now();
        state.last_failure_class = None;
    }

    pub(crate) fn mark_authenticated(&self) {
        // A quota-free liveness probe proves the bearer but must not erase a generation 429.
        // Generation quota/model failures live in the independent model-health axis below, so a
        // concrete successful request is free to clear stale auth/transport/global-probe cooling.
        // This matters when a concurrent stale-token 401 races with a successful refreshed turn:
        // verified success must not leave the profile quarantined for the full auth timeout.
        self.authenticated.store(true, Ordering::Release);
        self.cooling_until.store(0, Ordering::Release);
        self.quota_cooling_until.store(0, Ordering::Release);
        self.auth_failure_streak.store(0, Ordering::Release);
    }

    pub(crate) fn cool_until(&self, until: i64) {
        self.cooling_until.fetch_max(until, Ordering::AcqRel);
    }

    /// Cool on the hard axis: Google itself reported the profile is out of quota.
    pub(crate) fn cool_quota_until(&self, until: i64) {
        self.quota_cooling_until.fetch_max(until, Ordering::AcqRel);
    }

    /// Record an environment-derived auth rejection (upstream 401/403 that a fresh bearer did not
    /// resolve).
    ///
    /// Google declares a credential dead exactly once, by answering `invalid_grant` to a refresh —
    /// that is `mark_auth_failed`. A 401/403 on the generation surface after a successful refresh
    /// says something about the environment (entitlement, IP reputation, a Google-side blip), not
    /// about the token, so `authenticated` is deliberately left alone: the profile stays visible
    /// and countable, and only backs off. The streak escalates the backoff exactly like
    /// `mark_model_failure`, so a one-off blip costs seconds while a persistently rejected profile
    /// stops being hammered.
    pub(crate) fn mark_auth_blocked(&self, cfg: &GeminiConfig) {
        let now = pool::now();
        let streak = self
            .auth_failure_streak
            .fetch_add(1, Ordering::AcqRel)
            .saturating_add(1);
        let shift = streak.saturating_sub(1).min(30);
        let multiplier = 1_i64.checked_shl(shift).unwrap_or(i64::MAX);
        let delay = cfg
            .auth_blocked_cool_secs
            .saturating_mul(multiplier)
            .clamp(1, cfg.auth_quarantine_secs);
        self.cool_until(now.saturating_add(delay));
    }

    pub(crate) fn cool_model_until(&self, model_id: &str, until: i64) {
        let mut health = self
            .model_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = health.entry(model_id.to_string()).or_default();
        state.cooling_until = state.cooling_until.max(until);
    }

    pub(crate) fn mark_model_failure(
        &self,
        model_id: &str,
        class: &'static str,
        cfg: &GeminiConfig,
    ) {
        let now = pool::now();
        let mut health = self
            .model_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let state = health.entry(model_id.to_string()).or_default();
        state.failure_streak = state.failure_streak.saturating_add(1);
        let shift = state.failure_streak.saturating_sub(1).min(30);
        let multiplier = 1_i64.checked_shl(shift).unwrap_or(i64::MAX);
        let delay = cfg
            .model_failure_cool_secs
            .saturating_mul(multiplier)
            .clamp(1, cfg.model_failure_max_cool_secs);
        state.cooling_until = state.cooling_until.max(now.saturating_add(delay));
        state.last_failure_at = now;
        state.last_failure_class = Some(class);
    }

    fn cooling_until_for(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
        generation: bool,
    ) -> i64 {
        self.cooling_until_inner(model_id, cfg, now, generation, false)
    }

    /// Cooling that may legitimately deny a request: only what Google itself reported as exhausted
    /// quota. The soft environment axis is skipped, so a profile that merely backed off after a
    /// 401/403 or a transport fault is still reachable when it is the last capacity left.
    fn hard_cooling_until_for(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
        generation: bool,
    ) -> i64 {
        self.cooling_until_inner(model_id, cfg, now, generation, true)
    }

    fn cooling_until_inner(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
        generation: bool,
        hard_only: bool,
    ) -> i64 {
        let env = if hard_only {
            0
        } else {
            self.cooling_until.load(Ordering::Acquire)
        };
        let global = env.max(self.quota_cooling_until.load(Ordering::Acquire));
        if !generation {
            // countTokens is quota-free and is deliberately usable as a diagnostic even when
            // generation for this model is degraded or its generation quota is exhausted.
            return global;
        }
        let model = self
            .model_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(model_id)
            .map(|state| state.cooling_until)
            .unwrap_or(0);
        global.max(model).max(
            self.quota_blocked_until(
                model_id,
                cfg,
                now,
                cfg.health_probe_interval_secs
                    .saturating_mul(2)
                    .clamp(60, 3_600) as i64,
            ),
        )
    }

    /// Return `(snapshot_stale, remaining_fraction)` for soft steering. Never-arrived evidence is
    /// neutral rather than stale; a genuinely stale snapshot stays fail-open but loses a tie to a
    /// fresh profile so an old optimistic fraction cannot absorb the fleet.
    fn quota_steering(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
        stale_secs: i64,
    ) -> (bool, Option<f64>) {
        let quota = self
            .quota
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if quota.updated_at <= 0 {
            return (false, None);
        }
        if quota.updated_at.saturating_add(stale_secs) <= now {
            return (true, None);
        }
        (
            false,
            quota
                .buckets
                .iter()
                .filter(|bucket| {
                    cfg.quota_model_id_matches_wire(self.oauth_kind, model_id, &bucket.model_id)
                })
                .filter_map(|bucket| bucket.remaining_fraction)
                .min_by(f64::total_cmp),
        )
    }

    fn quota_reserve_for(&self, model_id: &str, cfg: &GeminiConfig) -> f64 {
        let mut input = Vec::with_capacity(self.id.len() + model_id.len() + 1);
        input.extend_from_slice(self.id.as_bytes());
        input.push(0);
        input.extend_from_slice(model_id.as_bytes());
        let digest = blake3::hash(&input);
        let unit = u16::from_le_bytes([digest.as_bytes()[0], digest.as_bytes()[1]]) as f64
            / u16::MAX as f64;
        (cfg.quota_reserve_fraction + cfg.quota_reserve_jitter * (unit * 2.0 - 1.0))
            .clamp(0.0, 0.95)
    }

    fn model_health_for(&self, model_id: &str) -> GeminiModelHealthState {
        self.model_health
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(model_id)
            .cloned()
            .unwrap_or_default()
    }

    fn quota_blocked_until(
        &self,
        model_id: &str,
        cfg: &GeminiConfig,
        now: i64,
        stale_secs: i64,
    ) -> i64 {
        let quota = self
            .quota
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if quota.updated_at <= 0 || quota.buckets.is_empty() {
            return 0;
        }
        let stale_at = quota.updated_at.saturating_add(stale_secs);
        let matching = quota
            .buckets
            .iter()
            .filter(|bucket| {
                cfg.quota_model_id_matches_wire(self.oauth_kind, model_id, &bucket.model_id)
            })
            .collect::<Vec<_>>();
        if matching.is_empty() {
            // A fresh official quota catalogue is also a per-profile availability catalogue. Once
            // it becomes stale, fail open and let the generation endpoint provide fresh evidence.
            return if stale_at > now { stale_at } else { 0 };
        }
        let explicitly_zero = matching.iter().filter(|bucket| {
            bucket.remaining_amount == Some(0) || bucket.remaining_fraction == Some(0.0)
        });
        if explicitly_zero.clone().next().is_none() {
            // Positive and unknown dimensions both fail open. A generation response is better
            // evidence than inventing exhaustion where Google did not report an explicit zero.
            return 0;
        }
        // Quota dimensions are conjunctive: one exhausted request/token bucket blocks the model
        // even when another dimension remains positive. Every exhausted dimension must recover,
        // so the latest known reset wins; bounded snapshot expiry remains a recovery path for a
        // malformed/missing reset value.
        explicitly_zero
            .map(|bucket| {
                bucket
                    .reset_time
                    .as_deref()
                    .and_then(parse_rfc3339_seconds)
                    .filter(|reset| *reset > now)
                    .unwrap_or_else(|| if stale_at > now { stale_at } else { 0 })
            })
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn mark_auth_failed(&self, until: i64) {
        self.authenticated.store(false, Ordering::Release);
        self.cool_until(until);
    }

    /// Queue one exact successful-turn event. Spend and the immutable token vector advance in one
    /// authority transaction; a later quota poll flushes this FIFO before reading cumulative spend.
    pub(crate) fn record_turn(&self, event: registry::ProviderTurnCalibrationEvent) {
        let Some(billing) = &self.billing else {
            self.calibration_persistence_ok
                .store(false, Ordering::Relaxed);
            return;
        };
        if !billing.record_gemini_turn_detached(event, &self.plan, Vec::new()) {
            elog::error("gemini-pool", "gemini turn record failed");
            self.calibration_persistence_ok
                .store(false, Ordering::Relaxed);
        }
    }

    async fn note_quota_summary(&self, buckets: &[GeminiSummaryBucket], observed_at: i64) {
        let mut all_persisted = self.billing.is_some();
        for bucket in buckets {
            let used_fraction_units =
                FRACTION_SCALE.saturating_sub(bucket.remaining_fraction_units);
            let persisted = match &self.billing {
                Some(billing) => {
                    billing
                        .observe_gemini_window(
                            self.id(),
                            &self.plan,
                            bucket.contract.id,
                            bucket.contract.kind,
                            bucket.contract.duration_mins,
                            bucket.resets_at,
                            used_fraction_units,
                            bucket.measurement_resolution_fraction_units,
                            observed_at,
                        )
                        .await
                }
                None => Err(anyhow::anyhow!("in-memory calibration")),
            };
            match persisted {
                Ok((spend_nano, row)) => {
                    self.spend_nano_total
                        .fetch_max(spend_nano, Ordering::Relaxed);
                    match WindowCalibration::from_row(row) {
                        Ok(calibration) => {
                            self.calibrations
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(bucket.contract.id.to_string(), calibration);
                        }
                        Err(error) => {
                            all_persisted = false;
                            elog::error(
                                "gemini-pool",
                                format!(
                                    "Gemini window calibration restore failed [{}]",
                                    error.root_cause()
                                ),
                            );
                        }
                    }
                }
                Err(error) => {
                    all_persisted = false;
                    let spend_nano = self.spend_nano_total.load(Ordering::Relaxed);
                    let observation = registry::GeminiExactWindowObservation {
                        profile_id: self.id().to_string(),
                        plan: self.plan.clone(),
                        bucket_id: bucket.contract.id.to_string(),
                        window_kind: bucket.contract.kind.to_string(),
                        window_duration_mins: bucket.contract.duration_mins,
                        resets_at: bucket.resets_at,
                        observed_at,
                        used_fraction_units,
                        measurement_resolution_fraction_units: bucket
                            .measurement_resolution_fraction_units,
                        gateway_spend_nano: spend_nano,
                        observation_source: "poll".to_owned(),
                        source_request_id: None,
                    };
                    let mut calibrations = self
                        .calibrations
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    let existing = calibrations
                        .remove(bucket.contract.id)
                        .map(WindowCalibration::into_row);
                    match calibration::apply_observation_with_history(existing, &[], &observation)
                        .and_then(WindowCalibration::from_row)
                    {
                        Ok(calibration) => {
                            calibrations.insert(bucket.contract.id.to_string(), calibration);
                        }
                        Err(local_error) => {
                            elog::warn(
                                "gemini-pool",
                                format!(
                                    "Gemini window calibration failed [{}]",
                                    local_error.root_cause()
                                ),
                            );
                        }
                    }
                    if self.billing.is_some() {
                        elog::error(
                            "gemini-pool",
                            format!(
                                "Gemini window calibration persistence failed [{}]",
                                error.root_cause()
                            ),
                        );
                    }
                }
            }
        }
        self.calibration_persistence_ok.store(
            all_persisted
                && self.billing.as_ref().is_some_and(|billing| {
                    let status = billing.gemini_calibration_delivery_status();
                    status.pending_events == 0
                        && status.dropped_events == 0
                        && status.persistence_ok
                }),
            Ordering::Relaxed,
        );
    }

    fn capacity_reports(&self, now: i64) -> Vec<GeminiWindowCapacityReport> {
        let summary = self
            .quota_summary
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let calibrations = self
            .calibrations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        summary
            .buckets
            .iter()
            .map(|bucket| {
                let calibration = calibrations.get(bucket.contract.id);
                let estimate = calibration.and_then(WindowCalibration::estimate);
                let observed_at = estimate
                    .map(|estimate| estimate.measured_at)
                    .unwrap_or(summary.updated_at);
                let nano_to_usd = |nano: i64| nano as f64 / 1e9;
                let used = FRACTION_SCALE.saturating_sub(bucket.remaining_fraction_units);
                let capacity_nano = estimate.map(|value| value.capacity_nano);
                let remaining_nano = calibration.and_then(|value| value.remaining_nano(used));
                let low_nano = estimate.and_then(|value| value.low_nano);
                let high_nano = estimate.and_then(|value| value.high_nano);
                let remaining_low_nano =
                    calibration.and_then(|value| value.remaining_low_nano(used));
                let remaining_high_nano =
                    calibration.and_then(|value| value.remaining_high_nano(used));
                GeminiWindowCapacityReport {
                    bucket_id: bucket.contract.id,
                    window_kind: bucket.contract.kind,
                    window_minutes: bucket.contract.duration_mins,
                    resets_at: bucket.resets_at,
                    observed_at,
                    data_age_seconds: now.saturating_sub(observed_at).max(0),
                    remaining_fraction_units: bucket.remaining_fraction_units,
                    used_fraction_units: used,
                    capacity_nano,
                    remaining_nano,
                    low_nano,
                    high_nano,
                    remaining_low_nano,
                    remaining_high_nano,
                    cap_usd: capacity_nano.map(nano_to_usd),
                    remaining_usd: remaining_nano.map(nano_to_usd),
                    low_usd: low_nano.map(nano_to_usd),
                    high_usd: high_nano.map(nano_to_usd),
                    remaining_low_usd: remaining_low_nano.map(nano_to_usd),
                    remaining_high_usd: remaining_high_nano.map(nano_to_usd),
                    observed_spend_nano: calibration
                        .map_or(0, |value| value.row().observed_spend_nano),
                    observed_fraction_units: calibration
                        .map_or(0, |value| value.row().observed_fraction_units),
                    source: estimate.map_or("unknown", |value| value.source.as_str()),
                    confidence: estimate.map_or(0.0, |value| value.confidence_bp as f64 / 10_000.0),
                    samples: calibration.map_or(0, |value| value.row().samples),
                }
            })
            .collect()
    }

    fn status(
        &self,
        cfg: &GeminiConfig,
        now: i64,
        disabled: bool,
        hidden: bool,
    ) -> GeminiProfileStatus {
        // Clone the small sanitized snapshot before computing derived cooling. Holding this read
        // guard while `cooling_until_for` takes a second read can deadlock on writer-preferring
        // RwLock implementations when the quota refresh is already waiting for its write guard.
        let (quota_updated_at, quotas) = {
            let quota = self
                .quota
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            (quota.updated_at, quota.buckets.clone())
        };
        let model_cooling = cfg
            .models
            .iter()
            .map(|model| {
                let wire_model_id = model.default_wire_model_id();
                let health = self.model_health_for(wire_model_id);
                GeminiModelCoolingStatus {
                    model_id: model.id.clone(),
                    cooling_until: self.cooling_until_for(wire_model_id, cfg, now, true),
                    failure_streak: health.failure_streak,
                    last_success_at: health.last_success_at,
                    last_failure_at: health.last_failure_at,
                    last_failure_class: health.last_failure_class.map(str::to_string),
                }
            })
            .collect();
        let lifecycle = crate::lifecycle::gemini(self.issued_at, &self.plan, now);
        GeminiProfileStatus {
            id: self.id.clone(),
            masked_email: self.masked_email.clone(),
            plan: self.plan.clone(),
            acquired_at: lifecycle.acquired_at,
            subscription_expires_at: lifecycle.subscription_expires_at,
            subscription_days_left: lifecycle.subscription_days_left,
            authenticated: self.authenticated.load(Ordering::Acquire),
            disabled,
            hidden,
            cooling_until: self
                .cooling_until
                .load(Ordering::Acquire)
                .max(self.quota_cooling_until.load(Ordering::Acquire)),
            inflight: self.inflight.load(Ordering::Acquire),
            last_probe_at: self.last_probe_at.load(Ordering::Acquire),
            quota_updated_at,
            quotas,
            model_cooling,
            spend_usd_total: self.spend_nano_total.load(Ordering::Relaxed) as f64 / 1e9,
            calibration_persistence_ok: self.calibration_persistence_ok.load(Ordering::Relaxed),
            capacities: self.capacity_reports(now),
        }
    }

    async fn probe(&self, cfg: &GeminiConfig) -> ProbeResult {
        self.last_probe_at.store(pool::now(), Ordering::Release);
        let mut token = match self.access_token(false).await {
            Ok(token) => token,
            Err(TokenError::Invalid) => return ProbeResult::Invalid,
            Err(TokenError::Blocked) => return ProbeResult::Blocked,
            Err(TokenError::Temporary) => return ProbeResult::Temporary,
        };
        for attempt in 0..=1 {
            let url = format!(
                "{}/v1internal:loadCodeAssist",
                cfg.upstream_for(self.oauth_kind)
            );
            let project = self.project_id().await;
            let body = match self.oauth_kind {
                OAuthKind::Antigravity => json!({"metadata": {"ideType": "ANTIGRAVITY"}}),
                OAuthKind::LegacyGeminiCli => json!({
                    "cloudaicompanionProject": project,
                    "metadata": {
                        "ideType": "IDE_UNSPECIFIED",
                        "platform": "PLATFORM_UNSPECIFIED",
                        "pluginType": "GEMINI",
                        "duetProject": project
                    },
                    "mode": "HEALTH_CHECK"
                }),
            };
            let response = self
                .request(
                    &url,
                    &token,
                    &cfg.background_user_agent(self.oauth_kind),
                    true,
                    (self.oauth_kind == OAuthKind::LegacyGeminiCli).then_some("application/json"),
                    "application/json",
                    bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                    Some(self.auxiliary_idle),
                )
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    self.refresh_quota(cfg, &token).await;
                    return ProbeResult::Healthy;
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    let headers = response.headers().clone();
                    return ProbeResult::RateLimited { headers, response };
                }
                Ok(response) if response.status().as_u16() == 401 && attempt == 0 => {
                    token = match self.access_token(true).await {
                        Ok(token) => token,
                        Err(TokenError::Invalid) => return ProbeResult::Invalid,
                        Err(TokenError::Blocked) => return ProbeResult::Blocked,
                        Err(TokenError::Temporary) => return ProbeResult::Temporary,
                    };
                }
                // A fresh bearer was just obtained (a revoked refresh token would have returned
                // `Invalid` above), so a still-rejecting liveness surface is the environment
                // talking, not Google revoking the credential. Back off, stay authenticated.
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    return ProbeResult::Blocked
                }
                Ok(_) | Err(_) => return ProbeResult::Temporary,
            }
        }
        // Unreachable while the loop returns on every arm; kept as the same conservative verdict
        // the arms use, so a future edit cannot silently turn a fallthrough into a revocation.
        ProbeResult::Blocked
    }

    async fn refresh_quota(&self, cfg: &GeminiConfig, token: &str) {
        if self.oauth_kind == OAuthKind::Antigravity {
            // These endpoints are independent evidence streams. In particular, a summary failure
            // must never discard the last-good per-model catalogue, and a catalogue hiccup must
            // not stop 5h/weekly calibration from advancing.
            tokio::join!(
                self.refresh_model_quota(cfg, token),
                self.refresh_quota_summary(cfg, token)
            );
        } else {
            self.refresh_model_quota(cfg, token).await;
        }
    }

    async fn refresh_model_quota(&self, cfg: &GeminiConfig, token: &str) {
        let operation = match self.oauth_kind {
            OAuthKind::Antigravity => "fetchAvailableModels",
            OAuthKind::LegacyGeminiCli => "retrieveUserQuota",
        };
        let url = format!(
            "{}/v1internal:{operation}",
            cfg.upstream_for(self.oauth_kind)
        );
        let body = json!({"project": self.project_id().await});
        let response = self
            .request(
                &url,
                token,
                &cfg.background_user_agent(self.oauth_kind),
                true,
                (self.oauth_kind == OAuthKind::LegacyGeminiCli).then_some("application/json"),
                "application/json",
                bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                Some(self.auxiliary_idle),
            )
            .await;
        let Ok(response) = response else {
            return;
        };
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > 256 * 1024)
        {
            return;
        }
        let Ok(bytes) = response.bytes_limited(256 * 1024).await else {
            return;
        };
        let mut buckets = match self.oauth_kind {
            OAuthKind::Antigravity => {
                let Ok(document) = serde_json::from_slice::<AvailableModelsResponse>(&bytes) else {
                    return;
                };
                document
                    .models
                    .into_iter()
                    .filter_map(|(model_id, model)| model.sanitized(model_id))
                    .take(256)
                    .collect::<Vec<_>>()
            }
            OAuthKind::LegacyGeminiCli => {
                let Ok(document) = serde_json::from_slice::<QuotaResponse>(&bytes) else {
                    return;
                };
                document
                    .buckets
                    .into_iter()
                    .filter_map(QuotaBucket::sanitized)
                    .take(256)
                    .collect::<Vec<_>>()
            }
        };
        buckets.sort_by(|left, right| left.model_id.cmp(&right.model_id));
        *self
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at: pool::now(),
            buckets,
        };
    }

    async fn refresh_quota_summary(&self, cfg: &GeminiConfig, token: &str) {
        let url = format!(
            "{}/v1internal:retrieveUserQuotaSummary",
            cfg.upstream_for(self.oauth_kind)
        );
        let body = json!({"project": self.project_id().await});
        let response = self
            .request(
                &url,
                token,
                &cfg.background_user_agent(self.oauth_kind),
                true,
                None,
                "application/json",
                bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                Some(self.auxiliary_idle),
            )
            .await;
        let Ok(response) = response else {
            return;
        };
        if !response.status().is_success()
            || response
                .content_length()
                .is_some_and(|length| length > 256 * 1024)
        {
            return;
        }
        let Ok(bytes) = response.bytes_limited(256 * 1024).await else {
            return;
        };
        let Ok(document) = serde_json::from_slice::<QuotaSummaryEnvelope>(&bytes) else {
            return;
        };
        let groups = document
            .quota_summary
            .map_or(document.groups, |summary| summary.groups);
        let mut buckets = Vec::new();
        let mut seen = HashSet::new();
        for bucket in groups.into_iter().flat_map(|group| group.buckets) {
            let Some(bucket_id) = bucket.bucket_id else {
                continue;
            };
            let Some(contract) = calibration::bucket_contract(&bucket_id) else {
                // `3p-5h`/`3p-weekly` belong to Claude/GPT and must never contaminate Gemini.
                continue;
            };
            if !seen.insert(contract.id) {
                // Ambiguous duplicated authority evidence is safer to omit than to pick by order.
                buckets
                    .retain(|existing: &GeminiSummaryBucket| existing.contract.id != contract.id);
                continue;
            }
            let Some(fraction) = bucket
                .remaining_fraction
                .as_ref()
                .and_then(ExactFraction::value)
            else {
                continue;
            };
            let Some(resets_at) = bucket
                .reset_time
                .as_deref()
                .and_then(parse_rfc3339_seconds)
                .filter(|reset| *reset > 0)
            else {
                continue;
            };
            buckets.push(GeminiSummaryBucket {
                contract,
                remaining_fraction_units: fraction.units,
                measurement_resolution_fraction_units: fraction.resolution_units,
                resets_at,
            });
        }
        if buckets.is_empty() {
            return;
        }
        buckets.sort_by_key(|bucket| bucket.contract.duration_mins);
        let observed_at = pool::now();
        self.note_quota_summary(&buckets, observed_at).await;
        *self
            .quota_summary
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSummarySnapshot {
            updated_at: observed_at,
            buckets,
        };
    }
}

#[derive(Deserialize, Zeroize, ZeroizeOnDrop)]
struct RefreshResponse {
    access_token: String,
    expires_in: i64,
    refresh_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaResponse {
    #[serde(default)]
    buckets: Vec<QuotaBucket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaBucket {
    model_id: Option<String>,
    remaining_amount: Option<String>,
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
    token_type: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvailableModelsResponse {
    #[serde(default)]
    models: HashMap<String, AvailableModel>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvailableModel {
    quota_info: Option<AvailableModelQuota>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AvailableModelQuota {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryEnvelope {
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
    quota_summary: Option<QuotaSummary>,
}

#[derive(Deserialize)]
struct QuotaSummary {
    #[serde(default)]
    groups: Vec<QuotaSummaryGroup>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryGroup {
    #[serde(default)]
    buckets: Vec<QuotaSummaryBucket>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaSummaryBucket {
    #[serde(alias = "bucket_id")]
    bucket_id: Option<String>,
    #[serde(alias = "reset_time")]
    reset_time: Option<String>,
    #[serde(alias = "remaining_fraction")]
    remaining_fraction: Option<ExactFraction>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ExactFraction {
    Number(serde_json::Number),
    String(String),
}

impl ExactFraction {
    fn value(&self) -> Option<ExactFractionValue> {
        let value = match self {
            Self::Number(value) => value.to_string(),
            Self::String(value) => value.trim().to_string(),
        };
        parse_fraction(&value)
    }

    #[cfg(test)]
    fn units(&self) -> Option<i64> {
        self.value().map(|value| value.units)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExactFractionValue {
    units: i64,
    resolution_units: i64,
}

/// Parse a JSON decimal into 10^-8 units without passing through binary floating point.
#[cfg(test)]
fn parse_fraction_units(value: &str) -> Option<i64> {
    parse_fraction(value).map(|value| value.units)
}

fn parse_fraction(value: &str) -> Option<ExactFractionValue> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return None;
    }
    let (mantissa, exponent) = match value.find(['e', 'E']) {
        Some(index) => (
            value.get(..index)?,
            value.get(index + 1..)?.parse::<i32>().ok()?,
        ),
        None => (value, 0),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some(parts) => parts,
        None => (mantissa, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let digits = format!("{whole}{fraction}");
    let coefficient = digits.parse::<i128>().ok()?;
    let power = 8i32
        .checked_add(exponent)?
        .checked_sub(fraction.len() as i32)?;
    let scaled = if power >= 0 {
        coefficient.checked_mul(10i128.checked_pow(power as u32)?)?
    } else {
        let divisor = 10i128.checked_pow(power.unsigned_abs())?;
        if coefficient % divisor != 0 {
            return None;
        }
        coefficient / divisor
    };
    let units = i64::try_from(scaled)
        .ok()
        .filter(|units| (0..=FRACTION_SCALE).contains(units))?;
    let resolution = if power >= 0 {
        10i128.checked_pow(power as u32)?
    } else {
        1
    };
    let resolution_units = i64::try_from(resolution).ok()?.clamp(1, FRACTION_SCALE);
    Some(ExactFractionValue {
        units,
        resolution_units,
    })
}

impl AvailableModel {
    fn sanitized(self, model_id: String) -> Option<GeminiQuotaBucketStatus> {
        if !valid_model_id(&model_id) {
            return None;
        }
        let quota = self.quota_info;
        Some(GeminiQuotaBucketStatus {
            model_id,
            remaining_amount: None,
            remaining_fraction: quota
                .as_ref()
                .and_then(|quota| quota.remaining_fraction)
                .filter(|value| value.is_finite())
                .map(|value| value.clamp(0.0, 1.0)),
            reset_time: bounded_quota_text(quota.and_then(|quota| quota.reset_time)),
            token_type: Some("antigravity_model".to_string()),
        })
    }
}

impl QuotaBucket {
    fn sanitized(self) -> Option<GeminiQuotaBucketStatus> {
        let model_id = self.model_id?;
        if !valid_model_id(&model_id) {
            return None;
        }
        let remaining_fraction = self
            .remaining_fraction
            .filter(|value| value.is_finite())
            .map(|value| value.clamp(0.0, 1.0));
        Some(GeminiQuotaBucketStatus {
            model_id,
            remaining_amount: self
                .remaining_amount
                .as_deref()
                .and_then(|value| value.parse::<u64>().ok()),
            remaining_fraction,
            reset_time: bounded_quota_text(self.reset_time),
            token_type: bounded_quota_text(self.token_type),
        })
    }
}

fn valid_model_id(model_id: &str) -> bool {
    !model_id.is_empty()
        && model_id.len() <= 128
        && model_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn bounded_quota_text(value: Option<String>) -> Option<String> {
    value.filter(|value| {
        !value.is_empty() && value.len() <= 128 && value.bytes().all(|byte| byte.is_ascii_graphic())
    })
}

fn parse_rfc3339_seconds(value: &str) -> Option<i64> {
    let bytes = value.as_bytes();
    if bytes.len() < 20
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[10] != b'T'
        || bytes[13] != b':'
        || bytes[16] != b':'
    {
        return None;
    }
    let number = |start: usize, end: usize| value.get(start..end)?.parse::<i64>().ok();
    let (year, month, day) = (number(0, 4)?, number(5, 7)?, number(8, 10)?);
    let (hour, minute, second) = (number(11, 13)?, number(14, 16)?, number(17, 19)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days_in_month = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut timezone_index = 19usize;
    if bytes.get(timezone_index) == Some(&b'.') {
        timezone_index += 1;
        while bytes.get(timezone_index).is_some_and(u8::is_ascii_digit) {
            timezone_index += 1;
        }
    }
    let offset = match bytes.get(timezone_index).copied()? {
        b'Z' if timezone_index + 1 == bytes.len() => 0,
        sign @ (b'+' | b'-') if timezone_index + 6 == bytes.len() => {
            if bytes[timezone_index + 3] != b':' {
                return None;
            }
            let hours = value
                .get(timezone_index + 1..timezone_index + 3)?
                .parse::<i64>()
                .ok()?;
            let minutes = value
                .get(timezone_index + 4..timezone_index + 6)?
                .parse::<i64>()
                .ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let seconds = hours * 3_600 + minutes * 60;
            if sign == b'+' {
                seconds
            } else {
                -seconds
            }
        }
        _ => return None,
    };
    let adjusted_year = year - i64::from(month <= 2);
    let era = if adjusted_year >= 0 {
        adjusted_year
    } else {
        adjusted_year - 399
    } / 400;
    let year_of_era = adjusted_year - era * 400;
    let shifted_month = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some(
        days.saturating_mul(86_400)
            .saturating_add(hour * 3_600 + minute * 60 + second)
            .saturating_sub(offset),
    )
}

/// Единственный статус, означающий мёртвый credential, — `400 invalid_grant`.
///
/// 401/403 приходят от окружения: Google отклоняет запрос по репутации IP прокси или блокирует
/// клиента, а сам grant при этом цел. Раньше все три схлопывались в `Invalid`, и живая оплаченная
/// подписка уходила из ротации навсегда с красным «ошибка auth» по причине, которой нет в токене.
fn classify_refresh_failure(status: u16, google_error: Option<&str>) -> TokenError {
    match (status, google_error) {
        (400, Some("invalid_grant")) => TokenError::Invalid,
        (401 | 403, _) => TokenError::Blocked,
        _ => TokenError::Temporary,
    }
}

enum ProbeResult {
    Healthy,
    RateLimited {
        headers: axum::http::HeaderMap,
        response: TransportResponse,
    },
    /// Google отозвал grant — профиль действительно нельзя использовать.
    Invalid,
    /// Запрос отклонён окружением (401/403): grant цел, путь временно недоступен.
    Blocked,
    Temporary,
}

fn read_private_file(path: &str, description: &str) -> anyhow::Result<Vec<u8>> {
    let path = Path::new(path);
    if !path.is_absolute() {
        bail!("{description} path must be absolute");
    }
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat {description} file"))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("{description} path must be a regular non-symlink file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("{description} file must not be accessible by group or other users");
        }
    }
    fs::read(path).with_context(|| format!("read {description} file"))
}

fn validate_private_directory(path: &Path, description: &str) -> anyhow::Result<()> {
    let metadata =
        fs::symlink_metadata(path).with_context(|| format!("stat {description} directory"))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("{description} directory must be a real non-symlink directory");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!("{description} directory must not be accessible by group or other users");
        }
    }
    Ok(())
}

fn load_profiles(cfg: &GeminiConfig) -> anyhow::Result<Vec<LoadedProfile>> {
    let roster_path = Path::new(&cfg.profiles_file);
    if !roster_path.exists() {
        return Ok(Vec::new());
    }
    let raw = read_private_file(&cfg.profiles_file, "Gemini profiles")
        .with_context(|| format!("read Gemini profiles file {}", cfg.profiles_file))?;
    let document: GeminiProfilesFile =
        serde_json::from_slice(&raw).context("parse Gemini profiles file")?;
    let roster_root = roster_path
        .parent()
        .context("Gemini profiles file has no parent")?;
    validate_private_directory(roster_root, "Gemini roster")?;
    let credential_root = roster_root.join("credentials");
    validate_private_directory(&credential_root, "Gemini credential")?;
    let mut ids = HashSet::new();
    let mut subjects = HashSet::new();
    let mut proxies = HashSet::new();
    let mut proxy_orders = HashSet::new();
    let mut profiles = Vec::with_capacity(document.profiles.len());
    for source in document.profiles {
        gemini_credential::validate_profile_id(&source.id)?;
        if !ids.insert(source.id.clone()) {
            bail!("duplicate Gemini profile id");
        }
        let expected = credential_root.join(format!("{}.json", source.id));
        if Path::new(&source.credential_file) != expected {
            bail!("Gemini credential path does not match the sealed roster layout");
        }
        let encrypted = read_private_file(&source.credential_file, "Gemini credential")?;
        let envelope = decode_envelope(&encrypted).context("parse Gemini credential envelope")?;
        let mut credential = cfg
            .credential_keys
            .open(&source.id, &envelope)
            .context("open Gemini credential envelope")?;
        if !subjects.insert(credential.subject.clone()) {
            bail!("duplicate Gemini subscription identity in profiles file");
        }
        if !credential.proxy.is_empty() {
            credential.proxy = gemini_credential::normalize_proxy_url(&credential.proxy)?;
            if !proxies.insert(credential.proxy.clone()) {
                bail!("duplicate Gemini profile proxy in profiles file");
            }
        }
        if credential.proxy_order_id > 0 && !proxy_orders.insert(credential.proxy_order_id) {
            bail!("duplicate Gemini IPRoyal order id in profiles file");
        }
        profiles.push(LoadedProfile {
            source,
            credential,
            fingerprint: *blake3::hash(&encrypted).as_bytes(),
        });
    }
    Ok(profiles)
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
    calibration_store: Option<Arc<AsyncBilling>>,
    profiles: RwLock<Vec<Arc<GeminiProfile>>>,
    /// Operator disables from `pool_member_disables`, refreshed from the engine authority. Kept
    /// beside the roster rather than inside it: the roster is the Auth Bot's sealed artifact and
    /// is replaced wholesale on every publication, so a disable written into it would not survive.
    /// An empty set is the correct startup default — the first refresh fills it, and until then a
    /// disabled profile is merely probed once, not routed to, because selection re-checks.
    disabled: RwLock<HashSet<String>>,
    /// Subset of `disabled` the operator also hid. Kept separate from routing state so a
    /// presentation choice can never influence which profile serves a request.
    hidden: RwLock<HashSet<String>>,
    /// Process-local reverse index for the affinity store's keyed opaque home ids. The key is
    /// secret-dependent, so it is built lazily once the shared AffinityStore is available and
    /// invalidated atomically with every roster generation change.
    affinity_profiles: RwLock<HashMap<String, String>>,
    cursor: AtomicU64,
    shutting_down: AtomicBool,
    abort_streams: AtomicBool,
    abort_notify: tokio::sync::Notify,
    probe_poke: tokio::sync::Notify,
    /// Start time of the last health sweep, so a data-path poke cannot spin the sweep loop.
    last_sweep_at: AtomicI64,
    background_tasks: Arc<ActiveTaskTracker>,
}

impl GeminiGateway {
    pub fn new(cfg: GeminiConfig) -> anyhow::Result<Self> {
        Self::new_with_calibration(cfg, None)
    }

    pub fn new_with_calibration(
        cfg: GeminiConfig,
        calibration_store: Option<Arc<AsyncBilling>>,
    ) -> anyhow::Result<Self> {
        if !cfg.enabled {
            bail!("Gemini provider is disabled");
        }
        if cfg.models.is_empty() {
            bail!("Gemini model allowlist is empty");
        }
        if cfg.model_failure_cool_secs <= 0
            || cfg.model_failure_max_cool_secs < cfg.model_failure_cool_secs
            || !cfg.quota_reserve_fraction.is_finite()
            || !(0.0..=1.0).contains(&cfg.quota_reserve_fraction)
            || !cfg.quota_reserve_jitter.is_finite()
            || !(0.0..=1.0).contains(&cfg.quota_reserve_jitter)
        {
            bail!("Gemini pool routing configuration is invalid");
        }
        if cfg.upstream.starts_with("https://") {
            if cfg.antigravity_version != gemini_credential::ANTIGRAVITY_VERSION
                || cfg.node_binary != gemini_credential::GEMINI_NODE_BINARY
                || cfg.node_version != gemini_credential::GEMINI_NODE_VERSION
                || cfg.node_sha256 != gemini_credential::GEMINI_NODE_SHA256
            {
                bail!(
                    "Gemini production wire profile does not match the reviewed Antigravity/Node tuple"
                );
            }
            attest_node_binary(&cfg)?;
        }
        let loaded = load_profiles(&cfg)?;
        let cfg = Arc::new(cfg);
        let profiles = loaded
            .into_iter()
            .map(|profile| {
                GeminiProfile::new(profile, &cfg, calibration_store.clone()).map(Arc::new)
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            cfg,
            calibration_store,
            profiles: RwLock::new(profiles),
            disabled: RwLock::new(HashSet::new()),
            hidden: RwLock::new(HashSet::new()),
            affinity_profiles: RwLock::new(HashMap::new()),
            cursor: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            abort_streams: AtomicBool::new(false),
            abort_notify: tokio::sync::Notify::new(),
            probe_poke: tokio::sync::Notify::new(),
            last_sweep_at: AtomicI64::new(0),
            background_tasks: Arc::new(ActiveTaskTracker::default()),
        })
    }

    pub fn config(&self) -> &GeminiConfig {
        &self.cfg
    }

    /// Wake the free provider quota sweep after an exact controlled turn. `Notify` coalesces a
    /// burst, while normal customer traffic keeps the configured background cadence unchanged.
    pub(crate) fn request_probe(&self) {
        self.probe_poke.notify_one();
    }

    /// Ask for an out-of-band health sweep from the data path.
    ///
    /// The request that just found no capacity is the freshest evidence the pool will get, and
    /// waiting a full background cadence to act on it is exactly how a recoverable pool stayed
    /// unusable for minutes. Rate-limited against the last sweep so a sustained failure cannot turn
    /// every customer request into another full roster probe.
    pub(crate) fn request_probe_rate_limited(&self) {
        let now = pool::now();
        let last = self.last_sweep_at.load(Ordering::Acquire);
        if now.saturating_sub(last) < self.cfg.min_probe_interval_secs {
            return;
        }
        self.probe_poke.notify_one();
    }

    pub async fn probe_requested(&self) {
        self.probe_poke.notified().await;
    }

    fn profiles_snapshot(&self) -> Vec<Arc<GeminiProfile>> {
        self.profiles
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn disabled_snapshot(&self) -> HashSet<String> {
        self.disabled
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub fn is_disabled(&self, profile_id: &str) -> bool {
        self.disabled
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains(profile_id)
    }

    /// Profiles that may actually receive traffic. Every selection, readiness and probe path uses
    /// this; only reporting and roster diffing use the raw snapshot, because an operator has to be
    /// able to see a disabled profile in order to put it back.
    fn routable_profiles(&self) -> Vec<Arc<GeminiProfile>> {
        let disabled = self.disabled_snapshot();
        if disabled.is_empty() {
            return self.profiles_snapshot();
        }
        self.profiles_snapshot()
            .into_iter()
            .filter(|profile| !disabled.contains(profile.id()))
            .collect()
    }

    /// Pull the operator disable set from the engine authority. Called on startup, on every roster
    /// reload and on the roster refresh tick, so the button takes effect without a slot restart.
    /// A read failure leaves the previous set in place: forgetting a disable would silently put a
    /// revoked or quarantined credential back into rotation, which is the failure we cannot have.
    pub async fn refresh_disabled(&self) {
        let Some(store) = self.calibration_store.as_ref() else {
            return;
        };
        match store.pool_member_disables(registry::PROVIDER_GOOGLE).await {
            Ok(disables) => {
                let hidden = disables
                    .iter()
                    .filter(|(_, hidden)| **hidden)
                    .map(|(id, _)| id.clone())
                    .collect();
                *self
                    .disabled
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) =
                    disables.into_keys().collect();
                *self
                    .hidden
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = hidden;
            }
            Err(error) => {
                elog::warn(
                    "gemini-pool",
                    format!("Gemini operator disable set refresh failed, keeping previous: {error:#}"),
                );
            }
        }
    }

    fn reload_profiles(&self) -> anyhow::Result<bool> {
        let loaded = load_profiles(&self.cfg)?;
        let current = self.profiles_snapshot();
        let mut next = Vec::with_capacity(loaded.len());
        for loaded_profile in loaded {
            if let Some(profile) = current
                .iter()
                .find(|profile| profile.matches(&loaded_profile))
            {
                next.push(profile.clone());
            } else {
                let profile = Arc::new(GeminiProfile::new(
                    loaded_profile,
                    &self.cfg,
                    self.calibration_store.clone(),
                )?);
                profile.authenticated.store(false, Ordering::Release);
                profile.cool_until(pool::now() + 1);
                next.push(profile);
            }
        }
        if next.len() == current.len()
            && next
                .iter()
                .zip(&current)
                .all(|(left, right)| Arc::ptr_eq(left, right))
        {
            return Ok(false);
        }
        *self
            .profiles
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = next;
        self.affinity_profiles
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        Ok(true)
    }

    fn affinity_profile_map(
        &self,
        affinity: &crate::AffinityStore,
    ) -> std::sync::RwLockReadGuard<'_, HashMap<String, String>> {
        let needs_build = self
            .affinity_profiles
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty();
        if needs_build {
            // Keep the roster read guard until the reverse index is published. Reload takes the
            // write side first and clears the index afterwards, so either this complete old map is
            // cleared by reload or the complete new generation is built—never a stale map after a
            // newer roster won the race.
            let profiles = self
                .profiles
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let next = profiles
                .iter()
                .map(|profile| (affinity.home_id(profile.id()), profile.id().to_string()))
                .collect();
            let mut cache = self
                .affinity_profiles
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cache.is_empty() {
                *cache = next;
            }
        }
        self.affinity_profiles
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[cfg(test)]
    pub(crate) fn select(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
        generation: bool,
    ) -> Option<GeminiLease> {
        self.select_routed(
            model_id,
            excluded,
            preferred_id,
            &HashSet::new(),
            false,
            generation,
        )
    }

    pub(crate) fn select_routed(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
        warm_profile_ids: &HashSet<String>,
        place_cache_root: bool,
        generation: bool,
    ) -> Option<GeminiLease> {
        self.select_routed_inner(
            model_id,
            excluded,
            preferred_id,
            warm_profile_ids,
            place_cache_root,
            generation,
            false,
        )
    }

    /// Last-resort selection that ignores the soft, environment-derived cooling axis.
    ///
    /// Environment cooling is our inference, not Google reporting exhausted capacity, so it must
    /// steer routing without ever being the reason a customer request finds nothing. The data path
    /// calls this only after the normal selection came back empty; hard quota cooling is still
    /// honoured, so a genuinely rate-limited pool still answers 429 instead of burning a turn.
    pub(crate) fn select_routed_ignoring_env_cooling(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
        warm_profile_ids: &HashSet<String>,
        place_cache_root: bool,
        generation: bool,
    ) -> Option<GeminiLease> {
        self.select_routed_inner(
            model_id,
            excluded,
            preferred_id,
            warm_profile_ids,
            place_cache_root,
            generation,
            true,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn select_routed_inner(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
        warm_profile_ids: &HashSet<String>,
        place_cache_root: bool,
        generation: bool,
        ignore_env_cooling: bool,
    ) -> Option<GeminiLease> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let profiles = self.routable_profiles();
        let now = pool::now();
        let quota_stale_secs = self
            .cfg
            .health_probe_interval_secs
            .saturating_mul(2)
            .clamp(60, 3_600) as i64;
        let mut candidates = profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| {
                let cooling = if ignore_env_cooling {
                    profile.hard_cooling_until_for(model_id, &self.cfg, now, generation)
                } else {
                    profile.cooling_until_for(model_id, &self.cfg, now, generation)
                };
                !excluded.contains(profile.id())
                    && cooling <= now
                    && profile.authenticated.load(Ordering::Acquire)
            })
            .map(|(index, profile)| {
                let (snapshot_stale, remaining) = if generation {
                    profile.quota_steering(model_id, &self.cfg, now, quota_stale_secs)
                } else {
                    (false, None)
                };
                let protected = remaining.is_none_or(|fraction| {
                    fraction > profile.quota_reserve_for(model_id, &self.cfg)
                });
                (
                    index,
                    profile.clone(),
                    snapshot_stale,
                    quota_rank(remaining),
                    protected,
                )
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return None;
        }
        // Preserve a configurable reserve only while at least one profile has safe headroom. If
        // every profile is below reserve, retain the single-profile service floor and use what is
        // left instead of inventing an outage before Google reports an explicit zero.
        if candidates.iter().any(|candidate| candidate.4) {
            candidates.retain(|candidate| candidate.4);
        }
        let offset = self.cursor.fetch_add(1, Ordering::Relaxed) as usize;
        candidates.sort_by(|left, right| {
            let left_preferred = usize::from(preferred_id == Some(left.1.id()));
            let right_preferred = usize::from(preferred_id == Some(right.1.id()));
            right_preferred
                .cmp(&left_preferred)
                // Resolved conversation affinity remains the hard first choice. For unbound work,
                // fresh evidence wins, then the live load envelope, then only a coarse near-wall
                // quota rank. Exact fraction-first sorting caused one slightly emptier profile to
                // absorb every concurrent request until its quota caught up.
                .then_with(|| left.2.cmp(&right.2))
                .then_with(|| {
                    left.1
                        .inflight
                        .load(Ordering::Acquire)
                        .cmp(&right.1.inflight.load(Ordering::Acquire))
                })
                .then_with(|| left.3.cmp(&right.3))
                .then_with(|| {
                    let len = profiles.len();
                    let left_order = (left.0 + len - offset % len) % len;
                    let right_order = (right.0 + len - offset % len) % len;
                    left_order.cmp(&right_order)
                })
        });

        // A new shared cache root seeds two profiles before reusing warmth. This keeps independent
        // sessions from collapsing onto the first subscription while still preserving cache reuse
        // once two competitive copies exist. Conversation affinity above remains stronger.
        let preferred_is_routable = preferred_id
            .is_some_and(|id| candidates.iter().any(|candidate| candidate.1.id() == id));
        let mut ordered = Vec::with_capacity(candidates.len());
        if !preferred_is_routable && place_cache_root {
            let warm_count = candidates
                .iter()
                .filter(|candidate| warm_profile_ids.contains(candidate.1.id()))
                .count();
            let primary = if warm_count < CACHE_ROOT_MIN_WARM_PROFILES {
                candidates
                    .iter()
                    .position(|candidate| !warm_profile_ids.contains(candidate.1.id()))
            } else {
                candidates
                    .iter()
                    .position(|candidate| warm_profile_ids.contains(candidate.1.id()))
            }
            .unwrap_or(0);
            ordered.push(candidates[primary].1.clone());
            for candidate in &candidates {
                if warm_profile_ids.contains(candidate.1.id())
                    && !ordered
                        .iter()
                        .any(|profile: &Arc<GeminiProfile>| Arc::ptr_eq(profile, &candidate.1))
                {
                    ordered.push(candidate.1.clone());
                }
            }
        }
        for candidate in candidates {
            if !ordered
                .iter()
                .any(|profile: &Arc<GeminiProfile>| Arc::ptr_eq(profile, &candidate.1))
            {
                ordered.push(candidate.1);
            }
        }
        let profile = ordered.into_iter().next()?;
        profile.inflight.fetch_add(1, Ordering::AcqRel);
        Some(GeminiLease { profile })
    }

    /// Route one forwarding-admin calibration request to exactly one opaque Gemini profile id.
    /// Soft reserve and normal load balancing are bypassed, but auth death, model cooling, explicit
    /// provider zero and shutdown remain hard gates. A failed target never spills to another id.
    pub(crate) fn select_operator_target(
        &self,
        model_id: &str,
        target_profile_id: &str,
        excluded: &HashSet<String>,
        generation: bool,
    ) -> Option<GeminiLease> {
        if self.shutting_down.load(Ordering::Acquire) || excluded.contains(target_profile_id) {
            return None;
        }
        let now = pool::now();
        let profiles = self.routable_profiles();
        let mut matches = profiles
            .into_iter()
            .filter(|profile| profile.id() == target_profile_id);
        let profile = matches.next()?;
        if matches.next().is_some()
            || !profile.authenticated.load(Ordering::Acquire)
            || profile.cooling_until_for(model_id, &self.cfg, now, generation) > now
        {
            return None;
        }
        profile.inflight.fetch_add(1, Ordering::AcqRel);
        Some(GeminiLease { profile })
    }

    pub(crate) fn profile_id_for_home(
        &self,
        affinity: &crate::AffinityStore,
        home: &str,
    ) -> Option<String> {
        self.affinity_profile_map(affinity).get(home).cloned()
    }

    pub(crate) fn profile_ids_for_homes(
        &self,
        affinity: &crate::AffinityStore,
        homes: &[String],
    ) -> HashSet<String> {
        if homes.is_empty() {
            return HashSet::new();
        }
        let profiles = self.affinity_profile_map(affinity);
        homes
            .iter()
            .filter_map(|home| profiles.get(home).cloned())
            .collect()
    }

    pub(crate) fn soonest_ready(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        generation: bool,
    ) -> Option<i64> {
        let now = pool::now();
        self.routable_profiles()
            .iter()
            .filter(|profile| {
                !excluded.contains(profile.id()) && profile.authenticated.load(Ordering::Acquire)
            })
            .filter_map(|profile| {
                let until = profile.cooling_until_for(model_id, &self.cfg, now, generation);
                if until > now {
                    Some(until)
                } else {
                    None
                }
            })
            .min()
    }

    pub(crate) fn has_authenticated_profiles(&self) -> bool {
        self.routable_profiles()
            .iter()
            .any(|profile| profile.authenticated.load(Ordering::Acquire))
    }

    pub(crate) fn track_background_task(&self) -> anyhow::Result<ActiveTaskGuard> {
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("Gemini provider is shutting down");
        }
        self.background_tasks
            .track()
            .ok_or_else(|| anyhow::anyhow!("Gemini provider is shutting down"))
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
        // Reuse one roster generation for the whole document. A concurrent atomic Auth Bot
        // publication must appear wholly before or wholly after this snapshot, never as profile
        // rows from one generation and model aggregates from another.
        let snapshot = self.profiles_snapshot();
        let disabled = self.disabled_snapshot();
        let hidden = self
            .hidden
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let profiles: Vec<_> = snapshot
            .iter()
            .map(|profile| {
                profile.status(
                    &self.cfg,
                    now,
                    disabled.contains(profile.id()),
                    hidden.contains(profile.id()),
                )
            })
            .collect();
        // Rows above list every profile so an operator can see and undo a disable. Aggregates
        // below must not: a disabled profile is not capacity, and counting it would report
        // readiness the pool will never actually route to.
        let snapshot: Vec<_> = snapshot
            .into_iter()
            .filter(|profile| !disabled.contains(profile.id()))
            .collect();
        let models = self
            .cfg
            .models
            .iter()
            .map(|model| {
                let wire_model_id = model.default_wire_model_id();
                let ready = snapshot
                    .iter()
                    .filter(|profile| profile.authenticated.load(Ordering::Acquire))
                    .map(|profile| profile.cooling_until_for(wire_model_id, &self.cfg, now, true))
                    .collect::<Vec<_>>();
                let health = snapshot
                    .iter()
                    .filter(|profile| profile.authenticated.load(Ordering::Acquire))
                    .map(|profile| profile.model_health_for(wire_model_id))
                    .collect::<Vec<_>>();
                GeminiModelStatus {
                    id: model.id.clone(),
                    available: ready.iter().filter(|until| **until <= now).count(),
                    healthy: health
                        .iter()
                        .filter(|state| {
                            state.last_success_at > 0
                                && state.last_success_at >= state.last_failure_at
                                && state.failure_streak == 0
                        })
                        .count(),
                    degraded: health
                        .iter()
                        .filter(|state| state.failure_streak > 0)
                        .count(),
                    unknown: health
                        .iter()
                        .filter(|state| state.last_success_at == 0 && state.last_failure_at == 0)
                        .count(),
                    soonest_ready: ready.into_iter().filter(|until| *until > now).min(),
                }
            })
            .collect::<Vec<_>>();
        GeminiOperationalStatus {
            available: profiles
                .iter()
                .filter(|profile| {
                    !profile.disabled
                        && profile.authenticated
                        && self.cfg.models.iter().any(|model| {
                            profile
                                .model_cooling
                                .iter()
                                .find(|cooling| cooling.model_id == model.id)
                                .is_none_or(|cooling| cooling.cooling_until <= now)
                                && profile.cooling_until <= now
                        })
                })
                .count(),
            authenticated: profiles
                .iter()
                .filter(|profile| !profile.disabled && profile.authenticated)
                .count(),
            soonest_ready: models.iter().filter_map(|model| model.soonest_ready).min(),
            models,
            profiles,
        }
    }

    pub async fn preflight(&self) -> anyhow::Result<()> {
        if self.profiles_snapshot().is_empty() {
            elog::warn("gemini-pool", "Gemini OAuth provider starting with an empty encrypted roster");
            return Ok(());
        }
        // Load the operator disables before the first probe, so a revoked credential that was
        // already pulled is never re-authenticated on boot.
        self.refresh_disabled().await;
        let profile_count = self.routable_profiles().len();
        if profile_count == 0 {
            // Every profile is disabled. That is a deliberate operator state, not a broken slot:
            // failing preflight here would make the switch able to prevent the slot from starting.
            elog::warn(
                "gemini-pool",
                "Gemini OAuth provider starting with every profile disabled by an operator",
            );
            return Ok(());
        }
        let healthy = self.probe_profiles().await;
        if healthy == 0 {
            bail!("Gemini provider preflight found no authenticated subscription");
        }
        if healthy < profile_count {
            elog::warn(
                "gemini-pool",
                format!(
                    "Gemini provider starting with {healthy}/{} authenticated profiles",
                    profile_count
                ),
            );
        }
        Ok(())
    }

    pub async fn probe_health(&self) {
        if !self.shutting_down.load(Ordering::Acquire) {
            if let Err(error) = self.reload_profiles() {
                // The message is deliberately generic. Parser/decryption errors may otherwise
                // reveal a deployment path or key id through journalctl.
                let _ = error;
                elog::warn("gemini-pool", "Gemini encrypted roster reload skipped");
            }
            // Re-read the operator switch on the same tick as the roster, so pressing the button
            // takes effect within one health interval instead of needing a slot restart.
            self.refresh_disabled().await;
            self.probe_profiles().await;
        }
    }

    #[cfg(test)]
    pub(crate) fn active_background_tasks(&self) -> usize {
        self.background_tasks.active()
    }

    pub async fn refresh_profiles(&self) -> bool {
        if self.shutting_down.load(Ordering::Acquire) {
            return false;
        }
        match self.reload_profiles() {
            Ok(true) => {
                self.probe_profiles().await;
                true
            }
            Ok(false) => false,
            Err(_) => {
                elog::warn("gemini-pool", "Gemini encrypted roster refresh skipped");
                false
            }
        }
    }

    async fn probe_profiles(&self) -> usize {
        let mut healthy = 0usize;
        let now = pool::now();
        self.last_sweep_at.store(now, Ordering::Release);
        // Disabled profiles are not probed at all. Probing refreshes the OAuth token, so a
        // credential the provider has revoked would otherwise be re-attempted on every sweep
        // forever — the exact churn this switch exists to stop.
        let profiles = self.routable_profiles();
        if profiles.is_empty() {
            return 0;
        }
        let probes = futures_util::stream::iter(profiles.iter().cloned().map(|profile| {
            let cfg = self.cfg.clone();
            async move {
                let result = profile.probe(&cfg).await;
                (profile, result)
            }
        }))
        .buffer_unordered(HEALTH_PROBE_CONCURRENCY.min(profiles.len()));
        tokio::pin!(probes);
        while let Some((profile, result)) = probes.next().await {
            match result {
                ProbeResult::Healthy => {
                    profile.mark_authenticated();
                    healthy += 1;
                }
                ProbeResult::RateLimited { headers, response } => {
                    // Google itself reported exhaustion: the hard axis, which may deny a request.
                    profile.authenticated.store(true, Ordering::Release);
                    profile.cool_quota_until(now + self.cfg.default_rate_limit_cool_secs);
                    healthy += 1;
                    let profile_id = profile.id().to_string();
                    let oauth_kind = profile.oauth_kind();
                    let applied_cool_secs = self.cfg.default_rate_limit_cool_secs;
                    let background = self.background_tasks.track();
                    // Cooling above is synchronous and unchanged. Diagnostic body collection runs
                    // afterwards under the normal shutdown barrier and auxiliary transport timeout.
                    if let Some(background) = background {
                        tokio::spawn(async move {
                            let _background = background;
                            let body = response.bytes_limited(64 * 1024).await.ok();
                            emit_probe_rate_limit_diagnostic(
                                &profile_id,
                                oauth_kind,
                                applied_cool_secs,
                                &headers,
                                body.as_deref().unwrap_or_default(),
                            );
                        });
                    }
                }
                ProbeResult::Invalid => {
                    profile.mark_auth_failed(now + self.cfg.auth_quarantine_secs);
                }
                // Окружение отклонило запрос, а не Google — токен. Профиль остаётся
                // аутентифицированным (панель не врёт «ошибка auth») и остывает по мягкой оси с
                // нарастающей паузой, но никогда не перестаёт быть последней доступной ёмкостью.
                ProbeResult::Blocked => {
                    profile.mark_auth_blocked(&self.cfg);
                    healthy += 1;
                }
                ProbeResult::Temporary => {
                    profile.cool_until(now + self.cfg.transport_cool_secs);
                }
            }
        }
        healthy
    }

    pub async fn shutdown_until(&self, deadline: Option<tokio::time::Instant>) {
        self.shutting_down.store(true, Ordering::Release);
        self.background_tasks.close();
        match deadline {
            Some(deadline) => {
                if tokio::time::timeout_at(deadline, self.background_tasks.wait_idle())
                    .await
                    .is_err()
                {
                    self.abort_active_streams();
                    self.background_tasks.wait_idle().await;
                }
            }
            None => self.background_tasks.wait_idle().await,
        }
        self.abort_active_streams();
        let profiles = self.profiles_snapshot();
        futures_util::stream::iter(profiles)
            .for_each_concurrent(HEALTH_PROBE_CONCURRENCY, |profile| async move {
                profile.transport.shutdown().await;
            })
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gemini_credential::{encode_envelope, CredentialKeyring};
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    /// Мёртвым credential объявляет только Google, и только словом `invalid_grant`. Всё остальное —
    /// окружение: отклонение по репутации IP прокси не имеет права выводить живую оплаченную
    /// подписку из ротации, а раньше 400/401/403 схлопывались в один вердикт.
    #[test]
    fn only_an_invalid_grant_marks_a_gemini_credential_dead() {
        assert!(matches!(
            classify_refresh_failure(400, Some("invalid_grant")),
            TokenError::Invalid
        ));
        for status in [401, 403] {
            assert!(
                matches!(classify_refresh_failure(status, None), TokenError::Blocked),
                "{status}"
            );
            assert!(
                matches!(
                    classify_refresh_failure(status, Some("access_denied")),
                    TokenError::Blocked
                ),
                "{status}"
            );
        }
        // 400 без `invalid_grant` — протокольная неисправность, а не отзыв.
        assert!(matches!(
            classify_refresh_failure(400, Some("invalid_request")),
            TokenError::Temporary
        ));
        assert!(matches!(
            classify_refresh_failure(400, None),
            TokenError::Temporary
        ));
        for status in [429, 500, 502, 503] {
            assert!(
                matches!(
                    classify_refresh_failure(status, None),
                    TokenError::Temporary
                ),
                "{status}"
            );
        }
    }

    fn fixture() -> (PathBuf, CredentialKeyring) {
        let mut random = [0u8; 8];
        getrandom::fill(&mut random).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "gemini-oauth-pool-{}-{}",
            std::process::id(),
            u64::from_le_bytes(random)
        ));
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let ring = CredentialKeyring::parse(&format!("current:{}", "66".repeat(32))).unwrap();
        (dir, ring)
    }

    fn credential(subject: &str, proxy: &str) -> GeminiCredential {
        GeminiCredential {
            version: 1,
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at: i64::MAX / 2,
            oauth_client_id: gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_ID.into(),
            oauth_client_secret: gemini_credential::GEMINI_OFFICIAL_OAUTH_CLIENT_SECRET.into(),
            token_uri: gemini_credential::GEMINI_OFFICIAL_TOKEN_URI.into(),
            subject: subject.into(),
            email: "owner@example.com".into(),
            project_id: "managed-project".into(),
            tier_id: "paid-tier".into(),
            tier_name: "Google AI Pro".into(),
            plan: "google_ai_pro".into(),
            proxy: proxy.into(),
            proxy_order_id: 0,
            issued_at: 1,
        }
    }

    #[test]
    fn operator_email_hint_never_exposes_the_full_google_identity() {
        let masked = mask_gemini_email("owner.account@example.com");
        assert_eq!(masked, "owne…");
        assert!(!masked.contains('@'));
        assert!(!masked.contains("example.com"));
    }

    fn write_credential(dir: &Path, ring: &CredentialKeyring, id: &str, subject: &str) -> PathBuf {
        write_credential_with_proxy(dir, ring, id, subject, "")
    }

    fn write_antigravity_credential(
        dir: &Path,
        ring: &CredentialKeyring,
        id: &str,
        subject: &str,
    ) -> PathBuf {
        let credential_dir = dir.join("credentials");
        fs::create_dir_all(&credential_dir).unwrap();
        fs::set_permissions(&credential_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = credential_dir.join(format!("{id}.json"));
        let mut credential = credential(subject, "");
        credential.oauth_client_id = gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_ID.into();
        credential.oauth_client_secret = gemini_credential::ANTIGRAVITY_OAUTH_CLIENT_SECRET.into();
        let envelope = ring.seal("current", id, &credential).unwrap();
        fs::write(&path, encode_envelope(&envelope).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn write_credential_with_proxy(
        dir: &Path,
        ring: &CredentialKeyring,
        id: &str,
        subject: &str,
        proxy: &str,
    ) -> PathBuf {
        write_credential_with_proxy_order(dir, ring, id, subject, proxy, 0)
    }

    fn write_credential_with_proxy_order(
        dir: &Path,
        ring: &CredentialKeyring,
        id: &str,
        subject: &str,
        proxy: &str,
        proxy_order_id: i64,
    ) -> PathBuf {
        let credential_dir = dir.join("credentials");
        fs::create_dir_all(&credential_dir).unwrap();
        fs::set_permissions(&credential_dir, fs::Permissions::from_mode(0o700)).unwrap();
        let path = credential_dir.join(format!("{id}.json"));
        let mut credential = credential(subject, proxy);
        credential.proxy_order_id = proxy_order_id;
        let envelope = ring.seal("current", id, &credential).unwrap();
        fs::write(&path, encode_envelope(&envelope).unwrap()).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        path
    }

    fn config(profiles_file: &Path, ring: CredentialKeyring) -> GeminiConfig {
        GeminiConfig {
            enabled: true,
            upstream: "http://127.0.0.1:1".to_string(),
            profiles_file: profiles_file.to_string_lossy().into_owned(),
            credential_keys: ring,
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
                    image_output: 0,
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
            generation_idle_timeout_secs: 5,
            max_transport_retries: 1,
            auth_quarantine_secs: 900,
            auth_blocked_cool_secs: 15,
            min_probe_interval_secs: 15,
            transport_cool_secs: 5,
            model_failure_cool_secs: 15,
            model_failure_max_cool_secs: 900,
            default_rate_limit_cool_secs: 60,
            quota_reserve_fraction: 0.05,
            quota_reserve_jitter: 0.01,
            health_probe_interval_secs: 60,
            reserve_overhead_tokens: 10,
            antigravity_version: gemini_credential::ANTIGRAVITY_VERSION.to_string(),
            node_binary: "/usr/bin/node".to_string(),
            node_version: "v24.18.0".to_string(),
            node_sha256: "0".repeat(64),
        }
    }

    fn write_roster(path: &Path, profiles: &[(&str, &Path)]) {
        let profiles = profiles
            .iter()
            .map(|(id, credential)| {
                json!({"id": id, "credential_file": credential.to_string_lossy()})
            })
            .collect::<Vec<_>>();
        fs::write(path, json!({"profiles": profiles}).to_string()).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }

    fn set_model_quota(profile: &GeminiProfile, updated_at: i64, remaining_fraction: f64) {
        *profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at,
            buckets: vec![GeminiQuotaBucketStatus {
                model_id: "gemini-test".to_string(),
                remaining_amount: None,
                remaining_fraction: Some(remaining_fraction),
                reset_time: None,
                token_type: Some("antigravity_model".to_string()),
            }],
        };
    }

    #[test]
    fn duplicate_accounts_are_rejected_even_with_different_projects_or_files() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "one", "same-subject");
        let second = write_credential(&dir, &ring, "two", "same-subject");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini subscription"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_profile_proxies_are_rejected_before_joining_rotation() {
        let (dir, ring) = fixture();
        let proxy = "http://user:pass@127.0.0.1:8080/";
        let first = write_credential_with_proxy(&dir, &ring, "one", "subject-one", proxy);
        let second = write_credential_with_proxy(&dir, &ring, "two", "subject-two", proxy);
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini profile proxy"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn semantically_duplicate_profile_proxies_are_rejected_after_normalization() {
        let (dir, ring) = fixture();
        let first = write_credential_with_proxy(
            &dir,
            &ring,
            "one",
            "subject-one",
            "http://user:pass@127.0.0.1/",
        );
        let second = write_credential_with_proxy(
            &dir,
            &ring,
            "two",
            "subject-two",
            "http://user:pass@127.0.0.1:80",
        );
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini profile proxy"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn symlinked_credential_directory_is_rejected_before_loading_profiles() {
        let (dir, ring) = fixture();
        let target = dir.join("credential-target");
        fs::create_dir(&target).unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o700)).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("credentials")).unwrap();
        let credential = write_credential(&dir, &ring, "one", "subject-one");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &credential)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error
            .to_string()
            .contains("directory must be a real non-symlink directory"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn duplicate_iproyal_orders_are_rejected_before_joining_rotation() {
        let (dir, ring) = fixture();
        let first = write_credential_with_proxy_order(
            &dir,
            &ring,
            "one",
            "subject-one",
            "http://user:pass@127.0.0.1:8080/",
            42,
        );
        let second = write_credential_with_proxy_order(
            &dir,
            &ring,
            "two",
            "subject-two",
            "http://user:pass@127.0.0.2:8080/",
            42,
        );
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &first), ("two", &second)]);
        let error = GeminiGateway::new(config(&roster, ring)).err().unwrap();
        assert!(error.to_string().contains("duplicate Gemini IPRoyal order"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn production_profile_without_a_dedicated_proxy_fails_closed() {
        let (dir, ring) = fixture();
        let credential = write_credential(&dir, &ring, "one", "subject-one");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("one", &credential)]);
        let mut cfg = config(&roster, ring);
        cfg.upstream = "https://cloudcode-pa.googleapis.com".into();
        let loaded = load_profiles(&cfg).unwrap().pop().unwrap();
        let error = GeminiProfile::new(loaded, &cfg, None).err().unwrap();
        assert!(error.to_string().contains("requires a dedicated proxy"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn antigravity_and_legacy_credentials_keep_distinct_pinned_wire_profiles() {
        let (dir, ring) = fixture();
        let roster = dir.join("profiles.json");
        let mut cfg = config(&roster, ring);
        cfg.upstream = "https://daily-cloudcode-pa.sandbox.googleapis.com".into();

        assert_eq!(
            cfg.upstream_for(OAuthKind::Antigravity),
            "https://daily-cloudcode-pa.sandbox.googleapis.com"
        );
        assert_eq!(
            cfg.upstream_for(OAuthKind::LegacyGeminiCli),
            super::super::config::LEGACY_GEMINI_UPSTREAM
        );
        assert_eq!(
            cfg.generation_upstream_for(OAuthKind::Antigravity, false, "gemini-3.6-flash-medium"),
            "https://daily-cloudcode-pa.sandbox.googleapis.com"
        );
        assert_eq!(
            cfg.generation_upstream_for(OAuthKind::Antigravity, true, "gemini-3.1-flash-image"),
            super::super::config::ANTIGRAVITY_MEDIA_UPSTREAM
        );
        assert_eq!(
            cfg.generation_upstream_for(OAuthKind::LegacyGeminiCli, true, "gemini-3.1-flash-image"),
            super::super::config::LEGACY_GEMINI_UPSTREAM
        );
        assert_eq!(
            cfg.generation_upstream_for(OAuthKind::Antigravity, false, "gemini-3-flash"),
            "https://daily-cloudcode-pa.sandbox.googleapis.com"
        );
        assert_eq!(
            cfg.user_agent(OAuthKind::Antigravity, "gemini-test"),
            "antigravity/hub/2.2.1 darwin/arm64"
        );
        assert_eq!(
            cfg.user_agent(OAuthKind::Antigravity, "gemini-3-flash"),
            "antigravity/hub/2.2.1 darwin/arm64"
        );
        assert!(cfg
            .user_agent(OAuthKind::LegacyGeminiCli, "gemini-test")
            .starts_with("GeminiCLI/0.53.0/gemini-test "));
        assert_eq!(
            cfg.refresh_user_agent(OAuthKind::Antigravity),
            "Go-http-client/2.0"
        );
        assert_eq!(
            cfg.refresh_user_agent(OAuthKind::LegacyGeminiCli),
            "google-api-nodejs-client/10.9.0"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn cooling_profile_is_skipped_and_identity_is_not_in_status() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "secret-subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "secret-subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        gateway.profiles_snapshot()[0].cool_until(pool::now() + 60);
        let lease = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(lease.profile().id(), "profile_b");
        let status = format!("{:?}", gateway.operational_status().await);
        assert!(!status.contains("secret-subject"));
        assert!(!status.contains("owner@example.com"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quota_zero_blocks_only_its_model_until_the_official_reset() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let mut cfg = config(&roster, ring);
        cfg.models.push(super::super::config::GeminiModel {
            id: "gemini-other".to_string(),
            display_name: "Gemini Other".to_string(),
            ..cfg.models[0].clone()
        });
        let gateway = GeminiGateway::new(cfg).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        *profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at: pool::now(),
            buckets: vec![GeminiQuotaBucketStatus {
                model_id: "gemini-test".to_string(),
                remaining_amount: Some(0),
                remaining_fraction: Some(0.0),
                reset_time: Some("2099-01-01T00:00:00Z".to_string()),
                token_type: Some("REQUESTS".to_string()),
            }],
        };
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .is_none());
        assert!(gateway
            .select("gemini-other", &HashSet::new(), None, true)
            .is_none());
        // A fresh official catalogue without gemini-other is negative availability evidence. Once
        // stale, the generation endpoint is allowed to refresh that evidence instead of wedging.
        profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .updated_at = pool::now() - 10_000;
        assert!(gateway
            .select("gemini-other", &HashSet::new(), None, true)
            .is_some());
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn one_exhausted_quota_dimension_blocks_even_if_another_is_positive() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        *profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at: pool::now(),
            buckets: vec![
                GeminiQuotaBucketStatus {
                    model_id: "gemini-test".to_string(),
                    remaining_amount: Some(5),
                    remaining_fraction: Some(0.5),
                    reset_time: None,
                    token_type: Some("TOKENS".to_string()),
                },
                GeminiQuotaBucketStatus {
                    model_id: "gemini-test".to_string(),
                    remaining_amount: Some(0),
                    remaining_fraction: Some(0.0),
                    reset_time: Some("2098-01-01T00:00:00Z".to_string()),
                    token_type: Some("REQUESTS".to_string()),
                },
                GeminiQuotaBucketStatus {
                    model_id: "gemini-test".to_string(),
                    remaining_amount: Some(0),
                    remaining_fraction: None,
                    reset_time: Some("2099-01-01T00:00:00Z".to_string()),
                    token_type: Some("DAILY_REQUESTS".to_string()),
                },
            ],
        };
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .is_none());
        assert_eq!(
            profile.quota_blocked_until("gemini-test", gateway.config(), pool::now(), 600),
            parse_rfc3339_seconds("2099-01-01T00:00:00Z").unwrap(),
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preview_generation_joins_both_antigravity_quota_rows() {
        let (dir, ring) = fixture();
        let first = write_antigravity_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let mut cfg = config(&roster, ring);
        cfg.models[0].id = "gemini-3-flash-preview".to_string();
        let gateway = GeminiGateway::new(cfg).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        *profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at: pool::now(),
            buckets: vec![GeminiQuotaBucketStatus {
                model_id: "gemini-3-flash-agent".to_string(),
                remaining_amount: Some(1),
                remaining_fraction: Some(0.5),
                reset_time: None,
                token_type: Some("REQUESTS".to_string()),
            }],
        };
        assert!(gateway
            .select("gemini-3-flash", &HashSet::new(), None, true,)
            .is_some());

        profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .buckets[0]
            .remaining_amount = Some(0);
        profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .buckets[0]
            .remaining_fraction = Some(0.0);
        assert!(gateway
            .select("gemini-3-flash", &HashSet::new(), None, true,)
            .is_none());

        let mut quota = profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        quota.buckets[0].remaining_amount = Some(1);
        quota.buckets[0].remaining_fraction = Some(0.5);
        quota.buckets.push(GeminiQuotaBucketStatus {
            model_id: "gemini-3-flash".to_string(),
            remaining_amount: Some(0),
            remaining_fraction: Some(0.0),
            reset_time: None,
            token_type: Some("REQUESTS".to_string()),
        });
        drop(quota);
        assert!(gateway
            .select("gemini-3-flash", &HashSet::new(), None, true,)
            .is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn model_backend_failure_is_exponential_and_does_not_disable_count_tokens_or_other_models(
    ) {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let mut cfg = config(&roster, ring);
        cfg.models.push(super::super::config::GeminiModel {
            id: "gemini-other".to_string(),
            display_name: "Gemini Other".to_string(),
            ..cfg.models[0].clone()
        });
        let gateway = GeminiGateway::new(cfg).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        profile.mark_model_failure("gemini-test", "backend", gateway.config());
        let first_health = profile.model_health_for("gemini-test");
        profile.mark_model_failure("gemini-test", "backend", gateway.config());
        let second_health = profile.model_health_for("gemini-test");
        assert_eq!(second_health.failure_streak, 2);
        assert!(second_health.cooling_until > first_health.cooling_until);
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .is_none());
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, false)
            .is_some());
        assert!(gateway
            .select("gemini-other", &HashSet::new(), None, true)
            .is_some());
        let status = gateway.operational_status().await;
        let failed = status.profiles[0]
            .model_cooling
            .iter()
            .find(|model| model.model_id == "gemini-test")
            .unwrap();
        assert_eq!(failed.failure_streak, 2);
        assert_eq!(failed.last_failure_class.as_deref(), Some("backend"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn one_profile_accepts_ten_thousand_immediate_leases() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let leases = (0..10_000)
            .map(|_| {
                gateway
                    .select("gemini-test", &HashSet::new(), None, true)
                    .expect("profile load must never reject local admission")
            })
            .collect::<Vec<_>>();
        assert_eq!(
            gateway.profiles_snapshot()[0]
                .inflight
                .load(Ordering::Acquire),
            10_000
        );
        drop(leases);
        assert_eq!(
            gateway.profiles_snapshot()[0]
                .inflight
                .load(Ordering::Acquire),
            0
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unbound_bursts_spread_by_live_load_before_exact_quota_headroom() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profiles = gateway.profiles_snapshot();
        // Both profiles are far from the quota wall. Exact fraction-first sorting used to send
        // both concurrent turns to profile_a merely because 0.90 > 0.80.
        set_model_quota(&profiles[0], pool::now(), 0.90);
        set_model_quota(&profiles[1], pool::now(), 0.80);

        let first = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        let second = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(first.profile().id(), "profile_a");
        assert_eq!(second.profile().id(), "profile_b");
        drop((first, second));

        // Once load is equal again, the atomic cursor—not a tiny fraction difference—rotates the
        // next idle choice instead of permanently herding onto one profile.
        let next = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(next.profile().id(), "profile_a");
        let _ = fs::remove_dir_all(dir);
    }

    /// The operator switch has to hold every path that could put a pulled profile back to work:
    /// selection, pinned-route selection, readiness, and — the reason it exists — the probe sweep,
    /// which refreshes OAuth tokens and would otherwise retry a revoked credential forever.
    #[tokio::test]
    async fn a_disabled_profile_leaves_rotation_without_leaving_the_report() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        for profile in gateway.profiles_snapshot() {
            profile.mark_authenticated();
        }
        assert_eq!(gateway.routable_profiles().len(), 2);

        gateway
            .disabled
            .write()
            .unwrap()
            .insert("profile_a".to_string());

        // Out of every selection path.
        assert!(gateway.is_disabled("profile_a"));
        assert!(!gateway.is_disabled("profile_b"));
        let routable = gateway.routable_profiles();
        assert_eq!(routable.len(), 1);
        assert_eq!(routable[0].id(), "profile_b");
        for _ in 0..8 {
            let lease = gateway
                .select("gemini-test", &HashSet::new(), None, true)
                .unwrap();
            assert_eq!(lease.profile().id(), "profile_b");
        }
        // Even an explicit pinned route must not resurrect it.
        assert!(gateway
            .select_operator_target("gemini-test", "profile_a", &HashSet::new(), true)
            .is_none());

        // Still listed, and flagged, so the panel can offer to put it back.
        let status = gateway.operational_status().await;
        assert_eq!(status.profiles.len(), 2);
        let row = status
            .profiles
            .iter()
            .find(|profile| profile.id == "profile_a")
            .unwrap();
        assert!(row.disabled);
        // But it is not counted as capacity.
        assert_eq!(status.authenticated, 1);

        // Disabling the last one leaves no routable profile and no readiness claim.
        gateway
            .disabled
            .write()
            .unwrap()
            .insert("profile_b".to_string());
        assert!(gateway.routable_profiles().is_empty());
        assert!(!gateway.has_authenticated_profiles());
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .is_none());
        assert!(gateway
            .soonest_ready("gemini-test", &HashSet::new(), true)
            .is_none());
        assert_eq!(gateway.operational_status().await.profiles.len(), 2);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_quota_is_fail_open_but_never_beats_fresh_evidence() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profiles = gateway.profiles_snapshot();
        set_model_quota(&profiles[0], pool::now() - 10_000, 0.99);
        set_model_quota(&profiles[1], pool::now(), 0.80);

        let lease = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(lease.profile().id(), "profile_b");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn resolved_conversation_stays_sticky_while_inflight_is_only_a_load_signal() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let occupied = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(occupied.profile().id(), "profile_a");

        let sticky = gateway
            .select("gemini-test", &HashSet::new(), Some("profile_a"), true)
            .unwrap();
        assert_eq!(sticky.profile().id(), "profile_a");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn preferred_profile_stays_sticky_at_any_inflight_depth() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let occupied = (0..1_000)
            .map(|_| {
                gateway
                    .select("gemini-test", &HashSet::new(), Some("profile_a"), true)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(occupied
            .iter()
            .all(|lease| lease.profile().id() == "profile_a"));
        let next = gateway
            .select("gemini-test", &HashSet::new(), Some("profile_a"), true)
            .unwrap();
        assert_eq!(next.profile().id(), "profile_a");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn concurrent_admission_accepts_every_request_without_a_profile_cap() {
        let (dir, ring) = fixture();
        let credentials = (0..4)
            .map(|index| {
                let id = format!("profile_{index}");
                let subject = format!("subject-{index}");
                let path = write_credential(&dir, &ring, &id, &subject);
                (id, path)
            })
            .collect::<Vec<_>>();
        let roster = dir.join("profiles.json");
        let roster_entries = credentials
            .iter()
            .map(|(id, path)| (id.as_str(), path.as_path()))
            .collect::<Vec<_>>();
        write_roster(&roster, &roster_entries);
        let gateway = Arc::new(GeminiGateway::new(config(&roster, ring)).unwrap());
        let workers = 64usize;
        let start = Arc::new(std::sync::Barrier::new(workers + 1));
        let release = Arc::new(AtomicBool::new(false));
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut threads = Vec::with_capacity(workers);
        for _ in 0..workers {
            let gateway = gateway.clone();
            let start = start.clone();
            let release = release.clone();
            let sender = sender.clone();
            threads.push(std::thread::spawn(move || {
                start.wait();
                let lease = gateway.select("gemini-test", &HashSet::new(), None, true);
                sender
                    .send(lease.as_ref().map(|lease| lease.profile().id().to_string()))
                    .unwrap();
                while !release.load(Ordering::Acquire) {
                    std::thread::yield_now();
                }
                drop(lease);
            }));
        }
        drop(sender);
        start.wait();
        let mut counts = HashMap::new();
        for selected in receiver.iter().take(workers).flatten() {
            *counts.entry(selected).or_insert(0usize) += 1;
        }
        release.store(true, Ordering::Release);
        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(counts.values().sum::<usize>(), workers);
        assert_eq!(counts.len(), 4);
        assert!(counts.values().all(|count| *count > 0));
        assert!(gateway
            .profiles_snapshot()
            .iter()
            .all(|profile| profile.inflight.load(Ordering::Acquire) == 0));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn shared_cache_root_seeds_two_profiles_then_reuses_warmth() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let third = write_credential(&dir, &ring, "profile_c", "subject-c");
        let roster = dir.join("profiles.json");
        write_roster(
            &roster,
            &[
                ("profile_a", &first),
                ("profile_b", &second),
                ("profile_c", &third),
            ],
        );
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let mut warm = HashSet::new();

        let first = gateway
            .select_routed("gemini-test", &HashSet::new(), None, &warm, true, true)
            .unwrap();
        assert_eq!(first.profile().id(), "profile_a");
        warm.insert(first.profile().id().to_string());
        drop(first);

        let second = gateway
            .select_routed("gemini-test", &HashSet::new(), None, &warm, true, true)
            .unwrap();
        assert_eq!(second.profile().id(), "profile_b");
        warm.insert(second.profile().id().to_string());
        drop(second);

        // The rotating global order now points at cold profile_c. With two warm copies present,
        // cache-root placement deliberately stays on one of those copies instead.
        let reused = gateway
            .select_routed("gemini-test", &HashSet::new(), None, &warm, true, true)
            .unwrap();
        assert!(warm.contains(reused.profile().id()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn high_inflight_does_not_publish_a_fake_provider_reset() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let _leases = (0..1_000)
            .map(|_| {
                gateway
                    .select("gemini-test", &HashSet::new(), None, true)
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert!(gateway
            .soonest_ready("gemini-test", &HashSet::new(), true)
            .is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn operator_target_uses_exact_opaque_id_and_never_spills() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();

        let lease = gateway
            .select_operator_target("gemini-test", "profile_b", &HashSet::new(), true)
            .unwrap();
        assert_eq!(lease.profile().id(), "profile_b");
        drop(lease);

        let excluded = HashSet::from(["profile_b".to_owned()]);
        assert!(gateway
            .select_operator_target("gemini-test", "profile_b", &excluded, true)
            .is_none());
        assert!(gateway
            .select_operator_target("gemini-test", "missing", &HashSet::new(), true)
            .is_none());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn verified_success_clears_global_quarantine_but_not_model_cooling() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        let now = pool::now();
        profile.mark_auth_failed(now + 600);
        profile.cool_model_until("gemini-test", now + 300);

        profile.mark_authenticated();

        assert!(profile.authenticated.load(Ordering::Acquire));
        assert_eq!(profile.cooling_until.load(Ordering::Acquire), 0);
        assert!(profile.cooling_until_for("gemini-test", gateway.config(), now, true) > now);
        assert_eq!(
            profile.cooling_until_for("gemini-test", gateway.config(), now, false),
            0
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// Живой инцидент 2026-08-05/06: девять раз за двое суток весь пул Gemini уходил в нулевую
    /// ёмкость на 105–300 секунд, и клиентам шёл `503 gemini_profiles_unauthenticated`. Токены при
    /// этом были живы — следующий health-свип возвращал ровно те же профили здоровыми. Убивал их
    /// наш собственный код: upstream 401/403 на generation трактовался как смерть credential, а
    /// вызов стоял внутри retry-цикла, поэтому ОДИН клиентский запрос проходил по всему ростеру.
    #[test]
    fn generation_auth_rejection_backs_off_without_deauthenticating_the_profile() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        let now = pool::now();

        profile.mark_auth_blocked(gateway.config());

        assert!(
            profile.authenticated.load(Ordering::Acquire),
            "401/403 на generation — это окружение, а не отзыв credential"
        );
        assert!(
            gateway.has_authenticated_profiles(),
            "пул обязан сохранять ёмкость: иначе весь трафик получает 503"
        );
        let cooled = profile.cooling_until.load(Ordering::Acquire);
        assert!(cooled > now && cooled <= now + gateway.config().auth_blocked_cool_secs);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn repeated_auth_rejections_escalate_and_cap_at_the_quarantine_ceiling() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profile = &gateway.profiles_snapshot()[0];
        let cfg = gateway.config();

        profile.mark_auth_blocked(cfg);
        let first_delay = profile.cooling_until.load(Ordering::Acquire) - pool::now();
        profile.mark_auth_blocked(cfg);
        let second_delay = profile.cooling_until.load(Ordering::Acquire) - pool::now();
        assert!(
            second_delay > first_delay,
            "повторный отказ должен удлинять паузу, иначе долбим заблокированный путь"
        );

        for _ in 0..40 {
            profile.mark_auth_blocked(cfg);
        }
        let capped = profile.cooling_until.load(Ordering::Acquire) - pool::now();
        assert!(capped <= cfg.auth_quarantine_secs);
        assert!(
            profile.authenticated.load(Ordering::Acquire),
            "сколько бы ни было отказов, средовая ось не снимает профиль с учёта"
        );

        profile.mark_authenticated();
        profile.mark_auth_blocked(cfg);
        assert_eq!(
            profile.cooling_until.load(Ordering::Acquire) - pool::now(),
            first_delay,
            "доказанный успех обнуляет streak"
        );
        let _ = fs::remove_dir_all(dir);
    }

    /// Подписка вправе отдыхать по реальным лимитам провайдера и не вправе — по нашей догадке о
    /// среде. Когда вся ёмкость остыла по средовой оси, запрос обязан дойти до попытки.
    #[test]
    fn environment_cooling_never_empties_the_pool_but_quota_cooling_still_denies() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let cfg_owned = gateway.config().clone();
        for profile in gateway.profiles_snapshot() {
            profile.mark_auth_blocked(&cfg_owned);
        }

        assert!(
            gateway
                .select_routed(
                    "gemini-test",
                    &HashSet::new(),
                    None,
                    &HashSet::new(),
                    false,
                    true
                )
                .is_none(),
            "обычный выбор уважает средовое охлаждение, пока есть куда деться"
        );
        assert!(
            gateway
                .select_routed_ignoring_env_cooling(
                    "gemini-test",
                    &HashSet::new(),
                    None,
                    &HashSet::new(),
                    false,
                    true
                )
                .is_some(),
            "но полностью средовое охлаждение не имеет права обнулить пул"
        );

        let now = pool::now();
        for profile in gateway.profiles_snapshot() {
            profile.cool_quota_until(now + 600);
        }
        assert!(
            gateway
                .select_routed_ignoring_env_cooling(
                    "gemini-test",
                    &HashSet::new(),
                    None,
                    &HashSet::new(),
                    false,
                    true
                )
                .is_none(),
            "реальный лимит провайдера остаётся жёстким: это честный 429, а не попытка"
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quota_headroom_protects_a_low_profile_but_preserves_the_single_profile_floor() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first), ("profile_b", &second)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profiles = gateway.profiles_snapshot();
        for (profile, remaining) in profiles.iter().zip([0.001, 0.9]) {
            *profile
                .quota
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
                updated_at: pool::now(),
                buckets: vec![GeminiQuotaBucketStatus {
                    model_id: "gemini-test".to_string(),
                    remaining_amount: None,
                    remaining_fraction: Some(remaining),
                    reset_time: None,
                    token_type: Some("antigravity_model".to_string()),
                }],
            };
        }
        let protected = gateway
            .select("gemini-test", &HashSet::new(), Some("profile_a"), true)
            .unwrap();
        assert_eq!(protected.profile().id(), "profile_b");
        drop(protected);
        profiles[1].cool_until(pool::now() + 60);
        let floor = gateway
            .select("gemini-test", &HashSet::new(), None, true)
            .unwrap();
        assert_eq!(floor.profile().id(), "profile_a");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quarantined_auth_is_not_reported_as_retryable_quota_cooling() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        gateway.profiles_snapshot()[0].mark_auth_failed(pool::now() + 600);
        assert!(!gateway.has_authenticated_profiles());
        assert_eq!(
            gateway.soonest_ready("gemini-test", &HashSet::new(), true),
            None,
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn quota_reset_parser_handles_offsets_leap_days_and_rejects_impossible_dates() {
        assert_eq!(parse_rfc3339_seconds("1970-01-01T03:00:00+03:00"), Some(0));
        assert_eq!(
            parse_rfc3339_seconds("2000-02-29T00:00:00.125Z"),
            Some(951_782_400)
        );
        for invalid in [
            "2025-02-29T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-01-01T24:00:00Z",
            "2026-01-01T00:00:00+24:00",
        ] {
            assert_eq!(parse_rfc3339_seconds(invalid), None, "accepted {invalid}");
        }
    }

    #[test]
    fn quota_fraction_parser_is_exact_and_rejects_sub_unit_or_out_of_range_values() {
        assert_eq!(parse_fraction_units("0.9999803"), Some(99_998_030));
        assert_eq!(parse_fraction_units("9.99132e-1"), Some(99_913_200));
        assert_eq!(parse_fraction_units("1"), Some(FRACTION_SCALE));
        assert_eq!(parse_fraction_units("0.000000001"), None);
        assert_eq!(parse_fraction_units("1.00000001"), None);
        assert_eq!(parse_fraction_units("-0.1"), None);
        assert_eq!(
            parse_fraction("0.4"),
            Some(ExactFractionValue {
                units: 40_000_000,
                resolution_units: 10_000_000,
            })
        );
        assert_eq!(
            parse_fraction("0.40000000"),
            Some(ExactFractionValue {
                units: 40_000_000,
                resolution_units: 1,
            })
        );
    }

    #[test]
    fn quota_summary_shape_accepts_the_official_and_nested_envelopes() {
        for document in [
            json!({
                "groups": [{
                    "displayName": "Gemini Models",
                    "buckets": [{
                        "bucketId": "gemini-5h",
                        "remainingFraction": 0.75,
                        "resetTime": "2099-01-01T00:00:00Z"
                    }]
                }]
            }),
            json!({
                "quotaSummary": {"groups": [{"buckets": [{
                    "bucket_id": "gemini-weekly",
                    "remaining_fraction": "0.5",
                    "reset_time": "2099-01-01T00:00:00Z"
                }]}]}
            }),
        ] {
            let parsed: QuotaSummaryEnvelope = serde_json::from_value(document).unwrap();
            let groups = parsed
                .quota_summary
                .map_or(parsed.groups, |summary| summary.groups);
            assert_eq!(groups.len(), 1);
            assert_eq!(
                groups[0].buckets[0]
                    .remaining_fraction
                    .as_ref()
                    .and_then(ExactFraction::units),
                Some(
                    if groups[0].buckets[0].bucket_id.as_deref() == Some("gemini-5h") {
                        75_000_000
                    } else {
                        50_000_000
                    }
                )
            );
        }
    }

    #[test]
    fn quota_summary_preserves_numeric_wire_decimal_resolution() {
        let parsed: QuotaSummaryEnvelope = serde_json::from_str(
            r#"{"groups":[{"buckets":[{"bucketId":"gemini-5h","remainingFraction":0.40000000,"resetTime":"2099-01-01T00:00:00Z"}]}]}"#,
        )
        .unwrap();
        let value = parsed.groups[0].buckets[0]
            .remaining_fraction
            .as_ref()
            .and_then(ExactFraction::value)
            .unwrap();
        assert_eq!(value.units, 40_000_000);
        assert_eq!(value.resolution_units, 1);
    }

    #[tokio::test]
    async fn atomically_published_profile_is_reloaded_without_restart() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring.clone())).unwrap();
        let affinity =
            crate::AffinityStore::new(None, Some("test-affinity-secret"), 3_600, 60, 10).unwrap();
        let first_home = affinity.home_id("profile_a");
        assert_eq!(
            gateway
                .profile_id_for_home(&affinity, &first_home)
                .as_deref(),
            Some("profile_a")
        );
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let staging = dir.join(".profiles.pending");
        write_roster(&staging, &[("profile_a", &first), ("profile_b", &second)]);
        fs::rename(staging, &roster).unwrap();
        assert!(gateway.reload_profiles().unwrap());
        assert_eq!(gateway.operational_status().await.profiles.len(), 2);
        let second_home = affinity.home_id("profile_b");
        assert_eq!(
            gateway
                .profile_id_for_home(&affinity, &second_home)
                .as_deref(),
            Some("profile_b")
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn empty_roster_can_boot_before_the_first_subscription_arrives() {
        let (dir, ring) = fixture();
        let missing = dir.join("profiles.json");
        let gateway = GeminiGateway::new(config(&missing, ring)).unwrap();
        assert!(gateway.profiles_snapshot().is_empty());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn generation_rate_limit_evidence_captures_positive_catalogue_without_mutation() {
        let (dir, ring) = fixture();
        let credential = write_antigravity_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &credential)]);
        let gateway = GeminiGateway::new(config(&roster, ring)).unwrap();
        let profile = gateway.profiles_snapshot().pop().unwrap();
        let now = pool::now();
        *profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = GeminiQuotaSnapshot {
            updated_at: now - 3,
            buckets: vec![GeminiQuotaBucketStatus {
                model_id: "gemini-integration-model".to_string(),
                remaining_amount: None,
                remaining_fraction: Some(0.9687),
                reset_time: None,
                token_type: None,
            }],
        };
        let before = profile.quota.read().unwrap().updated_at;
        let evidence =
            profile.rate_limit_quota_evidence("gemini-integration-model", gateway.config(), now);
        assert_eq!(
            evidence.to_string(),
            "catalog_state=fresh catalog_age_secs=3 catalog_matching_buckets=1 catalog_zero_buckets=0 catalog_positive_buckets=1 catalog_unknown_buckets=0 catalog_min_remaining_bp=9687 catalog_latest_reset_in_secs=none"
        );
        assert_eq!(profile.quota.read().unwrap().updated_at, before);
        let _ = fs::remove_dir_all(dir);
    }
}
