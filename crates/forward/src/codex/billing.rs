//! Shared customer admission and exact API-equivalent settlement for Codex turns.

use super::{CodexModel, CodexUsage};
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

/// Owns every admission guard until a non-streaming response is returned or a streaming upstream
/// task fully finishes. Client disconnect therefore never leaks money or concurrency slots.
pub(crate) struct CodexAdmission {
    _request_permit: tokio::sync::OwnedSemaphorePermit,
    _key_guard: Option<KeyGuard>,
    reservation: Option<Reservation>,
}

pub(crate) struct PendingCodexAdmission {
    tenant_scope: String,
    request_permit: tokio::sync::OwnedSemaphorePermit,
    key_guard: Option<KeyGuard>,
    authz: Authz,
}

impl PendingCodexAdmission {
    pub(crate) fn tenant_scope(&self) -> &str {
        &self.tenant_scope
    }

    pub(crate) async fn reserve(
        self,
        app: &AppState,
        model: &CodexModel,
        estimated_input_tokens: u64,
        reserve_overhead_tokens: u64,
    ) -> Result<CodexAdmission, AdmissionError> {
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    mult_bp,
                    ..
                },
                Some(billing),
            ) => {
                let estimated = estimated_input_tokens.saturating_add(reserve_overhead_tokens);
                let base = reserve_cost(model, estimated);
                let hold =
                    metering::apply_multiplier(base, *mult_bp).clamp(1, i64::MAX as i128) as i64;
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
                        eprintln!("Codex billing reservation failed: {error:#}");
                        return Err(AdmissionError::Unavailable);
                    }
                }
            }
            _ => None,
        };
        Ok(CodexAdmission {
            _request_permit: self.request_permit,
            _key_guard: self.key_guard,
            reservation,
        })
    }
}

impl CodexAdmission {
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

    pub(crate) fn settle(mut self, model: &CodexModel, usage: &CodexUsage) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let priced = price_usage(model, usage);
        let computed_charge = metering::apply_multiplier(priced.real_nano, reservation.mult_bp);
        let ceiling = reservation.hold.max(0) as i128 + metering::OVERDRAFT_NANO;
        let charge = computed_charge.clamp(0, ceiling).min(i64::MAX as i128) as i64;
        let now = pool::now();
        let usage_event = if priced.real_nano > 0 {
            Some(registry::UsageEventInput {
                model: model.id.clone(),
                input_tokens: priced.normal_input.min(i64::MAX as u64) as i64,
                output_tokens: usage.output_tokens.min(i64::MAX as u64) as i64,
                cache_read_tokens: priced.cached_input.min(i64::MAX as u64) as i64,
                cache_write_5m_tokens: 0,
                cache_write_1h_tokens: priced.cache_write_input.min(i64::MAX as u64) as i64,
                web_search_requests: 0,
                real_nano: priced.real_nano.min(i64::MAX as i128) as i64,
                speed: "standard".to_string(),
                inference_geo: String::new(),
                input_nano: priced.input_nano.min(i64::MAX as i128) as i64,
                output_nano: priced.output_nano.min(i64::MAX as i128) as i64,
                cache_read_nano: priced.cached_nano.min(i64::MAX as i128) as i64,
                cache_write_5m_nano: 0,
                cache_write_1h_nano: priced.cache_write_nano.min(i64::MAX as i128) as i64,
                web_search_nano: 0,
                priced_ts: now,
            })
        } else {
            None
        };
        reservation.billing.settle_detached(
            &reservation.request_id,
            &reservation.account_id,
            &reservation.key,
            reservation.hold,
            charge,
            None,
            usage_event,
        );
        reservation.guard.disarm();
        if charge > 0 {
            eprintln!(
                "💵 OpenAI-compatible request: −{} [{}]",
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
) -> Result<PendingCodexAdmission, AdmissionError> {
    if !app.authority_ready.load(Ordering::Acquire) {
        return Err(AdmissionError::Unavailable);
    }
    let request_permit = app.concurrency.clone().try_acquire_owned().map_err(|_| {
        Metrics::inc(&app.metrics.breaker_rejects);
        AdmissionError::Busy
    })?;
    let authz = authorize(app, headers, peer).await;
    let tenant_scope = match &authz {
        Authz::Admin { affinity_scope } => affinity_scope.clone(),
        Authz::Metered {
            account_id,
            available_nano,
            ..
        } if *available_nano > 0 => account_id.clone(),
        Authz::Metered { .. } => return Err(AdmissionError::LowBalance),
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    };
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

    Ok(PendingCodexAdmission {
        tenant_scope,
        request_permit,
        key_guard,
        authz,
    })
}

fn reserve_cost(model: &CodexModel, estimated_input_tokens: u64) -> i128 {
    let long = estimated_input_tokens > model.prices.long_context_threshold;
    let input_rate = model.prices.input.max(model.prices.cache_write_input);
    let input_rate = if long {
        metering::apply_multiplier(input_rate, model.prices.long_input_basis_points)
    } else {
        input_rate
    };
    let output_rate = if long {
        metering::apply_multiplier(model.prices.output, model.prices.long_output_basis_points)
    } else {
        model.prices.output
    };
    (estimated_input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((model.max_output_tokens as i128).saturating_mul(output_rate))
}

#[derive(Debug, Default, PartialEq, Eq)]
struct PricedUsage {
    normal_input: u64,
    cached_input: u64,
    cache_write_input: u64,
    input_nano: i128,
    cached_nano: i128,
    cache_write_nano: i128,
    output_nano: i128,
    real_nano: i128,
}

fn price_usage(model: &CodexModel, usage: &CodexUsage) -> PricedUsage {
    let cached_input = usage.cached_input_tokens.min(usage.input_tokens);
    let remaining = usage.input_tokens.saturating_sub(cached_input);
    let cache_write_input = usage.cache_write_input_tokens.min(remaining);
    let normal_input = remaining.saturating_sub(cache_write_input);
    let long = usage.input_tokens > model.prices.long_context_threshold;
    let input_multiplier = if long {
        model.prices.long_input_basis_points
    } else {
        10_000
    };
    let output_multiplier = if long {
        model.prices.long_output_basis_points
    } else {
        10_000
    };
    let input_nano = metering::apply_multiplier(
        (normal_input as i128).saturating_mul(model.prices.input),
        input_multiplier,
    );
    let cached_nano = metering::apply_multiplier(
        (cached_input as i128).saturating_mul(model.prices.cached_input),
        input_multiplier,
    );
    let cache_write_nano = metering::apply_multiplier(
        (cache_write_input as i128).saturating_mul(model.prices.cache_write_input),
        input_multiplier,
    );
    let output_nano = metering::apply_multiplier(
        (usage.output_tokens as i128).saturating_mul(model.prices.output),
        output_multiplier,
    );
    PricedUsage {
        normal_input,
        cached_input,
        cache_write_input,
        input_nano,
        cached_nano,
        cache_write_nano,
        output_nano,
        real_nano: input_nano
            .saturating_add(cached_nano)
            .saturating_add(cache_write_nano)
            .saturating_add(output_nano),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex::CodexPrices;

    fn model() -> CodexModel {
        CodexModel {
            id: "gpt-5.6-sol".to_string(),
            upstream: "gpt-5.6-sol".to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: 128_000,
            reasoning_efforts: vec!["medium".to_string()],
            prices: CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                long_context_threshold: 272_000,
                long_input_basis_points: 20_000,
                long_output_basis_points: 15_000,
            },
        }
    }

    #[test]
    fn prices_non_cached_cached_and_cache_write_without_double_counting() {
        let priced = price_usage(
            &model(),
            &CodexUsage {
                input_tokens: 1_000,
                cached_input_tokens: 400,
                cache_write_input_tokens: 100,
                output_tokens: 20,
                ..CodexUsage::default()
            },
        );
        assert_eq!(priced.normal_input, 500);
        assert_eq!(priced.cached_input, 400);
        assert_eq!(priced.cache_write_input, 100);
        assert_eq!(priced.input_nano, 500 * 5_000);
        assert_eq!(priced.cached_nano, 400 * 500);
        assert_eq!(priced.cache_write_nano, 100 * 6_250);
        assert_eq!(priced.output_nano, 20 * 30_000);
        assert_eq!(
            priced.real_nano,
            500 * 5_000 + 400 * 500 + 100 * 6_250 + 20 * 30_000
        );
    }

    #[test]
    fn long_context_multiplier_applies_to_all_input_buckets_and_output() {
        let priced = price_usage(
            &model(),
            &CodexUsage {
                input_tokens: 300_000,
                cached_input_tokens: 100_000,
                cache_write_input_tokens: 50_000,
                output_tokens: 10,
                ..CodexUsage::default()
            },
        );
        assert_eq!(priced.input_nano, 150_000 * 5_000 * 2);
        assert_eq!(priced.cached_nano, 100_000 * 500 * 2);
        assert_eq!(priced.cache_write_nano, 50_000 * 6_250 * 2);
        assert_eq!(priced.output_nano, 10 * 30_000 * 3 / 2);
    }

    #[test]
    fn reserve_covers_full_output_and_cache_write_rate() {
        let hold = reserve_cost(&model(), 1_000);
        assert_eq!(hold, 1_000 * 6_250 + 128_000 * 30_000);
    }
}
