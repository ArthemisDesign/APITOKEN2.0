//! Encrypted multi-account Gemini OAuth pool with single-flight refresh and bounded health probes.

use super::config::{GeminiConfig, GeminiProfileSpec, GeminiProfilesFile};
use super::transport::{attest_node_binary, ProfileTransport, TransportRequest, TransportResponse};
use anyhow::{bail, Context};
use futures_util::StreamExt;
use gemini_credential::{decode_envelope, GeminiCredential, OAuthKind, SecretString};
use serde::Deserialize;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use zeroize::{Zeroize, ZeroizeOnDrop};

const MAX_BACKGROUND_TASKS: u32 = 4_096;
const HEALTH_PROBE_CONCURRENCY: usize = 16;
const ACCESS_TOKEN_SKEW_SECS: i64 = 120;

#[derive(Clone, Debug)]
pub struct GeminiProfileStatus {
    pub id: String,
    pub authenticated: bool,
    pub cooling_until: i64,
    pub inflight: usize,
    pub last_probe_at: i64,
    pub quota_updated_at: i64,
    pub quotas: Vec<GeminiQuotaBucketStatus>,
    pub model_cooling: Vec<GeminiModelCoolingStatus>,
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
    pub soonest_ready: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TokenError {
    Invalid,
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
    oauth_kind: OAuthKind,
    credential: tokio::sync::Mutex<GeminiCredential>,
    transport: ProfileTransport,
    google_api_client: String,
    refresh_user_agent: String,
    inflight: AtomicUsize,
    cooling_until: AtomicI64,
    authenticated: AtomicBool,
    last_probe_at: AtomicI64,
    quota: RwLock<GeminiQuotaSnapshot>,
    model_cooling: Mutex<HashMap<String, i64>>,
}

#[derive(Default)]
struct GeminiQuotaSnapshot {
    updated_at: i64,
    buckets: Vec<GeminiQuotaBucketStatus>,
}

impl GeminiProfile {
    fn new(mut loaded: LoadedProfile, cfg: &GeminiConfig) -> anyhow::Result<Self> {
        gemini_credential::validate_profile_id(&loaded.source.id)?;
        let oauth_kind = loaded.credential.oauth_kind()?;
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
            source: loaded.source,
            fingerprint: loaded.fingerprint,
            oauth_kind,
            credential: tokio::sync::Mutex::new(loaded.credential),
            transport,
            google_api_client: cfg.google_api_client(),
            refresh_user_agent: cfg.refresh_user_agent(oauth_kind),
            inflight: AtomicUsize::new(0),
            cooling_until: AtomicI64::new(0),
            authenticated: AtomicBool::new(true),
            last_probe_at: AtomicI64::new(0),
            quota: RwLock::new(GeminiQuotaSnapshot::default()),
            model_cooling: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) fn oauth_kind(&self) -> OAuthKind {
        self.oauth_kind
    }

    fn matches(&self, loaded: &LoadedProfile) -> bool {
        self.source == loaded.source && self.fingerprint == loaded.fingerprint
    }

    pub(crate) async fn request(
        &self,
        url: &str,
        access_token: &str,
        user_agent: &str,
        accept: Option<&'static str>,
        content_type: &'static str,
        body: bytes::Bytes,
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
        if self.oauth_kind == OAuthKind::Antigravity {
            // The Antigravity Cloud Code surface requires the client-metadata header on generation
            // requests; without it streamGenerateContent is rejected with a generic
            // INVALID_ARGUMENT (generateContent is more lenient). The value is the reviewed
            // Antigravity identity (macOS/Gemini). The Node helper sorts headers, so insertion
            // order does not affect the wire fingerprint.
            headers.push((
                "client-metadata",
                SecretString::new(
                    r#"{"ideType":"ANTIGRAVITY","platform":"MACOS","pluginType":"GEMINI"}"#
                        .to_string(),
                ),
            ));
        }
        if let Some(accept) = accept {
            headers.push(("accept", SecretString::new(accept.to_string())));
        }
        self.transport
            .send(TransportRequest { url, headers, body })
            .await
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
            })
            .await
            .map_err(|_| TokenError::Temporary)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(if matches!(status, 400 | 401 | 403) {
                TokenError::Invalid
            } else {
                TokenError::Temporary
            });
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

    pub(crate) fn mark_healthy_for(&self, model_id: &str) {
        self.authenticated.store(true, Ordering::Release);
        self.cooling_until.store(0, Ordering::Release);
        self.model_cooling
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(model_id);
    }

    fn mark_authenticated(&self) {
        // A quota-free liveness probe proves the bearer but must not erase a generation 429.
        // Otherwise the next five-minute health cycle would prematurely reopen a daily/model
        // quota window whose RetryInfo is still in the future.
        self.authenticated.store(true, Ordering::Release);
    }

    pub(crate) fn cool_until(&self, until: i64) {
        self.cooling_until.fetch_max(until, Ordering::AcqRel);
    }

    pub(crate) fn cool_model_until(&self, model_id: &str, until: i64) {
        let mut cooling = self
            .model_cooling
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        cooling
            .entry(model_id.to_string())
            .and_modify(|current| *current = (*current).max(until))
            .or_insert(until);
    }

    fn cooling_until_for(&self, model_id: &str, cfg: &GeminiConfig, now: i64) -> i64 {
        let global = self.cooling_until.load(Ordering::Acquire);
        let model = self
            .model_cooling
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(model_id)
            .copied()
            .unwrap_or(0);
        global.max(model).max(
            self.quota_blocked_until(
                model_id,
                now,
                cfg.health_probe_interval_secs
                    .saturating_mul(2)
                    .clamp(60, 3_600) as i64,
            ),
        )
    }

    fn quota_blocked_until(&self, model_id: &str, now: i64, stale_secs: i64) -> i64 {
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
            .filter(|bucket| bucket.model_id == model_id)
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

    fn status(&self, cfg: &GeminiConfig, now: i64) -> GeminiProfileStatus {
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
        let mut model_cooling = self
            .model_cooling
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(model_id, cooling_until)| GeminiModelCoolingStatus {
                model_id: model_id.clone(),
                cooling_until: *cooling_until,
            })
            .collect::<Vec<_>>();
        for model in &cfg.models {
            let cooling_until = self.cooling_until_for(&model.id, cfg, now);
            if cooling_until > now {
                if let Some(existing) = model_cooling
                    .iter_mut()
                    .find(|status| status.model_id == model.id)
                {
                    existing.cooling_until = existing.cooling_until.max(cooling_until);
                } else {
                    model_cooling.push(GeminiModelCoolingStatus {
                        model_id: model.id.clone(),
                        cooling_until,
                    });
                }
            }
        }
        model_cooling.sort_by(|left, right| left.model_id.cmp(&right.model_id));
        GeminiProfileStatus {
            id: self.id.clone(),
            authenticated: self.authenticated.load(Ordering::Acquire),
            cooling_until: self.cooling_until.load(Ordering::Acquire),
            inflight: self.inflight.load(Ordering::Acquire),
            last_probe_at: self.last_probe_at.load(Ordering::Acquire),
            quota_updated_at,
            quotas,
            model_cooling,
        }
    }

    async fn probe(&self, cfg: &GeminiConfig) -> ProbeResult {
        self.last_probe_at.store(pool::now(), Ordering::Release);
        let mut token = match self.access_token(false).await {
            Ok(token) => token,
            Err(TokenError::Invalid) => return ProbeResult::Invalid,
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
                    (self.oauth_kind == OAuthKind::LegacyGeminiCli).then_some("application/json"),
                    "application/json",
                    bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
                )
                .await;
            match response {
                Ok(response) if response.status().is_success() => {
                    self.refresh_quota(cfg, &token).await;
                    return ProbeResult::Healthy;
                }
                Ok(response) if response.status().as_u16() == 429 => {
                    return ProbeResult::RateLimited
                }
                Ok(response) if response.status().as_u16() == 401 && attempt == 0 => {
                    token = match self.access_token(true).await {
                        Ok(token) => token,
                        Err(TokenError::Invalid) => return ProbeResult::Invalid,
                        Err(TokenError::Temporary) => return ProbeResult::Temporary,
                    };
                }
                Ok(response) if matches!(response.status().as_u16(), 401 | 403) => {
                    return ProbeResult::Invalid
                }
                Ok(_) | Err(_) => return ProbeResult::Temporary,
            }
        }
        ProbeResult::Invalid
    }

    async fn refresh_quota(&self, cfg: &GeminiConfig, token: &str) {
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
                (self.oauth_kind == OAuthKind::LegacyGeminiCli).then_some("application/json"),
                "application/json",
                bytes::Bytes::from(serde_json::to_vec(&body).unwrap_or_default()),
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

#[derive(Clone, Copy)]
enum ProbeResult {
    Healthy,
    RateLimited,
    Invalid,
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
    profiles: RwLock<Vec<Arc<GeminiProfile>>>,
    cursor: AtomicU64,
    shutting_down: AtomicBool,
    abort_streams: AtomicBool,
    abort_notify: tokio::sync::Notify,
    background_tasks: Arc<tokio::sync::Semaphore>,
}

impl GeminiGateway {
    pub fn new(cfg: GeminiConfig) -> anyhow::Result<Self> {
        if !cfg.enabled {
            bail!("Gemini provider is disabled");
        }
        if cfg.models.is_empty() {
            bail!("Gemini model allowlist is empty");
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
            .map(|profile| GeminiProfile::new(profile, &cfg).map(Arc::new))
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(Self {
            cfg,
            profiles: RwLock::new(profiles),
            cursor: AtomicU64::new(0),
            shutting_down: AtomicBool::new(false),
            abort_streams: AtomicBool::new(false),
            abort_notify: tokio::sync::Notify::new(),
            background_tasks: Arc::new(tokio::sync::Semaphore::new(MAX_BACKGROUND_TASKS as usize)),
        })
    }

    pub fn config(&self) -> &GeminiConfig {
        &self.cfg
    }

    fn profiles_snapshot(&self) -> Vec<Arc<GeminiProfile>> {
        self.profiles
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
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
                let profile = Arc::new(GeminiProfile::new(loaded_profile, &self.cfg)?);
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
        Ok(true)
    }

    pub(crate) fn select(
        &self,
        model_id: &str,
        excluded: &HashSet<String>,
        preferred_id: Option<&str>,
    ) -> Option<GeminiLease> {
        if self.shutting_down.load(Ordering::Acquire) {
            return None;
        }
        let profiles = self.profiles_snapshot();
        let now = pool::now();
        let ready = |profile: &&Arc<GeminiProfile>| {
            !excluded.contains(profile.id())
                && profile.cooling_until_for(model_id, &self.cfg, now) <= now
                && profile.authenticated.load(Ordering::Acquire)
        };
        let preferred = preferred_id.and_then(|id| {
            profiles
                .iter()
                .find(|profile| profile.id() == id && ready(profile))
        });
        let profile = if let Some(profile) = preferred {
            profile.clone()
        } else {
            let offset = self.cursor.fetch_add(1, Ordering::Relaxed) as usize;
            profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| ready(profile))
                .min_by_key(|(index, profile)| {
                    (
                        profile.inflight.load(Ordering::Acquire),
                        (index + profiles.len() - offset % profiles.len()) % profiles.len(),
                    )
                })?
                .1
                .clone()
        };
        profile.inflight.fetch_add(1, Ordering::AcqRel);
        Some(GeminiLease { profile })
    }

    pub(crate) fn profile_id_for_home(
        &self,
        affinity: &crate::AffinityStore,
        home: &str,
    ) -> Option<String> {
        self.profiles_snapshot()
            .iter()
            .find(|profile| affinity.home_id(profile.id()) == home)
            .map(|profile| profile.id().to_string())
    }

    pub(crate) fn soonest_ready(&self, model_id: &str, excluded: &HashSet<String>) -> Option<i64> {
        let now = pool::now();
        self.profiles_snapshot()
            .iter()
            .filter(|profile| {
                !excluded.contains(profile.id()) && profile.authenticated.load(Ordering::Acquire)
            })
            .map(|profile| profile.cooling_until_for(model_id, &self.cfg, now))
            .filter(|until| *until > now)
            .min()
    }

    pub(crate) fn has_authenticated_profiles(&self) -> bool {
        self.profiles_snapshot()
            .iter()
            .any(|profile| profile.authenticated.load(Ordering::Acquire))
    }

    pub(crate) fn track_background_task(
        &self,
    ) -> anyhow::Result<tokio::sync::OwnedSemaphorePermit> {
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("Gemini provider is shutting down");
        }
        let permit = self
            .background_tasks
            .clone()
            .try_acquire_owned()
            .context("Gemini background task limit reached")?;
        if self.shutting_down.load(Ordering::Acquire) {
            bail!("Gemini provider is shutting down");
        }
        Ok(permit)
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
        let profiles: Vec<_> = snapshot
            .iter()
            .map(|profile| profile.status(&self.cfg, now))
            .collect();
        let models = self
            .cfg
            .models
            .iter()
            .map(|model| {
                let ready = snapshot
                    .iter()
                    .filter(|profile| profile.authenticated.load(Ordering::Acquire))
                    .map(|profile| profile.cooling_until_for(&model.id, &self.cfg, now))
                    .collect::<Vec<_>>();
                GeminiModelStatus {
                    id: model.id.clone(),
                    available: ready.iter().filter(|until| **until <= now).count(),
                    soonest_ready: ready.into_iter().filter(|until| *until > now).min(),
                }
            })
            .collect::<Vec<_>>();
        GeminiOperationalStatus {
            available: profiles
                .iter()
                .filter(|profile| {
                    profile.authenticated
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
                .filter(|profile| profile.authenticated)
                .count(),
            soonest_ready: models.iter().filter_map(|model| model.soonest_ready).min(),
            models,
            profiles,
        }
    }

    pub async fn preflight(&self) -> anyhow::Result<()> {
        let profile_count = self.profiles_snapshot().len();
        if profile_count == 0 {
            eprintln!("Gemini OAuth provider starting with an empty encrypted roster");
            return Ok(());
        }
        let healthy = self.probe_profiles().await;
        if healthy == 0 {
            bail!("Gemini provider preflight found no authenticated subscription");
        }
        if healthy < profile_count {
            eprintln!(
                "Gemini provider starting with {healthy}/{} authenticated profiles",
                profile_count
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
                eprintln!("Gemini encrypted roster reload skipped");
            }
            self.probe_profiles().await;
        }
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
                eprintln!("Gemini encrypted roster refresh skipped");
                false
            }
        }
    }

    async fn probe_profiles(&self) -> usize {
        let mut healthy = 0usize;
        let now = pool::now();
        let profiles = self.profiles_snapshot();
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
                ProbeResult::RateLimited => {
                    profile.authenticated.store(true, Ordering::Release);
                    profile.cool_until(now + self.cfg.default_rate_limit_cool_secs);
                    healthy += 1;
                }
                ProbeResult::Invalid => {
                    profile.mark_auth_failed(now + self.cfg.auth_quarantine_secs);
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
        let barrier = self
            .background_tasks
            .clone()
            .acquire_many_owned(MAX_BACKGROUND_TASKS);
        let _background_barrier = match deadline {
            Some(deadline) => match tokio::time::timeout_at(deadline, barrier).await {
                Ok(permit) => permit.ok(),
                Err(_) => {
                    self.abort_active_streams();
                    self.background_tasks
                        .clone()
                        .acquire_many_owned(MAX_BACKGROUND_TASKS)
                        .await
                        .ok()
                }
            },
            None => barrier.await.ok(),
        };
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

    fn write_credential(dir: &Path, ring: &CredentialKeyring, id: &str, subject: &str) -> PathBuf {
        write_credential_with_proxy(dir, ring, id, subject, "")
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
            max_transport_retries: 1,
            auth_quarantine_secs: 900,
            transport_cool_secs: 5,
            default_rate_limit_cool_secs: 60,
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
        let error = GeminiProfile::new(loaded, &cfg).err().unwrap();
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
            cfg.user_agent(OAuthKind::Antigravity, "gemini-test"),
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
            .select("gemini-test", &HashSet::new(), None)
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
            .select("gemini-test", &HashSet::new(), None)
            .is_none());
        assert!(gateway
            .select("gemini-other", &HashSet::new(), None)
            .is_none());
        // A fresh official catalogue without gemini-other is negative availability evidence. Once
        // stale, the generation endpoint is allowed to refresh that evidence instead of wedging.
        profile
            .quota
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .updated_at = pool::now() - 10_000;
        assert!(gateway
            .select("gemini-other", &HashSet::new(), None)
            .is_some());
        assert!(gateway
            .select("gemini-test", &HashSet::new(), None)
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
            .select("gemini-test", &HashSet::new(), None)
            .is_none());
        assert_eq!(
            profile.quota_blocked_until("gemini-test", pool::now(), 600),
            parse_rfc3339_seconds("2099-01-01T00:00:00Z").unwrap(),
        );
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
        assert_eq!(gateway.soonest_ready("gemini-test", &HashSet::new()), None,);
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

    #[tokio::test]
    async fn atomically_published_profile_is_reloaded_without_restart() {
        let (dir, ring) = fixture();
        let first = write_credential(&dir, &ring, "profile_a", "subject-a");
        let roster = dir.join("profiles.json");
        write_roster(&roster, &[("profile_a", &first)]);
        let gateway = GeminiGateway::new(config(&roster, ring.clone())).unwrap();
        let second = write_credential(&dir, &ring, "profile_b", "subject-b");
        let staging = dir.join(".profiles.pending");
        write_roster(&staging, &[("profile_a", &first), ("profile_b", &second)]);
        fs::rename(staging, &roster).unwrap();
        assert!(gateway.reload_profiles().unwrap());
        assert_eq!(gateway.operational_status().await.profiles.len(), 2);
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
}
