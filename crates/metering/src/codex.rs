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

/// One advertised OpenAI-compatible model: public id, the id sent to app-server, and its rates as
/// of the requested moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CodexModelSpec {
    pub id: &'static str,
    pub upstream: &'static str,
    pub max_output_tokens: u64,
    pub reasoning_efforts: &'static [&'static str],
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

struct CatalogEntry {
    id: &'static str,
    upstream: &'static str,
    max_output_tokens: u64,
    reasoning_efforts: &'static [&'static str],
    /// Ordered oldest-first.
    schedule: &'static [CodexPriceEpoch],
}

const EFFORTS_WITH_MAX: &[&str] = &["none", "low", "medium", "high", "xhigh", "max"];
const EFFORTS_STANDARD: &[&str] = &["none", "low", "medium", "high", "xhigh"];

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

// OpenAI's standard pricing has no cache-*write* surcharge: cached reads are discounted, but
// writing the cache is free. So `cache_write_input` is pinned equal to the fresh `input` rate for
// every model — a client is never charged more than the real API for the same traffic. (The prior
// 5.6-family premium diverged from OpenAI; 5.5/5.4 already priced writes at the input rate.)
const GPT_56_SOL_SCHEDULE: &[CodexPriceEpoch] = &[CodexPriceEpoch {
    effective_from: 0,
    prices: prices(5_000, 500, 5_000, 30_000),
}];
const GPT_56_TERRA_SCHEDULE: &[CodexPriceEpoch] = &[CodexPriceEpoch {
    effective_from: 0,
    prices: prices(2_500, 250, 2_500, 15_000),
}];
const GPT_56_LUNA_SCHEDULE: &[CodexPriceEpoch] = &[CodexPriceEpoch {
    effective_from: 0,
    prices: prices(1_000, 100, 1_000, 6_000),
}];
const GPT_55_SCHEDULE: &[CodexPriceEpoch] = &[CodexPriceEpoch {
    effective_from: 0,
    prices: prices(5_000, 500, 5_000, 30_000),
}];
const GPT_54_SCHEDULE: &[CodexPriceEpoch] = &[CodexPriceEpoch {
    effective_from: 0,
    prices: prices(2_500, 250, 2_500, 15_000),
}];

/// The pinned catalog. `gpt-5.6` is the default alias and must stay bound to the same upstream
/// model and schedule as `gpt-5.6-sol`.
const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "gpt-5.6",
        upstream: "gpt-5.6-sol",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-sol",
        upstream: "gpt-5.6-sol",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        schedule: GPT_56_SOL_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-terra",
        upstream: "gpt-5.6-terra",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        schedule: GPT_56_TERRA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.6-luna",
        upstream: "gpt-5.6-luna",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_WITH_MAX,
        schedule: GPT_56_LUNA_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.5",
        upstream: "gpt-5.5",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
        schedule: GPT_55_SCHEDULE,
    },
    CatalogEntry {
        id: "gpt-5.4",
        upstream: "gpt-5.4",
        max_output_tokens: 128_000,
        reasoning_efforts: EFFORTS_STANDARD,
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

/// Resolve a schedule at a point in time: the newest entry that has already taken effect, or the
/// oldest entry when the timestamp precedes the whole schedule (a clock behind the first epoch must
/// still price, and the launch rate is the conservative choice there).
fn prices_at(schedule: &[CodexPriceEpoch], now_unix: i64) -> CodexPrices {
    let mut current = schedule
        .first()
        .expect("every catalog entry has at least one price epoch")
        .prices;
    for epoch in schedule {
        if epoch.effective_from <= now_unix {
            current = epoch.prices;
        }
    }
    current
}

#[cfg(test)]
mod tests {
    use super::*;

    const LAUNCH: CodexPrices = prices(5_000, 500, 5_000, 30_000);
    const LATER: CodexPrices = prices(9_000, 900, 11_250, 54_000);

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
        assert_eq!(alias.prices.input, 5_000);
        assert_eq!(alias.prices.output, 30_000);
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
            CodexPriceEpoch {
                effective_from: 0,
                prices: LAUNCH,
            },
            CodexPriceEpoch {
                effective_from: 2_000,
                prices: LATER,
            },
        ];
        assert_eq!(prices_at(&schedule, 0), LAUNCH);
        assert_eq!(prices_at(&schedule, 1_999), LAUNCH);
        assert_eq!(prices_at(&schedule, 2_000), LATER);
        assert_eq!(prices_at(&schedule, i64::MAX), LATER);
    }

    #[test]
    fn a_clock_before_the_first_epoch_uses_the_launch_rate() {
        let schedule = [CodexPriceEpoch {
            effective_from: 5_000,
            prices: LATER,
        }];
        assert_eq!(prices_at(&schedule, 0), LATER);
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
    fn cache_write_is_never_charged_above_fresh_input() {
        // OpenAI applies no cache-write surcharge; every advertised model must price a write at
        // (or below) the fresh input rate so a customer never pays more than the real API.
        for model in codex_catalog_at(0) {
            assert!(
                model.prices.cache_write_input <= model.prices.input,
                "{} charges a cache-write premium",
                model.id
            );
        }
    }
}
