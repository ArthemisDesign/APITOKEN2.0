//! Official GLM (Zhipu AI / Z.ai) Open Platform price catalogue, native Coding Plan credit
//! schedule and authoritative usage parsing.
//!
//! Internal customer valuation uses the reviewed Open Platform rates from
//! <https://docs.z.ai/guides/overview/pricing>; it is **not** a claim about what a GLM Coding
//! Plan subscription costs. The subscription publishes its own quota unit — credits — whose
//! official formula lives here as a second, **independent** ledger
//! (`docs/engine/GLM_PROVIDER.md` §5.3): it is never derived from the dollar rates. Values are
//! nanodollars per token (`$/M tokens * 1000`) and all arithmetic is checked integer math.
//!
//! Three properties of this provider drive the shape below and are documented in
//! `docs/engine/GLM_PROVIDER.md`:
//!
//! 1. **Billing follows the served model, not the requested one.** Requests to `glm-5.1` and
//!    `glm-5` are silently re-routed to `glm-5.2`, which has a different rate card than `glm-5`.
//!    Callers must resolve prices from the model the provider reports it served.
//! 2. **There is no published cache-write rate.** Cache storage is documented as
//!    "Limited-time Free" and no separate paid write leg exists. A cache write is by definition
//!    a miss, so — exactly as KIMI decided — write tokens carry the miss rate rather than
//!    silently zero. The field is explicit so the choice is visible and testable; when the
//!    "limited-time" note is lifted, the schedule is updated as a new epoch.
//! 3. **Credits are a native unit, not a derived one.** The multipliers below are the official
//!    published ones (reviewed 2026-08-03), stored as exact rationals in tenths, computed in
//!    fixed-point micro-credits with round-half-up only at the very end.

use serde_json::Value;

/// Reviewed identity of the effective-dated official dollar catalogue below.
/// Change this identity whenever any epoch/rate semantics change.
pub const GLM_TARIFF_SCHEDULE_ID: &str = "zhipu/zai-open-platform/2026-08-03";

/// Reviewed identity of the official native credit schedule (formula, multipliers, off-peak
/// rule). Independent from `GLM_TARIFF_SCHEDULE_ID`: one ledger never re-derives the other.
pub const GLM_CREDIT_SCHEDULE_ID: &str = "zhipu/glm-coding-plan-credits/2026-08-03";

/// Disjoint per-token rates in nanoUSD. No leg overlaps another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmPrices {
    /// Input served from the context cache ("cache hit").
    pub cached_input: i128,
    /// Input not served from cache ("cache miss").
    pub input: i128,
    /// Cache-creation input. GLM documents cache storage as "Limited-time Free" and publishes
    /// no separate write rate; a write is a miss, so this equals `input`. Kept as its own
    /// field so a future published rate is a one-line epoch.
    pub cache_write: i128,
    /// Output tokens. Reasoning tokens are a *subset* of output and are billed at this same
    /// rate, never as an additional leg.
    pub output: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmPriceEpoch {
    pub effective_from: i64,
    pub prices: GlmPrices,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmModelSpec {
    /// Official Open Platform model id, which is the tariff key.
    pub id: &'static str,
    pub display_name: &'static str,
    /// Accepted input context window in tokens.
    pub input_token_limit: u64,
    /// Published maximum output tokens per response.
    pub max_output_tokens: u64,
    pub prices: GlmPrices,
}

/// One subscription-facing model id and the official model whose rate card prices it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmSubscriptionModel {
    /// Model id as accepted on the Coding Plan Anthropic endpoint.
    pub alias: &'static str,
    /// Official Open Platform model id used for replacement-cost pricing.
    pub official_model: &'static str,
    /// Accepted input context window for this alias specifically.
    pub input_token_limit: u64,
    /// Published maximum output tokens for this alias specifically.
    pub max_output_tokens: u64,
}

/// Official native credit multipliers for one model, stored as exact rationals in tenths:
/// `6.9` is `69`. The provider formula is
/// `credits = (input × in_mult + cached_input × cache_mult + output × out_mult) / 10_000`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmCreditRates {
    /// Multiplier for fresh (uncached) input tokens, in tenths.
    pub input_tenths: i128,
    /// Multiplier for cached input tokens, in tenths.
    pub cached_input_tenths: i128,
    /// Multiplier for output tokens, in tenths.
    pub output_tenths: i128,
}

/// Authoritative terminal usage for one GLM turn, in the Anthropic wire shape the Coding
/// Plan's Anthropic-compatible endpoint speaks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GlmUsage {
    /// Uncached input. Disjoint from `cache_read_tokens` and `cache_write_tokens`.
    pub input_tokens: u64,
    /// Input served from cache.
    pub cache_read_tokens: u64,
    /// Input written into the cache on this turn.
    pub cache_write_tokens: u64,
    /// Total output, including reasoning tokens.
    pub output_tokens: u64,
    /// Subset of `output_tokens` reported as reasoning. Never billed separately.
    pub reasoning_output_tokens: u64,
}

/// Why a usage vector cannot be priced. Every variant fails closed rather than under-charging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlmUsageError {
    /// `reasoning_output_tokens` exceeded `output_tokens`, so the subset invariant is broken and
    /// the provider's accounting cannot be trusted for this turn.
    ReasoningExceedsOutput,
    /// Checked integer arithmetic overflowed.
    Overflow,
}

impl GlmUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
            .saturating_add(self.output_tokens)
    }

    pub fn is_zero(&self) -> bool {
        self.total_tokens() == 0
    }

    /// The subset invariant that must hold before any turn is priced or persisted.
    pub fn validate(&self) -> Result<(), GlmUsageError> {
        if self.reasoning_output_tokens > self.output_tokens {
            return Err(GlmUsageError::ReasoningExceedsOutput);
        }
        Ok(())
    }
}

struct CatalogEntry {
    id: &'static str,
    display_name: &'static str,
    input_token_limit: u64,
    max_output_tokens: u64,
    schedule: &'static [GlmPriceEpoch],
    /// Hot-override tariff family of the official dollar card: `zhipu/glm/<id>`.
    tariff_family: &'static str,
    /// Hot-override tariff family of the native credit card: `zhipu/glm-credits/<id>`. Present
    /// exactly when `credit_rates` is.
    credit_family: Option<&'static str>,
    /// Official native credit multipliers. `None` for models with no published multipliers
    /// (`glm-5.1`/`glm-5` exist only so a served id can be *priced*; the provider re-routes
    /// them to `glm-5.2`, and a served id without published multipliers must fail closed on
    /// the credit ledger rather than borrow a neighbour's rate).
    credit_rates: Option<&'static GlmCreditRates>,
}

const fn epoch(cached_input: i128, input: i128, output: i128) -> GlmPriceEpoch {
    GlmPriceEpoch {
        effective_from: 0,
        prices: GlmPrices {
            cached_input,
            input,
            // No published cache-write rate ("Limited-time Free" storage): a write is a miss,
            // so it carries the miss rate.
            cache_write: input,
            output,
        },
    }
}

// Rates below are `$/M tokens * 1000`, reviewed 2026-08-03 against
// docs.z.ai/guides/overview/pricing. GLM publishes no context-length tiering: one flat rate
// spans the whole window.

/// `glm-5.2`: $0.26 hit / $1.40 miss / $4.40 output, 1,000,000 context.
const SCHEDULE_GLM_5_2: &[GlmPriceEpoch] = &[epoch(260, 1_400, 4_400)];

/// `glm-5-turbo`: $0.24 hit / $1.20 miss / $4.00 output, 200,000 context.
const SCHEDULE_GLM_5_TURBO: &[GlmPriceEpoch] = &[epoch(240, 1_200, 4_000)];

/// `glm-4.7`: $0.11 hit / $0.60 miss / $2.20 output, 200,000 context.
const SCHEDULE_GLM_4_7: &[GlmPriceEpoch] = &[epoch(110, 600, 2_200)];

/// `glm-5`: $0.20 hit / $1.00 miss / $3.20 output. Not on the subscription, but reachable as
/// a requested id that the provider silently serves as `glm-5.2`.
const SCHEDULE_GLM_5: &[GlmPriceEpoch] = &[epoch(200, 1_000, 3_200)];

const CONTEXT_1M: u64 = 1_000_000;
const CONTEXT_200K: u64 = 200_000;
const MAX_OUTPUT: u64 = 131_072;

const CREDIT_GLM_5_2: GlmCreditRates = GlmCreditRates {
    input_tenths: 69,        // 6.9
    cached_input_tenths: 17, // 1.7
    output_tenths: 240,      // 24
};

const CREDIT_GLM_5_TURBO: GlmCreditRates = GlmCreditRates {
    input_tenths: 57,        // 5.7
    cached_input_tenths: 15, // 1.5
    output_tenths: 210,      // 21
};

const CREDIT_GLM_4_7: GlmCreditRates = GlmCreditRates {
    input_tenths: 46,        // 4.6
    cached_input_tenths: 12, // 1.2
    output_tenths: 160,      // 16
};

/// Official models that can appear as the **served** id in a response. `glm-5.1` and `glm-5`
/// are not subscription models, but a client can request them and the provider answers on
/// `glm-5.2`; their rows exist so a served (or echoed) id still resolves to an exact reviewed
/// rate card instead of failing open. Their context/output limits mirror `glm-5.2`, the model
/// that actually serves them; standalone limits were not reviewed. Models outside this table
/// (`glm-4.5`, `glm-4.6v`, future ids) are deliberately absent: an absent entry fails closed
/// at reserve.
const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "glm-5.2",
        display_name: "GLM-5.2",
        input_token_limit: CONTEXT_1M,
        max_output_tokens: MAX_OUTPUT,
        schedule: SCHEDULE_GLM_5_2,
        tariff_family: "zhipu/glm/glm-5.2",
        credit_family: Some("zhipu/glm-credits/glm-5.2"),
        credit_rates: Some(&CREDIT_GLM_5_2),
    },
    CatalogEntry {
        id: "glm-5-turbo",
        display_name: "GLM-5 Turbo",
        input_token_limit: CONTEXT_200K,
        max_output_tokens: MAX_OUTPUT,
        schedule: SCHEDULE_GLM_5_TURBO,
        tariff_family: "zhipu/glm/glm-5-turbo",
        credit_family: Some("zhipu/glm-credits/glm-5-turbo"),
        credit_rates: Some(&CREDIT_GLM_5_TURBO),
    },
    CatalogEntry {
        id: "glm-4.7",
        display_name: "GLM-4.7",
        input_token_limit: CONTEXT_200K,
        max_output_tokens: MAX_OUTPUT,
        schedule: SCHEDULE_GLM_4_7,
        tariff_family: "zhipu/glm/glm-4.7",
        credit_family: Some("zhipu/glm-credits/glm-4.7"),
        credit_rates: Some(&CREDIT_GLM_4_7),
    },
    CatalogEntry {
        // Officially the same rate card as glm-5.2 ("glm-5.2 (= glm-5.1)").
        id: "glm-5.1",
        display_name: "GLM-5.1",
        input_token_limit: CONTEXT_1M,
        max_output_tokens: MAX_OUTPUT,
        schedule: SCHEDULE_GLM_5_2,
        tariff_family: "zhipu/glm/glm-5.1",
        credit_family: None,
        credit_rates: None,
    },
    CatalogEntry {
        id: "glm-5",
        display_name: "GLM-5",
        input_token_limit: CONTEXT_1M,
        max_output_tokens: MAX_OUTPUT,
        schedule: SCHEDULE_GLM_5,
        tariff_family: "zhipu/glm/glm-5",
        credit_family: None,
        credit_rates: None,
    },
];

/// Subscription aliases accepted on the Coding Plan, mapped to the official tariff key.
///
/// `glm-5.2[1m]` is not a distinct model: it is the Claude Code spelling that selects the 1M
/// window. It resolves to the same tariff as `glm-5.2` and differs only in accepted context
/// (which for `glm-5.2` already is the 1M window).
const SUBSCRIPTION_MODELS: &[GlmSubscriptionModel] = &[
    GlmSubscriptionModel {
        alias: "glm-5.2",
        official_model: "glm-5.2",
        input_token_limit: CONTEXT_1M,
        max_output_tokens: MAX_OUTPUT,
    },
    GlmSubscriptionModel {
        alias: "glm-5.2[1m]",
        official_model: "glm-5.2",
        input_token_limit: CONTEXT_1M,
        max_output_tokens: MAX_OUTPUT,
    },
    GlmSubscriptionModel {
        alias: "glm-5-turbo",
        official_model: "glm-5-turbo",
        input_token_limit: CONTEXT_200K,
        max_output_tokens: MAX_OUTPUT,
    },
    GlmSubscriptionModel {
        alias: "glm-4.7",
        official_model: "glm-4.7",
        input_token_limit: CONTEXT_200K,
        max_output_tokens: MAX_OUTPUT,
    },
];

fn prices_at(schedule: &'static [GlmPriceEpoch], now_unix: i64) -> GlmPrices {
    let mut current = schedule[0].prices;
    for epoch in schedule {
        if epoch.effective_from <= now_unix {
            current = epoch.prices;
        }
    }
    current
}

pub fn glm_catalog_at(now_unix: i64) -> Vec<GlmModelSpec> {
    CATALOG
        .iter()
        .map(|entry| GlmModelSpec {
            id: entry.id,
            display_name: entry.display_name,
            input_token_limit: entry.input_token_limit,
            max_output_tokens: entry.max_output_tokens,
            prices: prices_at(entry.schedule, now_unix),
        })
        .collect()
}

/// Prices for an **official** model id. Unknown ids return `None` so the caller fails closed.
pub fn glm_prices_at(model_id: &str, now_unix: i64) -> Option<GlmPrices> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| prices_at(entry.schedule, now_unix))
}

/// Every subscription alias, for admission and catalogue construction.
pub fn glm_subscription_models() -> &'static [GlmSubscriptionModel] {
    SUBSCRIPTION_MODELS
}

/// Resolve a subscription-facing model id to its official tariff key.
///
/// Matching is case-insensitive because the provider's own tooling accepts mixed spellings, but
/// it is otherwise exact: an unrecognised id returns `None` rather than falling back to a
/// default, so an unknown or future model cannot be charged at a neighbour's rate.
pub fn glm_resolve_subscription_model(alias: &str) -> Option<GlmSubscriptionModel> {
    SUBSCRIPTION_MODELS
        .iter()
        .find(|entry| entry.alias.eq_ignore_ascii_case(alias))
        .copied()
}

/// Prices for a model id that may be either an official id or a subscription alias.
///
/// This is the lookup billing should use on the **served** model reported by the provider,
/// because a served model can differ from the requested one: requests to `glm-5.1`/`glm-5`
/// are silently served by `glm-5.2`.
pub fn glm_prices_for_served_model(model_id: &str, now_unix: i64) -> Option<GlmPrices> {
    glm_matched_tariff_at(model_id, now_unix).map(|(_, prices)| prices)
}

/// The hot-override tariff family and prices of the official rate card that prices `model_id`.
///
/// Same served-model resolution as `glm_prices_for_served_model`, additionally reporting WHICH
/// family the resolution used: `zhipu/glm/<official_model_id>` of the entry that priced the id,
/// so an alias and its official model share one override family.
pub fn glm_matched_tariff_at(model_id: &str, now_unix: i64) -> Option<(&'static str, GlmPrices)> {
    if let Some(entry) = CATALOG.iter().find(|entry| entry.id == model_id) {
        return Some((entry.tariff_family, prices_at(entry.schedule, now_unix)));
    }
    let resolved = glm_resolve_subscription_model(model_id)?;
    let entry = CATALOG.iter().find(|entry| entry.id == resolved.official_model)?;
    Some((entry.tariff_family, prices_at(entry.schedule, now_unix)))
}

/// Official native credit multipliers for a **served** model id (official id or alias).
///
/// Only the three subscription models have published multipliers. A served id without them —
/// e.g. an echoed `glm-5.1`/`glm-5`, which the provider re-routes to `glm-5.2` — returns
/// `None` so the credit ledger fails closed instead of borrowing `glm-5.2`'s rate.
pub fn glm_credit_rates_for_served_model(model_id: &str) -> Option<GlmCreditRates> {
    glm_matched_credit_rates_at(model_id).map(|(_, rates)| rates)
}

/// Same served-model resolution as `glm_credit_rates_for_served_model`, additionally reporting
/// the hot-override family of the credit card that matched: `zhipu/glm-credits/<official_id>`.
pub fn glm_matched_credit_rates_at(model_id: &str) -> Option<(&'static str, GlmCreditRates)> {
    if let Some(entry) = CATALOG.iter().find(|entry| entry.id == model_id) {
        return entry
            .credit_family
            .zip(entry.credit_rates.copied());
    }
    let resolved = glm_resolve_subscription_model(model_id)?;
    let entry = CATALOG
        .iter()
        .find(|entry| entry.id == resolved.official_model)?;
    entry.credit_family.zip(entry.credit_rates.copied())
}

/// Every compiled per-model tariff family (`zhipu/glm/<official_id>`) with its price vector as
/// of `now_unix`: the seeding/diff inventory behind the hot tariff override surface. Each entry
/// is taken from the same catalog row the matcher reads, so an enumerated price can never
/// diverge from the billed one.
pub fn glm_compiled_tariffs_at(now_unix: i64) -> Vec<(&'static str, GlmPrices)> {
    CATALOG
        .iter()
        .map(|entry| (entry.tariff_family, prices_at(entry.schedule, now_unix)))
        .collect()
}

/// Every compiled native credit family (`zhipu/glm-credits/<official_id>`) with its compiled
/// rates. Only models with published multipliers have a credit family; `glm-5.1`/`glm-5`
/// deliberately have none and are absent here, exactly as the matcher reports.
pub fn glm_compiled_credit_rates() -> Vec<(&'static str, GlmCreditRates)> {
    CATALOG
        .iter()
        .filter_map(|entry| entry.credit_family.zip(entry.credit_rates.copied()))
        .collect()
}

// ── usage parsing ────────────────────────────────────────────────────────────
//
// The exact cache-field names on GLM's Anthropic route are `unknown`
// (docs/engine/GLM_PROVIDER.md §6.1); the parser below accepts the standard Anthropic spellings
// and is tolerant of absent fields, so a missing cache breakdown reads as zero rather than
// breaking the turn. Billing still requires the served model and a non-absent `usage` object.

/// Parse one `usage` object from the Anthropic-compatible endpoint.
///
/// GLM documents no cache TTL split, so `cache_creation_input_tokens` is taken whole. If a
/// TTL-split object ever appears, its parts are summed rather than dropped. The reasoning
/// counter's wire name is likewise undocumented; both `reasoning_tokens` and
/// `reasoning_output_tokens` are accepted and the larger is kept — reasoning is a non-billed
/// subset of output, so the max only strengthens the subset invariant check.
pub fn usage_from_value(u: &Value) -> GlmUsage {
    let g = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    let split = u.get("cache_creation");
    let split_total = split
        .and_then(Value::as_object)
        .map(|obj| {
            obj.values()
                .filter_map(Value::as_u64)
                .fold(0u64, u64::saturating_add)
        })
        .unwrap_or(0);
    let cache_write = if split_total > 0 {
        split_total
    } else {
        g("cache_creation_input_tokens")
    };
    GlmUsage {
        input_tokens: g("input_tokens"),
        cache_read_tokens: g("cache_read_input_tokens"),
        cache_write_tokens: cache_write,
        output_tokens: g("output_tokens"),
        reasoning_output_tokens: g("reasoning_tokens").max(g("reasoning_output_tokens")),
    }
}

/// Parse a non-streaming response body. Returns `None` when the response carries no `usage`,
/// so the caller can distinguish "no authoritative usage" from "a genuine zero".
pub fn usage_from_response_value(value: &Value) -> Option<GlmUsage> {
    value.get("usage").map(usage_from_value)
}

pub fn usage_from_response_json(bytes: &[u8]) -> Option<GlmUsage> {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .as_ref()
        .and_then(usage_from_response_value)
}

/// Merge one SSE event into an accumulating snapshot.
///
/// `message_start` carries input and cache legs; `message_delta` carries the running output
/// count. Output is *replaced*, never summed, because the provider reports it cumulatively —
/// summing would multiply the charge by the number of deltas.
pub fn merge_stream_event(usage: &mut GlmUsage, value: &Value) {
    let event_usage = value
        .get("message")
        .and_then(|m| m.get("usage"))
        .or_else(|| value.get("usage"));
    let Some(parsed) = event_usage.map(usage_from_value) else {
        return;
    };
    if parsed.input_tokens > 0 {
        usage.input_tokens = parsed.input_tokens;
    }
    if parsed.cache_read_tokens > 0 {
        usage.cache_read_tokens = parsed.cache_read_tokens;
    }
    if parsed.cache_write_tokens > 0 {
        usage.cache_write_tokens = parsed.cache_write_tokens;
    }
    if parsed.output_tokens > 0 {
        usage.output_tokens = parsed.output_tokens;
    }
    if parsed.reasoning_output_tokens > 0 {
        usage.reasoning_output_tokens = parsed.reasoning_output_tokens;
    }
}

/// Accumulate terminal usage from a whole SSE body. Returns `None` when no event carried usage.
pub fn usage_from_sse(bytes: &[u8]) -> Option<GlmUsage> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut usage = GlmUsage::default();
    let mut seen = false;
    for raw in text.lines() {
        let line = raw.trim_start();
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        let json = rest.trim();
        if json.is_empty() || json == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(json) else {
            continue;
        };
        let before = usage.clone();
        merge_stream_event(&mut usage, &value);
        if usage != before {
            seen = true;
        }
    }
    if seen {
        Some(usage)
    } else {
        None
    }
}

// ── cost ─────────────────────────────────────────────────────────────────────

/// Exact official replacement cost of one turn, in nanoUSD.
///
/// Legs are disjoint: uncached input, cached input, cache-creation input and output are added
/// once each. Reasoning tokens are a subset of output and contribute through `output_tokens`
/// alone. Every multiplication and addition is checked; overflow fails closed.
pub fn cost_nanodollars(usage: &GlmUsage, prices: &GlmPrices) -> Result<i128, GlmUsageError> {
    usage.validate()?;
    let leg = |tokens: u64, rate: i128| -> Result<i128, GlmUsageError> {
        i128::from(tokens)
            .checked_mul(rate)
            .ok_or(GlmUsageError::Overflow)
    };
    let mut total = leg(usage.input_tokens, prices.input)?;
    for part in [
        leg(usage.cache_read_tokens, prices.cached_input)?,
        leg(usage.cache_write_tokens, prices.cache_write)?,
        leg(usage.output_tokens, prices.output)?,
    ] {
        total = total.checked_add(part).ok_or(GlmUsageError::Overflow)?;
    }
    Ok(total)
}

// ── native credits ───────────────────────────────────────────────────────────

/// Peak window of the official off-peak rule, in SGT wall time (UTC+8): Monday–Friday,
/// 14:00 inclusive to 18:00 exclusive. Saturdays and Sundays are always off-peak.
///
/// Pure arithmetic on unix time, no `chrono`: the crate depends only on `serde_json`.
/// 1970-01-01 was a Thursday, so with Monday numbered 0 the weekday of `days` days after the
/// epoch is `(days + 3) mod 7`.
pub fn glm_is_peak_utc(unix_secs: i64) -> bool {
    let sgt = unix_secs.saturating_add(8 * 3_600);
    let days = sgt.div_euclid(86_400);
    let secs_of_day = sgt.rem_euclid(86_400);
    let weekday = (days + 3).rem_euclid(7); // Monday = 0 … Sunday = 6
    let hour = secs_of_day / 3_600;
    weekday <= 4 && (14..18).contains(&hour)
}

/// Exact native credit cost of one turn, in fixed-point micro-credits (1 credit = 1e6).
///
/// The provider formula `credits = (input × in_mult + cached × cache_mult + output × out_mult)
/// / 10_000` with tenths-stored multipliers is `weighted / 100_000` credits, i.e. exactly
/// `weighted × 10` micro-credits — no division, no rounding. Off-peak is the exact half,
/// `weighted × 5`. The cache-write leg is absent from the official formula (storage is
/// "Limited-time Free"), and reasoning is a subset of output, so neither contributes.
/// Every multiplication and addition is checked; overflow fails closed.
pub fn glm_credit_cost_micro(
    usage: &GlmUsage,
    rates: &GlmCreditRates,
    off_peak: bool,
) -> Result<i128, GlmUsageError> {
    usage.validate()?;
    let leg = |tokens: u64, tenths: i128| -> Result<i128, GlmUsageError> {
        i128::from(tokens)
            .checked_mul(tenths)
            .ok_or(GlmUsageError::Overflow)
    };
    let mut weighted = leg(usage.input_tokens, rates.input_tenths)?;
    for part in [
        leg(usage.cache_read_tokens, rates.cached_input_tenths)?,
        leg(usage.output_tokens, rates.output_tenths)?,
    ] {
        weighted = weighted.checked_add(part).ok_or(GlmUsageError::Overflow)?;
    }
    let micro_per_weighted = if off_peak { 5 } else { 10 };
    weighted
        .checked_mul(micro_per_weighted)
        .ok_or(GlmUsageError::Overflow)
}

/// Whole native credits for one turn at a given moment, applying the official off-peak
/// schedule (×0.5 outside Monday–Friday 14:00–18:00 SGT) and rounding half-up to a whole
/// credit only at the very end. Reconciliation against the provider-side quota endpoint is
/// the calibration plane's job; per-turn round-half-up keeps this ledger deterministic.
pub fn glm_credits_at(
    usage: &GlmUsage,
    rates: &GlmCreditRates,
    unix_secs: i64,
) -> Result<i128, GlmUsageError> {
    let micro = glm_credit_cost_micro(usage, rates, !glm_is_peak_utc(unix_secs))?;
    micro
        .checked_add(500_000)
        .map(|v| v / 1_000_000)
        .ok_or(GlmUsageError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    /// unix time for a given SGT wall clock `days` after the epoch (1970-01-01, a Thursday).
    fn sgt(days: i64, h: i64, m: i64, s: i64) -> i64 {
        days * 86_400 + h * 3_600 + m * 60 + s - 8 * 3_600
    }

    const PEAK_MONDAY_15H: i64 = 4 * 86_400 + 15 * 3_600 - 8 * 3_600;
    const OFF_PEAK_MONDAY_12H: i64 = 4 * 86_400 + 12 * 3_600 - 8 * 3_600;

    #[test]
    fn tariff_schedule_ids_are_pinned() {
        assert_eq!(GLM_TARIFF_SCHEDULE_ID, "zhipu/zai-open-platform/2026-08-03");
        assert_eq!(
            GLM_CREDIT_SCHEDULE_ID,
            "zhipu/glm-coding-plan-credits/2026-08-03"
        );
    }

    #[test]
    fn catalogue_matches_reviewed_official_rates() {
        let g52 = glm_prices_at("glm-5.2", NOW).expect("glm-5.2 priced");
        assert_eq!(g52.cached_input, 260);
        assert_eq!(g52.input, 1_400);
        assert_eq!(g52.output, 4_400);

        let turbo = glm_prices_at("glm-5-turbo", NOW).expect("glm-5-turbo priced");
        assert_eq!(turbo.cached_input, 240);
        assert_eq!(turbo.input, 1_200);
        assert_eq!(turbo.output, 4_000);

        let g47 = glm_prices_at("glm-4.7", NOW).expect("glm-4.7 priced");
        assert_eq!(g47.cached_input, 110);
        assert_eq!(g47.input, 600);
        assert_eq!(g47.output, 2_200);

        // One million tokens of any leg costs exactly the published $/M rate.
        let one_million = GlmUsage {
            input_tokens: 1_000_000,
            ..GlmUsage::default()
        };
        assert_eq!(cost_nanodollars(&one_million, &g52).unwrap(), 1_400_000_000); // $1.40
        let one_million_cached = GlmUsage {
            cache_read_tokens: 1_000_000,
            ..GlmUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&one_million_cached, &g52).unwrap(),
            260_000_000
        ); // $0.26
        let one_million_out = GlmUsage {
            output_tokens: 1_000_000,
            ..GlmUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&one_million_out, &g47).unwrap(),
            2_200_000_000
        ); // $2.20
    }

    #[test]
    fn glm_5_1_and_glm_5_are_priced_in_the_served_catalog() {
        let g52 = glm_prices_at("glm-5.2", NOW).unwrap();
        let g51 = glm_prices_at("glm-5.1", NOW).expect("glm-5.1 priced");
        // Officially the same rate card: "glm-5.2 (= glm-5.1)".
        assert_eq!(g51, g52);

        let g5 = glm_prices_at("glm-5", NOW).expect("glm-5 priced");
        assert_eq!(g5.cached_input, 200);
        assert_eq!(g5.input, 1_000);
        assert_eq!(g5.output, 3_200);
        assert_ne!(g5, g52);
    }

    #[test]
    fn cache_write_carries_the_miss_rate_because_no_paid_leg_is_published() {
        for spec in glm_catalog_at(NOW) {
            assert_eq!(
                spec.prices.cache_write, spec.prices.input,
                "{} must price a cache write as a miss (storage is limited-time free)",
                spec.id
            );
        }
    }

    #[test]
    fn contexts_and_max_output_match_reviewed_limits() {
        let catalog = glm_catalog_at(NOW);
        let get = |id: &str| catalog.iter().find(|s| s.id == id).unwrap();
        assert_eq!(get("glm-5.2").input_token_limit, 1_000_000);
        assert_eq!(get("glm-5-turbo").input_token_limit, 200_000);
        assert_eq!(get("glm-4.7").input_token_limit, 200_000);
        for id in ["glm-5.2", "glm-5-turbo", "glm-4.7"] {
            assert_eq!(get(id).max_output_tokens, 131_072, "{id} max output");
        }
    }

    #[test]
    fn unknown_and_unsupported_models_fail_closed() {
        for id in [
            "glm-5.3",
            "glm-4.5",
            "glm-4.6v",
            "glm-5.2-highspeed",
            "kimi-k3",
        ] {
            assert!(glm_prices_at(id, NOW).is_none(), "{id} must not be priced");
            assert!(
                glm_prices_for_served_model(id, NOW).is_none(),
                "{id} must fail closed as served"
            );
            assert!(
                glm_credit_rates_for_served_model(id).is_none(),
                "{id} must fail closed for credits"
            );
        }
        assert!(glm_resolve_subscription_model("glm-9").is_none());
        assert!(glm_resolve_subscription_model("glm-5.2[2m]").is_none());
    }

    #[test]
    fn subscription_aliases_resolve_case_insensitively() {
        let g52 = glm_resolve_subscription_model("GLM-5.2").unwrap();
        assert_eq!(g52.official_model, "glm-5.2");
        assert_eq!(g52.input_token_limit, CONTEXT_1M);

        let turbo = glm_resolve_subscription_model("Glm-5-Turbo").unwrap();
        assert_eq!(turbo.official_model, "glm-5-turbo");
        assert_eq!(turbo.input_token_limit, CONTEXT_200K);

        let g47 = glm_resolve_subscription_model("glm-4.7").unwrap();
        assert_eq!(g47.official_model, "glm-4.7");
        assert_eq!(g47.max_output_tokens, MAX_OUTPUT);
    }

    #[test]
    fn bracket_form_is_a_window_selector_not_a_model() {
        let bracket = glm_resolve_subscription_model("glm-5.2[1m]").unwrap();
        let plain = glm_resolve_subscription_model("glm-5.2").unwrap();
        assert_eq!(bracket.official_model, plain.official_model);
        assert_eq!(bracket.input_token_limit, CONTEXT_1M);
        assert_eq!(bracket.max_output_tokens, MAX_OUTPUT);
        assert_eq!(
            glm_prices_for_served_model("glm-5.2[1m]", NOW),
            glm_prices_for_served_model("glm-5.2", NOW),
            "the window selector must not change the rate card"
        );
        assert_eq!(
            glm_credit_rates_for_served_model("GLM-5.2[1M]"),
            glm_credit_rates_for_served_model("glm-5.2"),
        );
    }

    #[test]
    fn served_reroute_is_billed_at_the_served_model() {
        // A client asks for glm-5 but the provider silently serves glm-5.2.
        let requested = glm_prices_for_served_model("glm-5", NOW).unwrap();
        let served = glm_prices_for_served_model("glm-5.2", NOW).unwrap();
        assert_ne!(requested, served);
        let usage = GlmUsage {
            input_tokens: 1_000,
            output_tokens: 1_000,
            ..GlmUsage::default()
        };
        // glm-5 rate: 1000*1000 + 1000*3200 = 4_200_000 nanoUSD.
        assert_eq!(cost_nanodollars(&usage, &requested).unwrap(), 4_200_000);
        // glm-5.2 rate: 1000*1400 + 1000*4400 = 5_800_000 nanoUSD. Billing the requested model
        // would undercharge this turn by 1_600_000 nanoUSD — about 28% of the true cost.
        assert_eq!(cost_nanodollars(&usage, &served).unwrap(), 5_800_000);
    }

    #[test]
    fn exact_cost_vectors_for_each_subscription_model() {
        let usage = GlmUsage {
            input_tokens: 1_000,
            cache_read_tokens: 2_000,
            cache_write_tokens: 3_000,
            output_tokens: 4_000,
            ..GlmUsage::default()
        };
        // glm-5.2: 1000*1400 + 2000*260 + 3000*1400 + 4000*4400
        let g52 = glm_prices_at("glm-5.2", NOW).unwrap();
        assert_eq!(cost_nanodollars(&usage, &g52).unwrap(), 23_720_000);
        // glm-5-turbo: 1000*1200 + 2000*240 + 3000*1200 + 4000*4000
        let turbo = glm_prices_at("glm-5-turbo", NOW).unwrap();
        assert_eq!(cost_nanodollars(&usage, &turbo).unwrap(), 21_280_000);
        // glm-4.7: 1000*600 + 2000*110 + 3000*600 + 4000*2200
        let g47 = glm_prices_at("glm-4.7", NOW).unwrap();
        assert_eq!(cost_nanodollars(&usage, &g47).unwrap(), 11_420_000);
    }

    #[test]
    fn disjoint_legs_are_each_counted_once() {
        let prices = glm_prices_at("glm-5.2", NOW).unwrap();
        let usage = GlmUsage {
            input_tokens: 10,
            cache_read_tokens: 20,
            cache_write_tokens: 30,
            output_tokens: 40,
            reasoning_output_tokens: 25,
        };
        // 10*1400 + 20*260 + 30*1400 + 40*4400 = 14_000 + 5_200 + 42_000 + 176_000
        assert_eq!(cost_nanodollars(&usage, &prices).unwrap(), 237_200);
    }

    #[test]
    fn reasoning_is_a_subset_of_output_and_adds_nothing() {
        let prices = glm_prices_at("glm-5.2", NOW).unwrap();
        let rates = glm_credit_rates_for_served_model("glm-5.2").unwrap();
        let without = GlmUsage {
            output_tokens: 100,
            ..GlmUsage::default()
        };
        let with = GlmUsage {
            output_tokens: 100,
            reasoning_output_tokens: 100,
            ..GlmUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&without, &prices).unwrap(),
            cost_nanodollars(&with, &prices).unwrap()
        );
        assert_eq!(
            glm_credit_cost_micro(&without, &rates, false).unwrap(),
            glm_credit_cost_micro(&with, &rates, false).unwrap()
        );
    }

    #[test]
    fn broken_subset_invariant_fails_closed() {
        let prices = glm_prices_at("glm-5.2", NOW).unwrap();
        let rates = glm_credit_rates_for_served_model("glm-5.2").unwrap();
        let usage = GlmUsage {
            output_tokens: 10,
            reasoning_output_tokens: 11,
            ..GlmUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&usage, &prices),
            Err(GlmUsageError::ReasoningExceedsOutput)
        );
        assert_eq!(
            glm_credit_cost_micro(&usage, &rates, false),
            Err(GlmUsageError::ReasoningExceedsOutput)
        );
        assert_eq!(
            glm_credits_at(&usage, &rates, PEAK_MONDAY_15H),
            Err(GlmUsageError::ReasoningExceedsOutput)
        );
    }

    #[test]
    fn overflow_fails_closed_instead_of_saturating() {
        let prices = GlmPrices {
            cached_input: i128::MAX,
            input: i128::MAX,
            cache_write: i128::MAX,
            output: i128::MAX,
        };
        let usage = GlmUsage {
            input_tokens: u64::MAX,
            ..GlmUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&usage, &prices),
            Err(GlmUsageError::Overflow)
        );
    }

    #[test]
    fn credit_vectors_match_the_official_formula() {
        // 100k tokens of a single leg: 100_000 × mult / 10_000 = 10 × mult credits, exact.
        let g52 = glm_credit_rates_for_served_model("glm-5.2").unwrap();
        let single = |field: char| {
            let mut u = GlmUsage {
                input_tokens: 0,
                ..GlmUsage::default()
            };
            match field {
                'i' => u.input_tokens = 100_000,
                'c' => u.cache_read_tokens = 100_000,
                _ => u.output_tokens = 100_000,
            }
            u
        };
        assert_eq!(
            glm_credits_at(&single('i'), &g52, PEAK_MONDAY_15H).unwrap(),
            69
        );
        assert_eq!(
            glm_credits_at(&single('c'), &g52, PEAK_MONDAY_15H).unwrap(),
            17
        );
        assert_eq!(
            glm_credits_at(&single('o'), &g52, PEAK_MONDAY_15H).unwrap(),
            240
        );

        // 10k tokens on every leg: 6.9+1.7+24 = 32.6 credits, exact in micro.
        let all_legs = GlmUsage {
            input_tokens: 10_000,
            cache_read_tokens: 10_000,
            output_tokens: 10_000,
            ..GlmUsage::default()
        };
        assert_eq!(
            glm_credit_cost_micro(&all_legs, &g52, false).unwrap(),
            32_600_000
        );
        assert_eq!(
            glm_credits_at(&all_legs, &g52, PEAK_MONDAY_15H).unwrap(),
            33
        );

        let turbo = glm_credit_rates_for_served_model("glm-5-turbo").unwrap();
        // 5.7+1.5+21 = 28.2 credits.
        assert_eq!(
            glm_credit_cost_micro(&all_legs, &turbo, false).unwrap(),
            28_200_000
        );
        assert_eq!(
            glm_credits_at(&all_legs, &turbo, PEAK_MONDAY_15H).unwrap(),
            28
        );

        let g47 = glm_credit_rates_for_served_model("glm-4.7").unwrap();
        // 4.6+1.2+16 = 21.8 credits.
        assert_eq!(
            glm_credit_cost_micro(&all_legs, &g47, false).unwrap(),
            21_800_000
        );
        assert_eq!(
            glm_credits_at(&all_legs, &g47, PEAK_MONDAY_15H).unwrap(),
            22
        );
    }

    #[test]
    fn credit_micro_is_exact_fixed_point_and_rounds_half_up_at_the_end() {
        let g52 = glm_credit_rates_for_served_model("glm-5.2").unwrap();
        // One fresh input token: 1 × 6.9 / 10_000 = 0.00069 credits = 690 micro, no rounding.
        let one = GlmUsage {
            input_tokens: 1,
            ..GlmUsage::default()
        };
        assert_eq!(glm_credit_cost_micro(&one, &g52, false).unwrap(), 690);
        assert_eq!(glm_credits_at(&one, &g52, PEAK_MONDAY_15H).unwrap(), 0);

        // 1000 output tokens on glm-4.7: 1000 × 16 / 10_000 = 1.6 credits → 2, half-up.
        let g47 = glm_credit_rates_for_served_model("glm-4.7").unwrap();
        let out = GlmUsage {
            output_tokens: 1_000,
            ..GlmUsage::default()
        };
        assert_eq!(glm_credit_cost_micro(&out, &g47, false).unwrap(), 1_600_000);
        assert_eq!(glm_credits_at(&out, &g47, PEAK_MONDAY_15H).unwrap(), 2);
    }

    #[test]
    fn off_peak_is_an_exact_half() {
        let g52 = glm_credit_rates_for_served_model("glm-5.2").unwrap();
        // Even: 240 peak credits → exactly 120 off-peak, nothing to round.
        let out = GlmUsage {
            output_tokens: 100_000,
            ..GlmUsage::default()
        };
        assert_eq!(
            glm_credit_cost_micro(&out, &g52, false).unwrap(),
            240_000_000
        );
        assert_eq!(
            glm_credit_cost_micro(&out, &g52, true).unwrap(),
            120_000_000
        );
        assert_eq!(
            glm_credits_at(&out, &g52, OFF_PEAK_MONDAY_12H).unwrap(),
            120
        );

        // Odd: 69 peak credits → 34.5 off-peak. The half is exact in micro (34_500_000);
        // only the final whole-credit rounding goes half-up to 35.
        let input = GlmUsage {
            input_tokens: 100_000,
            ..GlmUsage::default()
        };
        assert_eq!(
            glm_credit_cost_micro(&input, &g52, false).unwrap(),
            69_000_000
        );
        assert_eq!(
            glm_credit_cost_micro(&input, &g52, true).unwrap(),
            34_500_000
        );
        assert_eq!(glm_credits_at(&input, &g52, PEAK_MONDAY_15H).unwrap(), 69);
        assert_eq!(
            glm_credits_at(&input, &g52, OFF_PEAK_MONDAY_12H).unwrap(),
            35
        );
    }

    #[test]
    fn credit_overflow_fails_closed() {
        let rates = GlmCreditRates {
            input_tenths: i128::MAX,
            cached_input_tenths: i128::MAX,
            output_tenths: i128::MAX,
        };
        let usage = GlmUsage {
            input_tokens: u64::MAX,
            ..GlmUsage::default()
        };
        assert_eq!(
            glm_credit_cost_micro(&usage, &rates, false),
            Err(GlmUsageError::Overflow)
        );
        assert_eq!(
            glm_credits_at(&usage, &rates, PEAK_MONDAY_15H),
            Err(GlmUsageError::Overflow)
        );
    }

    #[test]
    fn peak_window_boundaries() {
        // 1970-01-01 was a Thursday: 14:00 SGT is peak, 13:59:59 is not.
        assert!(glm_is_peak_utc(sgt(0, 14, 0, 0)));
        assert!(!glm_is_peak_utc(sgt(0, 13, 59, 59)));

        // Monday (days = 4): peak includes 14:00 and 17:59:59, excludes 18:00.
        assert!(!glm_is_peak_utc(sgt(4, 13, 59, 59)));
        assert!(glm_is_peak_utc(sgt(4, 14, 0, 0)));
        assert!(glm_is_peak_utc(sgt(4, 17, 59, 59)));
        assert!(!glm_is_peak_utc(sgt(4, 18, 0, 0)));

        // Friday (days = 8): same boundaries hold.
        assert!(glm_is_peak_utc(sgt(8, 14, 0, 0)));
        assert!(glm_is_peak_utc(sgt(8, 17, 59, 59)));
        assert!(!glm_is_peak_utc(sgt(8, 18, 0, 0)));

        // Saturday (days = 9) and Sunday (days = 3) are always off-peak, even at 14:00–18:00.
        assert!(!glm_is_peak_utc(sgt(9, 14, 0, 0)));
        assert!(!glm_is_peak_utc(sgt(9, 15, 30, 0)));
        assert!(!glm_is_peak_utc(sgt(3, 15, 0, 0)));

        // Sanity: the constants used in the credit tests are what their names say.
        assert!(glm_is_peak_utc(PEAK_MONDAY_15H));
        assert!(!glm_is_peak_utc(OFF_PEAK_MONDAY_12H));
    }

    #[test]
    fn non_stream_usage_parses_anthropic_form_with_and_without_cache_fields() {
        let body = br#"{"usage":{"input_tokens":41,"output_tokens":10,
            "cache_read_input_tokens":7,"cache_creation_input_tokens":30,
            "reasoning_tokens":4}}"#;
        let usage = usage_from_response_json(body).unwrap();
        assert_eq!(usage.input_tokens, 41);
        assert_eq!(usage.cache_read_tokens, 7);
        assert_eq!(usage.cache_write_tokens, 30);
        assert_eq!(usage.output_tokens, 10);
        assert_eq!(usage.reasoning_output_tokens, 4);

        // The exact cache-field names on GLM's Anthropic route are unknown; absent fields
        // must parse as zero instead of breaking the turn.
        let bare = br#"{"usage":{"input_tokens":12,"output_tokens":5}}"#;
        let usage = usage_from_response_json(bare).unwrap();
        assert_eq!(usage.input_tokens, 12);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.cache_read_tokens, 0);
        assert_eq!(usage.cache_write_tokens, 0);
        assert_eq!(usage.reasoning_output_tokens, 0);

        // The alternate reasoning spelling is accepted too.
        let alt = br#"{"usage":{"output_tokens":9,"reasoning_output_tokens":3}}"#;
        assert_eq!(
            usage_from_response_json(alt)
                .unwrap()
                .reasoning_output_tokens,
            3
        );
    }

    #[test]
    fn ttl_split_cache_creation_is_summed_not_dropped() {
        let body = br#"{"usage":{"cache_creation":{"ephemeral_5m_input_tokens":20,
            "ephemeral_1h_input_tokens":10},"cache_creation_input_tokens":30}}"#;
        let usage = usage_from_response_json(body).unwrap();
        assert_eq!(usage.cache_write_tokens, 30);
    }

    #[test]
    fn stream_output_is_replaced_not_summed() {
        let sse = concat!(
            "data: {\"type\":\"message_start\",\"message\":{\"usage\":",
            "{\"input_tokens\":100,\"cache_read_input_tokens\":50,\"output_tokens\":1}}}\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":20}}\n",
            "data: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":37}}\n",
            "data: [DONE]\n"
        );
        let usage = usage_from_sse(sse.as_bytes()).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.cache_read_tokens, 50);
        // 37, not 1 + 20 + 37.
        assert_eq!(usage.output_tokens, 37);
    }

    #[test]
    fn missing_usage_is_absent_not_zero() {
        assert!(usage_from_response_json(br#"{"content":[]}"#).is_none());
        assert!(usage_from_sse(b"data: {\"type\":\"ping\"}\n").is_none());
        let zeroed = usage_from_response_json(br#"{"usage":{"input_tokens":0}}"#);
        assert_eq!(zeroed, Some(GlmUsage::default()));
    }

    #[test]
    fn schedule_is_effective_dated_and_stable_before_any_epoch() {
        // A turn priced at unix 0 must still resolve, so a clock skew cannot leave a turn
        // unpriceable.
        assert_eq!(glm_prices_at("glm-5.2", 0), glm_prices_at("glm-5.2", NOW));
    }

    #[test]
    fn matched_tariff_reports_the_official_family_and_identical_prices() {
        for (model, family) in [
            ("glm-5.2", "zhipu/glm/glm-5.2"),
            ("glm-5-turbo", "zhipu/glm/glm-5-turbo"),
            ("glm-4.7", "zhipu/glm/glm-4.7"),
            ("glm-5.1", "zhipu/glm/glm-5.1"),
            ("glm-5", "zhipu/glm/glm-5"),
        ] {
            let (matched_family, prices) = glm_matched_tariff_at(model, NOW).expect("priced");
            assert_eq!(matched_family, family, "{model} family");
            assert_eq!(
                Some(prices),
                glm_prices_for_served_model(model, NOW),
                "{model} helper prices must equal glm_prices_for_served_model"
            );
        }
        // An alias resolves to its official model's family, so one override covers both.
        assert_eq!(
            glm_matched_tariff_at("glm-5.2[1m]", NOW).map(|(family, _)| family),
            Some("zhipu/glm/glm-5.2")
        );
        assert_eq!(glm_matched_tariff_at("glm-9", NOW), None);
    }

    #[test]
    fn matched_credit_rates_report_the_per_model_family() {
        for (model, family) in [
            ("glm-5.2", "zhipu/glm-credits/glm-5.2"),
            ("glm-5-turbo", "zhipu/glm-credits/glm-5-turbo"),
            ("glm-4.7", "zhipu/glm-credits/glm-4.7"),
        ] {
            let (matched_family, rates) =
                glm_matched_credit_rates_at(model).expect("credit card exists");
            assert_eq!(matched_family, family, "{model} credit family");
            assert_eq!(
                Some(rates),
                glm_credit_rates_for_served_model(model),
                "{model} credit rates"
            );
        }
        // Aliases share their official model's credit family; ids without a published card fail
        // closed on both the rates and the family.
        assert_eq!(
            glm_matched_credit_rates_at("glm-5.2[1m]").map(|(family, _)| family),
            Some("zhipu/glm-credits/glm-5.2")
        );
        assert_eq!(glm_matched_credit_rates_at("glm-5.1"), None);
        assert_eq!(glm_matched_credit_rates_at("glm-9"), None);
    }

    #[test]
    fn compiled_tariff_enumeration_covers_every_matcher_family_with_identical_prices() {
        for ts in [0, NOW, i64::MAX] {
            let enumerated: std::collections::BTreeMap<&'static str, GlmPrices> =
                glm_compiled_tariffs_at(ts).into_iter().collect();
            assert_eq!(enumerated.len(), CATALOG.len(), "one family per catalog model");
            for entry in CATALOG {
                let (family, prices) = glm_matched_tariff_at(entry.id, ts).expect("priced");
                assert_eq!(
                    enumerated.get(family),
                    Some(&prices),
                    "{} family {family} at {ts} must enumerate identical prices",
                    entry.id
                );
            }
        }
        let credit_families: std::collections::BTreeMap<&'static str, GlmCreditRates> =
            glm_compiled_credit_rates().into_iter().collect();
        // Only models with published multipliers have a credit family; glm-5.1/glm-5 have none.
        assert_eq!(credit_families.len(), 3);
        for entry in CATALOG {
            match glm_matched_credit_rates_at(entry.id) {
                Some((family, rates)) => assert_eq!(
                    credit_families.get(family),
                    Some(&rates),
                    "{} credit family {family} must enumerate identical rates",
                    entry.id
                ),
                None => assert!(entry.credit_family.is_none(), "{} credit family", entry.id),
            }
        }
    }
}
