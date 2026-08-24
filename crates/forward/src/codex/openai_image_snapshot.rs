//! Immutable GPT Image 2 admission quote built from the reviewed tariff, possibly republished by
//! the hot tariff override book (family `openai/gpt-image-2`).

use crate::pricing::tariff_book::{self, PinnedTariff};
use crate::pricing::EnginePricingRequestId;
use anyhow::{ensure, Context, Result};
use registry::pricing::{
    LegacyPremiumModifiers, LegacyScalarAdmissionSnapshot, LegacyScalarAdmissionSnapshotInput,
    SnapshotOpenAiImageOperation, SnapshotProvider,
};

pub(super) const MAX_IMAGE_PROMPT_BYTES: usize = 128 * 1024;
/// The output-token ceiling of the reviewed `low` quality tier: the exhaustive 16/48/96-cell
/// maximum (659 tokens) of every request-valid resolution.
const LOW_QUALITY_MAX_OUTPUT_TOKENS: i128 = 659;
/// Each edit reference reserves the whole published 8,000,000-TPM input envelope: OpenAI publishes
/// no normative GPT Image 2 high-fidelity input formula, so this stays an absolute authorization
/// envelope rather than an expected price.
const REFERENCE_INPUT_ENVELOPE_TOKENS: i128 = 8_000_000;
// The pinned compiled-card hold figures, retained as the reference the price-parameterized
// formula is tested against (with the compiled rates the two are equal by construction).
#[cfg(test)]
const LOW_QUALITY_OUTPUT_CEILING_NANO: i64 = 19_770_000;
#[cfg(test)]
const PUBLIC_PROMPT_CEILING_NANO: i64 = MAX_IMAGE_PROMPT_BYTES as i64 * 5_000;
#[cfg(test)]
pub(super) const GENERATION_HOLD_NANO: i64 =
    PUBLIC_PROMPT_CEILING_NANO + LOW_QUALITY_OUTPUT_CEILING_NANO;
#[cfg(test)]
pub(super) const REFERENCE_ENVELOPE_NANO: i64 = 64_000_000_000;
pub(super) const MAX_EDIT_REFERENCES: usize = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OpenAiImageOperation {
    Generation,
    Edit { references: i64 },
}

impl OpenAiImageOperation {
    pub(super) fn edit(references: usize) -> Option<Self> {
        if (1..=MAX_EDIT_REFERENCES).contains(&references) {
            Some(Self::Edit {
                references: references as i64,
            })
        } else {
            None
        }
    }

    /// The conservative admission envelope under one explicit rate card. With the compiled
    /// constants this reproduces `GENERATION_HOLD_NANO`/`EDIT_HOLD_NANO` exactly (the unit test
    /// below pins that); a hot override reprices the same token ceilings from its own vector.
    pub(super) fn official_hold_nano(self, prices: &metering::OpenAiImagePrices) -> i64 {
        let generation = i128::from(MAX_IMAGE_PROMPT_BYTES as i64) * prices.fresh_text_input
            + LOW_QUALITY_MAX_OUTPUT_TOKENS * prices.image_output;
        let references = match self {
            Self::Generation => 0,
            Self::Edit { references } => i128::from(references),
        };
        (generation + references * REFERENCE_INPUT_ENVELOPE_TOKENS * prices.fresh_image_input)
            .min(i128::from(i64::MAX)) as i64
    }

    const fn snapshot(self) -> SnapshotOpenAiImageOperation {
        match self {
            Self::Generation => SnapshotOpenAiImageOperation::Generation,
            Self::Edit { .. } => SnapshotOpenAiImageOperation::Edit,
        }
    }

    const fn reference_count(self) -> i64 {
        match self {
            Self::Generation => 0,
            Self::Edit { references } => references,
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

/// One immutable image admission quote plus the hot override version it priced with (settlement
/// replays exactly that version; `None` is the compiled tariff).
pub(super) struct OpenAiImageQuote {
    snapshot: LegacyScalarAdmissionSnapshot,
    pin: Option<PinnedTariff>,
}

impl OpenAiImageQuote {
    pub(super) fn pinned_tariff(&self) -> Option<PinnedTariff> {
        self.pin.clone()
    }

    pub(super) fn into_snapshot(self) -> LegacyScalarAdmissionSnapshot {
        self.snapshot
    }
}

pub(super) fn openai_image_quote(input: OpenAiImageQuoteInput) -> Result<Option<OpenAiImageQuote>> {
    ensure!(
        input.quote_ts > 0,
        "OpenAI image quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "OpenAI image multiplier is outside the snapshot contract"
    );
    ensure!(
        input.payable_multiplier_bp == 0 || input.available_nano > 0,
        "priced OpenAI image quote requires positive available balance"
    );
    // A zero multiplier is a free-but-metered account: it holds nothing. Any paying account
    // holds at least one nanoUSD so the reservation is a real claim on the balance.
    let minimum_hold = i128::from(input.payable_multiplier_bp.min(1));
    let charged_hold_nano = metering::apply_multiplier(
        i128::from(input_official_hold(&input)?),
        input.payable_multiplier_bp,
    )
    .clamp(minimum_hold, i128::from(i64::MAX)) as i64;
    let _ = charged_hold_nano;
    build_openai_image_quote(input, 0).map(Some)
}

fn input_official_hold(input: &OpenAiImageQuoteInput) -> Result<i64> {
    let tariff = metering::openai_image_tariff(&input.requested_model_id)
        .map_err(|_| anyhow::anyhow!("unsupported OpenAI image model identity"))?;
    let resolved = tariff_book::reserve_base(
        &tariff_book::snapshot(),
        metering::openai_image_tariff_family(),
        input.quote_ts,
        tariff.prices,
        tariff_book::as_openai_image,
    );
    Ok(input.operation.official_hold_nano(&resolved.prices))
}

fn build_openai_image_quote(
    input: OpenAiImageQuoteInput,
    charged_hold_nano: i64,
) -> Result<OpenAiImageQuote> {
    ensure!(
        input.quote_ts > 0,
        "OpenAI image quote timestamp must be positive"
    );
    ensure!(
        (0..=10_000).contains(&input.payable_multiplier_bp),
        "OpenAI image multiplier is outside the snapshot contract"
    );
    let tariff = metering::openai_image_tariff(&input.requested_model_id)
        .map_err(|_| anyhow::anyhow!("unsupported OpenAI image model identity"))?;
    let resolved = tariff_book::reserve_base(
        &tariff_book::snapshot(),
        metering::openai_image_tariff_family(),
        input.quote_ts,
        tariff.prices,
        tariff_book::as_openai_image,
    );
    let official_hold_nano = input.operation.official_hold_nano(&resolved.prices);
    let tariff_schedule_id = resolved
        .pin
        .as_ref()
        .map(|pin| pin.schedule_id.clone())
        .unwrap_or_else(|| tariff.tariff_schedule_id.as_str().to_owned());
    let snapshot = LegacyScalarAdmissionSnapshot::new(LegacyScalarAdmissionSnapshotInput {
        request_id: input.request_id.as_str().to_owned(),
        account_id: input.account_id,
        provider: SnapshotProvider::OpenAi,
        requested_model_id: input.requested_model_id,
        canonical_model_id: tariff.canonical_model_id.to_owned(),
        alias_generation: metering::OPENAI_IMAGE_ALIAS_GENERATION,
        tariff_schedule_id,
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
    Ok(OpenAiImageQuote {
        snapshot,
        pin: resolved.pin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request_id() -> EnginePricingRequestId {
        EnginePricingRequestId::from_engine_uuid_v4("123e4567-e89b-42d3-a456-426614174000").unwrap()
    }

    /// The price-parameterized hold formula must reproduce the pinned hold constants under the
    /// compiled rate card, or an empty override book would change admission money.
    #[test]
    fn compiled_prices_reproduce_the_pinned_hold_constants() {
        let prices = metering::openai_image_compiled_tariff().1;
        assert_eq!(
            OpenAiImageOperation::Generation.official_hold_nano(&prices),
            GENERATION_HOLD_NANO
        );
        for references in 1..=MAX_EDIT_REFERENCES {
            assert_eq!(
                OpenAiImageOperation::edit(references)
                    .unwrap()
                    .official_hold_nano(&prices),
                references as i64 * REFERENCE_ENVELOPE_NANO + GENERATION_HOLD_NANO
            );
        }
    }

    #[test]
    fn edit_operation_bounds_reference_count() {
        assert!(OpenAiImageOperation::edit(0).is_none());
        assert!(OpenAiImageOperation::edit(1).is_some());
        assert!(OpenAiImageOperation::edit(MAX_EDIT_REFERENCES).is_some());
        assert!(OpenAiImageOperation::edit(MAX_EDIT_REFERENCES + 1).is_none());
    }

    #[test]
    fn quote_requires_positive_paid_balance_and_rejects_unknown_identity() {
        let quoted = openai_image_quote(OpenAiImageQuoteInput {
            request_id: request_id(),
            account_id: "acct".to_owned(),
            requested_model_id: metering::GPT_IMAGE_2_ALIAS.to_owned(),
            quote_ts: 1_800_000_000,
            payable_multiplier_bp: 10_000,
            operation: OpenAiImageOperation::Generation,
            available_nano: GENERATION_HOLD_NANO - 1,
        })
        .unwrap()
        .expect("paid image work starts whenever the balance is strictly positive");
        assert_eq!(quoted.into_snapshot().charged_hold_nano(), 0);

        let meter_only = openai_image_quote(OpenAiImageQuoteInput {
            request_id: request_id(),
            account_id: "service".to_owned(),
            requested_model_id: metering::GPT_IMAGE_2_ALIAS.to_owned(),
            quote_ts: 1_800_000_000,
            payable_multiplier_bp: 0,
            operation: OpenAiImageOperation::Generation,
            available_nano: 0,
        })
        .unwrap()
        .expect("zero-multiplier image usage must not require customer balance");
        assert_eq!(meter_only.into_snapshot().charged_hold_nano(), 0);

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
