//! OpenAI-compatible (Codex) price catalog.
//!
//! The Codex catalog lives here for the same reason the Claude tariffs do: money must be priced
//! from one audited, effective-dated table with unit tests, not from a literal embedded in the
//! composition layer. `server::config` selects which advertised ids are enabled, `forward` receives
//! the resolved prices, and neither invents a rate.
//!
//! Values are nanodollars per token (= $/Mtoken × 1000), copied from the official OpenAI standard
//! token-pricing table. They are never updated remotely at runtime: a price change is a reviewed
//! commit that adds a new schedule entry, so historical settlements stay explainable.

use crate::{TariffIdentityError, TariffScheduleId};

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
    /// OpenAI applies long-context pricing to the whole request above this input-token boundary.
    pub long_context_threshold: u64,
    pub long_input_basis_points: i64,
    pub long_output_basis_points: i64,
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
    /// kept beside the audited price schedule because it changes both customer settlement and the
    /// subscription-window spend used for capacity calibration.
    pub fast_multiplier_basis_points: Option<i64>,
    pub prices: CodexPrices,
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
    fast_multiplier_basis_points: Option<i64>,
    /// Ordered oldest-first.
    schedule: &'static [IdentifiedCodexPriceEpoch],
}

const EFFORTS_WITH_MAX: &[&str] = &["none", "low", "medium", "high", "xhigh", "max"];
const EFFORTS_STANDARD: &[&str] = &["none", "low", "medium", "high", "xhigh"];
const FAST_25X: Option<i64> = Some(25_000);
const FAST_20X: Option<i64> = Some(20_000);

/// OpenAI's long-context boundary and multipliers are uniform across the pinned catalog.
const fn prices(
    input: i128,
    cached_input: i128,
    cache_write_input: i128,
    output: i128,
) -> CodexPrices {
    CodexPrices {
        input,
        cached_input,
        cache_write_input,
        output,
        long_context_threshold: 272_000,
        long_input_basis_points: 20_000,
        long_output_basis_points: 15_000,
    }
}

// GPT-5.6 explicit/implicit cache writes are billed at 1.25x fresh input. Older advertised
// GPT-5.5/5.4 models retain their published input-rate write price. Keep this as a separate bucket:
// a write is neither a normal input token nor a discounted cache read.
// Source: https://developers.openai.com/api/docs/guides/latest-model#using-gpt-56
const GPT_56_SOL_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(5_000, 500, 6_250, 30_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-sol/epoch-0/v1"),
}];
const GPT_56_TERRA_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(2_500, 250, 3_125, 15_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-terra/epoch-0/v1"),
}];
const GPT_56_LUNA_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(1_000, 100, 1_250, 6_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.6-luna/epoch-0/v1"),
}];
const GPT_55_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(5_000, 500, 5_000, 30_000),
    },
    tariff_schedule_id: TariffScheduleId::from_static("openai/gpt-5.5/epoch-0/v1"),
}];
const GPT_54_SCHEDULE: &[IdentifiedCodexPriceEpoch] = &[IdentifiedCodexPriceEpoch {
    epoch: CodexPriceEpoch {
        effective_from: 0,
        prices: prices(2_500, 250, 2_500, 15_000),
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
        fast_multiplier_basis_points: FAST_25X,
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-sol",
        upstream: "gpt-5.6-sol",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        fast_multiplier_basis_points: FAST_25X,
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-terra",
        upstream: "gpt-5.6-terra",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        fast_multiplier_basis_points: FAST_25X,
        schedule: GPT_56_TERRA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-luna",
        upstream: "gpt-5.6-luna",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        fast_multiplier_basis_points: FAST_25X,
        schedule: GPT_56_LUNA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.5",
        upstream: "gpt-5.5",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
        fast_multiplier_basis_points: FAST_25X,
        schedule: GPT_55_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.4",
        upstream: "gpt-5.4",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
        fast_multiplier_basis_points: FAST_20X,
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
            fast_multiplier_basis_points: entry.fast_multiplier_basis_points,
            prices: prices_at(entry.schedule, now_unix),
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
    let service_tier_multiplier_basis_points = match service_tier {
        CodexServiceTier::Standard => 10_000,
        CodexServiceTier::Fast => entry
            .fast_multiplier_basis_points
            .ok_or(TariffIdentityError::UnsupportedModifier)?,
    };
    let prices = epoch.epoch.prices;
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
pub fn codex_fast_multiplier_basis_points(model_id: &str) -> Option<i64> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .and_then(|entry| entry.fast_multiplier_basis_points)
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

    const LAUNCH: CodexPrices = prices(5_000, 500, 6_250, 30_000);
    const LATER: CodexPrices = prices(9_000, 900, 11_250, 54_000);

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
            alias.fast_multiplier_basis_points,
            concrete.fast_multiplier_basis_points
        );
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
                codex_fast_multiplier_basis_points(model),
                Some(25_000),
                "{model}"
            );
        }
        assert_eq!(codex_fast_multiplier_basis_points("gpt-5.4"), Some(20_000));
        assert_eq!(codex_fast_multiplier_basis_points("gpt-4o"), None);
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
                prices(5_000, 500, 6_250, 30_000),
                25_000,
            ),
            (
                "gpt-5.6-sol",
                "gpt-5.6-sol",
                "openai/gpt-5.6-sol/epoch-0/v1",
                prices(5_000, 500, 6_250, 30_000),
                25_000,
            ),
            (
                "gpt-5.6-terra",
                "gpt-5.6-terra",
                "openai/gpt-5.6-terra/epoch-0/v1",
                prices(2_500, 250, 3_125, 15_000),
                25_000,
            ),
            (
                "gpt-5.6-luna",
                "gpt-5.6-luna",
                "openai/gpt-5.6-luna/epoch-0/v1",
                prices(1_000, 100, 1_250, 6_000),
                25_000,
            ),
            (
                "gpt-5.5",
                "gpt-5.5",
                "openai/gpt-5.5/epoch-0/v1",
                prices(5_000, 500, 5_000, 30_000),
                25_000,
            ),
            (
                "gpt-5.4",
                "gpt-5.4",
                "openai/gpt-5.4/epoch-0/v1",
                prices(2_500, 250, 2_500, 15_000),
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
}
