//! Official Kimi Open Platform price catalogue and authoritative usage parsing.
//!
//! Internal customer valuation uses the reviewed Open Platform rates from
//! <https://platform.kimi.ai/docs/pricing/chat>; it is **not** a claim about what a Kimi Code
//! subscription costs. The subscription publishes its own opaque quota units, which live in the
//! calibration ledgers — never here. Values are nanodollars per token (`$/M tokens * 1000`) and
//! all arithmetic is checked integer math.
//!
//! Two properties of this provider drive the shape below and are documented in
//! `docs/engine/KIMI_PROVIDER.md`:
//!
//! 1. **Billing follows the served model, not the requested one.** Disabling thinking re-routes
//!    both `k3` and `kimi-for-coding` to K2.6, which has a different rate card. Callers must
//!    resolve prices from the model the provider reports it served.
//! 2. **There is no published cache-write rate.** Kimi documents only a cache-hit and a
//!    cache-miss input rate, with caching described as automatic. Cache-creation tokens were by
//!    definition a miss, so they are priced at the miss rate rather than silently at zero. The
//!    field is explicit so the choice is visible and testable.

use serde_json::Value;

/// Reviewed identity of the effective-dated official catalogue below.
/// Change this identity whenever any epoch/rate semantics change.
pub const KIMI_TARIFF_SCHEDULE_ID: &str = "moonshot/kimi-open-platform/2026-08-03";

/// Disjoint per-token rates in nanoUSD. No leg overlaps another.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KimiPrices {
    /// Input served from the automatic context cache ("cache hit").
    pub cached_input: i128,
    /// Input not served from cache ("cache miss").
    pub input: i128,
    /// Cache-creation input. Kimi publishes no separate write rate; a write is a miss, so this
    /// equals `input`. Kept as its own field so a future published rate is a one-line epoch.
    pub cache_write: i128,
    /// Output tokens. Reasoning/thinking tokens are a *subset* of output and are billed at this
    /// same rate, never as an additional leg.
    pub output: i128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KimiPriceEpoch {
    pub effective_from: i64,
    pub prices: KimiPrices,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiModelSpec {
    /// Official Open Platform model id, which is the tariff key.
    pub id: &'static str,
    pub display_name: &'static str,
    /// Accepted input context window in tokens.
    pub input_token_limit: u64,
    pub prices: KimiPrices,
}

/// One subscription-facing model id and the official model whose rate card prices it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KimiSubscriptionModel {
    /// Model id as accepted on `api.kimi.com/coding`.
    pub alias: &'static str,
    /// Official Open Platform model id used for replacement-cost pricing.
    pub official_model: &'static str,
    /// Accepted input context window for this alias specifically.
    pub input_token_limit: u64,
}

/// Authoritative terminal usage for one Kimi turn, in the Anthropic wire shape the
/// subscription's Anthropic-compatible endpoint speaks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KimiUsage {
    /// Uncached input. Disjoint from `cache_read_tokens` and `cache_write_tokens`.
    pub input_tokens: u64,
    /// Input served from cache.
    pub cache_read_tokens: u64,
    /// Input written into the cache on this turn.
    pub cache_write_tokens: u64,
    /// Total output, including reasoning tokens.
    pub output_tokens: u64,
    /// Subset of `output_tokens` reported as internal reasoning. Never billed separately.
    pub reasoning_output_tokens: u64,
}

/// Why a usage vector cannot be priced. Every variant fails closed rather than under-charging.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KimiUsageError {
    /// `reasoning_output_tokens` exceeded `output_tokens`, so the subset invariant is broken and
    /// the provider's accounting cannot be trusted for this turn.
    ReasoningExceedsOutput,
    /// Checked integer arithmetic overflowed.
    Overflow,
}

impl KimiUsage {
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
    pub fn validate(&self) -> Result<(), KimiUsageError> {
        if self.reasoning_output_tokens > self.output_tokens {
            return Err(KimiUsageError::ReasoningExceedsOutput);
        }
        Ok(())
    }
}

struct CatalogEntry {
    id: &'static str,
    display_name: &'static str,
    input_token_limit: u64,
    /// Hot-override tariff family of the official dollar card: `moonshot/kimi/<id>`.
    tariff_family: &'static str,
    schedule: &'static [KimiPriceEpoch],
}

const fn epoch(cached_input: i128, input: i128, output: i128) -> KimiPriceEpoch {
    KimiPriceEpoch {
        effective_from: 0,
        prices: KimiPrices {
            cached_input,
            input,
            // No published cache-write rate: a write is a miss, so it carries the miss rate.
            cache_write: input,
            output,
        },
    }
}

// Rates below are `$/M tokens * 1000`, reviewed 2026-08-03 against
// platform.kimi.ai/docs/pricing/chat-k3, -chat-k27-code and -chat-k26.
// Kimi publishes no context-length tiering: one flat rate spans the whole window.

/// `kimi-k3`: $0.30 hit / $3.00 miss / $15.00 output, 1,048,576 context.
const SCHEDULE_K3: &[KimiPriceEpoch] = &[epoch(300, 3_000, 15_000)];

/// `kimi-k2.7-code`: $0.19 hit / $0.95 miss / $4.00 output, 262,144 context.
const SCHEDULE_K27_CODE: &[KimiPriceEpoch] = &[epoch(190, 950, 4_000)];

/// `kimi-k2.7-code-highspeed`: exactly double the base SKU on every leg.
const SCHEDULE_K27_CODE_HIGHSPEED: &[KimiPriceEpoch] = &[epoch(380, 1_900, 8_000)];

/// `kimi-k2.6`: $0.16 hit / $0.95 miss / $4.00 output, 262,144 context. Reachable without being
/// requested, because disabling thinking re-routes K3 and K2.7 Code here.
const SCHEDULE_K26: &[KimiPriceEpoch] = &[epoch(160, 950, 4_000)];

const CONTEXT_256K: u64 = 262_144;
const CONTEXT_1M: u64 = 1_048_576;

/// Official models the subscription can actually serve. Models retired from the platform
/// (`kimi-k2-*` on 2026-05-25, `kimi-latest`, `kimi-thinking-preview`, `moonshot-v1-*`,
/// `kimi-k2.5`) are deliberately absent: the subscription does not serve them, and an absent
/// entry fails closed at reserve.
const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "kimi-k3",
        display_name: "Kimi K3",
        input_token_limit: CONTEXT_1M,
        tariff_family: "moonshot/kimi/kimi-k3",
        schedule: SCHEDULE_K3,
    },
    CatalogEntry {
        id: "kimi-k2.7-code",
        display_name: "Kimi K2.7 Code",
        input_token_limit: CONTEXT_256K,
        tariff_family: "moonshot/kimi/kimi-k2.7-code",
        schedule: SCHEDULE_K27_CODE,
    },
    CatalogEntry {
        id: "kimi-k2.7-code-highspeed",
        display_name: "Kimi K2.7 Code HighSpeed",
        input_token_limit: CONTEXT_256K,
        tariff_family: "moonshot/kimi/kimi-k2.7-code-highspeed",
        schedule: SCHEDULE_K27_CODE_HIGHSPEED,
    },
    CatalogEntry {
        id: "kimi-k2.6",
        display_name: "Kimi K2.6",
        input_token_limit: CONTEXT_256K,
        tariff_family: "moonshot/kimi/kimi-k2.6",
        schedule: SCHEDULE_K26,
    },
];

/// Subscription aliases accepted on `api.kimi.com/coding`, mapped to the official tariff key.
///
/// `k3[1m]` is not a distinct model: it is the Claude Code spelling that selects the 1M window.
/// It resolves to the same tariff as `k3` and differs only in accepted context.
const SUBSCRIPTION_MODELS: &[KimiSubscriptionModel] = &[
    KimiSubscriptionModel {
        alias: "kimi-for-coding",
        official_model: "kimi-k2.7-code",
        input_token_limit: CONTEXT_256K,
    },
    KimiSubscriptionModel {
        alias: "kimi-for-coding-highspeed",
        official_model: "kimi-k2.7-code-highspeed",
        input_token_limit: CONTEXT_256K,
    },
    KimiSubscriptionModel {
        alias: "k3",
        official_model: "kimi-k3",
        input_token_limit: CONTEXT_1M,
    },
    KimiSubscriptionModel {
        alias: "k3[1m]",
        official_model: "kimi-k3",
        input_token_limit: CONTEXT_1M,
    },
    KimiSubscriptionModel {
        alias: "k3-256k",
        official_model: "kimi-k3",
        input_token_limit: CONTEXT_256K,
    },
];

fn prices_at(schedule: &'static [KimiPriceEpoch], now_unix: i64) -> KimiPrices {
    let mut current = schedule[0].prices;
    for epoch in schedule {
        if epoch.effective_from <= now_unix {
            current = epoch.prices;
        }
    }
    current
}

pub fn kimi_catalog_at(now_unix: i64) -> Vec<KimiModelSpec> {
    CATALOG
        .iter()
        .map(|entry| KimiModelSpec {
            id: entry.id,
            display_name: entry.display_name,
            input_token_limit: entry.input_token_limit,
            prices: prices_at(entry.schedule, now_unix),
        })
        .collect()
}

/// Prices for an **official** model id. Unknown ids return `None` so the caller fails closed.
pub fn kimi_prices_at(model_id: &str, now_unix: i64) -> Option<KimiPrices> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| prices_at(entry.schedule, now_unix))
}

/// Every subscription alias, for admission and catalogue construction.
pub fn kimi_subscription_models() -> &'static [KimiSubscriptionModel] {
    SUBSCRIPTION_MODELS
}

/// Resolve a subscription-facing model id to its official tariff key.
///
/// Matching is case-insensitive because the provider's own tooling accepts mixed spellings, but
/// it is otherwise exact: an unrecognised id returns `None` rather than falling back to a
/// default, so an unknown or future model cannot be charged at a neighbour's rate.
pub fn kimi_resolve_subscription_model(alias: &str) -> Option<KimiSubscriptionModel> {
    SUBSCRIPTION_MODELS
        .iter()
        .find(|entry| entry.alias.eq_ignore_ascii_case(alias))
        .copied()
}

/// Prices for a model id that may be either an official id or a subscription alias.
///
/// This is the lookup billing should use on the **served** model reported by the provider,
/// because a served model can be `kimi-k2.6` — a model no client ever asked for.
pub fn kimi_prices_for_served_model(model_id: &str, now_unix: i64) -> Option<KimiPrices> {
    kimi_matched_tariff_at(model_id, now_unix).map(|(_, prices)| prices)
}

/// The hot-override tariff family and prices of the official rate card that prices `model_id`.
///
/// Same served-model resolution as `kimi_prices_for_served_model`, additionally reporting WHICH
/// family the resolution used: `moonshot/kimi/<official_model_id>` of the entry that priced the
/// id, so an alias and its official model share one override family.
pub fn kimi_matched_tariff_at(model_id: &str, now_unix: i64) -> Option<(&'static str, KimiPrices)> {
    if let Some(entry) = CATALOG.iter().find(|entry| entry.id == model_id) {
        return Some((entry.tariff_family, prices_at(entry.schedule, now_unix)));
    }
    let resolved = kimi_resolve_subscription_model(model_id)?;
    let entry = CATALOG.iter().find(|entry| entry.id == resolved.official_model)?;
    Some((entry.tariff_family, prices_at(entry.schedule, now_unix)))
}

// ── usage parsing ────────────────────────────────────────────────────────────

/// Parse one `usage` object from the Anthropic-compatible endpoint.
///
/// Kimi documents no cache TTL split, so `cache_creation_input_tokens` is taken whole. If a
/// TTL-split object ever appears, its parts are summed rather than dropped.
pub fn usage_from_value(u: &Value) -> KimiUsage {
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
    KimiUsage {
        input_tokens: g("input_tokens"),
        cache_read_tokens: g("cache_read_input_tokens"),
        cache_write_tokens: cache_write,
        output_tokens: g("output_tokens"),
        reasoning_output_tokens: g("reasoning_output_tokens"),
    }
}

/// Parse a non-streaming response body. Returns `None` when the response carries no `usage`,
/// so the caller can distinguish "no authoritative usage" from "a genuine zero".
pub fn usage_from_response_value(value: &Value) -> Option<KimiUsage> {
    value.get("usage").map(usage_from_value)
}

pub fn usage_from_response_json(bytes: &[u8]) -> Option<KimiUsage> {
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
pub fn merge_stream_event(usage: &mut KimiUsage, value: &Value) {
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
pub fn usage_from_sse(bytes: &[u8]) -> Option<KimiUsage> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut usage = KimiUsage::default();
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
pub fn cost_nanodollars(usage: &KimiUsage, prices: &KimiPrices) -> Result<i128, KimiUsageError> {
    usage.validate()?;
    let leg = |tokens: u64, rate: i128| -> Result<i128, KimiUsageError> {
        i128::from(tokens)
            .checked_mul(rate)
            .ok_or(KimiUsageError::Overflow)
    };
    let mut total = leg(usage.input_tokens, prices.input)?;
    for part in [
        leg(usage.cache_read_tokens, prices.cached_input)?,
        leg(usage.cache_write_tokens, prices.cache_write)?,
        leg(usage.output_tokens, prices.output)?,
    ] {
        total = total.checked_add(part).ok_or(KimiUsageError::Overflow)?;
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn catalogue_matches_reviewed_official_rates() {
        let k3 = kimi_prices_at("kimi-k3", NOW).expect("k3 priced");
        assert_eq!(k3.cached_input, 300);
        assert_eq!(k3.input, 3_000);
        assert_eq!(k3.output, 15_000);

        let code = kimi_prices_at("kimi-k2.7-code", NOW).expect("k2.7 code priced");
        assert_eq!(code.cached_input, 190);
        assert_eq!(code.input, 950);
        assert_eq!(code.output, 4_000);

        let fast = kimi_prices_at("kimi-k2.7-code-highspeed", NOW).expect("k2.7 highspeed priced");
        assert_eq!(fast.cached_input, 380);
        assert_eq!(fast.input, 1_900);
        assert_eq!(fast.output, 8_000);

        let k26 = kimi_prices_at("kimi-k2.6", NOW).expect("k2.6 priced");
        assert_eq!(k26.cached_input, 160);
        assert_eq!(k26.input, 950);
        assert_eq!(k26.output, 4_000);
    }

    #[test]
    fn highspeed_is_exactly_double_the_base_sku() {
        let base = kimi_prices_at("kimi-k2.7-code", NOW).unwrap();
        let fast = kimi_prices_at("kimi-k2.7-code-highspeed", NOW).unwrap();
        assert_eq!(fast.cached_input, base.cached_input * 2);
        assert_eq!(fast.input, base.input * 2);
        assert_eq!(fast.output, base.output * 2);
    }

    #[test]
    fn cache_write_carries_the_miss_rate_because_none_is_published() {
        for spec in kimi_catalog_at(NOW) {
            assert_eq!(
                spec.prices.cache_write, spec.prices.input,
                "{} must price a cache write as a miss",
                spec.id
            );
        }
    }

    #[test]
    fn unknown_and_retired_models_fail_closed() {
        assert!(kimi_prices_at("kimi-k2-thinking", NOW).is_none());
        assert!(kimi_prices_at("kimi-k2.5", NOW).is_none());
        assert!(kimi_prices_at("moonshot-v1-128k", NOW).is_none());
        assert!(kimi_prices_at("kimi-k4", NOW).is_none());
        assert!(kimi_prices_for_served_model("kimi-k4", NOW).is_none());
    }

    #[test]
    fn subscription_aliases_resolve_to_official_tariff_keys() {
        let coding = kimi_resolve_subscription_model("kimi-for-coding").unwrap();
        assert_eq!(coding.official_model, "kimi-k2.7-code");
        assert_eq!(coding.input_token_limit, CONTEXT_256K);

        let fast = kimi_resolve_subscription_model("kimi-for-coding-highspeed").unwrap();
        assert_eq!(fast.official_model, "kimi-k2.7-code-highspeed");

        let k3 = kimi_resolve_subscription_model("k3").unwrap();
        assert_eq!(k3.official_model, "kimi-k3");
        assert_eq!(k3.input_token_limit, CONTEXT_1M);
    }

    #[test]
    fn bracket_form_is_the_same_tariff_as_k3_but_only_the_1m_window() {
        let bracket = kimi_resolve_subscription_model("k3[1m]").unwrap();
        let plain = kimi_resolve_subscription_model("k3").unwrap();
        let short = kimi_resolve_subscription_model("k3-256k").unwrap();
        assert_eq!(bracket.official_model, plain.official_model);
        assert_eq!(short.official_model, plain.official_model);
        assert_eq!(bracket.input_token_limit, CONTEXT_1M);
        assert_eq!(short.input_token_limit, CONTEXT_256K);
        assert_eq!(
            kimi_prices_for_served_model("k3-256k", NOW),
            kimi_prices_for_served_model("k3[1m]", NOW),
            "context mode must not change the rate card"
        );
    }

    #[test]
    fn thinking_disabled_reroute_is_billed_at_the_served_model() {
        // A client asks for k3 but the provider serves K2.6 because thinking was disabled.
        let requested = kimi_prices_for_served_model("k3", NOW).unwrap();
        let served = kimi_prices_for_served_model("kimi-k2.6", NOW).unwrap();
        assert_ne!(requested, served);
        let usage = KimiUsage {
            input_tokens: 1_000,
            output_tokens: 1_000,
            ..KimiUsage::default()
        };
        // K3 rate: 1000*3000 + 1000*15000 = 18_000_000 nanoUSD.
        assert_eq!(cost_nanodollars(&usage, &requested).unwrap(), 18_000_000);
        // K2.6 rate: 1000*950 + 1000*4000 = 4_950_000 nanoUSD. Billing the requested model
        // would overcharge the customer by 3.6x on this turn.
        assert_eq!(cost_nanodollars(&usage, &served).unwrap(), 4_950_000);
    }

    #[test]
    fn disjoint_legs_are_each_counted_once() {
        let prices = kimi_prices_at("kimi-k3", NOW).unwrap();
        let usage = KimiUsage {
            input_tokens: 10,
            cache_read_tokens: 20,
            cache_write_tokens: 30,
            output_tokens: 40,
            reasoning_output_tokens: 25,
        };
        // 10*3000 + 20*300 + 30*3000 + 40*15000 = 30_000 + 6_000 + 90_000 + 600_000
        assert_eq!(cost_nanodollars(&usage, &prices).unwrap(), 726_000);
    }

    #[test]
    fn reasoning_is_a_subset_of_output_and_adds_nothing() {
        let prices = kimi_prices_at("kimi-k3", NOW).unwrap();
        let without = KimiUsage {
            output_tokens: 100,
            ..KimiUsage::default()
        };
        let with = KimiUsage {
            output_tokens: 100,
            reasoning_output_tokens: 100,
            ..KimiUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&without, &prices).unwrap(),
            cost_nanodollars(&with, &prices).unwrap()
        );
    }

    #[test]
    fn broken_subset_invariant_fails_closed() {
        let prices = kimi_prices_at("kimi-k3", NOW).unwrap();
        let usage = KimiUsage {
            output_tokens: 10,
            reasoning_output_tokens: 11,
            ..KimiUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&usage, &prices),
            Err(KimiUsageError::ReasoningExceedsOutput)
        );
    }

    #[test]
    fn overflow_fails_closed_instead_of_saturating() {
        let prices = KimiPrices {
            cached_input: i128::MAX,
            input: i128::MAX,
            cache_write: i128::MAX,
            output: i128::MAX,
        };
        let usage = KimiUsage {
            input_tokens: u64::MAX,
            ..KimiUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&usage, &prices),
            Err(KimiUsageError::Overflow)
        );
    }

    #[test]
    fn zero_usage_costs_nothing() {
        let prices = kimi_prices_at("kimi-k3", NOW).unwrap();
        let usage = KimiUsage::default();
        assert!(usage.is_zero());
        assert_eq!(cost_nanodollars(&usage, &prices).unwrap(), 0);
    }

    #[test]
    fn missing_usage_is_absent_not_zero() {
        assert!(usage_from_response_json(br#"{"content":[]}"#).is_none());
        assert!(usage_from_sse(b"data: {\"type\":\"ping\"}\n").is_none());
        let zeroed = usage_from_response_json(br#"{"usage":{"input_tokens":0}}"#);
        assert_eq!(zeroed, Some(KimiUsage::default()));
    }

    #[test]
    fn non_stream_usage_parses_every_leg() {
        let body = br#"{"usage":{"input_tokens":41,"output_tokens":10,
            "cache_read_input_tokens":7,"cache_creation_input_tokens":30,
            "reasoning_output_tokens":4}}"#;
        let usage = usage_from_response_json(body).unwrap();
        assert_eq!(usage.input_tokens, 41);
        assert_eq!(usage.cache_read_tokens, 7);
        assert_eq!(usage.cache_write_tokens, 30);
        assert_eq!(usage.output_tokens, 10);
        assert_eq!(usage.reasoning_output_tokens, 4);
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
    fn schedule_is_effective_dated_and_stable_before_any_epoch() {
        // A turn priced at unix 0 must still resolve, so a clock skew cannot leave a turn
        // unpriceable.
        assert_eq!(kimi_prices_at("kimi-k3", 0), kimi_prices_at("kimi-k3", NOW));
    }

    #[test]
    fn tariff_schedule_id_is_pinned() {
        assert_eq!(
            KIMI_TARIFF_SCHEDULE_ID,
            "moonshot/kimi-open-platform/2026-08-03"
        );
    }

    #[test]
    fn matched_tariff_reports_the_official_family_and_identical_prices() {
        for (model, family) in [
            ("kimi-k3", "moonshot/kimi/kimi-k3"),
            ("kimi-k2.7-code", "moonshot/kimi/kimi-k2.7-code"),
            (
                "kimi-k2.7-code-highspeed",
                "moonshot/kimi/kimi-k2.7-code-highspeed",
            ),
            ("kimi-k2.6", "moonshot/kimi/kimi-k2.6"),
        ] {
            let (matched_family, prices) = kimi_matched_tariff_at(model, NOW).expect("priced");
            assert_eq!(matched_family, family, "{model} family");
            assert_eq!(
                Some(prices),
                kimi_prices_for_served_model(model, NOW),
                "{model} helper prices must equal kimi_prices_for_served_model"
            );
        }
        // Aliases resolve to their official model's family, so one override covers both.
        for alias in ["k3", "k3[1m]", "k3-256k", "kimi-for-coding"] {
            let (family, prices) = kimi_matched_tariff_at(alias, NOW).expect("alias priced");
            assert_eq!(Some(prices), kimi_prices_for_served_model(alias, NOW), "{alias}");
            assert!(
                family.starts_with("moonshot/kimi/"),
                "{alias} resolved family {family}"
            );
        }
        assert_eq!(
            kimi_matched_tariff_at("k3", NOW).map(|(family, _)| family),
            Some("moonshot/kimi/kimi-k3")
        );
        assert_eq!(kimi_matched_tariff_at("kimi-k4", NOW), None);
    }
}
