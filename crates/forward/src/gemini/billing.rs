//! Shared customer admission and exact native-Gemini settlement.

use super::config::GeminiModel;
use crate::metrics::Metrics;
use crate::proxy::{authorize, Authz, HoldGuard, KeyGuard};
use crate::state::AppState;
use axum::http::HeaderMap;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Unauthorized,
    Unavailable,
    Busy,
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
    _request_permit: tokio::sync::OwnedSemaphorePermit,
    _key_guard: Option<KeyGuard>,
    reservation: Option<Reservation>,
}

pub(crate) struct PendingGeminiAdmission {
    request_permit: tokio::sync::OwnedSemaphorePermit,
    key_guard: Option<KeyGuard>,
    authz: Authz,
}

impl PendingGeminiAdmission {
    pub(crate) fn without_reserve(self) -> GeminiAdmission {
        GeminiAdmission {
            _request_permit: self.request_permit,
            _key_guard: self.key_guard,
            reservation: None,
        }
    }

    pub(crate) async fn reserve(
        self,
        app: &AppState,
        model: &GeminiModel,
        estimated_input_tokens: u64,
        requested_output_tokens: u64,
        grounding_enabled: bool,
    ) -> Result<(GeminiAdmission, u64), AdmissionError> {
        let mut effective_output_tokens = requested_output_tokens.max(1);
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    mult_bp,
                    available_nano,
                },
                Some(billing),
            ) => {
                let Some((affordable_output_tokens, hold)) = reservation_for_budget(
                    model,
                    estimated_input_tokens,
                    requested_output_tokens,
                    grounding_enabled,
                    *mult_bp,
                    *available_nano,
                ) else {
                    return Err(AdmissionError::LowBalance);
                };
                effective_output_tokens = affordable_output_tokens;
                let request_id = crate::upstream::fresh_request_id();
                match billing
                    .reserve_request(&request_id, account_id, key, hold)
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
                _request_permit: self.request_permit,
                _key_guard: self.key_guard,
                reservation,
            },
            effective_output_tokens,
        ))
    }
}

impl GeminiAdmission {
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

    pub(crate) fn settle(mut self, model: &GeminiModel, usage: &metering::GeminiUsage) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let now = pool::now();
        let (charge, event) =
            settled_charge(model, usage, reservation.hold, reservation.mult_bp, now);
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
}

pub(crate) async fn begin_admission(
    app: &AppState,
    headers: &HeaderMap,
    peer: &SocketAddr,
) -> Result<PendingGeminiAdmission, AdmissionError> {
    if !app.authority_ready.load(Ordering::Acquire) {
        return Err(AdmissionError::Unavailable);
    }
    let request_permit = app.concurrency.clone().try_acquire_owned().map_err(|_| {
        Metrics::inc(&app.metrics.breaker_rejects);
        AdmissionError::Busy
    })?;
    let authz = authorize(app, headers, peer).await;
    match &authz {
        Authz::Admin { .. } => {}
        Authz::Metered { available_nano, .. } if *available_nano > 0 => {}
        Authz::Metered { .. } => return Err(AdmissionError::LowBalance),
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    }
    Metrics::inc(&app.metrics.requests);

    let key_guard = if let Authz::Metered { account_id, .. } = &authz {
        if !app
            .key_limiter
            .try_acquire(account_id, app.cfg.max_inflight_per_key)
        {
            Metrics::inc(&app.metrics.key_throttled);
            return Err(AdmissionError::Busy);
        }
        Some(KeyGuard::new(app.key_limiter.clone(), account_id.clone()))
    } else {
        None
    };
    Ok(PendingGeminiAdmission {
        request_permit,
        key_guard,
        authz,
    })
}

fn reserve_cost(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
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
        .saturating_add(grounding)
}

/// Fit the requested output ceiling into the customer's currently available balance. Gemini has
/// no safe mid-stream cutoff, so the cap must be written into `generationConfig.maxOutputTokens`
/// before the upstream request. The returned hold is therefore a true upper bound for the
/// conservative input/tool reserve plus the capped output, not merely `min(cost, balance)`.
fn reservation_for_budget(
    model: &GeminiModel,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    grounding_enabled: bool,
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
                grounding_enabled,
            ),
            mult_bp,
        )
        .clamp(1, i64::MAX as i128)
    };
    if cost(1) > budget {
        return None;
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
    let output_nano = (usage.output_tokens as i128).saturating_mul(output_rate);
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
        output_tokens: usage.output_tokens.min(i64::MAX as u64) as i64,
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

    #[test]
    fn settlement_maps_google_usage_without_losing_audio_cache_thought_or_search() {
        let usage = metering::GeminiUsage {
            input_tokens: 100,
            audio_input_tokens: 20,
            cached_input_tokens: 50,
            cached_audio_input_tokens: 10,
            output_tokens: 30,
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
    fn reserve_uses_the_most_expensive_input_bucket_and_full_requested_output() {
        let model = model();
        let cost = reserve_cost(&model, 1_000, 100, false);
        assert_eq!(cost, 1_000 * 1_000 + 100 * 2_500);
        assert!(reserve_cost(&model, 1_000, 100, true) > cost);
    }

    #[test]
    fn reservation_caps_output_before_upstream_without_exceeding_available_balance() {
        let model = model();
        let available =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 7, false), 10_000) as i64;
        let (output, hold) =
            reservation_for_budget(&model, 1_000, 100, false, 10_000, available).unwrap();
        assert_eq!(output, 7);
        assert_eq!(hold, available);

        let cannot_afford_one =
            metering::apply_multiplier(reserve_cost(&model, 1_000, 1, false), 10_000) as i64 - 1;
        assert!(
            reservation_for_budget(&model, 1_000, 100, false, 10_000, cannot_afford_one,).is_none()
        );
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
}
