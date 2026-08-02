//! Shared customer admission and exact native-Gemini settlement.

use super::config::GeminiModel;
use crate::metrics::{Metrics, StrictPricingProvider, StrictPricingRejectionReason};
use crate::pricing::EnginePricingRequestId;
use crate::proxy::{authorize, Authz, HoldGuard};
use crate::state::AppState;
use axum::http::HeaderMap;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;

const CALIBRATION_PROFILE_HEADER: &str = "x-apitoken-calibration-profile";
const CALIBRATION_REQUEST_ID_HEADER: &str = "x-apitoken-calibration-request-id";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Unauthorized,
    Unavailable,
    LowBalance,
}

struct Reservation {
    billing: std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: String,
    key: String,
    mult_bp: i64,
    hold: i64,
    request_id: String,
    guard: HoldGuard,
}

pub(crate) struct GeminiAdmission {
    reservation: Option<Reservation>,
    calibration_request_id: String,
}

pub(crate) struct PendingGeminiAdmission {
    authz: Authz,
    execution: registry::ExecutionAttempt,
    calibration_request_id: String,
    calibration_target: Option<String>,
}

impl PendingGeminiAdmission {
    pub(crate) fn affinity_scope(&self) -> Option<&str> {
        self.authz.affinity_scope()
    }

    pub(crate) fn calibration_target(&self) -> Option<&str> {
        self.calibration_target.as_deref()
    }

    pub(crate) fn without_reserve(self) -> GeminiAdmission {
        GeminiAdmission {
            reservation: None,
            calibration_request_id: self.calibration_request_id,
        }
    }

    pub(crate) async fn reserve(
        self,
        app: &AppState,
        model: &GeminiModel,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
        image_output_tokens: u64,
        grounding_enabled: bool,
        allow_output_cap: bool,
    ) -> Result<(GeminiAdmission, u64), AdmissionError> {
        let mut effective_output_tokens = requested_output_tokens.max(1);
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    mult_bp,
                    available_nano,
                    ..
                },
                Some(billing),
            ) => {
                let Some((affordable_output_tokens, hold)) = reservation_for_budget(
                    model,
                    estimated_input_tokens,
                    requested_output_tokens,
                    image_output_tokens,
                    grounding_enabled,
                    allow_output_cap,
                    *mult_bp,
                    *available_nano,
                ) else {
                    return Err(AdmissionError::LowBalance);
                };
                effective_output_tokens = affordable_output_tokens;
                let request_id = self.calibration_request_id.clone();
                match billing
                    .reserve_request_for_execution(
                        &request_id,
                        account_id,
                        key,
                        hold,
                        self.execution.clone(),
                    )
                    .await
                {
                    Ok(Some(_)) => Some(Reservation {
                        billing: billing.clone(),
                        account_id: account_id.clone(),
                        key: key.clone(),
                        mult_bp: *mult_bp,
                        hold,
                        request_id: request_id.clone(),
                        guard: HoldGuard::new(
                            Some(billing.clone()),
                            account_id.clone(),
                            key.clone(),
                            hold,
                            request_id,
                        ),
                    }),
                    Ok(None) => return Err(AdmissionError::LowBalance),
                    Err(error) => {
                        eprintln!("Gemini billing reservation failed: {error:#}");
                        return Err(AdmissionError::Unavailable);
                    }
                }
            }
            _ => None,
        };
        Ok((
            GeminiAdmission {
                reservation,
                calibration_request_id: self.calibration_request_id,
            },
            effective_output_tokens,
        ))
    }
}

impl GeminiAdmission {
    pub(crate) fn requires_usage(&self) -> bool {
        self.reservation.is_some()
    }

    pub(crate) async fn mark_delivering(&self) -> Result<(), AdmissionError> {
        let Some(reservation) = &self.reservation else {
            return Ok(());
        };
        match reservation
            .billing
            .mark_delivering(&reservation.request_id, 3_600)
            .await
        {
            Ok(true) => Ok(()),
            Ok(false) | Err(_) => Err(AdmissionError::Unavailable),
        }
    }

    /// Settle customer money when present and return one exact immutable provider event. Admin
    /// admissions have no reservation but keep the same request identity and calibration vector.
    pub(crate) fn settle(
        mut self,
        model: &GeminiModel,
        usage: Option<&metering::GeminiUsage>,
        profile_id: &str,
    ) -> Option<registry::ProviderTurnCalibrationEvent> {
        let now = pool::now();
        if let Some(mut reservation) = self.reservation.take() {
            let (charge, event) =
                settled_charge_or_hold(model, usage, reservation.hold, reservation.mult_bp, now);
            reservation.billing.settle_detached(
                &reservation.request_id,
                &reservation.account_id,
                &reservation.key,
                reservation.hold,
                charge,
                None,
                event,
            );
            reservation.guard.disarm();
            if charge > 0 {
                eprintln!(
                    "💵 Gemini request: −{} [{}]",
                    metering::nano_to_usd_string(charge as i128),
                    model.id
                );
            }
        }
        usage.filter(|usage| !usage.is_zero()).and_then(|usage| {
            gemini_calibration_event(&self.calibration_request_id, profile_id, model, usage, now)
        })
    }
}

fn gemini_calibration_event(
    request_id: &str,
    profile_id: &str,
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    now: i64,
) -> Option<registry::ProviderTurnCalibrationEvent> {
    if request_id.is_empty() || profile_id.is_empty() || usage.is_zero() {
        return None;
    }
    let prices = metering::gemini_prices_at(&model.id, now).unwrap_or(model.prices);
    let prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.audio_input_tokens)
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cached_audio_input_tokens);
    let long = prompt_tokens > prices.long_context_threshold;
    let (input_rate, audio_rate, cache_rate, cached_audio_rate, output_rate) = if long {
        (
            prices.long_input,
            prices.long_audio_input,
            prices.long_cached_input,
            prices.long_cached_audio_input,
            prices.long_output,
        )
    } else {
        (
            prices.input,
            prices.audio_input,
            prices.cached_input,
            prices.cached_audio_input,
            prices.output,
        )
    };
    let cost = |tokens: u64, rate: i128| i64::try_from(i128::from(tokens).checked_mul(rate)?).ok();
    let api_input_nanousd = cost(usage.input_tokens, input_rate)?;
    let api_audio_input_nanousd = cost(usage.audio_input_tokens, audio_rate)?;
    let api_cache_read_nanousd = cost(usage.cached_input_tokens, cache_rate)?;
    let api_cached_audio_input_nanousd = cost(usage.cached_audio_input_tokens, cached_audio_rate)?;
    let api_output_nanousd = cost(usage.output_tokens, output_rate)?;
    let api_image_output_nanousd = cost(usage.image_output_tokens, prices.image_output)?;
    let api_search_nanousd = i64::try_from(match prices.search {
        metering::GeminiSearchBilling::PerQuery { nano } => {
            i128::from(usage.search_queries).checked_mul(nano)?
        }
        metering::GeminiSearchBilling::PerGroundedPrompt { nano } => {
            i128::from(usage.grounded_search_prompts).checked_mul(nano)?
        }
    })
    .ok()?;
    let api_total_nanousd = [
        api_input_nanousd,
        api_audio_input_nanousd,
        api_cache_read_nanousd,
        api_cached_audio_input_nanousd,
        api_output_nanousd,
        api_image_output_nanousd,
        api_search_nanousd,
    ]
    .into_iter()
    .try_fold(0i64, i64::checked_add)?;
    if api_total_nanousd <= 0 {
        return None;
    }
    Some(registry::ProviderTurnCalibrationEvent {
        provider: registry::PROVIDER_GOOGLE.to_owned(),
        request_id: request_id.to_owned(),
        subject_id: profile_id.to_owned(),
        model_id: model.id.clone(),
        service_tier: "standard".to_owned(),
        inference_geo: "global".to_owned(),
        tariff_schedule_id: metering::gemini::TARIFF_SCHEDULE_ID.to_owned(),
        priced_ts: now,
        completed_at: now,
        input_tokens: i64::try_from(usage.input_tokens).ok()?,
        audio_input_tokens: i64::try_from(usage.audio_input_tokens).ok()?,
        cache_read_tokens: i64::try_from(
            usage
                .cached_input_tokens
                .checked_add(usage.cached_audio_input_tokens)?,
        )
        .ok()?,
        cached_audio_input_tokens: i64::try_from(usage.cached_audio_input_tokens).ok()?,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        output_tokens: i64::try_from(usage.output_tokens).ok()?,
        thinking_output_tokens: i64::try_from(usage.thinking_output_tokens).ok()?,
        image_output_tokens: i64::try_from(usage.image_output_tokens).ok()?,
        tool_prompt_tokens: i64::try_from(usage.tool_prompt_tokens).ok()?,
        search_queries: i64::try_from(usage.search_queries).ok()?,
        grounded_search_prompts: i64::try_from(usage.grounded_search_prompts).ok()?,
        api_input_nanousd,
        api_audio_input_nanousd,
        api_cache_read_nanousd,
        api_cached_audio_input_nanousd,
        api_cache_write_5m_nanousd: 0,
        api_cache_write_1h_nanousd: 0,
        api_output_nanousd,
        api_image_output_nanousd,
        api_search_nanousd,
        api_total_nanousd,
    })
}

fn settled_charge_or_hold(
    model: &GeminiModel,
    usage: Option<&metering::GeminiUsage>,
    hold: i64,
    mult_bp: i64,
    now: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    match usage.filter(|usage| !usage.is_zero()) {
        Some(usage) => settled_charge(model, usage, hold, mult_bp, now),
        // Once streaming bytes were delivered, missing usage must never turn a paid provider call
        // into a zero settlement. The conservative preflight hold is already bounded by balance;
        // no synthetic token event is invented because it would corrupt authoritative analytics.
        None => (hold.max(0), None),
    }
}

pub(crate) async fn begin_admission(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<PendingGeminiAdmission, AdmissionError> {
    if !app.authority_ready.load(Ordering::Acquire) {
        return Err(AdmissionError::Unavailable);
    }
    let execution = crate::execution::parse_execution_attempt(headers).map_err(|error| {
        eprintln!(
            "Gemini execution identity rejected class={}",
            error.as_str()
        );
        AdmissionError::Unavailable
    })?;
    let authz = authorize(app, headers, peer).await;
    match &authz {
        Authz::Admin { .. } => {}
        Authz::Metered {
            strict_policy: true,
            ..
        } => {
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::Gemini,
                StrictPricingRejectionReason::GeminiUnsupported,
            );
            return Err(AdmissionError::Unavailable);
        }
        Authz::Metered { available_nano, .. } if *available_nano > 0 => {}
        Authz::Metered { .. } => return Err(AdmissionError::LowBalance),
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    }
    Metrics::inc(&app.metrics.requests);

    let calibration_target = if matches!(authz, Authz::Admin { .. }) {
        match headers.get(CALIBRATION_PROFILE_HEADER) {
            Some(value) => {
                let value = value
                    .to_str()
                    .map_err(|_| AdmissionError::Unavailable)?
                    .trim();
                gemini_credential::validate_profile_id(value)
                    .map_err(|_| AdmissionError::Unavailable)?;
                Some(value.to_owned())
            }
            None => None,
        }
    } else {
        None
    };
    let calibration_request_id = match (
        calibration_target.as_ref(),
        headers.get(CALIBRATION_REQUEST_ID_HEADER),
    ) {
        (Some(_), Some(value)) => {
            let value = value.to_str().map_err(|_| AdmissionError::Unavailable)?;
            EnginePricingRequestId::from_engine_uuid_v4(value)
                .map(|value| value.as_str().to_owned())
                .ok_or(AdmissionError::Unavailable)?
        }
        (None, Some(_)) if matches!(authz, Authz::Admin { .. }) => {
            return Err(AdmissionError::Unavailable);
        }
        _ => crate::upstream::fresh_request_id(),
    };

    Ok(PendingGeminiAdmission {
        authz,
        execution,
        calibration_request_id,
        calibration_target,
    })
}

fn reserve_cost(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
) -> i128 {
    let prices = metering::gemini_prices_at(&model.id, pool::now()).unwrap_or(model.prices);
    let input_rate = [
        prices.input,
        prices.audio_input,
        prices.cached_input,
        prices.cached_audio_input,
        prices.long_input,
        prices.long_audio_input,
        prices.long_cached_input,
        prices.long_cached_audio_input,
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let output_rate = prices.output.max(prices.long_output);
    let image_output_rate = prices.image_output;
    let grounding = if grounding_enabled {
        match prices.search {
            // Reserve a deliberately conservative bounded query fanout. Final settlement remains
            // authoritative and the account overdraft floor bounds any larger provider fanout.
            metering::GeminiSearchBilling::PerQuery { nano } => nano.saturating_mul(32),
            metering::GeminiSearchBilling::PerGroundedPrompt { nano } => nano,
        }
    } else {
        0
    };
    (estimated_input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((requested_output_tokens as i128).saturating_mul(output_rate))
        .saturating_add((image_output_tokens as i128).saturating_mul(image_output_rate))
        .saturating_add(grounding)
}

/// Fit the requested output ceiling into the customer's currently available balance. Text routes
/// can write a lower `generationConfig.maxOutputTokens` before upstream. Image generation has a
/// fixed media-output component and is admitted only when the full request fits, because silently
/// lowering image size would change the requested product.
fn reservation_for_budget(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
    mult_bp: i64,
    available_nano: i64,
) -> Option<(u64, i64)> {
    let budget = available_nano.max(0) as i128;
    if budget == 0 {
        return None;
    }
    let requested = requested_output_tokens.max(1);
    if mult_bp <= 0 {
        // Keep a one-nanodollar reservation so the request lifecycle and zero-charge usage event
        // remain durable even for an explicitly free account.
        return Some((requested, 1));
    }
    let cost = |output_tokens| {
        metering::apply_multiplier(
            reserve_cost(
                model,
                estimated_input_tokens,
                output_tokens,
                image_output_tokens,
                grounding_enabled,
            ),
            mult_bp,
        )
        .clamp(1, i64::MAX as i128)
    };
    if cost(1) > budget {
        return None;
    }
    if !allow_output_cap {
        let hold = cost(requested);
        return (hold <= budget).then_some((requested, hold as i64));
    }
    let mut low = 1u64;
    let mut high = requested;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if cost(middle) <= budget {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    let hold = cost(low) as i64;
    Some((low, hold))
}

fn settled_charge(
    model: &GeminiModel,
    usage: &metering::GeminiUsage,
    hold: i64,
    mult_bp: i64,
    now: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    let prices = metering::gemini_prices_at(&model.id, now).unwrap_or(model.prices);
    let real = metering::gemini::cost_nanodollars(usage, &prices);
    let computed = metering::apply_multiplier(real, mult_bp);
    let ceiling = hold.max(0) as i128 + metering::OVERDRAFT_NANO;
    let charge = computed.clamp(0, ceiling).min(i64::MAX as i128) as i64;
    if real <= 0 {
        return (charge, None);
    }

    let prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.audio_input_tokens)
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cached_audio_input_tokens);
    let long = prompt_tokens > prices.long_context_threshold;
    let (input_rate, audio_rate, cached_rate, cached_audio_rate, output_rate) = if long {
        (
            prices.long_input,
            prices.long_audio_input,
            prices.long_cached_input,
            prices.long_cached_audio_input,
            prices.long_output,
        )
    } else {
        (
            prices.input,
            prices.audio_input,
            prices.cached_input,
            prices.cached_audio_input,
            prices.output,
        )
    };
    let input_nano = (usage.input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((usage.audio_input_tokens as i128).saturating_mul(audio_rate));
    let cache_nano = (usage.cached_input_tokens as i128)
        .saturating_mul(cached_rate)
        .saturating_add(
            (usage.cached_audio_input_tokens as i128).saturating_mul(cached_audio_rate),
        );
    let output_nano = (usage.output_tokens as i128)
        .saturating_mul(output_rate)
        .saturating_add((usage.image_output_tokens as i128).saturating_mul(prices.image_output));
    let (search_requests, search_nano) = match prices.search {
        metering::GeminiSearchBilling::PerQuery { nano } => (
            usage.search_queries,
            (usage.search_queries as i128).saturating_mul(nano),
        ),
        metering::GeminiSearchBilling::PerGroundedPrompt { nano } => (
            usage.grounded_search_prompts,
            (usage.grounded_search_prompts as i128).saturating_mul(nano),
        ),
    };
    let clamp = |value: i128| value.clamp(0, i64::MAX as i128) as i64;
    let event = registry::UsageEventInput {
        model: model.id.clone(),
        provider: registry::PROVIDER_GOOGLE.to_string(),
        input_tokens: usage
            .input_tokens
            .saturating_add(usage.audio_input_tokens)
            .min(i64::MAX as u64) as i64,
        output_tokens: usage
            .output_tokens
            .saturating_add(usage.image_output_tokens)
            .min(i64::MAX as u64) as i64,
        cache_read_tokens: usage
            .cached_input_tokens
            .saturating_add(usage.cached_audio_input_tokens)
            .min(i64::MAX as u64) as i64,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: search_requests.min(i64::MAX as u64) as i64,
        real_nano: clamp(real),
        speed: "standard".to_string(),
        inference_geo: String::new(),
        input_nano: clamp(input_nano),
        output_nano: clamp(output_nano),
        cache_read_nano: clamp(cache_nano),
        cache_write_5m_nano: 0,
        cache_write_1h_nano: 0,
        web_search_nano: clamp(search_nano),
        priced_ts: now,
    };
    (charge, Some(event))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AffinityStore, Breaker, Clients, PricingBridgeConfig, PricingShadowConfig, ProviderMode,
        ProxyConfig,
    };
    use pool::{Pool, Reserve};
    use registry::pricing::{FundingEnforcement, PolicyEnforcement, PricingMode};
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn proxy_config() -> Arc<ProxyConfig> {
        Arc::new(ProxyConfig {
            api_keys: Vec::new(),
            control_keys: Vec::new(),
            panel_keys: Vec::new(),
            default_mult_bp: 10_000,
            pricing_bridge: PricingBridgeConfig::disabled(),
            pricing_shadow: PricingShadowConfig::default(),
            trust_loopback: false,
            upstream: "http://127.0.0.1:1".to_string(),
            max_tries: 1,
            util_cap: 1.0,
            cool_secs: 1,
            smooth_wait_ms: 0,
            poll: false,
            inject_identity: false,
            identity: String::new(),
            inject_billing: false,
            cc_version: String::new(),
            cc_entrypoint: String::new(),
            default_beta: String::new(),
            user_agent: "gemini-strict-test".to_string(),
            user_agents: Vec::new(),
            ua_spread: 0,
            anthropic_version: String::new(),
            connect_timeout: 1,
            x_app: String::new(),
            stainless_lang: String::new(),
            stainless_runtime: String::new(),
            stainless_runtime_version: String::new(),
            stainless_package_version: String::new(),
            stainless_os: String::new(),
            stainless_arch: String::new(),
        })
    }

    fn app_state(billing: Arc<crate::billing::AsyncBilling>, path: &str) -> AppState {
        let cfg = proxy_config();
        AppState {
            provider: ProviderMode::Gemini,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                path.to_string(),
                None,
            )),
            data_db_path: Arc::new(path.to_string()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            codex: None,
            gemini: None,
            billing: Some(billing),
            pricing_shadow: None,
            pricing_manifest: Arc::new(crate::builtin_pricing_runtime_manifest()),
            authority_ready: Arc::new(AtomicBool::new(true)),
            breaker: Arc::new(Breaker::new(1)),
            metrics: Arc::new(Metrics::new()),
            probe_poke: None,
            cfg,
        }
    }

    fn model() -> GeminiModel {
        let spec = metering::gemini_catalog_at(0)
            .into_iter()
            .find(|spec| spec.id == "gemini-2.5-flash")
            .unwrap();
        GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    fn image_model() -> GeminiModel {
        let spec = metering::gemini_catalog_at(0)
            .into_iter()
            .find(|spec| spec.id == "gemini-3.1-flash-image")
            .unwrap();
        GeminiModel {
            id: spec.id.to_string(),
            display_name: spec.display_name.to_string(),
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    #[tokio::test]
    async fn admin_exact_target_accepts_only_a_canonical_calibration_request_id() {
        const ADMIN_KEY: &str = "gemini-calibration-admin";
        const REQUEST_ID: &str = "123e4567-e89b-42d3-a456-426614174000";
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-calibration-request-id-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        let mut app = app_state(Arc::clone(&billing), &path_string);
        Arc::make_mut(&mut app.cfg).api_keys = vec![ADMIN_KEY.to_owned()];
        let peer = "198.51.100.10:12345".parse().unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", ADMIN_KEY.parse().unwrap());
        headers.insert(CALIBRATION_PROFILE_HEADER, "profile_a".parse().unwrap());
        headers.insert(CALIBRATION_REQUEST_ID_HEADER, REQUEST_ID.parse().unwrap());

        let pending = begin_admission(&app, &headers, &peer).await.unwrap();
        assert_eq!(pending.calibration_target(), Some("profile_a"));
        assert_eq!(pending.calibration_request_id, REQUEST_ID);

        headers.insert(
            CALIBRATION_REQUEST_ID_HEADER,
            "123e4567-e89b-12d3-a456-426614174000".parse().unwrap(),
        );
        assert!(matches!(
            begin_admission(&app, &headers, &peer).await,
            Err(AdmissionError::Unavailable)
        ));

        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn strict_metered_gemini_fails_before_admission_or_reservation() {
        const ACCOUNT: &str = "strict-gemini-account";
        const KEY: &str = "strict-gemini-key";

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-strict-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing
            .topup(ACCOUNT, 1_000_000, Some("strict-gemini-seed"))
            .await
            .unwrap();

        let manifest = crate::builtin_pricing_runtime_manifest();
        let capability = &manifest.capabilities()[0];
        let connection = registry::open(&path_string).unwrap();
        connection
            .execute_batch(&format!(
                "INSERT INTO pricing_catalog_versions(
                     product_id,generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES('main',1,1,{},'{}','catalog-digest',1);
                 INSERT INTO provider_switch_versions(
                     generation,schema_version,capability_generation,capability_digest,
                     content_digest,created_ts
                 ) VALUES(1,1,{},'{}','switch-digest',1);
                 INSERT INTO account_policy_versions(
                     account_id,effective_version,policy_id,policy_version,source_policy_digest,
                     owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                     switch_generation,content_digest,replacement_locked,created_ts
                 ) VALUES(
                     '{ACCOUNT}',1,'gemini-test-policy',1,'source-policy','global_b2c','global',
                     'b2c','main',1,1,1,'policy-digest',0,1
                 );
                 INSERT INTO account_policy_bindings(
                     account_id,product_id,account_class,active_effective_version,
                     policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
                 ) VALUES('{ACCOUNT}','main','b2c',1,'strict','strict','verified',1);",
                capability.capability_generation(),
                capability.capability_digest(),
                capability.capability_generation(),
                capability.capability_digest(),
            ))
            .unwrap();
        let ack = registry::KeyActivationPolicyAck {
            effective_policy_version: 1,
            policy_digest: "policy-digest".to_string(),
        };
        billing
            .issue_key_with_policy_ack(KEY, ACCOUNT, None, None, None, Some(&ack))
            .await
            .unwrap();

        let app = app_state(Arc::clone(&billing), &path_string);
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", KEY.parse().unwrap());
        let peer = "198.51.100.10:12345".parse().unwrap();
        let result = begin_admission(&app, &headers, &peer).await;

        assert!(matches!(result, Err(AdmissionError::Unavailable)));
        assert_eq!(Metrics::get(&app.metrics.requests), 0);
        for mode in [PricingMode::Track, PricingMode::Discount] {
            for model_scope in [false, true] {
                assert_eq!(
                    app.metrics.strict_pricing_admitted_count(
                        StrictPricingProvider::Gemini,
                        mode,
                        model_scope,
                    ),
                    0
                );
            }
        }
        assert_eq!(
            app.metrics.strict_pricing_rejected_count(
                StrictPricingProvider::Gemini,
                StrictPricingRejectionReason::GeminiUnsupported,
            ),
            1
        );
        let reservations = connection
            .query_row("SELECT COUNT(*) FROM billing_reservations", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap();
        assert_eq!(reservations, 0);
        let auth = billing.key_auth(KEY).await.unwrap().unwrap();
        assert_eq!(auth.policy_enforcement, Some(PolicyEnforcement::Strict));
        assert_eq!(auth.funding_enforcement, Some(FundingEnforcement::Strict));

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn gemini_reservation_persists_router_execution_identity() {
        const ACCOUNT: &str = "grouped-gemini-account";
        const KEY: &str = "grouped-gemini-key";
        const GROUP: &str = "828f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-gemini-execution-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing =
            Arc::new(crate::billing::AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing.create_account(ACCOUNT, None, 10_000).await.unwrap();
        billing
            .topup(ACCOUNT, 1_000_000_000, Some("grouped-gemini-seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let app = app_state(Arc::clone(&billing), &path_string);
        let mut headers = HeaderMap::new();
        headers.insert("x-goog-api-key", KEY.parse().unwrap());
        headers.insert("x-apitoken-execution-group", GROUP.parse().unwrap());
        headers.insert("x-apitoken-attempt", "9".parse().unwrap());
        let peer = "198.51.100.10:12345".parse().unwrap();
        let pending = begin_admission(&app, &headers, &peer).await.unwrap();
        let (admission, _) = pending
            .reserve(&app, &model(), 1, 1, 0, false, true)
            .await
            .unwrap();
        let request_id = admission.reservation.as_ref().unwrap().request_id.clone();
        let connection = registry::open(&path_string).unwrap();
        let identity: (Option<String>, i32) = connection
            .query_row(
                "SELECT group_id,attempt FROM billing_reservations WHERE request_id=?1",
                [request_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(identity, (Some(GROUP.into()), 9));

        drop(connection);
        drop(admission);
        billing.flush().await.unwrap();
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn settlement_maps_google_usage_without_losing_audio_cache_thought_or_search() {
        let usage = metering::GeminiUsage {
            input_tokens: 100,
            tool_prompt_tokens: 5,
            audio_input_tokens: 20,
            cached_input_tokens: 50,
            cached_audio_input_tokens: 10,
            output_tokens: 30,
            thinking_output_tokens: 10,
            image_output_tokens: 0,
            search_queries: 7,
            grounded_search_prompts: 1,
        };
        let (charge, event) = settled_charge(&model(), &usage, i64::MAX, 10_000, 123);
        let event = event.unwrap();
        assert_eq!(event.provider, registry::PROVIDER_GOOGLE);
        assert_eq!(event.input_tokens, 120);
        assert_eq!(event.cache_read_tokens, 60);
        assert_eq!(event.output_tokens, 30);
        assert_eq!(event.web_search_requests, 1);
        assert_eq!(event.input_nano, 100 * 300 + 20 * 1_000);
        assert_eq!(event.cache_read_nano, 50 * 30 + 10 * 100);
        assert_eq!(event.output_nano, 30 * 2_500);
        assert_eq!(event.web_search_nano, 35_000_000);
        assert_eq!(event.real_nano, charge);
        assert_eq!(event.priced_ts, 123);
    }

    #[test]
    fn calibration_event_preserves_every_gemini_token_and_cost_leg() {
        let usage = metering::GeminiUsage {
            input_tokens: 100,
            tool_prompt_tokens: 5,
            audio_input_tokens: 20,
            cached_input_tokens: 50,
            cached_audio_input_tokens: 10,
            output_tokens: 30,
            thinking_output_tokens: 10,
            image_output_tokens: 0,
            search_queries: 7,
            grounded_search_prompts: 1,
        };
        let event =
            gemini_calibration_event("request-1", "opaque-profile", &model(), &usage, 123).unwrap();

        assert_eq!(event.provider, registry::PROVIDER_GOOGLE);
        assert_eq!(event.subject_id, "opaque-profile");
        assert_eq!(event.input_tokens, 100);
        assert_eq!(event.audio_input_tokens, 20);
        assert_eq!(event.cache_read_tokens, 60);
        assert_eq!(event.cached_audio_input_tokens, 10);
        assert_eq!(event.output_tokens, 30);
        assert_eq!(event.thinking_output_tokens, 10);
        assert_eq!(event.tool_prompt_tokens, 5);
        assert_eq!(event.search_queries, 7);
        assert_eq!(event.grounded_search_prompts, 1);
        assert_eq!(event.api_input_nanousd, 100 * 300);
        assert_eq!(event.api_audio_input_nanousd, 20 * 1_000);
        assert_eq!(event.api_cache_read_nanousd, 50 * 30);
        assert_eq!(event.api_cached_audio_input_nanousd, 10 * 100);
        assert_eq!(event.api_output_nanousd, 30 * 2_500);
        assert_eq!(event.api_search_nanousd, 35_000_000);
        assert_eq!(
            event.api_total_nanousd,
            event.api_input_nanousd
                + event.api_audio_input_nanousd
                + event.api_cache_read_nanousd
                + event.api_cached_audio_input_nanousd
                + event.api_output_nanousd
                + event.api_search_nanousd
        );
    }

    #[test]
    fn reserve_uses_the_most_expensive_input_bucket_and_full_requested_output() {
        let model = model();
        let cost = reserve_cost(&model, 1_000, 100, 0, false);
        assert_eq!(cost, 1_000 * 1_000 + 100 * 2_500);
        assert!(reserve_cost(&model, 1_000, 100, 0, true) > cost);
    }

    #[test]
    fn reservation_caps_output_before_upstream_without_exceeding_available_balance() {
        let model = model();
        let available =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 7, 0, false), 10_000) as i64;
        let (output, hold) =
            reservation_for_budget(&model, 1_000, 100, 0, false, true, 10_000, available).unwrap();
        assert_eq!(output, 7);
        assert_eq!(hold, available);

        let cannot_afford_one =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 1, 0, false), 10_000) as i64 - 1;
        assert!(reservation_for_budget(
            &model,
            1_000,
            100,
            0,
            false,
            true,
            10_000,
            cannot_afford_one,
        )
        .is_none());
    }

    #[test]
    fn image_reserve_and_settlement_use_the_official_media_output_sku() {
        let model = image_model();
        let full = reserve_cost(&model, 100, 32_768, 2_520, false);
        assert_eq!(full, 100 * 500 + 32_768 * 3_000 + 2_520 * 60_000);
        assert!(reservation_for_budget(
            &model,
            100,
            32_768,
            2_520,
            false,
            false,
            10_000,
            (full - 1) as i64,
        )
        .is_none());
        let admitted = reservation_for_budget(
            &model,
            100,
            32_768,
            2_520,
            false,
            false,
            10_000,
            full as i64,
        )
        .unwrap();
        assert_eq!(admitted, (32_768, full as i64));

        let usage = metering::GeminiUsage {
            input_tokens: 100,
            output_tokens: 20,
            image_output_tokens: 1_120,
            ..metering::GeminiUsage::default()
        };
        let (charge, event) = settled_charge(&model, &usage, i64::MAX, 10_000, 123);
        let expected = 100 * 500 + 20 * 3_000 + 1_120 * 60_000;
        let event = event.unwrap();
        assert_eq!(charge as i128, expected);
        assert_eq!(event.output_tokens, 1_140);
        assert_eq!(event.output_nano as i128, 20 * 3_000 + 1_120 * 60_000);
        assert_eq!(event.real_nano as i128, expected);
    }

    #[test]
    fn settlement_overrun_is_bounded_by_hold_plus_overdraft() {
        let usage = metering::GeminiUsage {
            input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            ..metering::GeminiUsage::default()
        };
        let (charge, _) = settled_charge(&model(), &usage, 17, 10_000, 0);
        assert_eq!(charge as i128, 17 + metering::OVERDRAFT_NANO);
    }

    #[test]
    fn missing_authoritative_usage_fails_closed_to_the_reserved_hold() {
        let (charge, event) = settled_charge_or_hold(&model(), None, 123_456, 10_000, 0);
        assert_eq!(charge, 123_456);
        assert!(event.is_none());

        let zero = metering::GeminiUsage::default();
        let (charge, event) = settled_charge_or_hold(&model(), Some(&zero), 654_321, 10_000, 0);
        assert_eq!(charge, 654_321);
        assert!(event.is_none());
    }
}
