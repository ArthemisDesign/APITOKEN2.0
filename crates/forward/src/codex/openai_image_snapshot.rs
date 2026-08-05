//! Immutable GPT Image 2 admission quote built only from reviewed runtime constants.

use crate::pricing::EnginePricingRequestId;
use anyhow::{ensure, Context, Result};
use registry::pricing::{
    LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot, LegacyScalarAdmissionSnapshotInput,
    SnapshotOpenAiImageOperation, SnapshotProvider,
};

pub(super) const MAX_IMAGE_PROMPT_BYTES: usize = 128 * 1024;
const LOW_QUALITY_OUTPUT_CEILING_NANO: i64 = 19_770_000;
const PUBLIC_PROMPT_CEILING_NANO: i64 = MAX_IMAGE_PROMPT_BYTES as i64 * 5_000;
pub(super) const GENERATION_HOLD_NANO: i64 =
    PUBLIC_PROMPT_CEILING_NANO + LOW_QUALITY_OUTPUT_CEILING_NANO;
pub(super) const EDIT_HOLD_NANO: i64 = 64_000_000_000 + GENERATION_HOLD_NANO;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenAiImageOperation {
    Generation,
    Edit,
}

impl OpenAiImageOperation {
    pub(super) const fn official_hold_nano(self) -> i64 {
        match self {
            Self::Generation => GENERATION_HOLD_NANO,
            Self::Edit => EDIT_HOLD_NANO,
        }
    }

    const fn snapshot(self) -> SnapshotOpenAiImageOperation {
        match self {
            Self::Generation => SnapshotOpenAiImageOperation::Generation,
            Self::Edit => SnapshotOpenAiImageOperation::Edit,
        }
    }

    const fn reference_count(self) -> i64 {
        match self {
            Self::Generation => 0,
            Self::Edit => 1,
        }
    }
}

pub(super) struct OpenAiImageQuoteInput {
    pub request_id: EnginePricingRequestId,
    pub account_id: String,
    pub requested_model_id: String,
    pub quote_ts: i64,
    pub payable_multiplier_bp: i64,
    pub operation: OpenAiImageOperation,
    pub available_nano: i64,
}

pub(super) fn openai_image_quote(
    input: OpenAiImageQuoteInput,
) -> Result<Option<LegacyScalarAdmissionSnapshot>> {
    ensure!(
        input.quote_ts > 0,
        "OpenAI image quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "OpenAI image multiplier is outside the snapshot contract"
    );
    ensure!(
        input.available_nano > 0,
        "OpenAI image quote requires positive available balance"
    );
    let tariff = metering::openai_image_tariff(&input.requested_model_id)
        .map_err(|_| anyhow::anyhow!("unsupported OpenAI image model identity"))?;
    let official_hold_nano = input.operation.official_hold_nano();
    let charged_hold_nano =
        metering::apply_multiplier(i128::from(official_hold_nano), input.payable_multiplier_bp)
            .clamp(1, i128::from(i64::MAX)) as i64;
    if charged_hold_nano > input.available_nano {
        return Ok(None);
    }
    let snapshot = LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
        request_id: input.request_id.as_str().to_owned(),
        account_id: input.account_id,
        provider: SnapshotProvider::OpenAi,
        requested_model_id: input.requested_model_id,
        canonical_model_id: tariff.canonical_model_id.to_owned(),
        alias_generation: metering::OPENAI_IMAGE_ALIAS_GENERATION,
        tariff_schedule_id: tariff.tariff_schedule_id.as_str().to_owned(),
        tariff_priced_ts: input.quote_ts,
        admission_ts: input.quote_ts,
        payable_multiplier_bp: input.payable_multiplier_bp,
        official_hold_nano,
        charged_hold_nano,
        premium_modifiers: LegacyPremiumModifiers::OpenAiImageV1 {
            operation: input.operation.snapshot(),
            background: "opaque".to_owned(),
            quality: "low".to_owned(),
            size: "auto".to_owned(),
            reference_count: input.operation.reference_count(),
        },
    })
    .context("build OpenAI image admission snapshot")?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    #[test]
    fn generation_and_edit_quotes_pin_distinct_bounded_operations() {
        for (operation, expected_hold, expected_references) in [
            (OpenAiImageOperation::Generation, GENERATION_HOLD_NANO, 0),
            (OpenAiImageOperation::Edit, EDIT_HOLD_NANO, 1),
        ] {
            let quote = openai_image_quote(OpenAiImageQuoteInput {
                request_id: request_id(),
                account_id: "acct".to_owned(),
                requested_model_id: metering::GPT_IMAGE_2_ALIAS.to_owned(),
                quote_ts: 1_800_000_000,
                payable_multiplier_bp: 2_000,
                operation,
                available_nano: i64::MAX,
            })
            .unwrap()
            .unwrap();
            assert_eq!(quote.canonical_model_id(), metering::GPT_IMAGE_2_SNAPSHOT);
            assert_eq!(quote.official_hold_nano(), expected_hold);
            assert_eq!(
                quote.charged_hold_nano(),
                metering::apply_multiplier(i128::from(expected_hold), 2_000) as i64
            );
            assert!(matches!(
                quote.premium_modifiers(),
                LegacyPremiumModifiers::OpenAiImageV1 { reference_count, .. }
                    if *reference_count == expected_references
            ));
            quote.validate().unwrap();
        }
    }

    #[test]
    fn quote_requires_the_full_hold_and_rejects_unknown_identity() {
        assert!(openai_image_quote(OpenAiImageQuoteInput {
            request_id: request_id(),
            account_id: "acct".to_owned(),
            requested_model_id: metering::GPT_IMAGE_2_ALIAS.to_owned(),
            quote_ts: 1_800_000_000,
            payable_multiplier_bp: 10_000,
            operation: OpenAiImageOperation::Generation,
            available_nano: GENERATION_HOLD_NANO - 1,
        })
        .unwrap()
        .is_none());

        assert!(openai_image_quote(OpenAiImageQuoteInput {
            request_id: request_id(),
            account_id: "acct".to_owned(),
            requested_model_id: "gpt-image-latest".to_owned(),
            quote_ts: 1_800_000_000,
            payable_multiplier_bp: 10_000,
            operation: OpenAiImageOperation::Generation,
            available_nano: i64::MAX,
        })
        .is_err());
    }
}
