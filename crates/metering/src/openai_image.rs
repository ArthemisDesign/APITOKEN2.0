//! GPT Image 2 tariff identity and pure token-cost arithmetic.
//!
//! This module does not publish a model or grant product access. It pins the official OpenAI rate
//! card for the alias and immutable snapshot reviewed in `research/GPT_IMAGE_2_EVIDENCE.md`.
//! Values are nanoUSD per token (`$/M tokens * 1000`), and every operation is checked integer math.

use crate::TariffScheduleId;

pub const OPENAI_IMAGE_TARIFF_SCHEDULE_ID: &str = "openai/gpt-image-2/2026-04-21/v1";
pub const OPENAI_IMAGE_ALIAS_GENERATION: i64 = 1;
pub const GPT_IMAGE_2_ALIAS: &str = "gpt-image-2";
pub const GPT_IMAGE_2_SNAPSHOT: &str = "gpt-image-2-2026-04-21";

/// Hot-override tariff family of the GPT Image 2 card: `OPENAI_IMAGE_TARIFF_SCHEDULE_ID` minus
/// its date/version suffix. One family covers both the alias and the immutable snapshot because
/// they share one reviewed rate card.
pub const OPENAI_IMAGE_TARIFF_FAMILY: &str = "openai/gpt-image-2";

/// The hot-override tariff family a GPT Image 2 resolution prices against.
pub fn openai_image_tariff_family() -> &'static str {
    OPENAI_IMAGE_TARIFF_FAMILY
}

/// Five disjoint GPT Image billing rates in nanoUSD per token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenAiImagePrices {
    pub fresh_text_input: i128,
    pub cached_text_input: i128,
    pub fresh_image_input: i128,
    pub cached_image_input: i128,
    pub image_output: i128,
}

/// Provider-reported usage shape expected for GPT Image responses.
///
/// Cached input is a subset of the corresponding total, not an additional token population. Cost
/// calculation validates that relationship and derives the fresh portions before applying rates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OpenAiImageUsage {
    pub total_text_input_tokens: u64,
    pub cached_text_input_tokens: u64,
    pub total_image_input_tokens: u64,
    pub cached_image_input_tokens: u64,
    pub image_output_tokens: u64,
}

/// Exact immutable tariff identity shared by the reviewed alias and snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpenAiImageTariffIdentity {
    pub canonical_model_id: &'static str,
    pub tariff_schedule_id: TariffScheduleId,
    pub schedule_effective_from: i64,
    pub prices: OpenAiImagePrices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenAiImageMeteringError {
    UnsupportedModelIdentity,
    CachedTextExceedsTotal,
    CachedImageExceedsTotal,
    Overflow,
}

const GPT_IMAGE_2_PRICES: OpenAiImagePrices = OpenAiImagePrices {
    fresh_text_input: 5_000,
    cached_text_input: 1_250,
    fresh_image_input: 8_000,
    cached_image_input: 2_000,
    image_output: 30_000,
};

/// Resolve only the exact reviewed alias or snapshot. Unknown and approximate IDs fail closed.
///
/// Both accepted IDs intentionally share one canonical snapshot and immutable schedule identity.
pub fn openai_image_tariff(
    model_id: &str,
) -> Result<OpenAiImageTariffIdentity, OpenAiImageMeteringError> {
    if model_id != GPT_IMAGE_2_ALIAS && model_id != GPT_IMAGE_2_SNAPSHOT {
        return Err(OpenAiImageMeteringError::UnsupportedModelIdentity);
    }

    Ok(OpenAiImageTariffIdentity {
        canonical_model_id: GPT_IMAGE_2_SNAPSHOT,
        tariff_schedule_id: TariffScheduleId::from_static(OPENAI_IMAGE_TARIFF_SCHEDULE_ID),
        schedule_effective_from: 0,
        prices: GPT_IMAGE_2_PRICES,
    })
}

impl OpenAiImageUsage {
    pub fn validate(self) -> Result<(), OpenAiImageMeteringError> {
        if self.cached_text_input_tokens > self.total_text_input_tokens {
            return Err(OpenAiImageMeteringError::CachedTextExceedsTotal);
        }
        if self.cached_image_input_tokens > self.total_image_input_tokens {
            return Err(OpenAiImageMeteringError::CachedImageExceedsTotal);
        }
        Ok(())
    }
}

/// Calculate exact replacement cost across five disjoint legs.
///
/// Total input counters include cached subsets, so fresh text/image input is derived by subtraction.
/// Every multiplication and addition is checked; malformed overlap or overflow fails closed.
pub fn openai_image_cost_nanodollars(
    usage: &OpenAiImageUsage,
    prices: &OpenAiImagePrices,
) -> Result<i128, OpenAiImageMeteringError> {
    usage.validate()?;

    let fresh_text = usage.total_text_input_tokens - usage.cached_text_input_tokens;
    let fresh_image = usage.total_image_input_tokens - usage.cached_image_input_tokens;
    let leg = |tokens: u64, rate: i128| {
        i128::from(tokens)
            .checked_mul(rate)
            .ok_or(OpenAiImageMeteringError::Overflow)
    };

    let mut total = 0_i128;
    for part in [
        leg(fresh_text, prices.fresh_text_input)?,
        leg(usage.cached_text_input_tokens, prices.cached_text_input)?,
        leg(fresh_image, prices.fresh_image_input)?,
        leg(usage.cached_image_input_tokens, prices.cached_image_input)?,
        leg(usage.image_output_tokens, prices.image_output)?,
    ] {
        total = total
            .checked_add(part)
            .ok_or(OpenAiImageMeteringError::Overflow)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NANO_PER_USD;

    #[test]
    fn five_rates_are_exact_at_one_million_tokens() {
        let prices = openai_image_tariff(GPT_IMAGE_2_ALIAS).unwrap().prices;
        let cases = [
            (
                OpenAiImageUsage {
                    total_text_input_tokens: 1_000_000,
                    ..OpenAiImageUsage::default()
                },
                5 * NANO_PER_USD,
            ),
            (
                OpenAiImageUsage {
                    total_text_input_tokens: 1_000_000,
                    cached_text_input_tokens: 1_000_000,
                    ..OpenAiImageUsage::default()
                },
                1_250_000_000,
            ),
            (
                OpenAiImageUsage {
                    total_image_input_tokens: 1_000_000,
                    ..OpenAiImageUsage::default()
                },
                8 * NANO_PER_USD,
            ),
            (
                OpenAiImageUsage {
                    total_image_input_tokens: 1_000_000,
                    cached_image_input_tokens: 1_000_000,
                    ..OpenAiImageUsage::default()
                },
                2 * NANO_PER_USD,
            ),
            (
                OpenAiImageUsage {
                    image_output_tokens: 1_000_000,
                    ..OpenAiImageUsage::default()
                },
                30 * NANO_PER_USD,
            ),
        ];

        for (usage, expected) in cases {
            assert_eq!(
                openai_image_cost_nanodollars(&usage, &prices).unwrap(),
                expected
            );
        }
    }

    #[test]
    fn mixed_usage_derives_fresh_inputs_and_counts_every_leg_once() {
        let prices = openai_image_tariff(GPT_IMAGE_2_SNAPSHOT).unwrap().prices;
        let usage = OpenAiImageUsage {
            total_text_input_tokens: 100,
            cached_text_input_tokens: 40,
            total_image_input_tokens: 200,
            cached_image_input_tokens: 50,
            image_output_tokens: 10,
        };

        // 60*5000 + 40*1250 + 150*8000 + 50*2000 + 10*30000.
        assert_eq!(
            openai_image_cost_nanodollars(&usage, &prices).unwrap(),
            1_950_000
        );
    }

    #[test]
    fn cached_overlap_exceeding_totals_is_rejected() {
        let prices = openai_image_tariff(GPT_IMAGE_2_ALIAS).unwrap().prices;
        let bad_text = OpenAiImageUsage {
            total_text_input_tokens: 9,
            cached_text_input_tokens: 10,
            ..OpenAiImageUsage::default()
        };
        assert_eq!(
            openai_image_cost_nanodollars(&bad_text, &prices),
            Err(OpenAiImageMeteringError::CachedTextExceedsTotal)
        );

        let bad_image = OpenAiImageUsage {
            total_image_input_tokens: 9,
            cached_image_input_tokens: 10,
            ..OpenAiImageUsage::default()
        };
        assert_eq!(
            openai_image_cost_nanodollars(&bad_image, &prices),
            Err(OpenAiImageMeteringError::CachedImageExceedsTotal)
        );
    }

    #[test]
    fn alias_and_snapshot_share_identity_but_unknown_ids_fail_closed() {
        let alias = openai_image_tariff(GPT_IMAGE_2_ALIAS).unwrap();
        let snapshot = openai_image_tariff(GPT_IMAGE_2_SNAPSHOT).unwrap();
        assert_eq!(alias, snapshot);
        assert_eq!(alias.canonical_model_id, GPT_IMAGE_2_SNAPSHOT);
        assert_eq!(
            alias.tariff_schedule_id.as_str(),
            OPENAI_IMAGE_TARIFF_SCHEDULE_ID
        );
        assert_eq!(alias.schedule_effective_from, 0);
        assert_eq!(
            openai_image_tariff("gpt-image-2-latest"),
            Err(OpenAiImageMeteringError::UnsupportedModelIdentity)
        );
    }

    #[test]
    fn overflow_is_checked() {
        let prices = OpenAiImagePrices {
            fresh_text_input: i128::MAX,
            cached_text_input: i128::MAX,
            fresh_image_input: i128::MAX,
            cached_image_input: i128::MAX,
            image_output: i128::MAX,
        };
        let usage = OpenAiImageUsage {
            total_text_input_tokens: u64::MAX,
            ..OpenAiImageUsage::default()
        };

        assert_eq!(
            openai_image_cost_nanodollars(&usage, &prices),
            Err(OpenAiImageMeteringError::Overflow)
        );
    }

    #[test]
    fn tariff_family_is_the_schedule_identity_without_date_and_version() {
        assert_eq!(openai_image_tariff_family(), "openai/gpt-image-2");
        assert!(
            OPENAI_IMAGE_TARIFF_SCHEDULE_ID.starts_with(OPENAI_IMAGE_TARIFF_FAMILY),
            "the family must remain a prefix of the pinned schedule id"
        );
        // Both accepted ids resolve to the same family the override authority targets.
        for model in [GPT_IMAGE_2_ALIAS, GPT_IMAGE_2_SNAPSHOT] {
            let identity = openai_image_tariff(model).unwrap();
            assert_eq!(
                identity.tariff_schedule_id.as_str(),
                OPENAI_IMAGE_TARIFF_SCHEDULE_ID
            );
        }
    }
}
