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
                let base = reserve_cost(model, estimated, pool::now());
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
        let now = pool::now();
        let (charge, usage_event) =
            settled_charge(model, usage, reservation.hold, reservation.mult_bp, now);
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

/// Compute the exact customer debit and immutable provider-usage record before handing either to
/// the asynchronous billing actor. Keeping this boundary pure makes the only Codex money mutation
/// exhaustively testable without substituting a second pricing implementation in the test.
fn settled_charge(
    model: &CodexModel,
    usage: &CodexUsage,
    hold: i64,
    mult_bp: i64,
    now: i64,
) -> (i64, Option<registry::UsageEventInput>) {
    let priced = price_usage(model, usage, now);
    let computed_charge = metering::apply_multiplier(priced.real_nano, mult_bp);
    let ceiling = hold.max(0) as i128 + metering::OVERDRAFT_NANO;
    let charge = computed_charge.clamp(0, ceiling).min(i64::MAX as i128) as i64;
    let usage_event = (priced.real_nano > 0).then(|| registry::UsageEventInput {
        model: model.id.clone(),
        provider: registry::PROVIDER_OPENAI.to_string(),
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
    });
    (charge, usage_event)
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

/// Rates for this model as of `now`. The pinned, effective-dated catalog in `metering` is
/// authoritative, so a reviewed price change takes effect at its own epoch without a restart. The
/// config-resolved rate remains the fallback for a model the catalog no longer advertises, which
/// keeps an in-flight settlement priced exactly as it was admitted.
fn effective_prices(model: &CodexModel, now: i64) -> metering::CodexPrices {
    metering::codex_prices_at(&model.id, now).unwrap_or(model.prices)
}

fn reserve_cost(model: &CodexModel, estimated_input_tokens: u64, now: i64) -> i128 {
    let prices = effective_prices(model, now);
    let long = estimated_input_tokens > prices.long_context_threshold;
    let input_rate = prices.input.max(prices.cache_write_input);
    let input_rate = if long {
        metering::apply_multiplier(input_rate, prices.long_input_basis_points)
    } else {
        input_rate
    };
    let output_rate = if long {
        metering::apply_multiplier(prices.output, prices.long_output_basis_points)
    } else {
        prices.output
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

fn price_usage(model: &CodexModel, usage: &CodexUsage, now: i64) -> PricedUsage {
    let prices = effective_prices(model, now);
    let cached_input = usage.cached_input_tokens.min(usage.input_tokens);
    let remaining = usage.input_tokens.saturating_sub(cached_input);
    let cache_write_input = usage.cache_write_input_tokens.min(remaining);
    let normal_input = remaining.saturating_sub(cache_write_input);
    let long = usage.input_tokens > prices.long_context_threshold;
    let input_multiplier = if long {
        prices.long_input_basis_points
    } else {
        10_000
    };
    let output_multiplier = if long {
        prices.long_output_basis_points
    } else {
        10_000
    };
    let input_nano = metering::apply_multiplier(
        (normal_input as i128).saturating_mul(prices.input),
        input_multiplier,
    );
    let cached_nano = metering::apply_multiplier(
        (cached_input as i128).saturating_mul(prices.cached_input),
        input_multiplier,
    );
    let cache_write_nano = metering::apply_multiplier(
        (cache_write_input as i128).saturating_mul(prices.cache_write_input),
        input_multiplier,
    );
    let output_nano = metering::apply_multiplier(
        (usage.output_tokens as i128).saturating_mul(prices.output),
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
    use crate::billing::AsyncBilling;
    use crate::codex::CodexPrices;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tokio::sync::Semaphore;

    static NEXT_SETTLEMENT_DB: AtomicU64 = AtomicU64::new(0);

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

    fn settlement_model() -> CodexModel {
        let mut model = model();
        // Deliberately stay outside the effective-dated production catalog so every expected value
        // below is pinned to this fixture rather than changing when a reviewed catalog epoch lands.
        model.id = "gpt-settlement-test".to_string();
        model.upstream = model.id.clone();
        model
    }

    async fn reserved_admission(
        mult_bp: i64,
        topup: i64,
        hold: i64,
        request_id: &str,
    ) -> (CodexAdmission, Arc<AsyncBilling>, PathBuf) {
        const ACCOUNT: &str = "codex-settlement-account";
        const KEY: &str = "sk-pool-codex-settlement";

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        // macOS can return the same wall-clock tick to tests that start in parallel. The
        // process-local sequence prevents two billing actors from opening the same SQLite fixture.
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-settlement-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start settlement test billing"),
        );
        billing
            .create_account(ACCOUNT, None, mult_bp)
            .await
            .unwrap();
        billing
            .topup(ACCOUNT, topup, Some("codex-settlement-seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        assert!(billing
            .reserve_request(request_id, ACCOUNT, KEY, hold)
            .await
            .unwrap()
            .is_some());

        let permit = Arc::new(Semaphore::new(1)).acquire_owned().await.unwrap();
        let admission = CodexAdmission {
            _request_permit: permit,
            _key_guard: None,
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_string(),
                key: KEY.to_string(),
                mult_bp,
                hold,
                request_id: request_id.to_string(),
                guard: HoldGuard::new(
                    Some(Arc::clone(&billing)),
                    ACCOUNT.to_string(),
                    KEY.to_string(),
                    hold,
                    request_id.to_string(),
                ),
            }),
        };
        (admission, billing, path)
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
            0,
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
            0,
        );
        assert_eq!(priced.input_nano, 150_000 * 5_000 * 2);
        assert_eq!(priced.cached_nano, 100_000 * 500 * 2);
        assert_eq!(priced.cache_write_nano, 50_000 * 6_250 * 2);
        assert_eq!(priced.output_nano, 10 * 30_000 * 3 / 2);
    }

    #[test]
    fn reserve_covers_full_output_and_cache_write_rate() {
        let hold = reserve_cost(&model(), 1_000, 0);
        assert_eq!(hold, 1_000 * 6_250 + 128_000 * 30_000);
    }

    #[test]
    fn pricing_comes_from_the_audited_catalog_not_the_config_copy() {
        // A config whose embedded rate drifted from the pinned catalog must still bill the
        // catalog rate: `metering` is the single audited source of money.
        let mut drifted = model();
        drifted.prices.input = 1;
        drifted.prices.output = 1;
        let usage = CodexUsage {
            input_tokens: 1_000,
            output_tokens: 20,
            ..CodexUsage::default()
        };
        let priced = price_usage(&drifted, &usage, 0);
        assert_eq!(priced.input_nano, 1_000 * 5_000);
        assert_eq!(priced.output_nano, 20 * 30_000);
    }

    #[test]
    fn a_model_outside_the_catalog_keeps_the_rate_it_was_admitted_with() {
        // Nothing can configure such a model today, but an in-flight settlement for a model the
        // catalog stopped advertising must never silently reprice to a default.
        let mut retired = model();
        retired.id = "gpt-retired".to_string();
        retired.prices.input = 7_000;
        let priced = price_usage(
            &retired,
            &CodexUsage {
                input_tokens: 10,
                ..CodexUsage::default()
            },
            0,
        );
        assert_eq!(priced.input_nano, 10 * 7_000);
    }

    #[test]
    fn settlement_applies_the_customer_multiplier_exactly_once_and_builds_openai_usage() {
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 20,
            ..CodexUsage::default()
        };
        let (charge, event) = settled_charge(&settlement_model(), &usage, 10_000_000, 5_000, 123);

        // Official cost: 500*5000 + 400*500 + 100*6250 + 20*30000 = 3,925,000.
        // The customer multiplier is 0.5 and must be applied once, not squared.
        assert_eq!(charge, 1_962_500);
        let event = event.expect("positive usage must produce an immutable usage event");
        assert_eq!(event.model, "gpt-settlement-test");
        assert_eq!(event.provider, registry::PROVIDER_OPENAI);
        assert_eq!(event.input_tokens, 500);
        assert_eq!(event.cache_read_tokens, 400);
        assert_eq!(event.cache_write_1h_tokens, 100);
        assert_eq!(event.output_tokens, 20);
        assert_eq!(event.input_nano, 2_500_000);
        assert_eq!(event.cache_read_nano, 200_000);
        assert_eq!(event.cache_write_1h_nano, 625_000);
        assert_eq!(event.output_nano, 600_000);
        assert_eq!(
            event.real_nano,
            event.input_nano
                + event.cache_read_nano
                + event.cache_write_1h_nano
                + event.output_nano
        );
        assert_eq!(event.priced_ts, 123);
    }

    #[test]
    fn settlement_clamps_overrun_to_the_reserved_hold_plus_one_dollar() {
        let usage = CodexUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..CodexUsage::default()
        };
        let hold = 17;
        let (charge, event) = settled_charge(&settlement_model(), &usage, hold, 10_000, 456);
        assert_eq!(charge as i128, hold as i128 + metering::OVERDRAFT_NANO);
        assert!(event.is_some());
    }

    #[test]
    fn zero_usage_neither_charges_nor_writes_a_usage_event() {
        let (charge, event) = settled_charge(
            &settlement_model(),
            &CodexUsage::default(),
            1_000_000,
            10_000,
            789,
        );
        assert_eq!(charge, 0);
        assert!(event.is_none());
    }

    #[test]
    fn settlement_saturates_hostile_token_counts_instead_of_wrapping() {
        let usage = CodexUsage {
            input_tokens: u64::MAX,
            cached_input_tokens: u64::MAX,
            cache_write_input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            ..CodexUsage::default()
        };
        let (charge, event) = settled_charge(&settlement_model(), &usage, i64::MAX, 10_000, 999);
        assert_eq!(charge, i64::MAX);
        let event = event.unwrap();
        assert_eq!(event.input_tokens, 0);
        assert_eq!(event.cache_read_tokens, i64::MAX);
        assert_eq!(event.cache_write_1h_tokens, 0);
        assert_eq!(event.output_tokens, i64::MAX);
        assert_eq!(event.real_nano, i64::MAX);
        assert_eq!(event.cache_read_nano, i64::MAX);
        assert_eq!(event.output_nano, i64::MAX);
    }

    #[tokio::test]
    async fn codex_admission_settle_debits_once_and_persists_provider_usage() {
        const TOPUP: i64 = 20_000_000;
        let (admission, billing, path) =
            reserved_admission(5_000, TOPUP, 10_000_000, "codex-settle").await;
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 20,
            ..CodexUsage::default()
        };

        admission.settle(&settlement_model(), &usage);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - 1_962_500);
        assert_eq!(account.spent_nano, 1_962_500);
        assert_eq!(account.reserved_nano, 0);

        let ledger = billing
            .ledger("codex-settlement-account", 10)
            .await
            .unwrap();
        let charges = ledger
            .iter()
            .filter(|row| row.kind == "charge")
            .collect::<Vec<_>>();
        assert_eq!(charges.len(), 1);
        assert_eq!(charges[0].amount_nano, 1_962_500);
        assert_eq!(charges[0].model.as_deref(), Some("gpt-settlement-test"));

        let usage_rows = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        assert_eq!(usage_rows.len(), 1);
        assert_eq!(usage_rows[0].charge_nano, 1_962_500);
        assert_eq!(usage_rows[0].real_nano, 3_925_000);
        let providers = billing.spend_by_provider(0).await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].provider, registry::PROVIDER_OPENAI);
        assert_eq!(providers[0].requests, 1);
        assert_eq!(providers[0].charge_nano, 1_962_500);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn dropping_codex_admission_without_settle_refunds_the_hold_once() {
        const TOPUP: i64 = 20_000_000;
        let (admission, billing, path) =
            reserved_admission(10_000, TOPUP, 10_000_000, "codex-drop").await;

        drop(admission);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP);
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, 0);
        assert!(billing
            .ledger("codex-settlement-account", 10)
            .await
            .unwrap()
            .iter()
            .all(|row| row.kind != "charge"));
        assert!(billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap()
            .is_empty());

        drop(billing);
        let _ = std::fs::remove_file(path);
    }
}
