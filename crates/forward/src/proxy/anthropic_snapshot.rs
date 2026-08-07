//! Anthropic legacy quote/snapshot builder for the default-off atomic bridge.
//!
//! This child module deliberately calls the existing private `cap_to_balance` implementation. It
//! therefore cannot drift to a second balance-cap formula. The caller owns rollout selection and
//! persistence; this module has no DB, env, metrics or clock.

use super::cap_to_balance;
use crate::pricing::tariff_book::{self, PinnedTariff};
use crate::pricing::{
    snapshot_identity_is_oversized, EnginePricingRequestId, PricingBridgeFallbackReason,
    PricingBridgePrepare,
};
use anyhow::{ensure, Context, Result};
use metering::{
    AnthropicAdmissionModifiers, AnthropicAdmissionTariffIdentity, AnthropicInferenceGeo,
    AnthropicSpeed, TariffIdentityError, Usage,
};
use registry::pricing::{
    LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot, LegacyScalarAdmissionSnapshotInput,
    SnapshotAnthropicInferenceGeo, SnapshotAnthropicSpeed, SnapshotProvider,
};

const MAX_CLIENT_OUTPUT_TOKENS: u64 = 2_000_000;
const DEFAULT_CLIENT_OUTPUT_TOKENS: u64 = 4_096;
const MAX_WEB_SEARCH_REQUESTS: u64 = 1_000;

pub(super) struct AnthropicLegacyQuoteInput {
    pub request_id: EnginePricingRequestId,
    pub account_id: String,
    pub requested_model_id: String,
    pub quote_ts: i64,
    pub payable_multiplier_bp: i64,
    pub modifiers: AnthropicAdmissionModifiers,
    /// The same conservative byte-derived upper bound used by live reserve.
    pub input_token_upper_bound: u64,
    /// Already parsed request-side maximum. The live parser bounds this at 1,000.
    pub web_search_requests: u64,
    /// Raw client value: zero means the legacy default, non-zero values are capped here.
    pub requested_max_output_tokens: u64,
}

pub(super) struct PreparedAnthropicLegacyQuote {
    request_id: EnginePricingRequestId,
    account_id: String,
    requested_model_id: String,
    quote_ts: i64,
    payable_multiplier_bp: i64,
    identity: AnthropicAdmissionTariffIdentity,
    /// Base reserve vector admission actually prices with: the hot override payload when the
    /// tariff book resolves one for the compiled family at `quote_ts`, else the compiled
    /// constants, with the geo premium applied on top in both cases (identical to
    /// `identity.effective_reserve_prices` while the book is empty).
    reserve_prices: metering::Prices,
    /// `<family>/v<version>` of the pinned override, or the compiled schedule id as today.
    tariff_schedule_id: String,
    pin: Option<PinnedTariff>,
    input_token_upper_bound: u64,
    web_search_requests: u64,
    client_max_output_tokens: u64,
}

pub(super) struct AnthropicLegacyQuote {
    effective_max_output_tokens: u64,
    snapshot: LegacyScalarAdmissionSnapshot,
}

impl AnthropicLegacyQuote {
    pub(super) const fn effective_max_output_tokens(&self) -> u64 {
        self.effective_max_output_tokens
    }

    pub(super) fn snapshot(&self) -> &LegacyScalarAdmissionSnapshot {
        &self.snapshot
    }

    pub(super) fn into_snapshot(self) -> LegacyScalarAdmissionSnapshot {
        self.snapshot
    }
}

pub(super) fn prepare_anthropic_legacy_quote(
    input: AnthropicLegacyQuoteInput,
) -> Result<PricingBridgePrepare<PreparedAnthropicLegacyQuote>> {
    ensure!(
        input.quote_ts > 0,
        "Anthropic bridge quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "Anthropic bridge multiplier is outside the legacy snapshot contract"
    );
    ensure!(
        input.web_search_requests <= MAX_WEB_SEARCH_REQUESTS,
        "Anthropic bridge web-search bound was not normalized by request parsing"
    );

    if snapshot_identity_is_oversized(&input.account_id)
        || snapshot_identity_is_oversized(&input.requested_model_id)
    {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::SnapshotIdentityOversized,
        ));
    }

    let identity = match metering::anthropic_tariff_capability_at(
        &input.requested_model_id,
        input.quote_ts,
        input.modifiers,
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
            anyhow::bail!("Anthropic canonicalizer rejected a prevalidated quote timestamp");
        }
    };

    let input_token_upper_bound = input.input_token_upper_bound.max(1);
    let client_max_output_tokens = if input.requested_max_output_tokens == 0 {
        DEFAULT_CLIENT_OUTPUT_TOKENS
    } else {
        input
            .requested_max_output_tokens
            .min(MAX_CLIENT_OUTPUT_TOKENS)
    };

    // Hot tariff override: the compiled strict schedule id is `<family>/v1`, so the family key is
    // the id minus its version suffix. An override replaces only the base vector; the geo premium
    // stays code-applied on top, exactly as it is over the compiled base today.
    let (reserve_prices, tariff_schedule_id, pin) =
        resolve_reserve_tariff(&tariff_book::snapshot(), &identity, input.quote_ts);

    let maximum_official_hold = official_hold_nano(
        input_token_upper_bound,
        input.web_search_requests,
        client_max_output_tokens,
        &reserve_prices,
    );
    if i64::try_from(maximum_official_hold).is_err() {
        return Ok(PricingBridgePrepare::Fallback(
            PricingBridgeFallbackReason::OfficialHoldOutOfRange,
        ));
    }

    Ok(PricingBridgePrepare::Eligible(
        PreparedAnthropicLegacyQuote {
            request_id: input.request_id,
            account_id: input.account_id,
            requested_model_id: input.requested_model_id,
            quote_ts: input.quote_ts,
            payable_multiplier_bp: input.payable_multiplier_bp,
            identity,
            reserve_prices,
            tariff_schedule_id,
            pin,
            input_token_upper_bound,
            web_search_requests: input.web_search_requests,
            client_max_output_tokens,
        },
    ))
}

impl PreparedAnthropicLegacyQuote {
    /// The hot override version this quote priced with; settlement replays exactly this version.
    pub(super) fn pinned_tariff(&self) -> Option<PinnedTariff> {
        self.pin.clone()
    }

    /// Quote the same prepared identity at a possibly refreshed balance. `None` is the ordinary
    /// legacy not-affordable outcome, not an eligibility fallback.
    pub(super) fn quote(&self, balance_nano: i128) -> Result<Option<AnthropicLegacyQuote>> {
        let web_buffer_nano = (self.web_search_requests as i128) * metering::WEB_SEARCH_NANO;
        let Some((effective_max_output_tokens, charged_hold_nano)) = cap_to_balance(
            balance_nano,
            self.input_token_upper_bound as i128,
            web_buffer_nano,
            &self.reserve_prices,
            self.payable_multiplier_bp,
            self.client_max_output_tokens,
        ) else {
            return Ok(None);
        };

        let official_hold_nano = official_hold_nano(
            self.input_token_upper_bound,
            self.web_search_requests,
            effective_max_output_tokens,
            &self.reserve_prices,
        );
        let official_hold_nano = i64::try_from(official_hold_nano)
            .context("prepared Anthropic official hold exceeded its checked maximum")?;
        let expected_charged_hold = if self.payable_multiplier_bp == 0 {
            0
        } else {
            metering::apply_multiplier(official_hold_nano as i128, self.payable_multiplier_bp)
                .min(i64::MAX as i128) as i64
        };
        ensure!(
            charged_hold_nano == expected_charged_hold,
            "Anthropic snapshot quote drifted from the live legacy reserve"
        );

        let modifiers = LegacyPremiumModifiers::AnthropicV1 {
            speed: match self.identity.modifiers.speed {
                AnthropicSpeed::Standard => SnapshotAnthropicSpeed::Standard,
                AnthropicSpeed::Fast => SnapshotAnthropicSpeed::Fast,
            },
            inference_geo: match self.identity.modifiers.inference_geo {
                AnthropicInferenceGeo::Global => SnapshotAnthropicInferenceGeo::Global,
                AnthropicInferenceGeo::Us => SnapshotAnthropicInferenceGeo::Us,
            },
            inference_geo_basis_points: self.identity.modifiers.inference_geo_basis_points(),
        };
        let snapshot = LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
            request_id: self.request_id.as_str().to_owned(),
            account_id: self.account_id.clone(),
            provider: SnapshotProvider::Anthropic,
            requested_model_id: self.requested_model_id.clone(),
            canonical_model_id: self.identity.canonical_model_id.to_owned(),
            alias_generation: self.identity.alias_generation,
            tariff_schedule_id: self.tariff_schedule_id.clone(),
            tariff_priced_ts: self.quote_ts,
            admission_ts: self.quote_ts,
            payable_multiplier_bp: self.payable_multiplier_bp,
            official_hold_nano,
            charged_hold_nano,
            premium_modifiers: modifiers,
        })
        .context("build validated Anthropic legacy admission snapshot")?;

        Ok(Some(AnthropicLegacyQuote {
            effective_max_output_tokens,
            snapshot,
        }))
    }
}

/// The reserve tariff decision, pure for testing: the override base vector and pin when the book
/// resolves the compiled family at `quote_ts`, compiled constants otherwise, with the geo premium
/// applied on top in both cases.
fn resolve_reserve_tariff(
    book: &tariff_book::TariffBookSnapshot,
    identity: &AnthropicAdmissionTariffIdentity,
    quote_ts: i64,
) -> (metering::Prices, String, Option<PinnedTariff>) {
    let family = identity
        .tariff_schedule_id
        .as_str()
        .strip_suffix("/v1")
        .unwrap_or_else(|| identity.tariff_schedule_id.as_str());
    let fast = identity.modifiers.speed == AnthropicSpeed::Fast;
    let compiled_base = metering::model_prices_reserve_for_speed_at(
        identity.canonical_model_id,
        quote_ts,
        fast,
    );
    let resolved = tariff_book::reserve_base(
        book,
        family,
        quote_ts,
        compiled_base,
        tariff_book::as_anthropic,
    );
    let reserve_prices = if identity.modifiers.inference_geo == AnthropicInferenceGeo::Us {
        metering::premium_prices_ceil(
            resolved.prices,
            identity.modifiers.inference_geo_basis_points(),
        )
    } else {
        resolved.prices
    };
    let tariff_schedule_id = resolved
        .pin
        .as_ref()
        .map(|pin| pin.schedule_id.clone())
        .unwrap_or_else(|| identity.tariff_schedule_id.as_str().to_owned());
    (reserve_prices, tariff_schedule_id, resolved.pin)
}

fn official_hold_nano(
    input_token_upper_bound: u64,
    web_search_requests: u64,
    output_tokens: u64,
    reserve_prices: &metering::Prices,
) -> i128 {
    metering::cost_nanodollars(
        &Usage {
            output_tokens,
            cache_write_1h_tokens: input_token_upper_bound,
            web_search_requests,
            ..Usage::default()
        },
        reserve_prices,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const QUOTE_TS: i64 = 1_788_220_800;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    fn input(
        model: &str,
        speed: AnthropicSpeed,
        geo: AnthropicInferenceGeo,
        multiplier: i64,
    ) -> AnthropicLegacyQuoteInput {
        AnthropicLegacyQuoteInput {
            request_id: request_id(),
            account_id: "account-1".to_string(),
            requested_model_id: model.to_string(),
            quote_ts: QUOTE_TS,
            payable_multiplier_bp: multiplier,
            modifiers: AnthropicAdmissionModifiers {
                speed,
                inference_geo: geo,
            },
            input_token_upper_bound: 2_048,
            web_search_requests: 2,
            requested_max_output_tokens: 1_000,
        }
    }

    fn eligible(input: AnthropicLegacyQuoteInput) -> PreparedAnthropicLegacyQuote {
        match prepare_anthropic_legacy_quote(input).unwrap() {
            PricingBridgePrepare::Eligible(prepared) => prepared,
            PricingBridgePrepare::Fallback(reason) => panic!("unexpected fallback: {reason:?}"),
        }
    }

    #[test]
    fn every_audited_model_modifier_and_scalar_builds_the_exact_legacy_quote() {
        let models = [
            "claude-opus-4-8",
            "claude-opus-4-7",
            "claude-sonnet-5",
            "claude-sonnet-4-6",
            "claude-haiku-4-5",
        ];
        for model in models {
            for speed in [AnthropicSpeed::Standard, AnthropicSpeed::Fast] {
                for geo in [AnthropicInferenceGeo::Global, AnthropicInferenceGeo::Us] {
                    for multiplier in [0, 1, 900, 2_000, 10_000] {
                        let prepared = eligible(input(model, speed, geo, multiplier));
                        let quote = prepared.quote(i64::MAX as i128).unwrap().unwrap();
                        let snapshot = quote.snapshot();
                        assert_eq!(snapshot.provider(), SnapshotProvider::Anthropic);
                        assert_eq!(snapshot.requested_model_id(), model);
                        assert_eq!(snapshot.canonical_model_id(), model);
                        assert_eq!(snapshot.tariff_priced_ts(), QUOTE_TS);
                        assert_eq!(snapshot.admission_ts(), QUOTE_TS);
                        assert_eq!(snapshot.payable_multiplier_bp(), multiplier);
                        assert_eq!(quote.effective_max_output_tokens(), 1_000);
                        let expected_charged = if multiplier == 0 {
                            0
                        } else {
                            metering::apply_multiplier(
                                snapshot.official_hold_nano() as i128,
                                multiplier,
                            ) as i64
                        };
                        assert_eq!(snapshot.charged_hold_nano(), expected_charged);
                        snapshot.validate().unwrap();
                    }
                }
            }
        }
    }

    #[test]
    fn refreshed_balance_changes_only_the_final_quote_not_frozen_identity() {
        let prepared = eligible(input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Us,
            2_000,
        ));
        let high = prepared.quote(1_000_000_000).unwrap().unwrap();
        // A concurrent reserve may leave the refreshed balance just above the existing -$1 floor.
        // This value still covers fixed input/search, but not the full requested output.
        let low = prepared.quote(-992_000_000).unwrap().unwrap();
        assert!(low.effective_max_output_tokens() < high.effective_max_output_tokens());
        assert_eq!(low.snapshot().request_id(), high.snapshot().request_id());
        assert_eq!(low.snapshot().tariff_priced_ts(), QUOTE_TS);
        assert_eq!(low.snapshot().admission_ts(), QUOTE_TS);
        assert_ne!(
            low.snapshot().snapshot_digest(),
            high.snapshot().snapshot_digest()
        );
    }

    #[test]
    fn sonnet_epoch_and_request_modifiers_are_pinned_in_snapshot_identity() {
        let mut before = input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Global,
            2_000,
        );
        before.quote_ts = QUOTE_TS - 1;
        let before = eligible(before).quote(i64::MAX as i128).unwrap().unwrap();
        let after = eligible(input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Us,
            2_000,
        ))
        .quote(i64::MAX as i128)
        .unwrap()
        .unwrap();

        assert_eq!(
            before.snapshot().tariff_schedule_id(),
            "anthropic/standard/sonnet-5-intro/v1"
        );
        assert_eq!(
            after.snapshot().tariff_schedule_id(),
            "anthropic/standard/sonnet-current/v1"
        );
        assert!(after.snapshot().official_hold_nano() > before.snapshot().official_hold_nano());
        assert_ne!(
            before.snapshot().snapshot_digest(),
            after.snapshot().snapshot_digest()
        );
    }

    #[test]
    fn expected_ineligible_inputs_fallback_before_money() {
        for model in [
            "claude-sonnet-5-latest",
            "claude-sonnet-5-20260730",
            "CLAUDE-SONNET-5",
            "unknown",
        ] {
            assert!(matches!(
                prepare_anthropic_legacy_quote(input(
                    model,
                    AnthropicSpeed::Standard,
                    AnthropicInferenceGeo::Global,
                    2_000,
                ))
                .unwrap(),
                PricingBridgePrepare::Fallback(
                    PricingBridgeFallbackReason::UnsupportedModelIdentity
                )
            ));
        }

        let mut oversized_identity = input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Global,
            2_000,
        );
        oversized_identity.account_id = "a".repeat(513);
        assert!(matches!(
            prepare_anthropic_legacy_quote(oversized_identity).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::SnapshotIdentityOversized)
        ));

        let mut boundary_identity = input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Global,
            2_000,
        );
        boundary_identity.account_id = "a".repeat(512);
        let boundary = eligible(boundary_identity)
            .quote(i64::MAX as i128)
            .unwrap()
            .unwrap();
        assert_eq!(boundary.snapshot().account_id().len(), 512);

        let mut oversized_hold = input(
            "claude-opus-4-8",
            AnthropicSpeed::Fast,
            AnthropicInferenceGeo::Us,
            10_000,
        );
        oversized_hold.input_token_upper_bound = u64::MAX;
        assert!(matches!(
            prepare_anthropic_legacy_quote(oversized_hold).unwrap(),
            PricingBridgePrepare::Fallback(PricingBridgeFallbackReason::OfficialHoldOutOfRange)
        ));
    }

    #[test]
    fn malformed_trusted_preflight_is_a_hard_error() {
        let mut invalid_clock = input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Global,
            2_000,
        );
        invalid_clock.quote_ts = 0;
        assert!(prepare_anthropic_legacy_quote(invalid_clock).is_err());

        let mut invalid_scalar = input(
            "claude-sonnet-5",
            AnthropicSpeed::Standard,
            AnthropicInferenceGeo::Global,
            2_000,
        );
        invalid_scalar.payable_multiplier_bp = 10_001;
        assert!(prepare_anthropic_legacy_quote(invalid_scalar).is_err());
    }

    fn override_book(family: &str, input: i128, output: i128) -> Arc<tariff_book::TariffBookSnapshot> {
        tariff_book::TariffBookSnapshot::from_rows(vec![tariff_book::test_row(
            family,
            2,
            0,
            serde_json::json!({
                "input": input.to_string(),
                "output": output.to_string(),
                "cache_read": "500",
                "cache_write_5m": "6250",
                "cache_write_1h": "10000"
            }),
        )])
        .unwrap()
    }

    /// An override row replaces the reserve base vector and pins its exact version; the compiled
    /// schedule id and prices are untouched while the book is empty.
    #[test]
    fn an_override_replaces_the_reserve_base_and_pins_its_version() {
        let identity = metering::anthropic_tariff_capability_at(
            "claude-sonnet-4-6",
            QUOTE_TS,
            AnthropicAdmissionModifiers {
                speed: AnthropicSpeed::Standard,
                inference_geo: AnthropicInferenceGeo::Global,
            },
        )
        .unwrap();
        let (compiled_prices, compiled_id, no_pin) =
            resolve_reserve_tariff(&tariff_book::TariffBookSnapshot::empty(), &identity, QUOTE_TS);
        assert_eq!(compiled_prices, identity.effective_reserve_prices);
        assert_eq!(compiled_id, "anthropic/standard/sonnet-current/v1");
        assert!(no_pin.is_none());

        let book = override_book("anthropic/standard/sonnet-current", 3_100, 15_500);
        let (prices, schedule_id, pin) = resolve_reserve_tariff(&book, &identity, QUOTE_TS);
        assert_eq!(prices.input, 3_100);
        assert_eq!(prices.output, 15_500);
        assert_eq!(prices.cache_read, 500);
        assert_eq!(schedule_id, "anthropic/standard/sonnet-current/v2");
        let pin = pin.expect("the override version is pinned");
        assert_eq!(pin.family, "anthropic/standard/sonnet-current");
        assert_eq!(pin.version, 2);
    }

    /// The US-inference geo premium stays code-applied ON TOP of the override base vector,
    /// exactly as over the compiled one.
    #[test]
    fn the_geo_premium_applies_on_top_of_the_override_base() {
        let identity = metering::anthropic_tariff_capability_at(
            "claude-sonnet-4-6",
            QUOTE_TS,
            AnthropicAdmissionModifiers {
                speed: AnthropicSpeed::Standard,
                inference_geo: AnthropicInferenceGeo::Us,
            },
        )
        .unwrap();
        let book = override_book("anthropic/standard/sonnet-current", 3_100, 15_500);
        let (prices, _, pin) = resolve_reserve_tariff(&book, &identity, QUOTE_TS);
        let expected = metering::premium_prices_ceil(
            metering::Prices {
                input: 3_100,
                output: 15_500,
                cache_read: 500,
                cache_write_5m: 6_250,
                cache_write_1h: 10_000,
            },
            11_000,
        );
        assert_eq!(prices, expected);
        assert!(pin.is_some());
    }
}
