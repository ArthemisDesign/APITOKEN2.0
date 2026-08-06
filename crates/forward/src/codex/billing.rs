//! Shared customer admission and exact API-equivalent settlement for Codex turns.

use super::openai_image_snapshot::{
    openai_image_quote, OpenAiImageOperation, OpenAiImageQuoteInput,
};
use super::openai_snapshot::{prepare_codex_legacy_quote, CodexLegacyQuoteInput};
use super::{CodexModel, CodexUsage};
use crate::metrics::{Metrics, StrictPricingProvider, StrictPricingRejectionReason};
use crate::pricing::{
    build_policy_admission_snapshot, EnginePricingRequestId, PricingBridgeDecision,
    PricingBridgePrepare, PricingResolution, PricingResolutionRequest, RuntimePricingManifest,
};
use crate::proxy::{authorize, Authz, HoldGuard};
use crate::state::AppState;
use anyhow::Context as _;
use axum::http::HeaderMap;
use registry::pricing::{
    BillingModeV2, LegacyPricingPathClosedV2, LegacyScalarReserveConflict,
    LegacyScalarReserveOutcome, PolicyReserveConflict, PolicyReserveOutcome, PolicyRuleScope,
    PricingMode, PricingReleaseQuoteV2, PricingReleaseReserveConflictV2,
    PricingReleaseReserveOutcomeV2, SnapshotProvider,
};
use std::net::SocketAddr;
use std::sync::atomic::Ordering;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AdmissionError {
    Unauthorized,
    Unavailable,
    LowBalance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodexSettlementPricing {
    LegacyScalar,
    LegacyStrict,
    ReleaseV2,
}

type CodexReserveResult = (String, i64, i64, Option<i64>, CodexSettlementPricing);

enum LegacyCodexReserveResult {
    Reserved(CodexReserveResult),
    ReleaseActivated,
}

struct Reservation {
    billing: std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: String,
    key: String,
    mult_bp: i64,
    hold: i64,
    tariff_priced_ts: Option<i64>,
    policy_fast: Option<bool>,
    settlement_pricing: CodexSettlementPricing,
    request_id: String,
    guard: HoldGuard,
}

/// Owns the exact billing reservation until a non-streaming response is returned or a streaming
/// upstream task fully finishes. Codex capacity is governed by its upstream subscription pool;
/// local global/per-key concurrency ceilings are intentionally not applied to this provider.
pub(crate) struct CodexAdmission {
    reservation: Option<Reservation>,
}

pub(crate) struct OpenAiImageAdmission {
    reservation: Option<Reservation>,
}

pub(crate) struct PendingCodexAdmission {
    tenant_scope: String,
    authz: Authz,
    execution: registry::ExecutionAttempt,
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
        requested_output_tokens: Option<u64>,
        reserve_overhead_tokens: u64,
        fast: bool,
    ) -> Result<CodexAdmission, AdmissionError> {
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    mult_bp,
                    available_nano,
                    strict_policy,
                    paid_available_nano,
                    track_available_nano,
                    ..
                },
                Some(billing),
            ) => {
                let (request_id, hold, reservation_mult_bp, tariff_priced_ts, settlement_pricing) =
                    reserve_codex_metered(
                        app,
                        billing,
                        account_id,
                        key,
                        model,
                        estimated_input_tokens,
                        requested_output_tokens,
                        reserve_overhead_tokens,
                        fast,
                        *mult_bp,
                        *available_nano,
                        *strict_policy,
                        *paid_available_nano,
                        *track_available_nano,
                        &self.execution,
                    )
                    .await?;
                Some(Reservation {
                    billing: billing.clone(),
                    account_id: account_id.clone(),
                    key: key.clone(),
                    mult_bp: reservation_mult_bp,
                    hold,
                    tariff_priced_ts,
                    policy_fast: tariff_priced_ts.map(|_| fast),
                    settlement_pricing,
                    request_id: request_id.clone(),
                    guard: HoldGuard::new(
                        Some(billing.clone()),
                        account_id.clone(),
                        key.clone(),
                        hold,
                        request_id,
                    ),
                })
            }
            _ => None,
        };
        Ok(CodexAdmission { reservation })
    }

    pub(crate) async fn reserve_image(
        self,
        app: &AppState,
        requested_model_id: &str,
        operation: OpenAiImageOperation,
    ) -> Result<OpenAiImageAdmission, AdmissionError> {
        let reservation = match (&self.authz, &app.billing) {
            (
                Authz::Metered {
                    account_id,
                    key,
                    mult_bp,
                    available_nano,
                    strict_policy,
                    paid_available_nano,
                    track_available_nano,
                    ..
                },
                Some(billing),
            ) => Some(
                reserve_openai_image_metered(
                    app,
                    billing,
                    account_id,
                    key,
                    requested_model_id,
                    operation,
                    *mult_bp,
                    *available_nano,
                    *strict_policy,
                    *paid_available_nano,
                    *track_available_nano,
                    &self.execution,
                )
                .await?,
            ),
            _ => None,
        };
        Ok(OpenAiImageAdmission { reservation })
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_openai_image_metered(
    app: &AppState,
    billing: &std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: &str,
    key: &str,
    requested_model_id: &str,
    operation: OpenAiImageOperation,
    legacy_mult_bp: i64,
    available_nano: i64,
    strict_policy: bool,
    paid_available_nano: Option<i64>,
    track_available_nano: Option<i64>,
    execution: &registry::ExecutionAttempt,
) -> Result<Reservation, AdmissionError> {
    let request_id = crate::upstream::fresh_request_id();
    let typed_request_id = EnginePricingRequestId::from_engine_uuid_v4(&request_id)
        .ok_or(AdmissionError::Unavailable)?;
    let tariff = metering::openai_image_tariff(requested_model_id)
        .map_err(|_| AdmissionError::Unavailable)?;

    for pass in 0..2 {
        let resolution = match billing
            .pricing_release_resolution_v2(
                account_id,
                SnapshotProvider::OpenAi.as_str(),
                tariff.canonical_model_id,
            )
            .await
        {
            Ok(resolution) => resolution,
            // The image snapshot may be reviewed and priced in `metering` before its catalog
            // generation is activated. Serve it from the exact legacy tariff meanwhile instead of
            // refusing a model we already know the price of.
            Err(error) if registry::pricing::is_model_unpriced(&error) => {
                elog::warn(
                    "codex-billing",
                    format!(
                        "OpenAI image release has no catalog price, using the exact legacy tariff: {error:#}"
                    ),
                );
                None
            }
            Err(error) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI image pricing release resolution failed: {error:#}"),
                );
                return Err(AdmissionError::Unavailable);
            }
        };
        if let Some(resolution) = resolution {
            let multiplier = resolution.payable_multiplier_bp().unwrap_or(10_000);
            let quote_budget = match resolution.billing_mode() {
                BillingModeV2::Balance => available_nano,
                BillingModeV2::MeterOnly => i64::MAX,
            };
            if resolution.billing_mode() == BillingModeV2::Balance && quote_budget <= 0 {
                return Err(AdmissionError::LowBalance);
            }
            let quote = openai_image_quote(OpenAiImageQuoteInput {
                request_id: typed_request_id.clone(),
                account_id: account_id.to_owned(),
                requested_model_id: requested_model_id.to_owned(),
                quote_ts: pool::now(),
                payable_multiplier_bp: multiplier,
                operation,
                available_nano: quote_budget,
            })
            .map_err(|error| {
                elog::error("codex-billing", format!("OpenAI image release quote failed: {error:#}"));
                AdmissionError::Unavailable
            })?
            .ok_or(AdmissionError::LowBalance)?;
            let release_quote =
                PricingReleaseQuoteV2::from_legacy_snapshot(&quote).map_err(|error| {
                    elog::error(
                        "codex-billing",
                        format!("OpenAI image release quote conversion failed: {error:#}"),
                    );
                    AdmissionError::Unavailable
                })?;
            match billing
                .reserve_request_with_pricing_release_v2_for_execution(
                    key,
                    resolution,
                    release_quote,
                    execution.clone(),
                )
                .await
            {
                Ok(PricingReleaseReserveOutcomeV2::Inserted(receipt))
                | Ok(PricingReleaseReserveOutcomeV2::Unchanged(receipt)) => {
                    let snapshot = receipt.snapshot;
                    return Ok(image_reservation(
                        billing,
                        account_id,
                        key,
                        request_id,
                        snapshot.charged_hold_nano,
                        snapshot
                            .rule
                            .as_ref()
                            .map(|rule| rule.payable_multiplier_bp)
                            .unwrap_or(0),
                        Some(snapshot.tariff_priced_ts),
                        CodexSettlementPricing::ReleaseV2,
                    ));
                }
                Ok(PricingReleaseReserveOutcomeV2::NotReserved) => {
                    return Err(AdmissionError::LowBalance)
                }
                Ok(PricingReleaseReserveOutcomeV2::Conflict(
                    PricingReleaseReserveConflictV2::ActiveReleaseChanged,
                ))
                | Ok(PricingReleaseReserveOutcomeV2::NoActiveRelease) => continue,
                Ok(PricingReleaseReserveOutcomeV2::Conflict(
                    PricingReleaseReserveConflictV2::ExistingReservationWithoutReleaseSnapshot,
                )) if pass == 0 => {}
                Ok(PricingReleaseReserveOutcomeV2::Conflict(conflict)) => {
                    elog::error(
                        "codex-billing",
                        format!("OpenAI image release reserve conflict: {conflict:?}"),
                    );
                    return Err(AdmissionError::Unavailable);
                }
                Ok(PricingReleaseReserveOutcomeV2::AbortedBeforeCommit) => {
                    return Err(AdmissionError::Unavailable)
                }
                Err(error) => {
                    elog::error(
                        "codex-billing",
                        format!("OpenAI image release reserve failed: {error:#}"),
                    );
                    return Err(AdmissionError::Unavailable);
                }
            }
        }

        if available_nano <= 0 {
            return Err(AdmissionError::LowBalance);
        }
        if strict_policy {
            let bundle = billing
                .pricing_read_bundle(account_id)
                .await
                .map_err(|error| {
                    elog::error(
                        "codex-billing",
                        format!("strict OpenAI image pricing bundle read failed: {error:#}"),
                    );
                    AdmissionError::Unavailable
                })?;
            let resolved = match crate::pricing::resolve_pricing(
                &bundle,
                &PricingResolutionRequest {
                    account_id: account_id.to_owned(),
                    provider_id: SnapshotProvider::OpenAi.as_str().to_owned(),
                    requested_model_id: requested_model_id.to_owned(),
                    canonical_model_id: tariff.canonical_model_id.to_owned(),
                },
                &RuntimePricingManifest::from_evidence(&app.pricing_manifest),
            ) {
                PricingResolution::Resolved(resolved) => resolved,
                PricingResolution::Rejected(reason) => {
                    elog::error(
                        "codex-billing",
                        format!("strict OpenAI image pricing rejected: {}", reason.code()),
                    );
                    return Err(AdmissionError::Unavailable);
                }
            };
            let strict_available = match resolved.rule.pricing_mode {
                PricingMode::Track => track_available_nano.unwrap_or(0),
                PricingMode::Discount => paid_available_nano.unwrap_or(0),
            };
            if strict_available <= 0 {
                return Err(AdmissionError::LowBalance);
            }
            let quote_ts = pool::now();
            let quote = openai_image_quote(OpenAiImageQuoteInput {
                request_id: typed_request_id.clone(),
                account_id: account_id.to_owned(),
                requested_model_id: requested_model_id.to_owned(),
                quote_ts,
                payable_multiplier_bp: resolved.rule.payable_multiplier_bp,
                operation,
                available_nano: strict_available,
            })
            .map_err(|error| {
                elog::error("codex-billing", format!("strict OpenAI image quote failed: {error:#}"));
                AdmissionError::Unavailable
            })?
            .ok_or(AdmissionError::LowBalance)?;
            let policy = build_policy_admission_snapshot(account_id, resolved.clone(), quote)
                .map_err(|error| {
                    elog::error(
                        "codex-billing",
                        format!("strict OpenAI image snapshot build failed: {error:#}"),
                    );
                    AdmissionError::Unavailable
                })?;
            match billing
                .reserve_request_with_policy_snapshot_for_execution(key, policy, execution.clone())
                .await
            {
                Ok(PolicyReserveOutcome::Inserted(receipt))
                | Ok(PolicyReserveOutcome::Unchanged(receipt)) => {
                    return Ok(image_reservation(
                        billing,
                        account_id,
                        key,
                        request_id,
                        receipt.snapshot.charged_hold_nano(),
                        resolved.rule.payable_multiplier_bp,
                        Some(quote_ts),
                        CodexSettlementPricing::LegacyStrict,
                    ));
                }
                Ok(PolicyReserveOutcome::NotReserved) => return Err(AdmissionError::LowBalance),
                Ok(PolicyReserveOutcome::Conflict(PolicyReserveConflict::ActivePricingRelease)) => {
                    continue
                }
                Ok(PolicyReserveOutcome::Conflict(conflict)) => {
                    elog::error(
                        "codex-billing",
                        format!("strict OpenAI image reserve conflict: {conflict:?}"),
                    );
                    return Err(AdmissionError::Unavailable);
                }
                Ok(PolicyReserveOutcome::AbortedBeforeCommit) | Err(_) => {
                    return Err(AdmissionError::Unavailable)
                }
            }
        }

        let quote = openai_image_quote(OpenAiImageQuoteInput {
            request_id: typed_request_id.clone(),
            account_id: account_id.to_owned(),
            requested_model_id: requested_model_id.to_owned(),
            quote_ts: pool::now(),
            payable_multiplier_bp: legacy_mult_bp,
            operation,
            available_nano,
        })
        .map_err(|error| {
            elog::error("codex-billing", format!("OpenAI image legacy quote failed: {error:#}"));
            AdmissionError::Unavailable
        })?
        .ok_or(AdmissionError::LowBalance)?;
        match billing
            .reserve_request_with_legacy_snapshot_for_execution(key, quote, execution.clone())
            .await
        {
            Ok(LegacyScalarReserveOutcome::Inserted(receipt))
            | Ok(LegacyScalarReserveOutcome::Unchanged(receipt)) => {
                let snapshot = receipt.snapshot;
                return Ok(image_reservation(
                    billing,
                    account_id,
                    key,
                    request_id,
                    snapshot.charged_hold_nano(),
                    snapshot.payable_multiplier_bp(),
                    None,
                    CodexSettlementPricing::LegacyScalar,
                ));
            }
            Ok(LegacyScalarReserveOutcome::Conflict(
                LegacyScalarReserveConflict::ActivePricingRelease,
            )) => continue,
            Ok(LegacyScalarReserveOutcome::NotReserved) => return Err(AdmissionError::LowBalance),
            Ok(LegacyScalarReserveOutcome::Conflict(conflict)) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI image legacy reserve conflict: {conflict:?}"),
                );
                return Err(AdmissionError::Unavailable);
            }
            Ok(LegacyScalarReserveOutcome::AbortedBeforeCommit) | Err(_) => {
                return Err(AdmissionError::Unavailable)
            }
        }
    }
    Err(AdmissionError::Unavailable)
}

#[allow(clippy::too_many_arguments)]
fn image_reservation(
    billing: &std::sync::Arc<crate::billing::AsyncBilling>,
    account_id: &str,
    key: &str,
    request_id: String,
    hold: i64,
    mult_bp: i64,
    tariff_priced_ts: Option<i64>,
    settlement_pricing: CodexSettlementPricing,
) -> Reservation {
    Reservation {
        billing: billing.clone(),
        account_id: account_id.to_owned(),
        key: key.to_owned(),
        mult_bp,
        hold,
        tariff_priced_ts,
        policy_fast: None,
        settlement_pricing,
        guard: HoldGuard::new(
            Some(billing.clone()),
            account_id.to_owned(),
            key.to_owned(),
            hold,
            request_id.clone(),
        ),
        request_id,
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_metered(
    app: &AppState,
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    legacy_mult_bp: i64,
    available_nano: i64,
    strict_policy: bool,
    paid_available_nano: Option<i64>,
    track_available_nano: Option<i64>,
    execution: &registry::ExecutionAttempt,
) -> Result<CodexReserveResult, AdmissionError> {
    let request_id = crate::upstream::fresh_request_id();
    for pass in 0..2 {
        if let Some(reserved) = reserve_codex_release_v2(
            billing,
            account_id,
            key,
            model,
            estimated_input_tokens,
            requested_output_tokens,
            reserve_overhead_tokens,
            fast,
            available_nano,
            &request_id,
            execution,
        )
        .await?
        {
            return Ok(reserved);
        }
        if pass != 0 {
            elog::error(
                "codex-billing",
                "OpenAI legacy reserve closed but no active release could be resolved",
            );
            return Err(AdmissionError::Unavailable);
        }
        if available_nano <= 0 {
            return Err(AdmissionError::LowBalance);
        }

        let legacy = if strict_policy {
            reserve_codex_strict(
                app,
                billing,
                account_id,
                key,
                model,
                estimated_input_tokens,
                requested_output_tokens,
                reserve_overhead_tokens,
                fast,
                paid_available_nano,
                track_available_nano,
                &request_id,
                execution,
            )
            .await?
        } else {
            reserve_codex_legacy_mode(
                app,
                billing,
                account_id,
                key,
                model,
                estimated_input_tokens,
                requested_output_tokens,
                reserve_overhead_tokens,
                fast,
                legacy_mult_bp,
                available_nano,
                &request_id,
                execution,
            )
            .await?
        };
        match legacy {
            LegacyCodexReserveResult::Reserved(reserved) => return Ok(reserved),
            LegacyCodexReserveResult::ReleaseActivated => continue,
        }
    }
    Err(AdmissionError::Unavailable)
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_release_v2(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
) -> Result<Option<CodexReserveResult>, AdmissionError> {
    for _ in 0..3 {
        let resolution = match billing
            .pricing_release_resolution_v2(
                account_id,
                SnapshotProvider::OpenAi.as_str(),
                &model.upstream,
            )
            .await
        {
            Ok(resolution) => resolution,
            Err(error) if registry::pricing::is_model_unpriced(&error) => {
                elog::warn(
                    "codex-billing",
                    format!("OpenAI release has no catalog price, using the exact legacy tariff: {error:#}"),
                );
                return Ok(None);
            }
            Err(error) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI pricing release resolution failed: {error:#}"),
                );
                return Err(AdmissionError::Unavailable);
            }
        };
        let Some(resolution) = resolution else {
            return Ok(None);
        };
        let multiplier = resolution.payable_multiplier_bp().unwrap_or(10_000);
        let typed_request_id = EnginePricingRequestId::from_engine_uuid_v4(request_id)
            .ok_or(AdmissionError::Unavailable)?;
        let prepared = match prepare_codex_legacy_quote(CodexLegacyQuoteInput {
            request_id: typed_request_id,
            account_id: account_id.to_owned(),
            model: model.clone(),
            quote_ts: pool::now(),
            payable_multiplier_bp: multiplier,
            estimated_input_tokens,
            reserve_overhead_tokens,
            requested_output_tokens,
            fast,
        }) {
            Ok(PricingBridgePrepare::Eligible(prepared)) => prepared,
            Ok(PricingBridgePrepare::Fallback(reason)) => {
                elog::error(
                    "codex-billing",
                    format!(
                        "OpenAI release-v2 quote rejected canonical input: {}",
                        reason.code()
                    ),
                );
                return Err(AdmissionError::Unavailable);
            }
            Err(error) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI release-v2 quote preparation failed: {error:#}"),
                );
                return Err(AdmissionError::Unavailable);
            }
        };
        let quote_budget = match resolution.billing_mode() {
            BillingModeV2::Balance => available_nano,
            BillingModeV2::MeterOnly => i64::MAX,
        };
        let quote = match prepared.quote(quote_budget) {
            Ok(Some(quote)) => quote,
            Ok(None) if resolution.billing_mode() == BillingModeV2::Balance => {
                return Err(AdmissionError::LowBalance)
            }
            Ok(None) => return Err(AdmissionError::Unavailable),
            Err(error) => {
                elog::error("codex-billing", format!("OpenAI release-v2 quote failed: {error:#}"));
                return Err(AdmissionError::Unavailable);
            }
        };
        let release_quote =
            PricingReleaseQuoteV2::from_legacy_snapshot(quote.snapshot()).map_err(|error| {
                elog::error(
                    "codex-billing",
                    format!("OpenAI release-v2 quote conversion failed: {error:#}"),
                );
                AdmissionError::Unavailable
            })?;
        match billing
            .reserve_request_with_pricing_release_v2_for_execution(
                key,
                resolution,
                release_quote,
                execution.clone(),
            )
            .await
        {
            Ok(PricingReleaseReserveOutcomeV2::Inserted(receipt))
            | Ok(PricingReleaseReserveOutcomeV2::Unchanged(receipt)) => {
                let snapshot = receipt.snapshot;
                let settlement_multiplier = snapshot
                    .rule
                    .as_ref()
                    .map(|rule| rule.payable_multiplier_bp)
                    .unwrap_or(0);
                return Ok(Some((
                    request_id.to_owned(),
                    snapshot.charged_hold_nano,
                    settlement_multiplier,
                    Some(snapshot.tariff_priced_ts),
                    CodexSettlementPricing::ReleaseV2,
                )));
            }
            Ok(PricingReleaseReserveOutcomeV2::NotReserved) => {
                return Err(AdmissionError::LowBalance)
            }
            Ok(PricingReleaseReserveOutcomeV2::Conflict(
                PricingReleaseReserveConflictV2::ActiveReleaseChanged,
            )) => continue,
            Ok(PricingReleaseReserveOutcomeV2::Conflict(
                PricingReleaseReserveConflictV2::ExistingReservationWithoutReleaseSnapshot,
            )) => return Ok(None),
            Ok(PricingReleaseReserveOutcomeV2::NoActiveRelease) => continue,
            Ok(PricingReleaseReserveOutcomeV2::Conflict(conflict)) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI release-v2 reserve conflict: {conflict:?}"),
                );
                return Err(AdmissionError::Unavailable);
            }
            Ok(PricingReleaseReserveOutcomeV2::AbortedBeforeCommit) => {
                return Err(AdmissionError::Unavailable)
            }
            Err(error) => {
                elog::error(
                    "codex-billing",
                    format!("OpenAI release-v2 reserve failed: {error:#}"),
                );
                return Err(AdmissionError::Unavailable);
            }
        }
    }
    elog::warn("codex-billing", "OpenAI release-v2 head changed repeatedly during admission");
    Err(AdmissionError::Unavailable)
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_legacy_mode(
    app: &AppState,
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
) -> Result<LegacyCodexReserveResult, AdmissionError> {
    let provider = SnapshotProvider::OpenAi;
    if !app.cfg.pricing_bridge.enabled() {
        app.metrics.pricing_bridge_fallback(
            provider,
            crate::pricing::PricingBridgeFallbackReason::BridgeDisabled,
        );
        return reserve_codex_legacy(
            billing,
            account_id,
            key,
            model,
            estimated_input_tokens,
            requested_output_tokens,
            reserve_overhead_tokens,
            fast,
            mult_bp,
            available_nano,
            request_id,
            execution,
        )
        .await;
    }
    let typed_request_id = EnginePricingRequestId::from_engine_uuid_v4(request_id)
        .ok_or(AdmissionError::Unavailable)?;
    match app.cfg.pricing_bridge.decision(provider, &typed_request_id) {
        PricingBridgeDecision::Fallback(reason) => {
            app.metrics.pricing_bridge_fallback(provider, reason);
            reserve_codex_legacy(
                billing,
                account_id,
                key,
                model,
                estimated_input_tokens,
                requested_output_tokens,
                reserve_overhead_tokens,
                fast,
                mult_bp,
                available_nano,
                request_id,
                execution,
            )
            .await
        }
        PricingBridgeDecision::Selected => {
            app.metrics.pricing_bridge_selected(provider);
            let quote_ts = pool::now();
            let prepared = match prepare_codex_legacy_quote(CodexLegacyQuoteInput {
                request_id: typed_request_id,
                account_id: account_id.to_owned(),
                model: model.clone(),
                quote_ts,
                payable_multiplier_bp: mult_bp,
                estimated_input_tokens,
                reserve_overhead_tokens,
                requested_output_tokens,
                fast,
            }) {
                Ok(PricingBridgePrepare::Eligible(prepared)) => prepared,
                Ok(PricingBridgePrepare::Fallback(reason)) => {
                    app.metrics.pricing_bridge_fallback(provider, reason);
                    return reserve_codex_legacy(
                        billing,
                        account_id,
                        key,
                        model,
                        estimated_input_tokens,
                        requested_output_tokens,
                        reserve_overhead_tokens,
                        fast,
                        mult_bp,
                        available_nano,
                        request_id,
                        execution,
                    )
                    .await;
                }
                Err(error) => {
                    app.metrics.pricing_bridge_failure(provider);
                    elog::error(
                        "codex-billing",
                        format!("OpenAI pricing bridge preparation failed: {error:#}"),
                    );
                    return Err(AdmissionError::Unavailable);
                }
            };
            let _bridge_latency = app.metrics.pricing_bridge_latency_timer(provider);
            let quote = match prepared.quote(available_nano) {
                Ok(Some(quote)) => quote,
                Ok(None) => {
                    app.metrics.pricing_bridge_not_reserved(provider);
                    return Err(AdmissionError::LowBalance);
                }
                Err(error) => {
                    app.metrics.pricing_bridge_failure(provider);
                    elog::error(
                        "codex-billing",
                        format!("OpenAI pricing bridge quote failed: {error:#}"),
                    );
                    return Err(AdmissionError::Unavailable);
                }
            };
            let hold = quote.snapshot().charged_hold_nano();
            match billing
                .reserve_request_with_legacy_snapshot_for_execution(
                    key,
                    quote.into_snapshot(),
                    execution.clone(),
                )
                .await
            {
                Ok(LegacyScalarReserveOutcome::Inserted(receipt)) => {
                    app.metrics.pricing_bridge_inserted(provider);
                    if let Some(shadow) = &app.pricing_shadow {
                        shadow.try_enqueue(&receipt.snapshot);
                    }
                    Ok(LegacyCodexReserveResult::Reserved((
                        request_id.to_owned(),
                        hold,
                        mult_bp,
                        None,
                        CodexSettlementPricing::LegacyScalar,
                    )))
                }
                Ok(LegacyScalarReserveOutcome::Unchanged(receipt)) => {
                    app.metrics.pricing_bridge_unchanged(provider);
                    if let Some(shadow) = &app.pricing_shadow {
                        shadow.try_enqueue(&receipt.snapshot);
                    }
                    Ok(LegacyCodexReserveResult::Reserved((
                        request_id.to_owned(),
                        hold,
                        mult_bp,
                        None,
                        CodexSettlementPricing::LegacyScalar,
                    )))
                }
                Ok(LegacyScalarReserveOutcome::Conflict(
                    LegacyScalarReserveConflict::ActivePricingRelease,
                )) => Ok(LegacyCodexReserveResult::ReleaseActivated),
                Ok(LegacyScalarReserveOutcome::NotReserved) => {
                    app.metrics.pricing_bridge_not_reserved(provider);
                    Err(AdmissionError::LowBalance)
                }
                Ok(LegacyScalarReserveOutcome::Conflict(conflict)) => {
                    app.metrics.pricing_bridge_conflict(provider);
                    elog::error(
                        "codex-billing",
                        format!("OpenAI pricing bridge reserve conflict: {conflict:?}"),
                    );
                    Err(AdmissionError::Unavailable)
                }
                Ok(LegacyScalarReserveOutcome::AbortedBeforeCommit) => {
                    app.metrics.pricing_bridge_failure(provider);
                    Err(AdmissionError::Unavailable)
                }
                Err(error) => {
                    app.metrics.pricing_bridge_failure(provider);
                    elog::error(
                        "codex-billing",
                        format!("OpenAI pricing bridge reservation failed: {error:#}"),
                    );
                    Err(AdmissionError::Unavailable)
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_strict(
    app: &AppState,
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    paid_available_nano: Option<i64>,
    track_available_nano: Option<i64>,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
) -> Result<LegacyCodexReserveResult, AdmissionError> {
    let provider = SnapshotProvider::OpenAi;
    let quote_ts = pool::now();
    let bundle = billing
        .pricing_read_bundle(account_id)
        .await
        .map_err(|error| {
            elog::error(
                "codex-billing",
                format!("strict OpenAI pricing bundle read failed: {error:#}"),
            );
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::ReadUnavailable,
            );
            AdmissionError::Unavailable
        })?;
    let manifest = RuntimePricingManifest::from_evidence(&app.pricing_manifest);
    let resolved = match crate::pricing::resolve_pricing(
        &bundle,
        &PricingResolutionRequest {
            account_id: account_id.to_owned(),
            provider_id: provider.as_str().to_owned(),
            requested_model_id: model.id.clone(),
            canonical_model_id: model.upstream.clone(),
        },
        &manifest,
    ) {
        PricingResolution::Resolved(resolved) => resolved,
        PricingResolution::Rejected(reason) => {
            elog::error(
                "codex-billing",
                format!(
                    "strict OpenAI admission rejected by pricing policy: {}",
                    reason.code()
                ),
            );
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::from_resolution(reason),
            );
            return Err(AdmissionError::Unavailable);
        }
    };
    let strict_model_scope = matches!(&resolved.rule.scope, PolicyRuleScope::Model { .. });
    let available_nano = match resolved.rule.pricing_mode {
        PricingMode::Track => track_available_nano.unwrap_or(0),
        PricingMode::Discount => paid_available_nano.unwrap_or(0),
    };
    if available_nano <= 0 {
        app.metrics.strict_pricing_rejected(
            StrictPricingProvider::OpenAi,
            StrictPricingRejectionReason::LowBalance,
        );
        return Err(AdmissionError::LowBalance);
    }
    let Some(typed_request_id) = EnginePricingRequestId::from_engine_uuid_v4(request_id) else {
        app.metrics.strict_pricing_rejected(
            StrictPricingProvider::OpenAi,
            StrictPricingRejectionReason::InvalidContract,
        );
        return Err(AdmissionError::Unavailable);
    };
    let prepared = match prepare_codex_legacy_quote(CodexLegacyQuoteInput {
        request_id: typed_request_id,
        account_id: account_id.to_owned(),
        model: model.clone(),
        quote_ts,
        payable_multiplier_bp: resolved.rule.payable_multiplier_bp,
        estimated_input_tokens,
        reserve_overhead_tokens,
        requested_output_tokens,
        fast,
    }) {
        Ok(PricingBridgePrepare::Eligible(prepared)) => prepared,
        Ok(PricingBridgePrepare::Fallback(reason)) => {
            elog::error(
                "codex-billing",
                format!("strict OpenAI quote rejected canonical input: {}", reason.code()),
            );
            let metric_reason = match reason {
                crate::PricingBridgeFallbackReason::UnsupportedModelIdentity
                | crate::PricingBridgeFallbackReason::UnsupportedModifier => {
                    StrictPricingRejectionReason::UnsupportedModel
                }
                crate::PricingBridgeFallbackReason::BridgeDisabled
                | crate::PricingBridgeFallbackReason::NotSampled
                | crate::PricingBridgeFallbackReason::SnapshotIdentityOversized
                | crate::PricingBridgeFallbackReason::OfficialHoldOutOfRange => {
                    StrictPricingRejectionReason::QuoteInvariant
                }
            };
            app.metrics
                .strict_pricing_rejected(StrictPricingProvider::OpenAi, metric_reason);
            return Err(AdmissionError::Unavailable);
        }
        Err(error) => {
            elog::error("codex-billing", format!("strict OpenAI quote failed: {error:#}"));
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::QuoteInvariant,
            );
            return Err(AdmissionError::Unavailable);
        }
    };
    let quote = match prepared.quote(available_nano) {
        Ok(Some(quote)) => quote,
        Ok(None) => {
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::LowBalance,
            );
            return Err(AdmissionError::LowBalance);
        }
        Err(error) => {
            elog::error("codex-billing", format!("strict OpenAI balance quote failed: {error:#}"));
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::QuoteInvariant,
            );
            return Err(AdmissionError::Unavailable);
        }
    };
    let hold = quote.snapshot().charged_hold_nano();
    let policy_snapshot =
        build_policy_admission_snapshot(account_id, resolved.clone(), quote.into_snapshot())
            .map_err(|error| {
                elog::error(
                    "codex-billing",
                    format!("strict OpenAI snapshot build failed: {error:#}"),
                );
                app.metrics.strict_pricing_rejected(
                    StrictPricingProvider::OpenAi,
                    StrictPricingRejectionReason::SnapshotInvariant,
                );
                AdmissionError::Unavailable
            })?;
    match billing
        .reserve_request_with_policy_snapshot_for_execution(key, policy_snapshot, execution.clone())
        .await
    {
        Ok(PolicyReserveOutcome::Inserted(_)) | Ok(PolicyReserveOutcome::Unchanged(_)) => {
            app.metrics.strict_pricing_admitted(
                StrictPricingProvider::OpenAi,
                resolved.rule.pricing_mode,
                strict_model_scope,
            );
            Ok(LegacyCodexReserveResult::Reserved((
                request_id.to_owned(),
                hold,
                resolved.rule.payable_multiplier_bp,
                Some(quote_ts),
                CodexSettlementPricing::LegacyStrict,
            )))
        }
        Ok(PolicyReserveOutcome::NotReserved) => {
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::LowBalance,
            );
            Err(AdmissionError::LowBalance)
        }
        Ok(PolicyReserveOutcome::Conflict(PolicyReserveConflict::ActivePricingRelease)) => {
            Ok(LegacyCodexReserveResult::ReleaseActivated)
        }
        Ok(PolicyReserveOutcome::Conflict(conflict)) => {
            elog::error(
                "codex-billing",
                format!("strict OpenAI reserve conflict: {conflict:?}"),
            );
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::ReserveConflict,
            );
            Err(AdmissionError::Unavailable)
        }
        Ok(PolicyReserveOutcome::AbortedBeforeCommit) => {
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::HandoffAborted,
            );
            Err(AdmissionError::Unavailable)
        }
        Err(error) => {
            elog::error("codex-billing", format!("strict OpenAI reserve failed: {error:#}"));
            app.metrics.strict_pricing_rejected(
                StrictPricingProvider::OpenAi,
                StrictPricingRejectionReason::ReserveUnavailable,
            );
            Err(AdmissionError::Unavailable)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn reserve_codex_legacy(
    billing: &crate::billing::AsyncBilling,
    account_id: &str,
    key: &str,
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    reserve_overhead_tokens: u64,
    fast: bool,
    mult_bp: i64,
    available_nano: i64,
    request_id: &str,
    execution: &registry::ExecutionAttempt,
) -> Result<LegacyCodexReserveResult, AdmissionError> {
    let estimated = estimated_input_tokens.saturating_add(reserve_overhead_tokens);
    let base = reserve_cost(model, estimated, requested_output_tokens, pool::now(), fast);
    let hold = metering::apply_multiplier(base, mult_bp).clamp(1, i64::MAX as i128) as i64;
    // Preserve the scalar admission contract exactly: a conservative full-output estimate is
    // capped to the account balance, while exact settlement remains bounded by hold + overdraft.
    let hold = hold.min(available_nano.max(1));
    match billing
        .reserve_request_for_execution(request_id, account_id, key, hold, execution.clone())
        .await
    {
        Ok(Some(_)) => Ok(LegacyCodexReserveResult::Reserved((
            request_id.to_owned(),
            hold,
            mult_bp,
            None,
            CodexSettlementPricing::LegacyScalar,
        ))),
        Ok(None) => Err(AdmissionError::LowBalance),
        Err(error) if error.downcast_ref::<LegacyPricingPathClosedV2>().is_some() => {
            Ok(LegacyCodexReserveResult::ReleaseActivated)
        }
        Err(error) => {
            elog::error("codex-billing", format!("Codex billing reservation failed: {error:#}"));
            Err(AdmissionError::Unavailable)
        }
    }
}

impl OpenAiImageAdmission {
    /// Return the engine-owned money identity for a metered image request. Public success responses
    /// expose this value as `x-request-id`, so an operator can correlate the exact reservation,
    /// release snapshot, usage event, and terminal settlement without relying on upstream metadata.
    pub(crate) fn request_id(&self) -> Option<&str> {
        self.reservation
            .as_ref()
            .map(|reservation| reservation.request_id.as_str())
    }

    pub(crate) async fn mark_delivering(&mut self) -> Result<(), AdmissionError> {
        let Some(reservation) = &mut self.reservation else {
            return Ok(());
        };
        match reservation
            .billing
            .mark_delivering(&reservation.request_id, 3_600)
            .await
        {
            Ok(true) => {
                // From this point a cancellation or transport loss is ambiguous: normal durable
                // recovery must charge the full hold instead of HoldGuard refunding an executed turn.
                reservation.guard.disarm();
                Ok(())
            }
            Ok(false) | Err(_) => Err(AdmissionError::Unavailable),
        }
    }

    pub(crate) fn settle(mut self, model_id: &str, usage: &metering::OpenAiImageUsage) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let priced_ts = reservation.tariff_priced_ts.unwrap_or_else(pool::now);
        let strict = reservation.settlement_pricing == CodexSettlementPricing::LegacyStrict;
        let (charge, usage_event) = settled_openai_image_charge(
            model_id,
            usage,
            reservation.hold,
            reservation.mult_bp,
            priced_ts,
            reservation.settlement_pricing,
        );
        if strict && charge > reservation.hold {
            elog::error(
                "codex-billing",
                "strict OpenAI image usage exceeded its admission hold; leaving reservation for full-hold recovery",
            );
            reservation.guard.disarm();
            return;
        }
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
            elog::info(
                "codex-billing",
                format!(
                    "OpenAI image request charged {} [{}]",
                    metering::nano_to_usd_string(charge as i128),
                    model_id
                ),
            );
        }
    }

    /// A successful provider turn with malformed terminal usage cannot be refunded: execution is
    /// already authoritative. Leave the reservation for normal full-hold recovery rather than
    /// inventing token counts or making an executed image free.
    pub(crate) fn retain_full_hold(mut self) {
        if let Some(mut reservation) = self.reservation.take() {
            reservation.guard.disarm();
        }
    }
}

fn settled_openai_image_charge(
    model_id: &str,
    usage: &metering::OpenAiImageUsage,
    hold: i64,
    mult_bp: i64,
    priced_ts: i64,
    settlement_pricing: CodexSettlementPricing,
) -> (i64, Option<registry::UsageEventInput>) {
    let Ok(tariff) = metering::openai_image_tariff(model_id) else {
        return (hold.max(0), None);
    };
    let Ok(real_nano) = metering::openai_image_cost_nanodollars(usage, &tariff.prices) else {
        return (hold.max(0), None);
    };
    let computed_charge = match settlement_pricing {
        CodexSettlementPricing::ReleaseV2 => metering::apply_multiplier_floor(real_nano, mult_bp),
        CodexSettlementPricing::LegacyScalar | CodexSettlementPricing::LegacyStrict => {
            metering::apply_multiplier(real_nano, mult_bp)
        }
    };
    let ceiling = i128::from(hold.max(0)) + metering::OVERDRAFT_NANO;
    let charge = computed_charge.clamp(0, ceiling).min(i64::MAX as i128) as i64;
    let fresh_text = usage
        .total_text_input_tokens
        .saturating_sub(usage.cached_text_input_tokens);
    let fresh_image = usage
        .total_image_input_tokens
        .saturating_sub(usage.cached_image_input_tokens);
    let input_nano = i128::from(fresh_text) * tariff.prices.fresh_text_input
        + i128::from(fresh_image) * tariff.prices.fresh_image_input;
    let cache_read_nano = i128::from(usage.cached_text_input_tokens)
        * tariff.prices.cached_text_input
        + i128::from(usage.cached_image_input_tokens) * tariff.prices.cached_image_input;
    let output_nano = i128::from(usage.image_output_tokens) * tariff.prices.image_output;
    let input_tokens = usage
        .total_text_input_tokens
        .saturating_add(usage.total_image_input_tokens);
    let cache_read_tokens = usage
        .cached_text_input_tokens
        .saturating_add(usage.cached_image_input_tokens);
    let usage_event = (real_nano > 0).then(|| registry::UsageEventInput {
        model: model_id.to_owned(),
        provider: registry::PROVIDER_OPENAI.to_owned(),
        input_tokens: input_tokens.min(i64::MAX as u64) as i64,
        output_tokens: usage.image_output_tokens.min(i64::MAX as u64) as i64,
        cache_read_tokens: cache_read_tokens.min(i64::MAX as u64) as i64,
        cache_write_5m_tokens: 0,
        cache_write_1h_tokens: 0,
        web_search_requests: 0,
        real_nano: real_nano.min(i64::MAX as i128) as i64,
        speed: "standard".to_owned(),
        inference_geo: String::new(),
        input_nano: input_nano.min(i64::MAX as i128) as i64,
        output_nano: output_nano.min(i64::MAX as i128) as i64,
        cache_read_nano: cache_read_nano.min(i64::MAX as i128) as i64,
        cache_write_5m_nano: 0,
        cache_write_1h_nano: 0,
        web_search_nano: 0,
        priced_ts,
    });
    (charge, usage_event)
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

    pub(crate) fn settle(
        mut self,
        model: &CodexModel,
        usage: &CodexUsage,
        requested_output_tokens: Option<u64>,
        fast: bool,
    ) {
        let Some(mut reservation) = self.reservation.take() else {
            return;
        };
        let priced_ts = reservation.tariff_priced_ts.unwrap_or_else(pool::now);
        let effective_fast = reservation.policy_fast.unwrap_or(fast);
        let strict = reservation.settlement_pricing == CodexSettlementPricing::LegacyStrict;
        let (charge, usage_event) = settled_charge(
            model,
            usage,
            reservation.hold,
            reservation.mult_bp,
            if strict {
                None
            } else {
                requested_output_tokens
            },
            priced_ts,
            effective_fast,
            reservation.settlement_pricing,
        );
        if strict && charge > reservation.hold {
            elog::error(
                "codex-billing",
                "strict OpenAI usage exceeded its admission hold; leaving reservation for full-hold recovery",
            );
            reservation.guard.disarm();
            return;
        }
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
            elog::info(
                "codex-billing",
                format!(
                    "OpenAI-compatible request: −{} [{}]",
                    metering::nano_to_usd_string(charge as i128),
                    model.id
                ),
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
    requested_output_tokens: Option<u64>,
    now: i64,
    fast: bool,
    settlement_pricing: CodexSettlementPricing,
) -> (i64, Option<registry::UsageEventInput>) {
    let priced = price_usage(model, usage, now, fast);
    // Honest billing: the transport cannot hard-stop generation, so the model may emit more output
    // than the client's requested cap. The provider truly consumed those tokens (the immutable
    // usage_event and window calibration below record the real figures), but the customer is never
    // charged past the ceiling it asked for — the overage is absorbed by the pool, matching the
    // real API where hitting `max_tokens` stops generation and bills only up to the cap.
    let charge_basis_nano = match requested_output_tokens {
        Some(cap) if usage.output_tokens > cap => {
            let mut capped_usage = usage.clone();
            capped_usage.output_tokens = cap;
            price_usage(model, &capped_usage, now, fast).real_nano
        }
        _ => priced.real_nano,
    };
    let computed_charge = match settlement_pricing {
        CodexSettlementPricing::ReleaseV2 => {
            metering::apply_multiplier_floor(charge_basis_nano, mult_bp)
        }
        CodexSettlementPricing::LegacyScalar | CodexSettlementPricing::LegacyStrict => {
            metering::apply_multiplier(charge_basis_nano, mult_bp)
        }
    };
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
        speed: if fast { "fast" } else { "standard" }.to_string(),
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
    let execution = crate::execution::parse_execution_attempt(headers).map_err(|error| {
        elog::error(
            "codex-billing",
            format!("OpenAI execution identity rejected class={}", error.as_str()),
        );
        AdmissionError::Unavailable
    })?;
    let authz = authorize(app, headers, peer).await;
    let tenant_scope = match &authz {
        Authz::Admin { affinity_scope } => affinity_scope.clone(),
        // Balance is enforced only after model-specific release resolution. Service assignments
        // use meter_only and must remain admissible at zero balance.
        Authz::Metered { account_id, .. } => account_id.clone(),
        Authz::Unauthorized => {
            Metrics::inc(&app.metrics.auth_failures);
            return Err(AdmissionError::Unauthorized);
        }
        Authz::Unavailable => return Err(AdmissionError::Unavailable),
    };
    Metrics::inc(&app.metrics.requests);

    Ok(PendingCodexAdmission {
        tenant_scope,
        authz,
        execution,
    })
}

/// Rates for this model as of `now`. The pinned, effective-dated catalog in `metering` is
/// authoritative, so a reviewed price change takes effect at its own epoch without a restart. The
/// config-resolved rate remains the fallback for a model the catalog no longer advertises, which
/// keeps an in-flight settlement priced exactly as it was admitted.
fn effective_prices(model: &CodexModel, now: i64) -> metering::CodexPrices {
    metering::codex_prices_at(&model.id, now).unwrap_or(model.prices)
}

pub(super) fn reserve_cost(
    model: &CodexModel,
    estimated_input_tokens: u64,
    requested_output_tokens: Option<u64>,
    now: i64,
    fast: bool,
) -> i128 {
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
    // Reserve the output leg against the client's requested cap when it supplied one, not the
    // model's full max_output_tokens. Settlement bills output capped to the same figure, so the
    // hold stays a correct ceiling while a small `max_tokens` no longer holds the worst-case
    // ~$4 that false-402'd low-balance clients running several turns at once.
    let output_tokens = requested_output_tokens
        .unwrap_or(model.max_output_tokens)
        .min(model.max_output_tokens);
    let standard = (estimated_input_tokens as i128)
        .saturating_mul(input_rate)
        .saturating_add((output_tokens as i128).saturating_mul(output_rate));
    apply_fast_multiplier(prices, standard, fast)
}

/// Exact official-price cost of one completed turn, used for per-home window-capacity
/// calibration. Pure pricing only: customer money moves exclusively through `settled_charge`.
#[cfg(test)]
pub(crate) fn price_real_nano(
    model: &CodexModel,
    usage: &CodexUsage,
    now: i64,
    fast: bool,
) -> i128 {
    price_usage(model, usage, now, fast).real_nano
}

/// Build one complete immutable dual-ledger event. Reasoning is a subset of output and cached
/// input is a subset of input; both are recorded diagnostically but never added twice. Integer
/// conversions fail closed so hostile counters cannot saturate into plausible evidence.
pub(crate) fn price_calibration_event(
    request_id: &str,
    home_id: &str,
    model: &CodexModel,
    usage: &CodexUsage,
    completed_at: i64,
    fast: bool,
    provider_reported_tier: Option<&str>,
) -> anyhow::Result<registry::CodexTurnCalibrationEvent> {
    if usage.input_tokens == 0 && usage.output_tokens == 0 {
        anyhow::bail!("Codex turn completed without billable usage evidence");
    }
    let priced = price_usage(model, usage, completed_at, fast);
    let service_tier = if fast {
        metering::CodexServiceTier::Fast
    } else {
        metering::CodexServiceTier::Standard
    };
    let tariff = metering::codex_tariff_capability_at(
        &model.id,
        completed_at,
        service_tier,
        usage.input_tokens,
    )
    .map_err(|error| anyhow::anyhow!("Codex tariff identity unavailable: {error:?}"))?;
    let credits = metering::codex_credit_cost_nano(
        &model.id,
        usage.input_tokens,
        priced.cached_input,
        usage.output_tokens,
        fast,
    )
    .context("Codex subscription credit rate unavailable")?;
    let token = |value: u64, name: &'static str| {
        i64::try_from(value).with_context(|| format!("Codex {name} exceeds bigint"))
    };
    let money = |value: i128, name: &'static str| {
        i64::try_from(value).with_context(|| format!("Codex {name} exceeds bigint"))
    };
    Ok(registry::CodexTurnCalibrationEvent {
        request_id: request_id.to_owned(),
        home_id: home_id.to_owned(),
        model_id: model.id.clone(),
        service_tier: if fast { "fast" } else { "standard" }.to_owned(),
        provider_reported_tier: provider_reported_tier.map(str::to_owned),
        api_tariff_schedule_id: tariff.tariff_schedule_id.as_str().to_owned(),
        credit_schedule_id: metering::CODEX_CREDIT_SCHEDULE_ID.to_owned(),
        completed_at,
        input_tokens: token(usage.input_tokens, "input tokens")?,
        cached_input_tokens: token(priced.cached_input, "cached input tokens")?,
        cache_write_input_tokens: token(priced.cache_write_input, "cache-write input tokens")?,
        output_tokens: token(usage.output_tokens, "output tokens")?,
        reasoning_output_tokens: token(
            usage.reasoning_output_tokens.min(usage.output_tokens),
            "reasoning output tokens",
        )?,
        api_input_nanousd: money(priced.input_nano, "API input cost")?,
        api_cached_input_nanousd: money(priced.cached_nano, "API cached-input cost")?,
        api_cache_write_nanousd: money(priced.cache_write_nano, "API cache-write cost")?,
        api_output_nanousd: money(priced.output_nano, "API output cost")?,
        api_total_nanousd: money(priced.real_nano, "API total cost")?,
        chatgpt_input_nanocredits: money(credits.input_credit_nano, "ChatGPT input credits")?,
        chatgpt_cached_input_nanocredits: money(
            credits.cached_input_credit_nano,
            "ChatGPT cached-input credits",
        )?,
        chatgpt_output_nanocredits: money(credits.output_credit_nano, "ChatGPT output credits")?,
        chatgpt_total_nanocredits: money(credits.total_credit_nano, "ChatGPT total credits")?,
    })
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

fn price_usage(model: &CodexModel, usage: &CodexUsage, now: i64, fast: bool) -> PricedUsage {
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
    let input_nano = apply_fast_multiplier(prices, input_nano, fast);
    let cached_nano = metering::apply_multiplier(
        (cached_input as i128).saturating_mul(prices.cached_input),
        input_multiplier,
    );
    let cached_nano = apply_fast_multiplier(prices, cached_nano, fast);
    let cache_write_nano = metering::apply_multiplier(
        (cache_write_input as i128).saturating_mul(prices.cache_write_input),
        input_multiplier,
    );
    let cache_write_nano = apply_fast_multiplier(prices, cache_write_nano, fast);
    let output_nano = metering::apply_multiplier(
        (usage.output_tokens as i128).saturating_mul(prices.output),
        output_multiplier,
    );
    let output_nano = apply_fast_multiplier(prices, output_nano, fast);
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

fn apply_fast_multiplier(prices: metering::CodexPrices, amount: i128, fast: bool) -> i128 {
    if !fast {
        return amount;
    }
    metering::apply_multiplier(amount, prices.api_fast_multiplier_basis_points)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::affinity::AffinityStore;
    use crate::billing::AsyncBilling;
    use crate::breaker::Breaker;
    use crate::codex::CodexPrices;
    use crate::config::ProxyConfig;
    use crate::upstream::Clients;
    use crate::{PricingBridgeConfig, PricingBridgeFallbackReason, ProviderMode};
    use pool::{Pool, Reserve};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use std::time::{SystemTime, UNIX_EPOCH};

    static NEXT_SETTLEMENT_DB: AtomicU64 = AtomicU64::new(0);

    fn model() -> CodexModel {
        CodexModel {
            id: "gpt-5.6-sol".to_string(),
            upstream: "gpt-5.6-sol".to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: 128_000,
            reasoning_efforts: vec!["medium".to_string()],
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: Some(25_000),
            prices: CodexPrices {
                input: 5_000,
                cached_input: 500,
                cache_write_input: 6_250,
                output: 30_000,
                api_fast_multiplier_basis_points: 25_000,
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

    fn bridge_proxy_config(pricing_bridge: PricingBridgeConfig) -> Arc<ProxyConfig> {
        Arc::new(ProxyConfig {
            api_keys: Vec::new(),
            control_keys: Vec::new(),
            panel_keys: Vec::new(),
            default_mult_bp: 10_000,
            pricing_bridge,
            pricing_shadow: crate::pricing::PricingShadowConfig::default(),
            trust_loopback: false,
            upstream: "http://127.0.0.1:1".to_string(),
            claudestore_fallback: None,
            max_tries: 1,
            util_cap: 1.0,
            cool_secs: 60,
            smooth_wait_ms: 0,
            poll: false,
            inject_identity: false,
            identity: String::new(),
            inject_billing: false,
            cc_version: String::new(),
            cc_entrypoint: String::new(),
            default_beta: String::new(),
            user_agent: "pricing-bridge-test".to_string(),
            user_agents: Vec::new(),
            ua_spread: 0,
            anthropic_version: String::new(),
            connect_timeout: 1,
            read_timeout: 120,
            nonstream_read_timeout: 1800,
            x_app: String::new(),
            stainless_lang: String::new(),
            stainless_runtime: String::new(),
            stainless_runtime_version: String::new(),
            stainless_package_version: String::new(),
            stainless_os: String::new(),
            stainless_arch: String::new(),
        })
    }

    fn bridge_app(billing: Arc<AsyncBilling>, pricing_bridge: PricingBridgeConfig) -> AppState {
        let cfg = bridge_proxy_config(pricing_bridge);
        AppState {
            provider: ProviderMode::OpenAi,
            authority: Arc::new(registry::authority::AuthorityConfig::new(
                ":memory:".to_string(),
                None,
            )),
            data_db_path: Arc::new(":memory:".to_string()),
            pool: Arc::new(Pool::new(Vec::new(), Reserve::FULL, 1.0, 1.0)),
            affinity: Arc::new(AffinityStore::new(None, None, 3_600, 60, 10).unwrap()),
            clients: Arc::new(Clients::new(&cfg)),
            codex: None,
            gemini: None,
            kimi: None,
            glm: None,
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

    async fn pending_bridge_admission(
        pricing_bridge: PricingBridgeConfig,
        execution: registry::ExecutionAttempt,
    ) -> (CodexAdmission, AppState, Arc<AsyncBilling>, PathBuf) {
        const ACCOUNT: &str = "codex-bridge-account";
        const KEY: &str = "sk-pool-codex-bridge";
        const TOPUP: i64 = 20_000_000;

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-bridge-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start bridge test billing"),
        );
        billing.create_account(ACCOUNT, None, 2_000).await.unwrap();
        billing
            .topup(ACCOUNT, TOPUP, Some("codex-bridge-seed"))
            .await
            .unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let app = bridge_app(Arc::clone(&billing), pricing_bridge);
        let pending = PendingCodexAdmission {
            tenant_scope: ACCOUNT.to_string(),
            execution,
            authz: Authz::Metered {
                account_id: ACCOUNT.to_string(),
                key: KEY.to_string(),
                mult_bp: 2_000,
                available_nano: TOPUP,
                strict_policy: false,
                paid_available_nano: None,
                track_available_nano: None,
            },
        };
        let admission = pending
            .reserve(&app, &model(), 100, Some(10), 0, false)
            .await
            .unwrap();
        (admission, app, billing, path)
    }

    fn terra_model() -> CodexModel {
        let mut terra = model();
        terra.id = "gpt-5.6-terra".to_string();
        terra.upstream = terra.id.clone();
        terra
    }

    fn strict_pending() -> PendingCodexAdmission {
        PendingCodexAdmission {
            tenant_scope: "strict-codex-account".to_string(),
            execution: registry::ExecutionAttempt::direct(),
            authz: Authz::Metered {
                account_id: "strict-codex-account".to_string(),
                key: "strict-codex-key".to_string(),
                mult_bp: 5_000,
                available_nano: 1_000_000,
                strict_policy: true,
                paid_available_nano: Some(600_000),
                track_available_nano: Some(1_000_000),
            },
        }
    }

    async fn strict_codex_fixture() -> (AppState, Arc<AsyncBilling>, PathBuf) {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-strict-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let path_string = path.to_string_lossy().into_owned();
        let billing = Arc::new(AsyncBilling::start(path_string.clone(), 1).unwrap());
        billing
            .create_account("strict-codex-account", None, 5_000)
            .await
            .unwrap();
        billing
            .topup("strict-codex-account", 1_000_000, Some("strict-codex-seed"))
            .await
            .unwrap();

        let manifest = crate::builtin_pricing_runtime_manifest();
        let capability = &manifest.capabilities()[0];
        let conn = registry::open(&path_string).unwrap();
        conn.execute(
            "INSERT INTO pricing_catalog_versions(
                 product_id,generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES('main',1,1,?1,?2,'catalog-digest',1)",
            (
                capability.capability_generation(),
                capability.capability_digest(),
            ),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO provider_switch_versions(
                 generation,schema_version,capability_generation,capability_digest,
                 content_digest,created_ts
             ) VALUES(1,1,?1,?2,'switch-digest',1)",
            (
                capability.capability_generation(),
                capability.capability_digest(),
            ),
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO pricing_catalog_entries(
                 product_id,generation,provider_id,canonical_model_id,enabled
             ) VALUES
                 ('main',1,'openai','gpt-5.6-sol',1),
                 ('main',1,'openai','gpt-5.6-terra',1);
             INSERT INTO pricing_catalog_heads(product_id,active_generation,updated_ts)
             VALUES('main',1,1);
             INSERT INTO provider_switch_entries(
                 generation,provider_id,scope_type,product_id,segment,catalog_generation,enabled
             ) VALUES
                 (1,'openai','master','','',NULL,1),
                 (1,'openai','segment','main','b2c',1,1);
             INSERT INTO provider_switch_head(singleton,active_generation,updated_ts)
             VALUES(1,1,1);
             INSERT INTO account_policy_versions(
                 account_id,effective_version,policy_id,policy_version,source_policy_digest,
                 owner_type,owner_id,account_class,product_id,schema_version,catalog_generation,
                 switch_generation,content_digest,replacement_locked,created_ts
             ) VALUES(
                 'strict-codex-account',1,'b2c:global',1,'source-policy','global_b2c','global',
                 'b2c','main',1,1,1,'policy-digest',0,1
             );
             INSERT INTO account_policy_rules(
                 account_id,effective_version,rule_id,rule_digest,scope_type,provider_id,
                 canonical_model_id,pricing_mode,rule_origin,discount_bps,payable_multiplier_bp,
                 track_eligible,retention_eligible,commission_eligible
             ) VALUES
                 ('strict-codex-account',1,'openai-track','openai-track-digest','provider',
                  'openai',NULL,'track','managed',NULL,5000,1,1,1),
                 ('strict-codex-account',1,'openai-static-sol','openai-static-sol-digest','model',
                  'openai','gpt-5.6-sol','discount','managed',0,10000,0,0,0);
             INSERT INTO account_policy_bindings(
                 account_id,product_id,account_class,active_effective_version,
                 policy_enforcement,funding_enforcement,reconciliation_state,updated_ts
             ) VALUES(
                 'strict-codex-account','main','b2c',1,'strict','strict','verified',1
             );
             INSERT INTO funding_buckets(
                 bucket_id,account_id,source_type,source_ref,eligibility,balance_nano,
                 reserved_nano,spent_nano,version,status,created_ts,updated_ts
             ) VALUES
                 ('strict-codex-bonus','strict-codex-account','welcome_track_bonus','welcome',
                  'track',400000,0,0,1,'active',1,1),
                 ('strict-codex-paid','strict-codex-account','paid','seed',
                  'any',600000,0,0,1,'active',2,2);",
        )
        .unwrap();
        drop(conn);
        billing
            .issue_key_with_policy_ack(
                "strict-codex-key",
                "strict-codex-account",
                None,
                None,
                None,
                Some(&registry::KeyActivationPolicyAck {
                    effective_policy_version: 1,
                    policy_digest: "policy-digest".to_string(),
                }),
            )
            .await
            .unwrap();
        let app = bridge_app(Arc::clone(&billing), PricingBridgeConfig::disabled());
        (app, billing, path)
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

        let admission = CodexAdmission {
            reservation: Some(Reservation {
                billing: Arc::clone(&billing),
                account_id: ACCOUNT.to_string(),
                key: KEY.to_string(),
                mult_bp,
                hold,
                tariff_priced_ts: None,
                policy_fast: None,
                settlement_pricing: CodexSettlementPricing::LegacyScalar,
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
            false,
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
            false,
        );
        assert_eq!(priced.input_nano, 150_000 * 5_000 * 2);
        assert_eq!(priced.cached_nano, 100_000 * 500 * 2);
        assert_eq!(priced.cache_write_nano, 50_000 * 6_250 * 2);
        assert_eq!(priced.output_nano, 10 * 30_000 * 3 / 2);
    }

    #[test]
    fn reserve_covers_full_output_and_cache_write_rate() {
        let hold = reserve_cost(&model(), 1_000, None, 0, false);
        assert_eq!(hold, 1_000 * 6_250 + 128_000 * 30_000);
    }

    #[test]
    fn reserve_output_leg_follows_the_requested_cap() {
        let full = reserve_cost(&model(), 1_000, None, 0, false);
        // A small requested cap holds only that many output tokens, not the model's 128k max.
        let capped = reserve_cost(&model(), 1_000, Some(500), 0, false);
        assert_eq!(capped, 1_000 * 6_250 + 500 * 30_000);
        assert!(capped < full);
        // A cap above the model maximum is clamped to the model maximum.
        assert_eq!(
            reserve_cost(&model(), 1_000, Some(10_000_000), 0, false),
            full
        );
    }

    #[test]
    fn fast_mode_multiplies_reserve_settlement_ledger_and_capacity_spend() {
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 10,
            ..CodexUsage::default()
        };
        // Admission reserves every estimated input token at the most expensive possible input
        // bucket (a GPT-5.6 cache write); exact settlement below uses the actual fresh-input bucket.
        let fast_reserve = (100 * 6_250 + 10 * 30_000) * 5 / 2;
        let fast_usage = (100 * 5_000 + 10 * 30_000) * 5 / 2;

        assert_eq!(
            reserve_cost(&model(), 100, Some(10), 0, true),
            fast_reserve as i128
        );
        assert_eq!(
            price_real_nano(&model(), &usage, 0, true),
            fast_usage as i128
        );

        let (charge, event) = settled_charge(
            &model(),
            &usage,
            i64::MAX,
            10_000,
            None,
            0,
            true,
            CodexSettlementPricing::LegacyScalar,
        );
        assert_eq!(charge, fast_usage);
        let event = event.expect("fast usage must produce a usage event");
        assert_eq!(event.speed, "fast");
        assert_eq!(event.input_nano, 100 * 5_000 * 5 / 2);
        assert_eq!(event.output_nano, 10 * 30_000 * 5 / 2);
        assert_eq!(event.real_nano, fast_usage);
    }

    #[test]
    fn calibration_event_keeps_exact_api_credit_legs_and_reasoning_as_output_subset() {
        let usage = CodexUsage {
            input_tokens: 1_000,
            cached_input_tokens: 400,
            cache_write_input_tokens: 100,
            output_tokens: 100,
            reasoning_output_tokens: 80,
            ..CodexUsage::default()
        };
        let event = price_calibration_event(
            "turn-1",
            "home-1",
            &model(),
            &usage,
            1_785_369_601,
            true,
            Some("priority"),
        )
        .unwrap();
        assert_eq!(event.service_tier, "fast");
        assert_eq!(event.provider_reported_tier.as_deref(), Some("priority"));
        assert_eq!(
            event.api_tariff_schedule_id,
            "openai/gpt-5.6-sol/2026-07-30/v2"
        );
        assert_eq!(event.credit_schedule_id, metering::CODEX_CREDIT_SCHEDULE_ID);
        assert_eq!(event.reasoning_output_tokens, 80);
        // API Fast is 2x in this epoch.
        assert_eq!(event.api_input_nanousd, 5_000_000);
        assert_eq!(event.api_cached_input_nanousd, 400_000);
        assert_eq!(event.api_cache_write_nanousd, 1_250_000);
        assert_eq!(event.api_output_nanousd, 6_000_000);
        assert_eq!(event.api_total_nanousd, 12_650_000);
        // Subscription Fast is independently 2.5x; cache writes use fresh-input credits and
        // reasoning is not added again because it is already in output_tokens.
        assert_eq!(event.chatgpt_input_nanocredits, 187_500_000);
        assert_eq!(event.chatgpt_cached_input_nanocredits, 12_500_000);
        assert_eq!(event.chatgpt_output_nanocredits, 187_500_000);
        assert_eq!(event.chatgpt_total_nanocredits, 387_500_000);
    }

    #[test]
    fn settlement_caps_billed_output_but_records_actual_usage() {
        // The model generated 1_000 output tokens; the client asked for at most 100.
        let usage = CodexUsage {
            input_tokens: 100,
            output_tokens: 1_000,
            ..CodexUsage::default()
        };
        // Uncapped: bill every generated token (fixture rates: input 5_000, output 30_000).
        let (uncapped, _) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            None,
            0,
            false,
            CodexSettlementPricing::LegacyScalar,
        );
        assert_eq!(uncapped, 100 * 5_000 + 1_000 * 30_000);
        // Honest billing: charge only up to the requested 100 output tokens.
        let (capped, event) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            Some(100),
            0,
            false,
            CodexSettlementPricing::LegacyScalar,
        );
        assert_eq!(capped, 100 * 5_000 + 100 * 30_000);
        assert!(capped < uncapped);
        // The immutable provider-usage record still reflects the real consumption, so window
        // calibration and accounting stay truthful even though the customer paid less.
        let event = event.expect("positive usage must produce a usage event");
        assert_eq!(event.output_tokens, 1_000);
        assert_eq!(event.real_nano, 100 * 5_000 + 1_000 * 30_000);
    }

    #[tokio::test]
    async fn release_v2_settlement_keeps_the_requested_output_cap_and_does_not_use_strict_recovery()
    {
        const TOPUP: i64 = 50_000_000;
        const HOLD: i64 = 4_000_000;
        let (mut admission, billing, path) =
            reserved_admission(10_000, TOPUP, HOLD, "codex-release-v2-settle").await;
        let reservation = admission.reservation.as_mut().unwrap();
        reservation.tariff_priced_ts = Some(1);
        reservation.policy_fast = Some(false);
        reservation.settlement_pricing = CodexSettlementPricing::ReleaseV2;

        admission.settle(
            &settlement_model(),
            &CodexUsage {
                input_tokens: 100,
                output_tokens: 1_000,
                ..CodexUsage::default()
            },
            Some(100),
            false,
        );
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - 3_500_000);
        assert_eq!(account.spent_nano, 3_500_000);
        assert_eq!(account.reserved_nano, 0);
        let usage = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].charge_nano, 3_500_000);
        assert_eq!(usage[0].real_nano, 30_500_000);

        drop(billing);
        let _ = std::fs::remove_file(path);
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
        let priced = price_usage(&drifted, &usage, 0, false);
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
            false,
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
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            10_000_000,
            5_000,
            None,
            123,
            false,
            CodexSettlementPricing::LegacyScalar,
        );

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
    fn release_v2_settlement_floors_where_the_legacy_scalar_rounds_half_up() {
        // 1 input token × 5_000 nano = 5_000 real; × 5_001 bp = 25_005_000 / 10_000. The
        // fractional remainder (5_000) sits exactly on the half-up boundary: immutable legacy
        // arithmetic charges 2_501, the release-v2 contract floor charges exactly 2_500.
        let usage = CodexUsage {
            input_tokens: 1,
            ..CodexUsage::default()
        };
        let (legacy, _) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            5_001,
            None,
            123,
            false,
            CodexSettlementPricing::LegacyScalar,
        );
        let (release, _) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            5_001,
            None,
            123,
            false,
            CodexSettlementPricing::ReleaseV2,
        );
        assert_eq!(legacy, 2_501);
        assert_eq!(release, 2_500);
    }

    #[test]
    fn settlement_clamps_overrun_to_the_reserved_hold_plus_one_dollar() {
        let usage = CodexUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..CodexUsage::default()
        };
        let hold = 17;
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            hold,
            10_000,
            None,
            456,
            false,
            CodexSettlementPricing::LegacyScalar,
        );
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
            None,
            789,
            false,
            CodexSettlementPricing::LegacyScalar,
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
        let (charge, event) = settled_charge(
            &settlement_model(),
            &usage,
            i64::MAX,
            10_000,
            None,
            999,
            false,
            CodexSettlementPricing::LegacyScalar,
        );
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
    async fn strict_codex_uses_exact_policy_scope_and_funding_without_scalar_fallback() {
        let (app, billing, path) = strict_codex_fixture().await;
        let static_admission = strict_pending()
            .reserve(&app, &model(), 1, Some(1), 0, false)
            .await
            .unwrap();
        let static_request = static_admission
            .reservation
            .as_ref()
            .unwrap()
            .request_id
            .clone();
        let track_admission = strict_pending()
            .reserve(&app, &terra_model(), 1, Some(1), 0, false)
            .await
            .unwrap();
        let track_request = track_admission
            .reservation
            .as_ref()
            .unwrap()
            .request_id
            .clone();

        let conn = registry::open(path.to_str().unwrap()).unwrap();
        let funding_for = |request_id: &str| {
            conn.query_row(
                "SELECT group_concat(bucket_id, ',') FROM (
                     SELECT bucket_id FROM reservation_funding_allocations
                      WHERE request_id=?1 ORDER BY allocation_order
                 )",
                [request_id],
                |row| row.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(funding_for(&static_request), "strict-codex-paid");
        assert_eq!(funding_for(&track_request), "strict-codex-bonus");
        let snapshot_kinds: (i64, i64) = conn
            .query_row(
                "SELECT COUNT(*) FILTER (WHERE snapshot_kind='policy_v1'),
                        COUNT(*) FILTER (WHERE snapshot_kind='legacy_scalar')
                   FROM pricing_admission_snapshots",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(snapshot_kinds, (2, 0));
        assert_eq!(
            app.metrics.strict_pricing_admitted_count(
                crate::StrictPricingProvider::OpenAi,
                PricingMode::Discount,
                true,
            ),
            1
        );
        assert_eq!(
            app.metrics.strict_pricing_admitted_count(
                crate::StrictPricingProvider::OpenAi,
                PricingMode::Track,
                false,
            ),
            1
        );
        drop(conn);

        drop(static_admission);
        drop(track_admission);
        billing.flush().await.unwrap();
        let account = billing
            .account("strict-codex-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (account.balance_nano, account.reserved_nano),
            (1_000_000, 0)
        );
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn strict_codex_over_hold_usage_is_left_for_full_hold_recovery() {
        let (app, billing, path) = strict_codex_fixture().await;
        let admission = strict_pending()
            .reserve(&app, &model(), 1, Some(1), 0, false)
            .await
            .unwrap();
        let reservation = admission.reservation.as_ref().unwrap();
        let request_id = reservation.request_id.clone();
        let hold = reservation.hold;
        admission.settle(
            &model(),
            &CodexUsage {
                input_tokens: u64::MAX,
                output_tokens: u64::MAX,
                ..CodexUsage::default()
            },
            Some(1),
            false,
        );
        billing.flush().await.unwrap();

        let conn = registry::open(path.to_str().unwrap()).unwrap();
        let durable: (String, i64) = conn
            .query_row(
                "SELECT state,
                        (SELECT COUNT(*) FROM billing_settlement_outbox
                          WHERE request_id=?1)
                   FROM billing_reservations WHERE request_id=?1",
                [request_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(durable, ("reserved".to_string(), 0));
        let account = billing
            .account("strict-codex-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.reserved_nano, hold);

        drop(conn);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn strict_codex_missing_policy_fails_before_any_scalar_reservation() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-missing-policy-{}-{unique}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1).expect("start billing"),
        );
        billing
            .create_account("missing-policy-account", None, 10_000)
            .await
            .unwrap();
        billing
            .topup(
                "missing-policy-account",
                100_000,
                Some("missing-policy-seed"),
            )
            .await
            .unwrap();
        let app = bridge_app(Arc::clone(&billing), PricingBridgeConfig::disabled());

        let result = reserve_codex_strict(
            &app,
            &billing,
            "missing-policy-account",
            "missing-policy-key",
            &model(),
            1,
            Some(1),
            0,
            false,
            Some(100_000),
            Some(100_000),
            "123e4567-e89b-42d3-a456-426614174000",
            &registry::ExecutionAttempt::direct(),
        )
        .await;
        assert!(matches!(result, Err(AdmissionError::Unavailable)));
        assert_eq!(
            app.metrics.strict_pricing_rejected_count(
                crate::StrictPricingProvider::OpenAi,
                crate::StrictPricingRejectionReason::MissingPolicy,
            ),
            1
        );
        let conn = registry::open(path.to_str().unwrap()).unwrap();
        let reservations: i64 = conn
            .query_row("SELECT COUNT(*) FROM billing_reservations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(reservations, 0);

        drop(conn);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn sampled_codex_admission_atomically_persists_snapshot_and_keeps_cancel_lifecycle() {
        let config = PricingBridgeConfig::from_parts(true, 10_000).unwrap();
        let (admission, app, billing, path) =
            pending_bridge_admission(config, registry::ExecutionAttempt::direct()).await;
        let reservation = admission.reservation.as_ref().unwrap();
        let request_id = reservation.request_id.clone();
        let hold = reservation.hold;

        assert_eq!(
            app.metrics
                .pricing_bridge_selected_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            app.metrics
                .pricing_bridge_inserted_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            app.metrics
                .pricing_bridge_latency_count(SnapshotProvider::OpenAi),
            1
        );
        assert_eq!(
            app.metrics.pricing_bridge_fallback_count(
                SnapshotProvider::OpenAi,
                PricingBridgeFallbackReason::BridgeDisabled,
            ),
            0
        );

        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let snapshot =
            registry::pricing::sqlite_legacy_scalar_admission_snapshot(&connection, &request_id)
                .unwrap()
                .expect("sampled admission must persist its actual snapshot");
        assert_eq!(snapshot.provider(), SnapshotProvider::OpenAi);
        assert_eq!(snapshot.account_id(), "codex-bridge-account");
        assert_eq!(snapshot.requested_model_id(), "gpt-5.6-sol");
        assert_eq!(snapshot.charged_hold_nano(), hold);

        drop(admission);
        billing.flush().await.unwrap();
        let account = billing
            .account("codex-bridge-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, 20_000_000);
        assert_eq!(account.reserved_nano, 0);
        assert!(registry::pricing::sqlite_legacy_scalar_admission_snapshot(
            &connection,
            &request_id,
        )
        .unwrap()
        .is_some());

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn dormant_release_keeps_the_legacy_zero_balance_gate() {
        const ACCOUNT: &str = "codex-zero-balance";
        const KEY: &str = "codex-zero-balance-key";

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = NEXT_SETTLEMENT_DB.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "claude-api-codex-zero-balance-{}-{unique}-{sequence}.sqlite",
            std::process::id(),
        ));
        let billing = Arc::new(
            AsyncBilling::start(path.to_string_lossy().into_owned(), 1)
                .expect("start zero-balance test billing"),
        );
        billing.create_account(ACCOUNT, None, 2_000).await.unwrap();
        billing
            .issue_key(KEY, ACCOUNT, None, None, None)
            .await
            .unwrap();
        let app = bridge_app(Arc::clone(&billing), PricingBridgeConfig::disabled());

        let result = reserve_codex_metered(
            &app,
            &billing,
            ACCOUNT,
            KEY,
            &model(),
            100,
            Some(10),
            0,
            false,
            2_000,
            0,
            false,
            None,
            None,
            &registry::ExecutionAttempt::direct(),
        )
        .await;
        assert!(matches!(result, Err(AdmissionError::LowBalance)));
        let account = billing.account(ACCOUNT).await.unwrap().unwrap();
        assert_eq!((account.balance_nano, account.reserved_nano), (0, 0));
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let reservations: i64 = connection
            .query_row("SELECT COUNT(*) FROM billing_reservations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(reservations, 0);

        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn disabled_codex_bridge_preserves_scalar_reserve_without_snapshot() {
        let (admission, app, billing, path) = pending_bridge_admission(
            PricingBridgeConfig::disabled(),
            registry::ExecutionAttempt::direct(),
        )
        .await;
        let request_id = admission.reservation.as_ref().unwrap().request_id.clone();

        assert_eq!(
            app.metrics
                .pricing_bridge_selected_count(SnapshotProvider::OpenAi),
            0
        );
        assert_eq!(
            app.metrics.pricing_bridge_fallback_count(
                SnapshotProvider::OpenAi,
                PricingBridgeFallbackReason::BridgeDisabled,
            ),
            1
        );
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        assert!(registry::pricing::sqlite_legacy_scalar_admission_snapshot(
            &connection,
            &request_id,
        )
        .unwrap()
        .is_none());

        drop(admission);
        billing.flush().await.unwrap();
        drop(connection);
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn codex_reservation_persists_router_execution_identity() {
        const GROUP: &str = "728f47a2-9b2d-4dc4-8f11-4d43b7d8b62a";
        let (admission, app, billing, path) = pending_bridge_admission(
            PricingBridgeConfig::disabled(),
            registry::ExecutionAttempt::grouped(GROUP, 8).unwrap(),
        )
        .await;
        let request_id = admission.reservation.as_ref().unwrap().request_id.clone();
        let connection = registry::open(path.to_str().unwrap()).unwrap();
        let identity: (Option<String>, i32) = connection
            .query_row(
                "SELECT group_id,attempt FROM billing_reservations WHERE request_id=?1",
                [request_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(identity, (Some(GROUP.into()), 8));

        drop(connection);
        drop(admission);
        billing.flush().await.unwrap();
        drop(app);
        drop(billing);
        let _ = std::fs::remove_file(path);
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

        admission.settle(&settlement_model(), &usage, None, false);
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

    #[test]
    fn openai_image_settlement_preserves_modality_cost_buckets() {
        let usage = metering::OpenAiImageUsage {
            total_text_input_tokens: 100,
            cached_text_input_tokens: 40,
            total_image_input_tokens: 200,
            cached_image_input_tokens: 50,
            image_output_tokens: 10,
        };
        let (charge, event) = settled_openai_image_charge(
            metering::GPT_IMAGE_2_ALIAS,
            &usage,
            10_000_000,
            5_000,
            1_800_000_000,
            CodexSettlementPricing::LegacyScalar,
        );
        assert_eq!(charge, 975_000);
        let event = event.unwrap();
        assert_eq!(event.provider, registry::PROVIDER_OPENAI);
        assert_eq!(event.model, metering::GPT_IMAGE_2_ALIAS);
        assert_eq!(event.input_tokens, 300);
        assert_eq!(event.output_tokens, 10);
        assert_eq!(event.cache_read_tokens, 90);
        assert_eq!(event.input_nano, 1_500_000);
        assert_eq!(event.cache_read_nano, 150_000);
        assert_eq!(event.output_nano, 300_000);
        assert_eq!(event.real_nano, 1_950_000);
        assert_eq!(event.priced_ts, 1_800_000_000);
    }

    #[tokio::test]
    async fn image_settlement_closes_hold_and_persists_exact_usage() {
        const TOPUP: i64 = 20_000_000;
        const HOLD: i64 = 10_000_000;
        let (mut codex_admission, billing, path) =
            reserved_admission(5_000, TOPUP, HOLD, "image-exact-settlement").await;
        let mut admission = OpenAiImageAdmission {
            reservation: codex_admission.reservation.take(),
        };
        assert_eq!(admission.request_id(), Some("image-exact-settlement"));
        admission.mark_delivering().await.unwrap();
        admission.settle(
            metering::GPT_IMAGE_2_ALIAS,
            &metering::OpenAiImageUsage {
                total_text_input_tokens: 100,
                cached_text_input_tokens: 40,
                total_image_input_tokens: 200,
                cached_image_input_tokens: 50,
                image_output_tokens: 10,
            },
        );
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - 975_000);
        assert_eq!(account.spent_nano, 975_000);
        assert_eq!(account.reserved_nano, 0);
        let usage = billing
            .usage_by_model("codex-settlement-account", 0)
            .await
            .unwrap();
        assert_eq!(usage.len(), 1);
        assert_eq!(usage[0].provider, registry::PROVIDER_OPENAI);
        assert_eq!(usage[0].model, metering::GPT_IMAGE_2_ALIAS);
        assert_eq!(usage[0].charge_nano, 975_000);
        assert_eq!(usage[0].real_nano, 1_950_000);

        drop(billing);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn image_delivery_fence_prevents_cancellation_refund() {
        const TOPUP: i64 = 20_000_000;
        const HOLD: i64 = 10_000_000;
        let (mut codex_admission, billing, path) =
            reserved_admission(10_000, TOPUP, HOLD, "image-delivery-fence").await;
        let mut admission = OpenAiImageAdmission {
            reservation: codex_admission.reservation.take(),
        };

        admission.mark_delivering().await.unwrap();
        drop(admission);
        billing.flush().await.unwrap();

        let account = billing
            .account("codex-settlement-account")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.balance_nano, TOPUP - HOLD);
        assert_eq!(account.spent_nano, 0);
        assert_eq!(account.reserved_nano, HOLD);

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
