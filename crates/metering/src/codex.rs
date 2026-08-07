//! OpenAI-compatible (Codex) price catalog.
//!
//! The Codex catalog lives here for the same reason the Claude tariffs do: money must be priced
//! from one audited, effective-dated table with unit tests, not from a literal embedded in the
//! composition layer. `server::config` selects which advertised ids are enabled, `forward` receives
//! the resolved prices, and neither invents a rate.
//!
//! API values are nanodollars per token (= $/Mtoken × 1000), copied from the official OpenAI
//! pricing table. ChatGPT-subscription values are nanocredits per token (= credits/Mtoken × 1000),
//! copied from the separate Codex credit rate card. The units deliberately never convert into one
//! another: one describes public API replacement cost, the other describes subscription quota.
//! Neither is updated remotely at runtime; a change is a reviewed commit so historical settlement
//! and calibration remain explainable.

use crate::{apply_multiplier, TariffIdentityError, TariffScheduleId};

/// Official API-equivalent token rates in nanodollars per token.
///
/// The rates are attached to each advertised model rather than inferred from a substring at
/// settlement time. Updating a public alias therefore cannot silently change money.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexPrices {
    pub input: i128,
    pub cached_input: i128,
    pub cache_write_input: i128,
    pub output: i128,
    /// Public API Fast-mode price multiplier. This is not the ChatGPT-subscription credit
    /// multiplier: GPT-5.6 API Fast is 2x after 2026-07-30 while subscription Fast is 2.5x.
    pub api_fast_multiplier_basis_points: i64,
    /// OpenAI applies long-context pricing to the whole request above this input-token boundary.
    pub long_context_threshold: u64,
    pub long_input_basis_points: i64,
    pub long_output_basis_points: i64,
}

/// Official ChatGPT Codex credit rates in nanocredits per token.
///
/// The published subscription card has exactly three priced classes. `cached_input` is a subset
/// of `input`; reasoning tokens are a subset of `output` and therefore use the output rate. The
/// card publishes no cache-write premium or long-context multiplier, so neither is invented here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexCreditRates {
    pub input: i128,
    pub cached_input: i128,
    pub output: i128,
}

/// Exact credit breakdown for one completed ChatGPT-backed turn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodexCreditUsage {
    pub fresh_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub input_credit_nano: i128,
    pub cached_input_credit_nano: i128,
    pub output_credit_nano: i128,
    pub total_credit_nano: i128,
}

/// One advertised OpenAI-compatible model: public id, the id sent upstream, and its rates as
/// of the requested moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexModelSpec {
    pub id: &'static str,
    pub upstream: &'static str,
    pub max_output_tokens: u64,
    pub reasoning_efforts: &'static [&'static str],
    /// ChatGPT-subscription credit multiplier for Codex Fast mode.
    ///
    /// `None` means the model must not accept the Fast/priority service tier. The multiplier is
    /// kept separately from API pricing because GPT-5.6 currently uses different multipliers.
    pub subscription_fast_multiplier_basis_points: Option<i64>,
    pub prices: CodexPrices,
    pub credit_rates: CodexCreditRates,
}

/// A price that takes effect at `effective_from` (unix seconds) and holds until the next entry.
///
/// A future rate change is added as a new entry with its announcement date instead of overwriting
/// the current one, so a settlement replayed at its own timestamp reproduces exactly what the
/// customer was charged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexPriceEpoch {
    pub effective_from: i64,
    pub prices: CodexPrices,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch,
    /// A future price change must add a new epoch with a new id instead of reusing this one.
    tariff_schedule_id: TariffScheduleId,
}

/// Version of the audited public-id to canonical-id mapping below.
pub const CODEX_ALIAS_GENERATION: i64 = 1;
/// Reviewed identity of the native ChatGPT Codex credit card used by calibration events.
pub const CODEX_CREDIT_SCHEDULE_ID: &str = "chatgpt/codex-credits/2026-07-30/v1";

/// Hot-override tariff family root of the native ChatGPT Codex credit card:
/// `CODEX_CREDIT_SCHEDULE_ID` minus its date/version suffix. Per-model override families append
/// the canonical upstream model id (`chatgpt/codex-credits/<model>`) because the compiled card
/// prices each model separately.
pub const CODEX_CREDIT_FAMILY: &str = "chatgpt/codex-credits";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexServiceTier {
    Standard,
    Fast,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexContextTier {
    Standard,
    Long,
}

/// Request-side modifiers that affect the official reserve for a Codex model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexTariffModifiers {
    pub service_tier: CodexServiceTier,
    pub service_tier_multiplier_basis_points: i64,
    pub context_tier: CodexContextTier,
    pub input_multiplier_basis_points: i64,
    pub output_multiplier_basis_points: i64,
}

/// Exact, versioned tariff identity for a model that is safe to persist in a pricing snapshot.
///
/// The caller retains the requested id separately. `canonical_model_id` is the audited upstream
/// pricing identity, so `gpt-5.6` and `gpt-5.6-sol` cannot acquire different policy rules or rates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CodexAdmissionTariffIdentity {
    pub canonical_model_id: &'static str,
    pub max_output_tokens: u64,
    pub alias_generation: i64,
    pub tariff_schedule_id: TariffScheduleId,
    pub schedule_effective_from: i64,
    pub prices: CodexPrices,
    pub modifiers: CodexTariffModifiers,
}

struct CatalogEntry {
    id: &'static str,
    upstream: &'static str,
    max_output_tokens: u64,
    reasoning_efforts: &'static [&'static str],
    subscription_fast_multiplier_basis_points: Option<i64>,
    credit_rates: CodexCreditRates,
    /// Hot-override tariff family of the API-equivalent price card: `openai/codex/<upstream>`.
    /// Keyed by the canonical upstream identity so the default alias and its concrete model share
    /// one family, exactly as they share one schedule.
    tariff_family: &'static str,
    /// Hot-override tariff family of the native credit card: `chatgpt/codex-credits/<upstream>`.
    credit_family: &'static str,
    /// Ordered oldest-first.
    schedule: &'static [IdentifiedCodexPriceEpoch],
}

const EFFORTS_WITH_MAX: &[&str] = &["none", "low", "medium", "high", "xhigh", "max"];
const EFFORTS_STANDARD: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const FAST_25X: Option<i64> = Some(25_000);
const FAST_20X: Option<i64> = Some(20_000);
const PRICE_CUT_2026_07_30: i64 = 1_785_369_600;

/// OpenAI's long-context boundary and multipliers are uniform across the pinned catalog.
const fn prices(
    input: i128,
    cached_input: i128,
    cache_write_input: i128,
    output: i128,
    api_fast_multiplier_basis_points: i64,
) -> CodexPrices {
    CodexPrices {
        input,
        cached_input,
        cache_write_input,
        output,
        api_fast_multiplier_basis_points,
        long_context_threshold: 272_000,
        long_input_basis_points: 20_000,
        long_output_basis_points: 15_000,
    }
}

const fn credits(input: i128, cached_input: i128, output: i128) -> CodexCreditRates {
    CodexCreditRates {
        input,
        cached_input,
        output,
    }
}

// GPT-5.6 explicit/implicit cache writes are billed at 1.25x fresh input. Older advertised
// GPT-5.5/5.4 models retain their published input-rate write price. Keep this as a separate bucket:
// a write is neither a normal input token nor a discounted cache read.
// Source: https://developers.openai.com/api/docs/guides/latest-model#using-gpt-56
const GPT_56_SOL_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: 0,
            prices: prices(5_000, 500, 6_250, 30_000, 25_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-sol/epoch-0/v1"),
    },
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: PRICE_CUT_2026_07_30,
            prices: prices(5_000, 500, 6_250, 30_000, 20_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-sol/2026-07-30/v2"),
    },
];
const GPT_56_TERRA_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: 0,
            prices: prices(2_500, 250, 3_125, 15_000, 25_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-terra/epoch-0/v1"),
    },
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: PRICE_CUT_2026_07_30,
            prices: prices(2_000, 200, 2_500, 12_000, 20_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-terra/2026-07-30/v2"),
    },
];
const GPT_56_LUNA_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: 0,
            prices: prices(1_000, 100, 1_250, 6_000, 25_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-luna/epoch-0/v1"),
    },
    IdentifiedCodexPriceEpoch {
        epoch: CodexPriceEpoch {
            effective_from: PRICE_CUT_2026_07_30,
            prices: prices(200, 20, 250, 1_200, 20_000),
        },
        tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-luna/2026-07-30/v2"),
    },
];
const GPT_55_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(5_000, 500, 5_000, 30_000, 25_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.5/epoch-0/v1"),
}];
const GPT_54_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(2_500, 250, 2_500, 15_000, 20_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.4/epoch-0/v1"),
}];

/// The pinned catalog. `gpt-5.6` is the default alias and must stay bound to the same upstream
/// model and schedule as `gpt-5.6-sol`.
const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "gpt-5.6",
        upstream: "gpt-5.6-sol",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        subscription_fast_multiplier_basis_points: FAST_25X,
        credit_rates: credits(125_000, 12_500, 750_000),
        tariff_family: "openai/codex/gpt-5.6-sol",
        credit_family: "chatgpt/codex-credits/gpt-5.6-sol",
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-sol",
        upstream: "gpt-5.6-sol",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        subscription_fast_multiplier_basis_points: FAST_25X,
        credit_rates: credits(125_000, 12_500, 750_000),
        tariff_family: "openai/codex/gpt-5.6-sol",
        credit_family: "chatgpt/codex-credits/gpt-5.6-sol",
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-terra",
        upstream: "gpt-5.6-terra",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        subscription_fast_multiplier_basis_points: FAST_25X,
        credit_rates: credits(50_000, 5_000, 300_000),
        tariff_family: "openai/codex/gpt-5.6-terra",
        credit_family: "chatgpt/codex-credits/gpt-5.6-terra",
        schedule: GPT_56_TERRA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-luna",
        upstream: "gpt-5.6-luna",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        subscription_fast_multiplier_basis_points: FAST_25X,
        credit_rates: credits(5_000, 500, 30_000),
        tariff_family: "openai/codex/gpt-5.6-luna",
        credit_family: "chatgpt/codex-credits/gpt-5.6-luna",
        schedule: GPT_56_LUNA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.5",
        upstream: "gpt-5.5",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
        subscription_fast_multiplier_basis_points: FAST_25X,
        credit_rates: credits(125_000, 12_500, 750_000),
        tariff_family: "openai/codex/gpt-5.5",
        credit_family: "chatgpt/codex-credits/gpt-5.5",
        schedule: GPT_55_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.4",
        upstream: "gpt-5.4",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
        subscription_fast_multiplier_basis_points: FAST_20X,
        credit_rates: credits(62_500, 6_250, 375_000),
        tariff_family: "openai/codex/gpt-5.4",
        credit_family: "chatgpt/codex-credits/gpt-5.4",
        schedule: GPT_54_SCHEDULE,
    },
];

/// Every advertised model, priced as of `now_unix`.
pub fn codex_catalog_at(now_unix: i64) -> Vec<CodexModelSpec> {
    CATALOG
        .iter()
        .map(|entry| CodexModelSpec {
            id: entry.id,
            upstream: entry.upstream,
            max_output_tokens: entry.max_output_tokens,
            reasoning_efforts: entry.reasoning_efforts,
            subscription_fast_multiplier_basis_points: entry
                .subscription_fast_multiplier_basis_points,
            prices: prices_at(entry.schedule, now_unix),
            credit_rates: entry.credit_rates,
        })
        .collect()
}

/// Prices for one advertised id as of `now_unix`. `None` means the id is not in the pinned catalog
/// — an unknown OpenAI model is rejected at configuration time rather than billed at a guess.
pub fn codex_prices_at(model_id: &str, now_unix: i64) -> Option<CodexPrices> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| prices_at(entry.schedule, now_unix))
}

/// The hot-override tariff family and prices of the catalog entry that prices `model_id`.
///
/// Same lookup as `codex_prices_at`, additionally reporting WHICH family the resolution used so a
/// hot override row can target it. The family is keyed by the canonical upstream identity, so the
/// `gpt-5.6` alias and `gpt-5.6-sol` share `openai/codex/gpt-5.6-sol`.
pub fn codex_matched_tariff_at(model_id: &str, now_unix: i64) -> Option<(&'static str, CodexPrices)> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| (entry.tariff_family, prices_at(entry.schedule, now_unix)))
}

/// The shared root of every native ChatGPT Codex credit override family
/// (`CODEX_CREDIT_SCHEDULE_ID` minus its date/version suffix).
pub fn codex_credit_rates_family() -> &'static str {
    CODEX_CREDIT_FAMILY
}

/// Exact current ChatGPT credit rates for one advertised model.
pub fn codex_credit_rates(model_id: &str) -> Option<CodexCreditRates> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| entry.credit_rates)
}

/// Same lookup as `codex_credit_rates`, additionally reporting the per-model hot-override family
/// (`chatgpt/codex-credits/<upstream>`) the compiled credit card resolved to.
pub fn codex_matched_credit_rates_at(model_id: &str) -> Option<(&'static str, CodexCreditRates)> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| (entry.credit_family, entry.credit_rates))
}

/// Every compiled per-model API tariff family with its price vector as of `now_unix`.
///
/// This is the seeding/diff inventory behind the hot tariff override surface: each entry is
/// `(tariff_family, prices)` taken from the same catalog row the matcher reads, so an enumerated
/// price can never diverge from the billed one. Families are keyed by the canonical upstream
/// identity, so the `gpt-5.6` default alias and `gpt-5.6-sol` yield one shared entry (the first
/// catalog occurrence wins).
pub fn codex_compiled_tariffs_at(now_unix: i64) -> Vec<(&'static str, CodexPrices)> {
    let mut tariffs: Vec<(&'static str, CodexPrices)> = Vec::new();
    for entry in CATALOG {
        if tariffs.iter().any(|(family, _)| *family == entry.tariff_family) {
            continue;
        }
        tariffs.push((entry.tariff_family, prices_at(entry.schedule, now_unix)));
    }
    tariffs
}

/// Every compiled per-model native credit family (`chatgpt/codex-credits/<upstream>`) with its
/// compiled rates. Same shared-family dedupe as `codex_compiled_tariffs_at`.
pub fn codex_compiled_credit_rates() -> Vec<(&'static str, CodexCreditRates)> {
    let mut families: Vec<(&'static str, CodexCreditRates)> = Vec::new();
    for entry in CATALOG {
        if families.iter().any(|(family, _)| *family == entry.credit_family) {
            continue;
        }
        families.push((entry.credit_family, entry.credit_rates));
    }
    families
}

/// Resolve an exact snapshot identity without changing any live pricing behavior.
///
/// Unknown ids return a typed fallback; an unknown model must never be presented to the policy
/// shadow as an audited canonical identity.
pub fn codex_tariff_capability_at(
    model_id: &str,
    priced_ts: i64,
    service_tier: CodexServiceTier,
    estimated_input_tokens: u64,
) -> Result<CodexAdmissionTariffIdentity, TariffIdentityError> {
    if priced_ts <= 0 {
        return Err(TariffIdentityError::InvalidPricedTimestamp);
    }
    let entry = CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .ok_or(TariffIdentityError::UnsupportedModelIdentity)?;
    let epoch = price_epoch_at(entry.schedule, priced_ts);
    let prices = epoch.epoch.prices;
    let service_tier_multiplier_basis_points = match service_tier {
        CodexServiceTier::Standard => 10_000,
        CodexServiceTier::Fast => {
            entry
                .subscription_fast_multiplier_basis_points
                .ok_or(TariffIdentityError::UnsupportedModifier)?;
            prices.api_fast_multiplier_basis_points
        }
    };
    let (context_tier, input_multiplier_basis_points, output_multiplier_basis_points) =
        if estimated_input_tokens > prices.long_context_threshold {
            (
                CodexContextTier::Long,
                prices.long_input_basis_points,
                prices.long_output_basis_points,
            )
        } else {
            (CodexContextTier::Standard, 10_000, 10_000)
        };

    Ok(CodexAdmissionTariffIdentity {
        canonical_model_id: entry.upstream,
        max_output_tokens: entry.max_output_tokens,
        alias_generation: CODEX_ALIAS_GENERATION,
        tariff_schedule_id: epoch.tariff_schedule_id,
        schedule_effective_from: epoch.epoch.effective_from,
        prices,
        modifiers: CodexTariffModifiers {
            service_tier,
            service_tier_multiplier_basis_points,
            context_tier,
            input_multiplier_basis_points,
            output_multiplier_basis_points,
        },
    })
}

/// ChatGPT-subscription credit multiplier for Fast mode on one advertised model.
///
/// The provider currently consumes 2.5x credits for GPT-5.6/5.5 Fast turns and 2x for GPT-5.4.
/// Returning `None` is also the capability gate: an unknown or unsupported model cannot silently
/// opt into priority service and bypass the corresponding reserve.
pub fn codex_subscription_fast_multiplier_basis_points(model_id: &str) -> Option<i64> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .and_then(|entry| entry.subscription_fast_multiplier_basis_points)
}

/// Price one authoritative response-usage payload in official ChatGPT subscription credits.
///
/// `input_tokens` is the provider's total input count; cached tokens are its subset. Reasoning is
/// already included in `output_tokens`, so the caller must not add it again.
pub fn codex_credit_cost_nano(
    model_id: &str,
    input_tokens: u64,
    cached_input_tokens: u64,
    output_tokens: u64,
    fast: bool,
) -> Option<CodexCreditUsage> {
    let rates = codex_credit_rates(model_id)?;
    let cached_input_tokens = cached_input_tokens.min(input_tokens);
    let fresh_input_tokens = input_tokens.saturating_sub(cached_input_tokens);
    let fast_multiplier = if fast {
        codex_subscription_fast_multiplier_basis_points(model_id)?
    } else {
        10_000
    };
    let input_credit_nano = apply_multiplier(
        i128::from(fresh_input_tokens).saturating_mul(rates.input),
        fast_multiplier,
    );
    let cached_input_credit_nano = apply_multiplier(
        i128::from(cached_input_tokens).saturating_mul(rates.cached_input),
        fast_multiplier,
    );
    let output_credit_nano = apply_multiplier(
        i128::from(output_tokens).saturating_mul(rates.output),
        fast_multiplier,
    );
    Some(CodexCreditUsage {
        fresh_input_tokens,
        cached_input_tokens,
        output_tokens,
        input_credit_nano,
        cached_input_credit_nano,
        output_credit_nano,
        total_credit_nano: input_credit_nano
            .saturating_add(cached_input_credit_nano)
            .saturating_add(output_credit_nano),
    })
}

/// Resolve a schedule at a point in time: the newest entry that has already taken effect, or the
/// oldest entry when the timestamp precedes the whole schedule (a clock behind the first epoch must
/// still price, and the launch rate is the conservative choice there).
fn prices_at(schedule: &[IdentifiedCodexPriceEpoch], now_unix: i64) -> CodexPrices {
    price_epoch_at(schedule, now_unix).epoch.prices
}

fn price_epoch_at(
    schedule: &[IdentifiedCodexPriceEpoch],
    now_unix: i64,
) -> &IdentifiedCodexPriceEpoch {
    let mut current = schedule
        .first()
        .expect("every catalog entry has at least one price epoch");
    for epoch in schedule {
        if epoch.epoch.effective_from <= now_unix {
            current = epoch;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH: CodexPrices = prices(5_000, 500, 6_250, 30_000, 25_000);
    const LATER: CodexPrices = prices(9_000, 900, 11_250, 54_000, 20_000);

    const fn identified_epoch(
        effective_from: i64,
        tariff_schedule_id: &'static str,
        epoch_prices: CodexPrices,
    ) -> IdentifiedCodexPriceEpoch {
        IdentifiedCodexPriceEpoch {
            epoch: CodexPriceEpoch {
                effective_from,
                prices: epoch_prices,
            },
            tariff_schedule_id: TariffScheduleId::from_static(tariff_schedule_id),
        }
    }

    #[test]
    fn catalog_advertises_the_pinned_models_and_rates() {
        let catalog = codex_catalog_at(0);
        let ids: Vec<_> = catalog.iter().map(|model| model.id).collect();
        assert_eq!(
            ids,
            vec![
                "gpt-5.6",
                "gpt-5.6-sol",
                "gpt-5.6-terra",
                "gpt-5.6-luna",
                "gpt-5.5",
                "gpt-5.4"
            ]
        );
        // The default alias and its concrete model must never drift apart in price or upstream id.
        let alias = catalog.iter().find(|model| model.id == "gpt-5.6").unwrap();
        let concrete = catalog
            .iter()
            .find(|model| model.id == "gpt-5.6-sol")
            .unwrap();
        assert_eq!(alias.upstream, concrete.upstream);
        assert_eq!(alias.prices, concrete.prices);
        assert_eq!(
            alias.subscription_fast_multiplier_basis_points,
            concrete.subscription_fast_multiplier_basis_points
        );
        assert_eq!(alias.credit_rates, concrete.credit_rates);
        assert_eq!(alias.prices.input, 5_000);
        assert_eq!(alias.prices.output, 30_000);
    }

    #[test]
    fn fast_mode_is_enabled_with_the_published_subscription_multipliers() {
        for model in [
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-5.5",
        ] {
            assert_eq!(
                codex_subscription_fast_multiplier_basis_points(model),
                Some(25_000),
                "{model}"
            );
        }
        assert_eq!(
            codex_subscription_fast_multiplier_basis_points("gpt-5.4"),
            Some(20_000)
        );
        assert_eq!(
            codex_subscription_fast_multiplier_basis_points("gpt-4o"),
            None
        );
    }

    #[test]
    fn unknown_model_has_no_price() {
        assert!(codex_prices_at("gpt-4o", 0).is_none());
        assert!(codex_prices_at("claude-opus-5", 0).is_none());
        assert_eq!(codex_prices_at("gpt-5.6", 0), Some(LAUNCH));
    }

    #[test]
    fn schedule_resolves_the_newest_effective_entry() {
        let schedule = [
            identified_epoch(0, "test/launch", LAUNCH),
            identified_epoch(2_000, "test/later", LATER),
        ];
        assert_eq!(prices_at(&schedule, 0), LAUNCH);
        assert_eq!(prices_at(&schedule, 1_999), LAUNCH);
        assert_eq!(prices_at(&schedule, 2_000), LATER);
        assert_eq!(prices_at(&schedule, i64::MAX), LATER);
    }

    #[test]
    fn a_clock_before_the_first_epoch_uses_the_launch_rate() {
        let schedule = [identified_epoch(5_000, "test/later", LATER)];
        assert_eq!(prices_at(&schedule, 0), LATER);
        assert_eq!(
            price_epoch_at(&schedule, 1).tariff_schedule_id.as_str(),
            "test/later"
        );
    }

    #[test]
    fn tariff_identity_unifies_alias_and_canonical_model() {
        let alias =
            codex_tariff_capability_at("gpt-5.6", 1, CodexServiceTier::Standard, 0).unwrap();
        let canonical =
            codex_tariff_capability_at("gpt-5.6-sol", 1, CodexServiceTier::Standard, 0).unwrap();

        assert_eq!(alias, canonical);
        assert_eq!(alias.canonical_model_id, "gpt-5.6-sol");
        assert_eq!(alias.max_output_tokens, 128_000);
        assert_eq!(alias.alias_generation, CODEX_ALIAS_GENERATION);
        assert_eq!(
            alias.tariff_schedule_id.as_str(),
            "openai/gpt-5.6-sol/epoch-0/v1"
        );
        assert_eq!(alias.schedule_effective_from, 0);
        assert_eq!(alias.prices, codex_prices_at("gpt-5.6", 1).unwrap());
        assert_eq!(
            alias.modifiers,
            CodexTariffModifiers {
                service_tier: CodexServiceTier::Standard,
                service_tier_multiplier_basis_points: 10_000,
                context_tier: CodexContextTier::Standard,
                input_multiplier_basis_points: 10_000,
                output_multiplier_basis_points: 10_000,
            }
        );
    }

    #[test]
    fn tariff_identity_pins_fast_modifier_and_effective_epoch() {
        let fast =
            codex_tariff_capability_at("gpt-5.4", i64::MAX, CodexServiceTier::Fast, 0).unwrap();
        assert_eq!(fast.canonical_model_id, "gpt-5.4");
        assert_eq!(
            fast.tariff_schedule_id.as_str(),
            "openai/gpt-5.4/epoch-0/v1"
        );
        assert_eq!(
            fast.modifiers,
            CodexTariffModifiers {
                service_tier: CodexServiceTier::Fast,
                service_tier_multiplier_basis_points: 20_000,
                context_tier: CodexContextTier::Standard,
                input_multiplier_basis_points: 10_000,
                output_multiplier_basis_points: 10_000,
            }
        );

        let schedule = [
            identified_epoch(0, "test/launch", LAUNCH),
            identified_epoch(2_000, "test/later", LATER),
        ];
        assert_eq!(
            price_epoch_at(&schedule, 1_999).tariff_schedule_id.as_str(),
            "test/launch"
        );
        assert_eq!(
            price_epoch_at(&schedule, 2_000).tariff_schedule_id.as_str(),
            "test/later"
        );
    }

    #[test]
    fn tariff_identity_rejects_unknown_or_ambiguous_ids() {
        for model in ["", "gpt-5.6-latest", "Gpt-5.6", "gpt-4o"] {
            assert_eq!(
                codex_tariff_capability_at(model, 1, CodexServiceTier::Standard, 0),
                Err(TariffIdentityError::UnsupportedModelIdentity),
                "{model}"
            );
        }
        for priced_ts in [i64::MIN, -1, 0] {
            assert_eq!(
                codex_tariff_capability_at("gpt-5.6", priced_ts, CodexServiceTier::Standard, 0,),
                Err(TariffIdentityError::InvalidPricedTimestamp)
            );
        }
    }

    #[test]
    fn tariff_capability_has_golden_identity_for_every_model_and_modifier() {
        for (requested, canonical, schedule_id, expected_prices, fast_basis_points) in [
            (
                "gpt-5.6",
                "gpt-5.6-sol",
                "openai/gpt-5.6-sol/epoch-0/v1",
                prices(5_000, 500, 6_250, 30_000, 25_000),
                25_000,
            ),
            (
                "gpt-5.6-sol",
                "gpt-5.6-sol",
                "openai/gpt-5.6-sol/epoch-0/v1",
                prices(5_000, 500, 6_250, 30_000, 25_000),
                25_000,
            ),
            (
                "gpt-5.6-terra",
                "gpt-5.6-terra",
                "openai/gpt-5.6-terra/epoch-0/v1",
                prices(2_500, 250, 3_125, 15_000, 25_000),
                25_000,
            ),
            (
                "gpt-5.6-luna",
                "gpt-5.6-luna",
                "openai/gpt-5.6-luna/epoch-0/v1",
                prices(1_000, 100, 1_250, 6_000, 25_000),
                25_000,
            ),
            (
                "gpt-5.5",
                "gpt-5.5",
                "openai/gpt-5.5/epoch-0/v1",
                prices(5_000, 500, 5_000, 30_000, 25_000),
                25_000,
            ),
            (
                "gpt-5.4",
                "gpt-5.4",
                "openai/gpt-5.4/epoch-0/v1",
                prices(2_500, 250, 2_500, 15_000, 20_000),
                20_000,
            ),
        ] {
            for service_tier in [CodexServiceTier::Standard, CodexServiceTier::Fast] {
                for (estimated_input_tokens, context_tier, input_bp, output_bp) in [
                    (272_000, CodexContextTier::Standard, 10_000, 10_000),
                    (272_001, CodexContextTier::Long, 20_000, 15_000),
                ] {
                    let identity = codex_tariff_capability_at(
                        requested,
                        1,
                        service_tier,
                        estimated_input_tokens,
                    )
                    .unwrap();
                    assert_eq!(identity.canonical_model_id, canonical, "{requested}");
                    assert_eq!(identity.max_output_tokens, 128_000, "{requested}");
                    assert_eq!(identity.alias_generation, 1, "{requested}");
                    assert_eq!(identity.tariff_schedule_id.as_str(), schedule_id);
                    assert_eq!(identity.schedule_effective_from, 0);
                    assert_eq!(identity.prices, expected_prices, "{requested}");
                    assert_eq!(identity.modifiers.service_tier, service_tier);
                    assert_eq!(
                        identity.modifiers.service_tier_multiplier_basis_points,
                        if service_tier == CodexServiceTier::Fast {
                            fast_basis_points
                        } else {
                            10_000
                        },
                        "{requested}"
                    );
                    assert_eq!(identity.modifiers.context_tier, context_tier);
                    assert_eq!(identity.modifiers.input_multiplier_basis_points, input_bp);
                    assert_eq!(identity.modifiers.output_multiplier_basis_points, output_bp);
                }
            }
        }
    }

    #[test]
    fn long_context_policy_is_uniform_across_the_catalog() {
        for model in codex_catalog_at(0) {
            assert_eq!(model.prices.long_context_threshold, 272_000);
            assert_eq!(model.prices.long_input_basis_points, 20_000);
            assert_eq!(model.prices.long_output_basis_points, 15_000);
            assert_eq!(model.max_output_tokens, 128_000);
        }
    }

    #[test]
    fn cached_input_is_always_cheaper_than_fresh_input() {
        for model in codex_catalog_at(0) {
            assert!(model.prices.cached_input < model.prices.input);
            assert!(model.prices.output > model.prices.input);
        }
    }

    #[test]
    fn cache_write_rates_match_the_model_family_contract() {
        for model in codex_catalog_at(0) {
            if model.id.starts_with("gpt-5.6") {
                assert_eq!(
                    model.prices.cache_write_input,
                    model.prices.input * 5 / 4,
                    "{} must charge the published 1.25x GPT-5.6 cache-write rate",
                    model.id
                );
            } else {
                assert_eq!(
                    model.prices.cache_write_input, model.prices.input,
                    "{} must retain its input-rate cache-write price",
                    model.id
                );
            }
        }
    }

    #[test]
    fn july_price_cut_is_effective_dated_without_repricing_history() {
        let before = PRICE_CUT_2026_07_30 - 1;
        let after = PRICE_CUT_2026_07_30;

        let terra_before = codex_prices_at("gpt-5.6-terra", before).unwrap();
        let terra_after = codex_prices_at("gpt-5.6-terra", after).unwrap();
        assert_eq!(terra_before.input, 2_500);
        assert_eq!(terra_before.cached_input, 250);
        assert_eq!(terra_before.output, 15_000);
        assert_eq!(terra_before.api_fast_multiplier_basis_points, 25_000);
        assert_eq!(terra_after.input, 2_000);
        assert_eq!(terra_after.cached_input, 200);
        assert_eq!(terra_after.output, 12_000);
        assert_eq!(terra_after.api_fast_multiplier_basis_points, 20_000);

        let luna_after = codex_prices_at("gpt-5.6-luna", after).unwrap();
        assert_eq!(luna_after.input, 200);
        assert_eq!(luna_after.cached_input, 20);
        assert_eq!(luna_after.output, 1_200);
        assert_eq!(luna_after.api_fast_multiplier_basis_points, 20_000);

        let sol_after =
            codex_tariff_capability_at("gpt-5.6-sol", after, CodexServiceTier::Fast, 0).unwrap();
        assert_eq!(
            sol_after.tariff_schedule_id.as_str(),
            "openai/gpt-5.6-sol/2026-07-30/v2"
        );
        assert_eq!(
            sol_after.modifiers.service_tier_multiplier_basis_points,
            20_000
        );
        assert_eq!(
            codex_subscription_fast_multiplier_basis_points("gpt-5.6-sol"),
            Some(25_000)
        );
    }

    #[test]
    fn credit_pricing_keeps_cached_and_reasoning_semantics_exact() {
        let usage = codex_credit_cost_nano("gpt-5.6-terra", 1_000, 800, 100, false).unwrap();
        assert_eq!(usage.fresh_input_tokens, 200);
        assert_eq!(usage.cached_input_tokens, 800);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.input_credit_nano, 10_000_000);
        assert_eq!(usage.cached_input_credit_nano, 4_000_000);
        assert_eq!(usage.output_credit_nano, 30_000_000);
        assert_eq!(usage.total_credit_nano, 44_000_000);

        let fast = codex_credit_cost_nano("gpt-5.6-terra", 1_000, 800, 100, true).unwrap();
        assert_eq!(fast.total_credit_nano, 110_000_000);
        assert!(codex_credit_cost_nano("gpt-4o", 1, 0, 1, false).is_none());
    }

    #[test]
    fn credit_catalog_matches_every_published_model_rate() {
        for (model, expected) in [
            ("gpt-5.6", credits(125_000, 12_500, 750_000)),
            ("gpt-5.6-sol", credits(125_000, 12_500, 750_000)),
            ("gpt-5.6-terra", credits(50_000, 5_000, 300_000)),
            ("gpt-5.6-luna", credits(5_000, 500, 30_000)),
            ("gpt-5.5", credits(125_000, 12_500, 750_000)),
            ("gpt-5.4", credits(62_500, 6_250, 375_000)),
        ] {
            assert_eq!(codex_credit_rates(model), Some(expected), "{model}");
        }
    }

    #[test]
    fn matched_tariff_reports_the_upstream_family_and_identical_prices() {
        for (model, family) in [
            ("gpt-5.6", "openai/codex/gpt-5.6-sol"),
            ("gpt-5.6-sol", "openai/codex/gpt-5.6-sol"),
            ("gpt-5.6-terra", "openai/codex/gpt-5.6-terra"),
            ("gpt-5.6-luna", "openai/codex/gpt-5.6-luna"),
            ("gpt-5.5", "openai/codex/gpt-5.5"),
            ("gpt-5.4", "openai/codex/gpt-5.4"),
        ] {
            for ts in [0, PRICE_CUT_2026_07_30 - 1, PRICE_CUT_2026_07_30, i64::MAX] {
                let (matched_family, prices) =
                    codex_matched_tariff_at(model, ts).expect("catalog model priced");
                assert_eq!(matched_family, family, "{model} family");
                assert_eq!(
                    Some(prices),
                    codex_prices_at(model, ts),
                    "{model} helper prices must equal codex_prices_at"
                );
            }
            let (credit_family, rates) =
                codex_matched_credit_rates_at(model).expect("catalog model has credit rates");
            assert_eq!(
                credit_family,
                family.replacen("openai/codex/", "chatgpt/codex-credits/", 1),
                "{model} credit family"
            );
            assert_eq!(Some(rates), codex_credit_rates(model), "{model} credit rates");
        }
        // The default alias and its concrete model share one override family, exactly as they
        // share one schedule; an unknown id has neither prices nor a family.
        assert_eq!(
            codex_matched_tariff_at("gpt-5.6", 0),
            codex_matched_tariff_at("gpt-5.6-sol", 0)
        );
        assert_eq!(codex_credit_rates_family(), "chatgpt/codex-credits");
        assert_eq!(codex_matched_tariff_at("gpt-4o", 0), None);
        assert_eq!(codex_matched_credit_rates_at("gpt-4o"), None);
    }

    #[test]
    fn compiled_tariff_enumeration_covers_every_matcher_family_with_identical_prices() {
        let catalog_models: Vec<&'static str> = CATALOG.iter().map(|entry| entry.id).collect();
        for ts in [0, PRICE_CUT_2026_07_30 - 1, PRICE_CUT_2026_07_30, i64::MAX] {
            let enumerated: std::collections::BTreeMap<&'static str, CodexPrices> =
                codex_compiled_tariffs_at(ts).into_iter().collect();
            // The default alias and its concrete model share one family: one entry per upstream.
            assert_eq!(enumerated.len(), 5, "one family per canonical upstream at {ts}");
            for model in &catalog_models {
                let (family, prices) = codex_matched_tariff_at(model, ts).expect("priced");
                assert_eq!(
                    enumerated.get(family),
                    Some(&prices),
                    "{model} family {family} at {ts} must enumerate identical prices"
                );
            }
        }
        let credit_families: std::collections::BTreeMap<&'static str, CodexCreditRates> =
            codex_compiled_credit_rates().into_iter().collect();
        assert_eq!(credit_families.len(), 5, "one credit family per canonical upstream");
        for model in &catalog_models {
            let (family, rates) = codex_matched_credit_rates_at(model).expect("credit card");
            assert_eq!(
                credit_families.get(family),
                Some(&rates),
                "{model} credit family {family} must enumerate identical rates"
            );
        }
    }
}
