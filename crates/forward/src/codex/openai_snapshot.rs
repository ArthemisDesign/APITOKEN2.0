//! OpenAI legacy quote/snapshot builder for the default-off atomic bridge.
//!
//! Pricing comes from the existing private `billing::reserve_cost` function and the audited
//! `metering` capability. The caller owns rollout selection and persistence; this module has no DB,
//! env, metrics or clock and cannot invent provider, tariff, modifier or hold identity.

use super::{billing::reserve_cost_with_prices, CodexModel};
#[cfg(test)]
use super::billing::reserve_cost;
use crate::pricing::tariff_book::{self, PinnedTariff};
use crate::pricing::{
    snapshot_identity_is_oversized, EnginePricingRequestId, PricingBridgeFallbackReason,
    PricingBridgePrepare,
};
use anyhow::{ensure, Context, Result};
use metering::{
    CodexAdmissionTariffIdentity, CodexContextTier, CodexServiceTier, TariffIdentityError,
};
use registry::pricing::{
    LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot, LegacyScalarAdmissionSnapshotInput,
    SnapshotOpenAiContextTier, SnapshotOpenAiServiceTier, SnapshotProvider,
};

pub(super) struct CodexLegacyQuoteInput {
    pub request_id: EnginePricingRequestId,
    pub account_id: String,
    pub model: CodexModel,
    pub quote_ts: i64,
    pub payable_multiplier_bp: i64,
    pub estimated_input_tokens: u64,
    pub reserve_overhead_tokens: u64,
    pub requested_output_tokens: Option<u64>,
    pub fast: bool,
}

pub(super) struct PreparedCodexLegacyQuote {
    request_id: EnginePricingRequestId,
    account_id: String,
    requested_model_id: String,
    quote_ts: i64,
    payable_multiplier_bp: i64,
    identity: CodexAdmissionTariffIdentity,
    official_hold_nano: i64,
    /// `<family>/v<version>` of the pinned hot override, or the compiled schedule id as today.
    tariff_schedule_id: String,
    pin: Option<PinnedTariff>,
}

pub(super) struct CodexLegacyQuote {
    snapshot: LegacyScalarAdmissionSnapshot,
    pin: Option<PinnedTariff>,
}

impl CodexLegacyQuote {
    pub(super) fn snapshot(&self) -> &LegacyScalarAdmissionSnapshot {
        &self.snapshot
    }

    /// The hot override version this quote priced with; settlement replays exactly this version.
    pub(super) fn pinned_tariff(&self) -> Option<PinnedTariff> {
        self.pin.clone()
    }

    pub(super) fn into_snapshot(self) -> LegacyScalarAdmissionSnapshot {
        self.snapshot
    }
}

pub(super) fn prepare_codex_legacy_quote(
    input: CodexLegacyQuoteInput,
) -> Result<PricingBridgePrepare<PreparedCodexLegacyQuote>> {
    ensure!(
        input.quote_ts > 0,
        "OpenAI bridge quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "OpenAI bridge multiplier is outside the legacy snapshot contract"
    );
    if snapshot_identity_is_oversized(&input.account_id)
        || snapshot_identity_is_oversized(&input.model.id)
    {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::SnapshotIdentityOversized,
        ));
    }

    let estimated_input_tokens = input
        .estimated_input_tokens
        .saturating_add(input.reserve_overhead_tokens);
    let service_tier = if input.fast {
        CodexServiceTier::Fast
    } else {
        CodexServiceTier::Standard
    };
    let identity = match metering::codex_tariff_capability_at(
        &input.model.id,
        input.quote_ts,
        service_tier,
        estimated_input_tokens,
    ) {
        Ok(identity) => identity,
        Err(TariffIdentityError::UnsupportedModelIdentity) => {
            return Ok(PricingBridgePrepare::Fallback(
                PricingBridgeFallbackReason::UnsupportedModelIdentity,
            ));
        }
        Err(TariffIdentityError::UnsupportedModifier) => {
            return Ok(PricingBridgePrepare::Fallback(
                PricingBridgeFallbackReason::UnsupportedModifier,
            ));
        }
        Err(TariffIdentityError::InvalidPricedTimestamp) => {
            anyhow::bail!("OpenAI canonicalizer rejected a prevalidated quote timestamp");
        }
    };

    ensure!(
        input.model.upstream == identity.canonical_model_id,
        "OpenAI runtime model upstream differs from its audited canonical identity"
    );
    ensure!(
        input.model.max_output_tokens == identity.max_output_tokens,
        "OpenAI runtime max-output limit differs from its audited capability"
    );
    if input.fast {
        ensure!(
            input.model.fast_multiplier_basis_points.is_some(),
            "OpenAI runtime Fast capability is disabled for an audited Fast quote"
        );
    }

    // Hot tariff override: the book replaces only the base price vector of the compiled family;
    // the long-context and Fast modifiers stay code-applied on top inside the reserve formula.
    let (prices, tariff_schedule_id, pin) = resolve_reserve_tariff(
        &tariff_book::snapshot(),
        &input.model,
        input.quote_ts,
        identity.tariff_schedule_id.as_str(),
    );
    let official_hold_nano = reserve_cost_with_prices(
        &input.model,
        prices,
        estimated_input_tokens,
        input.requested_output_tokens,
        input.fast,
    );
    let official_hold_nano = match i64::try_from(official_hold_nano) {
        Ok(value) => value,
        Err(_) => {
            return Ok(PricingBridgePrepare::Fallback(
                PricingBridgeFallbackReason::OfficialHoldOutOfRange,
            ));
        }
    };

    Ok(PricingBridgePrepare::Eligible(PreparedCodexLegacyQuote {
        request_id: input.request_id,
        account_id: input.account_id,
        requested_model_id: input.model.id,
        quote_ts: input.quote_ts,
        payable_multiplier_bp: input.payable_multiplier_bp,
        identity,
        official_hold_nano,
        tariff_schedule_id,
        pin,
    }))
}

/// The reserve tariff decision, pure for testing: the override base vector and pin when the book
/// resolves the compiled family at `quote_ts`, compiled constants otherwise.
fn resolve_reserve_tariff(
    book: &tariff_book::TariffBookSnapshot,
    model: &CodexModel,
    quote_ts: i64,
    compiled_schedule_id: &str,
) -> (metering::CodexPrices, String, Option<PinnedTariff>) {
    let compiled = super::billing::effective_prices(model, quote_ts);
    let resolved = match metering::codex_matched_tariff_at(&model.id, quote_ts) {
        Some((family, _)) => {
            tariff_book::reserve_base(book, family, quote_ts, compiled, tariff_book::as_codex)
        }
        None => tariff_book::ReserveBase {
            prices: compiled,
            pin: None,
        },
    };
    let tariff_schedule_id = resolved
        .pin
        .as_ref()
        .map(|pin| pin.schedule_id.clone())
        .unwrap_or_else(|| compiled_schedule_id.to_owned());
    (resolved.prices, tariff_schedule_id, resolved.pin)
}

impl PreparedCodexLegacyQuote {
    /// Apply only the existing account-balance cap to the frozen official quote. `None` matches the
    /// live admission precondition for a non-positive available balance.
    pub(super) fn quote(&self, available_nano: i64) -> Result<Option<CodexLegacyQuote>> {
        if available_nano <= 0 {
            return Ok(None);
        }
        let charged_hold_nano =
            metering::apply_multiplier(self.official_hold_nano as i128, self.payable_multiplier_bp)
                .clamp(1, i64::MAX as i128) as i64;
        let charged_hold_nano = charged_hold_nano.min(available_nano.max(1));
        self.build_quote(charged_hold_nano).map(Some)
    }

    /// The service meter-only strict lane: the same frozen official identity with an exactly zero
    /// charged hold and no balance gate. Only a service-class payable-0 resolution may call it;
    /// the legacy clamp-to-one above is the deliberate customer-class admission minimum and is
    /// not reused here.
    pub(super) fn quote_service_meter_only(&self) -> Result<CodexLegacyQuote> {
        self.build_quote(0)
    }

    fn build_quote(&self, charged_hold_nano: i64) -> Result<CodexLegacyQuote> {
        let modifiers = LegacyPremiumModifiers::OpenAiV1 {
            service_tier: match self.identity.modifiers.service_tier {
                CodexServiceTier::Standard => SnapshotOpenAiServiceTier::Standard,
                CodexServiceTier::Fast => SnapshotOpenAiServiceTier::Fast,
            },
            service_tier_multiplier_basis_points: self
                .identity
                .modifiers
                .service_tier_multiplier_basis_points,
            context_tier: match self.identity.modifiers.context_tier {
                CodexContextTier::Standard => SnapshotOpenAiContextTier::Standard,
                CodexContextTier::Long => SnapshotOpenAiContextTier::Long,
            },
            input_multiplier_basis_points: self.identity.modifiers.input_multiplier_basis_points,
            output_multiplier_basis_points: self.identity.modifiers.output_multiplier_basis_points,
        };
        let snapshot = LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: self.request_id.as_str().to_owned(),
            account_id: self.account_id.clone(),
            provider: SnapshotProvider::OpenAi,
            requested_model_id: self.requested_model_id.clone(),
            canonical_model_id: self.identity.canonical_model_id.to_owned(),
            alias_generation: self.identity.alias_generation,
            tariff_schedule_id: self.tariff_schedule_id.clone(),
            tariff_priced_ts: self.quote_ts,
            admission_ts: self.quote_ts,
            payable_multiplier_bp: self.payable_multiplier_bp,
            official_hold_nano: self.official_hold_nano,
            charged_hold_nano,
            premium_modifiers: modifiers,
        })
        .context("build validated OpenAI legacy admission snapshot")?;

        Ok(CodexLegacyQuote {
            snapshot,
            pin: self.pin.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTE_TS: i64 = 1_800_000_000;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    fn model(model_id: &str) -> CodexModel {
        let spec = metering::codex_catalog_at(QUOTE_TS)
            .into_iter()
            .find(|spec| spec.id == model_id)
            .unwrap();
        CodexModel {
            id: spec.id.to_string(),
            upstream: spec.upstream.to_string(),
            created: 0,
            owned_by: "test".to_string(),
            max_output_tokens: spec.max_output_tokens,
            reasoning_efforts: spec
                .reasoning_efforts
                .iter()
                .map(|effort| (*effort).to_string())
                .collect(),
            input_modalities: vec!["text".to_string(), "image".to_string()],
            output_modalities: vec!["text".to_string()],
            tool_calling: true,
            structured_outputs: true,
            fast_multiplier_basis_points: spec.subscription_fast_multiplier_basis_points,
            prices: spec.prices,
        }
    }

    fn input(
        model_id: &str,
        estimated_input_tokens: u64,
        requested_output_tokens: Option<u64>,
        fast: bool,
    ) -> CodexLegacyQuoteInput {
        CodexLegacyQuoteInput {
            request_id: request_id(),
            account_id: "account-1".to_string(),
            model: model(model_id),
            quote_ts: QUOTE_TS,
            payable_multiplier_bp: 2_000,
            estimated_input_tokens,
            reserve_overhead_tokens: 0,
            requested_output_tokens,
            fast,
        }
    }

    fn eligible(input: CodexLegacyQuoteInput) -> PreparedCodexLegacyQuote {
        match prepare_codex_legacy_quote(input).unwrap() {
            PricingBridgePrepare::Eligible(prepared) => prepared,
            PricingBridgePrepare::Fallback(reason) => panic!("unexpected fallback: {reason:?}"),
        }
    }

    #[test]
    fn every_catalog_quote_matches_the_existing_reserve_formula() {
        let model_ids: Vec<_> = metering::codex_catalog_at(QUOTE_TS)
            .into_iter()
            .map(|spec| spec.id)
            .collect();
        for model_id in model_ids {
            for fast in [false, true] {
                for estimated_input_tokens in [272_000, 272_001] {
                    for requested_output_tokens in [None, Some(500), Some(u64::MAX)] {
                        for multiplier in [0, 1, 900, 2_000, 10_000] {
                            let legacy_model = model(model_id);
                            let expected = reserve_cost(
                                &legacy_model,
                                estimated_input_tokens,
                                requested_output_tokens,
                                QUOTE_TS,
                                fast,
                            );
                            let mut quote_input = input(
                                model_id,
                                estimated_input_tokens,
                                requested_output_tokens,
                                fast,
                            );
                            quote_input.payable_multiplier_bp = multiplier;
                            let prepared = eligible(quote_input);
                            assert_eq!(prepared.official_hold_nano as i128, expected);
                            let quote = prepared.quote(i64::MAX).unwrap().unwrap();
                            let snapshot = quote.snapshot();
                            assert_eq!(snapshot.provider(), SnapshotProvider::OpenAi);
                            assert_eq!(snapshot.requested_model_id(), model_id);
                            assert_eq!(snapshot.official_hold_nano() as i128, expected);
                            assert_eq!(
                                snapshot.charged_hold_nano(),
                                metering::apply_multiplier(expected, multiplier)
                                    .clamp(1, i64::MAX as i128)
                                    as i64
                            );
                            assert_eq!(snapshot.tariff_priced_ts(), QUOTE_TS);
                            assert_eq!(snapshot.admission_ts(), QUOTE_TS);
                            snapshot.validate().unwrap();
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn overhead_crosses_the_strict_long_context_boundary() {
        let mut boundary = input("gpt-5.6-sol", 271_999, Some(1), false);
        boundary.reserve_overhead_tokens = 2;
        let quote = eligible(boundary).quote(i64::MAX).unwrap().unwrap();
        assert!(matches!(
            quote.snapshot().premium_modifiers(),
            LegacyPremiumModifiers::OpenAiV1 {
                context_tier: SnapshotOpenAiContextTier::Long,
                input_multiplier_basis_points: 20_000,
                output_multiplier_basis_points: 15_000,
                ..
            }
        ));
    }

    #[test]
    fn public_alias_and_balance_cap_preserve_frozen_official_identity() {
        let prepared = eligible(input("gpt-5.6", 1_000, Some(1_000), true));
        let full = prepared.quote(i64::MAX).unwrap().unwrap();
        let capped = prepared.quote(123).unwrap().unwrap();
        assert_eq!(full.snapshot().requested_model_id(), "gpt-5.6");
        assert_eq!(full.snapshot().canonical_model_id(), "gpt-5.6-sol");
        assert_eq!(
            full.snapshot().tariff_schedule_id(),
            "openai/gpt-5.6-sol/2026-07-30/v2"
        );
        assert!(matches!(
            full.snapshot().premium_modifiers(),
            LegacyPremiumModifiers::OpenAiV1 {
                service_tier: SnapshotOpenAiServiceTier::Fast,
                service_tier_multiplier_basis_points: 20_000,
                ..
            }
        ));
        assert_eq!(
            full.snapshot().official_hold_nano(),
            capped.snapshot().official_hold_nano()
        );
        assert_eq!(capped.snapshot().charged_hold_nano(), 123);
        assert_ne!(
            full.snapshot().snapshot_digest(),
            capped.snapshot().snapshot_digest()
        );
        assert!(prepared.quote(0).unwrap().is_none());

        let mut zero_scalar = input("gpt-5.6", 1_000, Some(1_000), false);
        zero_scalar.payable_multiplier_bp = 0;
        let zero_scalar = eligible(zero_scalar).quote(i64::MAX).unwrap().unwrap();
        assert!(zero_scalar.snapshot().official_hold_nano() > 0);
        // This is the exact existing Codex admission minimum; shadow later rejects it as a
        // balance/scalar-capped actual rather than rewriting history.
        assert_eq!(zero_scalar.snapshot().charged_hold_nano(), 1);
    }

    /// An override row replaces the reserve base vector and pins its exact version; the compiled
    /// schedule id and prices are untouched while the book is empty.
    #[test]
    fn an_override_replaces_the_reserve_base_and_pins_its_version() {
        let model = model("gpt-5.6-sol");
        let compiled = crate::codex::billing::effective_prices(&model, QUOTE_TS);
        const COMPILED_ID: &str = "openai/gpt-5.6-sol/2026-07-30/v2";
        let (prices, schedule_id, pin) = resolve_reserve_tariff(
            &tariff_book::TariffBookSnapshot::empty(),
            &model,
            QUOTE_TS,
            COMPILED_ID,
        );
        assert_eq!(prices, compiled);
        assert_eq!(schedule_id, COMPILED_ID);
        assert!(pin.is_none());

        let book = tariff_book::TariffBookSnapshot::from_rows(vec![tariff_book::test_row(
            "openai/codex/gpt-5.6-sol",
            2,
            0,
            serde_json::json!({
                "input": "10000",
                "cached_input": "1000",
                "cache_write_input": "12500",
                "output": "60000",
                "api_fast_multiplier_basis_points": 25000,
                "long_context_threshold": 272000,
                "long_input_basis_points": 20000,
                "long_output_basis_points": 15000
            }),
        )])
        .unwrap();
        let (prices, schedule_id, pin) =
            resolve_reserve_tariff(&book, &model, QUOTE_TS, COMPILED_ID);
        assert_eq!(prices.input, 10_000);
        assert_eq!(prices.output, 60_000);
        assert_eq!(schedule_id, "openai/codex/gpt-5.6-sol/v2");
        let pin = pin.expect("the override version is pinned");
        assert_eq!(pin.family, "openai/codex/gpt-5.6-sol");
        assert_eq!(pin.version, 2);
        // The hold formula runs on the override vector: input bills the max(input, cache-write)
        // rate, output the override output rate.
        let hold = reserve_cost_with_prices(&model, prices, 1_000, Some(500), false);
        assert_eq!(hold, 1_000 * 12_500 + 500 * 60_000);
    }

    #[test]
    fn expected_ineligible_quotes_fallback_before_money() {
        let mut unknown = input("gpt-5.6-sol", 1_000, Some(1_000), false);
        unknown.model.id = "gpt-unknown".to_string();
        unknown.model.upstream = "gpt-unknown".to_string();
        assert!(matches!(
            prepare_codex_legacy_quote(unknown).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::UnsupportedModelIdentity)
        ));

        let mut oversized_identity = input("gpt-5.6-sol", 1_000, Some(1_000), false);
        oversized_identity.account_id = "a".repeat(513);
        assert!(matches!(
            prepare_codex_legacy_quote(oversized_identity).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::SnapshotIdentityOversized)
        ));

        assert!(matches!(
            prepare_codex_legacy_quote(input("gpt-5.6-sol", u64::MAX, None, true)).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::OfficialHoldOutOfRange)
        ));
    }

    #[test]
    fn malformed_trusted_capability_is_a_hard_error() {
        let mut wrong_upstream = input("gpt-5.6", 1_000, Some(1_000), false);
        wrong_upstream.model.upstream = "gpt-5.6-terra".to_string();
        assert!(prepare_codex_legacy_quote(wrong_upstream).is_err());

        let mut wrong_output_limit = input("gpt-5.6", 1_000, Some(1_000), false);
        wrong_output_limit.model.max_output_tokens -= 1;
        assert!(prepare_codex_legacy_quote(wrong_output_limit).is_err());

        let mut invalid_clock = input("gpt-5.6-sol", 1_000, Some(1_000), false);
        invalid_clock.quote_ts = 0;
        assert!(prepare_codex_legacy_quote(invalid_clock).is_err());

        let mut invalid_scalar = input("gpt-5.6-sol", 1_000, Some(1_000), false);
        invalid_scalar.payable_multiplier_bp = 10_001;
        assert!(prepare_codex_legacy_quote(invalid_scalar).is_err());
    }
}
