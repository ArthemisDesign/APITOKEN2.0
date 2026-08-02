//! Gemini legacy quote/snapshot builder for the default-off atomic bridge.
//!
//! The builder freezes one `metering::gemini` price epoch and reuses the exact legacy reserve
//! arithmetic from `billing`. It has no DB, env, metrics or clock; the live caller owns rollout
//! selection and persistence.

use super::{
    billing::{reservation_for_budget_with_prices, reserve_cost_with_prices},
    config::GeminiModel,
};
use crate::pricing::{
    snapshot_identity_is_oversized, EnginePricingRequestId, PricingBridgeFallbackReason,
    PricingBridgePrepare,
};
use anyhow::{ensure, Context, Result};
use registry::pricing::{
    LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot, LegacyScalarAdmissionSnapshotInput,
    SnapshotGeminiContextRate, SnapshotGeminiSearchBilling, SnapshotProvider,
};

const GEMINI_ALIAS_GENERATION: i64 = 1;

pub(super) struct GeminiLegacyQuoteInput {
    pub request_id: EnginePricingRequestId,
    pub account_id: String,
    pub model: GeminiModel,
    pub quote_ts: i64,
    pub payable_multiplier_bp: i64,
    pub estimated_input_tokens: u64,
    pub requested_output_tokens: u64,
    pub image_output_tokens: u64,
    pub grounding_enabled: bool,
    pub allow_output_cap: bool,
}

pub(super) struct PreparedGeminiLegacyQuote {
    request_id: EnginePricingRequestId,
    account_id: String,
    requested_model_id: String,
    quote_ts: i64,
    payable_multiplier_bp: i64,
    prices: metering::GeminiPrices,
    estimated_input_tokens: u64,
    requested_output_tokens: u64,
    image_output_tokens: u64,
    grounding_enabled: bool,
    allow_output_cap: bool,
}

pub(super) struct GeminiLegacyQuote {
    effective_output_tokens: u64,
    snapshot: LegacyScalarAdmissionSnapshot,
}

impl GeminiLegacyQuote {
    pub(super) const fn effective_output_tokens(&self) -> u64 {
        self.effective_output_tokens
    }

    pub(super) fn snapshot(&self) -> &LegacyScalarAdmissionSnapshot {
        &self.snapshot
    }

    pub(super) fn into_snapshot(self) -> LegacyScalarAdmissionSnapshot {
        self.snapshot
    }
}

pub(super) fn prepare_gemini_legacy_quote(
    input: GeminiLegacyQuoteInput,
) -> Result<PricingBridgePrepare<PreparedGeminiLegacyQuote>> {
    ensure!(
        input.quote_ts > 0,
        "Gemini bridge quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "Gemini bridge multiplier is outside the legacy snapshot contract"
    );
    if snapshot_identity_is_oversized(&input.account_id)
        || snapshot_identity_is_oversized(&input.model.id)
    {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::SnapshotIdentityOversized,
        ));
    }

    let Some(spec) = metering::gemini_catalog_at(input.quote_ts)
        .into_iter()
        .find(|spec| spec.id == input.model.id)
    else {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::UnsupportedModelIdentity,
        ));
    };
    ensure!(
        input.model.input_token_limit == spec.input_token_limit
            && input.model.output_token_limit == spec.output_token_limit,
        "Gemini runtime model limits differ from the audited capability"
    );
    let prices = spec.prices;
    let maximum_official_hold = reserve_cost_with_prices(
        prices,
        input.estimated_input_tokens,
        input.requested_output_tokens.max(1),
        input.image_output_tokens,
        input.grounding_enabled,
    );
    if i64::try_from(maximum_official_hold).is_err() {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::OfficialHoldOutOfRange,
        ));
    }

    Ok(PricingBridgePrepare::Eligible(PreparedGeminiLegacyQuote {
        request_id: input.request_id,
        account_id: input.account_id,
        requested_model_id: input.model.id,
        quote_ts: input.quote_ts,
        payable_multiplier_bp: input.payable_multiplier_bp,
        prices,
        estimated_input_tokens: input.estimated_input_tokens,
        requested_output_tokens: input.requested_output_tokens,
        image_output_tokens: input.image_output_tokens,
        grounding_enabled: input.grounding_enabled,
        allow_output_cap: input.allow_output_cap,
    }))
}

impl PreparedGeminiLegacyQuote {
    pub(super) fn quote(&self, available_nano: i64) -> Result<Option<GeminiLegacyQuote>> {
        let Some((effective_output_tokens, charged_hold_nano)) = reservation_for_budget_with_prices(
            self.prices,
            self.estimated_input_tokens,
            self.requested_output_tokens,
            self.image_output_tokens,
            self.grounding_enabled,
            self.allow_output_cap,
            self.payable_multiplier_bp,
            available_nano,
        ) else {
            return Ok(None);
        };
        let official_hold_nano = i64::try_from(reserve_cost_with_prices(
            self.prices,
            self.estimated_input_tokens,
            effective_output_tokens,
            self.image_output_tokens,
            self.grounding_enabled,
        ))
        .context("prepared Gemini official hold exceeded its checked maximum")?;
        let expected_charged_hold =
            metering::apply_multiplier(i128::from(official_hold_nano), self.payable_multiplier_bp)
                .clamp(1, i64::MAX as i128)
                .min(i128::from(available_nano.max(1))) as i64;
        ensure!(
            charged_hold_nano == expected_charged_hold,
            "Gemini snapshot quote drifted from the live legacy reserve"
        );

        let (search_billing, search_reserve_units) = match self.prices.search {
            metering::GeminiSearchBilling::PerQuery { .. } => (
                SnapshotGeminiSearchBilling::PerQuery,
                if self.grounding_enabled { 32 } else { 0 },
            ),
            metering::GeminiSearchBilling::PerGroundedPrompt { .. } => (
                SnapshotGeminiSearchBilling::PerGroundedPrompt,
                if self.grounding_enabled { 1 } else { 0 },
            ),
        };
        let snapshot = LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: self.request_id.as_str().to_owned(),
            account_id: self.account_id.clone(),
            provider: SnapshotProvider::Google,
            requested_model_id: self.requested_model_id.clone(),
            canonical_model_id: self.requested_model_id.clone(),
            alias_generation: GEMINI_ALIAS_GENERATION,
            tariff_schedule_id: metering::gemini::TARIFF_SCHEDULE_ID.to_owned(),
            tariff_priced_ts: self.quote_ts,
            admission_ts: self.quote_ts,
            payable_multiplier_bp: self.payable_multiplier_bp,
            official_hold_nano,
            charged_hold_nano,
            premium_modifiers: LegacyPremiumModifiers::GeminiV1 {
                context_rate: SnapshotGeminiContextRate::ConservativeMaximum,
                search_billing,
                grounding_enabled: self.grounding_enabled,
                search_reserve_units,
            },
        })
        .context("build validated Gemini legacy admission snapshot")?;

        Ok(Some(GeminiLegacyQuote {
            effective_output_tokens,
            snapshot,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUOTE_TS: i64 = 1_800_000_000;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    fn model(model_id: &str) -> GeminiModel {
        let spec = metering::gemini_catalog_at(QUOTE_TS)
            .into_iter()
            .find(|spec| spec.id == model_id)
            .unwrap();
        GeminiModel {
            id: spec.id.to_owned(),
            display_name: spec.display_name.to_owned(),
            input_token_limit: spec.input_token_limit,
            output_token_limit: spec.output_token_limit,
            prices: spec.prices,
        }
    }

    fn input(model_id: &str, multiplier: i64) -> GeminiLegacyQuoteInput {
        GeminiLegacyQuoteInput {
            request_id: request_id(),
            account_id: "account-google-1".to_owned(),
            model: model(model_id),
            quote_ts: QUOTE_TS,
            payable_multiplier_bp: multiplier,
            estimated_input_tokens: 2_048,
            requested_output_tokens: 1_024,
            image_output_tokens: if model_id == "gemini-3.1-flash-image" {
                1_290
            } else {
                0
            },
            grounding_enabled: true,
            allow_output_cap: model_id != "gemini-3.1-flash-image",
        }
    }

    fn eligible(input: GeminiLegacyQuoteInput) -> PreparedGeminiLegacyQuote {
        match prepare_gemini_legacy_quote(input).unwrap() {
            PricingBridgePrepare::Eligible(prepared) => prepared,
            PricingBridgePrepare::Fallback(reason) => panic!("unexpected fallback: {reason:?}"),
        }
    }

    #[test]
    fn every_subscription_model_builds_the_exact_legacy_quote() {
        for model_id in [
            "gemini-3.1-flash-image",
            "gemini-3.6-flash",
            "gemini-3.5-flash",
            "gemini-3-flash-preview",
            "gemini-3.1-pro-preview",
            "gemini-3.1-flash-lite",
            "gemini-2.5-flash",
            "gemini-2.5-flash-lite",
        ] {
            for multiplier in [0, 1, 2_000, 5_000, 10_000] {
                let prepared = eligible(input(model_id, multiplier));
                let quote = prepared.quote(i64::MAX).unwrap().unwrap();
                let snapshot = quote.snapshot();
                assert_eq!(snapshot.provider(), SnapshotProvider::Google);
                assert_eq!(snapshot.requested_model_id(), model_id);
                assert_eq!(snapshot.canonical_model_id(), model_id);
                assert_eq!(snapshot.tariff_priced_ts(), QUOTE_TS);
                assert_eq!(snapshot.admission_ts(), QUOTE_TS);
                assert_eq!(snapshot.payable_multiplier_bp(), multiplier);
                assert_eq!(quote.effective_output_tokens(), 1_024);
                let expected = metering::apply_multiplier(
                    i128::from(snapshot.official_hold_nano()),
                    multiplier,
                )
                .clamp(1, i64::MAX as i128) as i64;
                assert_eq!(snapshot.charged_hold_nano(), expected);
                snapshot.validate().unwrap();
            }
        }
    }

    #[test]
    fn text_balance_cap_and_image_full_fit_match_legacy_semantics() {
        let text = eligible(input("gemini-3.1-pro-preview", 10_000));
        let high = text.quote(i64::MAX).unwrap().unwrap();
        let fixed_one_token =
            reserve_cost_with_prices(text.prices, text.estimated_input_tokens, 1, 0, true) as i64;
        let low = text.quote(fixed_one_token + 1).unwrap().unwrap();
        assert!(low.effective_output_tokens() < high.effective_output_tokens());
        assert_eq!(low.snapshot().charged_hold_nano(), fixed_one_token);

        let image = eligible(input("gemini-3.1-flash-image", 10_000));
        let full = image.quote(i64::MAX).unwrap().unwrap();
        assert!(image
            .quote(full.snapshot().charged_hold_nano() - 1)
            .unwrap()
            .is_none());
    }

    #[test]
    fn search_contract_is_typed_per_provider_tariff() {
        let per_query = eligible(input("gemini-3.6-flash", 5_000))
            .quote(i64::MAX)
            .unwrap()
            .unwrap();
        assert!(matches!(
            per_query.snapshot().premium_modifiers(),
            LegacyPremiumModifiers::GeminiV1 {
                search_billing: SnapshotGeminiSearchBilling::PerQuery,
                grounding_enabled: true,
                search_reserve_units: 32,
                ..
            }
        ));

        let per_prompt = eligible(input("gemini-2.5-flash", 5_000))
            .quote(i64::MAX)
            .unwrap()
            .unwrap();
        assert!(matches!(
            per_prompt.snapshot().premium_modifiers(),
            LegacyPremiumModifiers::GeminiV1 {
                search_billing: SnapshotGeminiSearchBilling::PerGroundedPrompt,
                grounding_enabled: true,
                search_reserve_units: 1,
                ..
            }
        ));
    }

    #[test]
    fn expected_ineligible_inputs_fallback_before_money() {
        let mut unknown = input("gemini-3.6-flash", 5_000);
        unknown.model.id = "gemini-latest".to_owned();
        assert!(matches!(
            prepare_gemini_legacy_quote(unknown).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::UnsupportedModelIdentity)
        ));

        let mut oversized = input("gemini-3.6-flash", 5_000);
        oversized.account_id = "a".repeat(513);
        assert!(matches!(
            prepare_gemini_legacy_quote(oversized).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::SnapshotIdentityOversized)
        ));

        let mut overflow = input("gemini-3.1-flash-image", 10_000);
        overflow.image_output_tokens = u64::MAX;
        assert!(matches!(
            prepare_gemini_legacy_quote(overflow).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::OfficialHoldOutOfRange)
        ));
    }

    #[test]
    fn malformed_trusted_preflight_is_a_hard_error() {
        let mut clock = input("gemini-3.6-flash", 5_000);
        clock.quote_ts = 0;
        assert!(prepare_gemini_legacy_quote(clock).is_err());

        let mut multiplier = input("gemini-3.6-flash", 5_000);
        multiplier.payable_multiplier_bp = 10_001;
        assert!(prepare_gemini_legacy_quote(multiplier).is_err());

        let mut limits = input("gemini-3.6-flash", 5_000);
        limits.model.output_token_limit -= 1;
        assert!(prepare_gemini_legacy_quote(limits).is_err());
    }
}
