//! Native Gemini-compatible price catalog and authoritative usage parsing.
//!
//! Internal customer valuation uses reviewed Developer API rates from
//! <https://ai.google.dev/gemini-api/docs/pricing>; it is not a claim about subscription cost.
//! They are pinned and effective-dated for the same reason as the Codex catalog: a remote alias or
//! pricing-page edit must never silently change customer money. Values are nanodollars per token
//! (`$/M tokens * 1000`) and all arithmetic remains integer-only.

use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GeminiSearchBilling {
    /// Gemini 3 bills each query emitted by Google Search grounding.
    PerQuery { nano: i128 },
    /// Gemini 2.5 bills a grounded prompt once, regardless of the internal query count.
    PerGroundedPrompt { nano: i128 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeminiPrices {
    pub input: i128,
    pub audio_input: i128,
    pub cached_input: i128,
    pub cached_audio_input: i128,
    /// Candidate and thinking tokens share the published output rate.
    pub output: i128,
    /// Generated image tokens have their own media-output rate. Text-only models keep this zero.
    pub image_output: i128,
    /// Models with tiered long-context pricing apply the higher rates to the whole request.
    pub long_context_threshold: u64,
    pub long_input: i128,
    pub long_audio_input: i128,
    pub long_cached_input: i128,
    pub long_cached_audio_input: i128,
    pub long_output: i128,
    pub search: GeminiSearchBilling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeminiPriceEpoch {
    pub effective_from: i64,
    pub prices: GeminiPrices,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GeminiModelSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    pub input_token_limit: u64,
    pub output_token_limit: u64,
    pub prices: GeminiPrices,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeminiUsage {
    /// Uncached, non-audio input. `toolUsePromptTokenCount` is included here.
    pub input_tokens: u64,
    pub audio_input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cached_audio_input_tokens: u64,
    /// Text candidate + thinking tokens. Image candidate tokens are split out below.
    pub output_tokens: u64,
    pub image_output_tokens: u64,
    /// Both raw facts are retained because Gemini 3 and 2.5 bill Search differently.
    pub search_queries: u64,
    pub grounded_search_prompts: u64,
}

impl GeminiUsage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_add(self.audio_input_tokens)
            .saturating_add(self.cached_input_tokens)
            .saturating_add(self.cached_audio_input_tokens)
            .saturating_add(self.output_tokens)
            .saturating_add(self.image_output_tokens)
    }

    pub fn is_zero(&self) -> bool {
        self.total_tokens() == 0 && self.search_queries == 0 && self.grounded_search_prompts == 0
    }
}

struct CatalogEntry {
    id: &'static str,
    display_name: &'static str,
    input_token_limit: u64,
    output_token_limit: u64,
    schedule: &'static [GeminiPriceEpoch],
}

const SEARCH_GEMINI_3: GeminiSearchBilling = GeminiSearchBilling::PerQuery { nano: 14_000_000 };
const SEARCH_GEMINI_25: GeminiSearchBilling =
    GeminiSearchBilling::PerGroundedPrompt { nano: 35_000_000 };

const fn flat_prices(
    input: i128,
    audio_input: i128,
    cached_input: i128,
    cached_audio_input: i128,
    output: i128,
    search: GeminiSearchBilling,
) -> GeminiPrices {
    GeminiPrices {
        input,
        audio_input,
        cached_input,
        cached_audio_input,
        output,
        image_output: 0,
        long_context_threshold: u64::MAX,
        long_input: input,
        long_audio_input: audio_input,
        long_cached_input: cached_input,
        long_cached_audio_input: cached_audio_input,
        long_output: output,
        search,
    }
}

// Official standard paid-tier rates, rechecked 2026-07-31:
// https://ai.google.dev/gemini-api/docs/pricing#gemini-3.6-flash
const GEMINI_36_FLASH: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: flat_prices(1_500, 1_500, 150, 150, 7_500, SEARCH_GEMINI_3),
}];
// https://ai.google.dev/gemini-api/docs/pricing#gemini-3.5-flash
const GEMINI_35_FLASH: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: flat_prices(1_500, 1_500, 150, 150, 9_000, SEARCH_GEMINI_3),
}];
// https://ai.google.dev/gemini-api/docs/pricing#gemini-3.1-flash-lite
const GEMINI_31_FLASH_LITE: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: flat_prices(250, 500, 25, 50, 1_500, SEARCH_GEMINI_3),
}];
// Official standard paid-tier rates for `gemini-3.1-pro-preview`:
// https://ai.google.dev/gemini-api/docs/pricing#gemini-3.1-pro-preview
const GEMINI_31_PRO_PREVIEW: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: GeminiPrices {
        input: 2_000,
        audio_input: 2_000,
        cached_input: 200,
        cached_audio_input: 200,
        output: 12_000,
        image_output: 0,
        long_context_threshold: 200_000,
        long_input: 4_000,
        long_audio_input: 4_000,
        long_cached_input: 400,
        long_cached_audio_input: 400,
        long_output: 18_000,
        search: SEARCH_GEMINI_3,
    },
}];
const GEMINI_25_PRO: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: GeminiPrices {
        input: 1_250,
        audio_input: 1_250,
        cached_input: 125,
        cached_audio_input: 125,
        output: 10_000,
        image_output: 0,
        long_context_threshold: 200_000,
        long_input: 2_500,
        long_audio_input: 2_500,
        long_cached_input: 250,
        long_cached_audio_input: 250,
        long_output: 15_000,
        search: SEARCH_GEMINI_25,
    },
}];
const GEMINI_25_FLASH: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: flat_prices(300, 1_000, 30, 100, 2_500, SEARCH_GEMINI_25),
}];
const GEMINI_25_FLASH_LITE: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: flat_prices(100, 300, 10, 30, 400, SEARCH_GEMINI_25),
}];

// Official standard paid-tier rates, rechecked 2026-07-31:
// https://ai.google.dev/gemini-api/docs/pricing#gemini-3.1-flash-image
const GEMINI_31_FLASH_IMAGE: &[GeminiPriceEpoch] = &[GeminiPriceEpoch {
    effective_from: 0,
    prices: GeminiPrices {
        input: 500,
        audio_input: 500,
        cached_input: 500,
        cached_audio_input: 500,
        output: 3_000,
        image_output: 60_000,
        long_context_threshold: u64::MAX,
        long_input: 500,
        long_audio_input: 500,
        long_cached_input: 500,
        long_cached_audio_input: 500,
        long_output: 3_000,
        search: SEARCH_GEMINI_3,
    },
}];

const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "gemini-3.1-flash-image",
        display_name: "Gemini 3.1 Flash Image (Nano Banana 2)",
        input_token_limit: 131_072,
        output_token_limit: 32_768,
        schedule: GEMINI_31_FLASH_IMAGE,
    },
    CatalogEntry {
        id: "gemini-3.6-flash",
        display_name: "Gemini 3.6 Flash",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_36_FLASH,
    },
    CatalogEntry {
        id: "gemini-3.5-flash",
        display_name: "Gemini 3.5 Flash",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_35_FLASH,
    },
    CatalogEntry {
        id: "gemini-3.1-flash-lite",
        display_name: "Gemini 3.1 Flash-Lite",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_31_FLASH_LITE,
    },
    CatalogEntry {
        id: "gemini-3.1-pro-preview",
        display_name: "Gemini 3.1 Pro Preview",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_31_PRO_PREVIEW,
    },
    CatalogEntry {
        id: "gemini-2.5-pro",
        display_name: "Gemini 2.5 Pro",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_25_PRO,
    },
    CatalogEntry {
        id: "gemini-2.5-flash",
        display_name: "Gemini 2.5 Flash",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_25_FLASH,
    },
    CatalogEntry {
        id: "gemini-2.5-flash-lite",
        display_name: "Gemini 2.5 Flash-Lite",
        input_token_limit: 1_048_576,
        output_token_limit: 65_536,
        schedule: GEMINI_25_FLASH_LITE,
    },
];

fn prices_at(schedule: &[GeminiPriceEpoch], now_unix: i64) -> GeminiPrices {
    let mut current = schedule
        .first()
        .expect("every Gemini model has at least one price epoch")
        .prices;
    for epoch in schedule {
        if epoch.effective_from <= now_unix {
            current = epoch.prices;
        }
    }
    current
}

pub fn gemini_catalog_at(now_unix: i64) -> Vec<GeminiModelSpec> {
    CATALOG
        .iter()
        .map(|entry| GeminiModelSpec {
            id: entry.id,
            display_name: entry.display_name,
            input_token_limit: entry.input_token_limit,
            output_token_limit: entry.output_token_limit,
            prices: prices_at(entry.schedule, now_unix),
        })
        .collect()
}

pub fn gemini_prices_at(model_id: &str, now_unix: i64) -> Option<GeminiPrices> {
    CATALOG
        .iter()
        .find(|entry| entry.id == model_id)
        .map(|entry| prices_at(entry.schedule, now_unix))
}

fn count_modality(details: &Value, modality: &str) -> u64 {
    details
        .as_array()
        .into_iter()
        .flatten()
        .filter(|detail| {
            detail
                .get("modality")
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(modality))
        })
        .filter_map(|detail| detail.get("tokenCount").and_then(Value::as_u64))
        .fold(0, u64::saturating_add)
}

/// Parse one GenerateContentResponse. Google reports cumulative usage in the final streaming
/// response, so callers should replace their snapshot whenever this returns `Some`.
pub fn usage_from_response_value(value: &Value) -> Option<GeminiUsage> {
    let metadata = value.get("usageMetadata")?;
    let mut usage = usage_from_metadata(metadata);
    merge_grounding_usage(&mut usage, value);
    Some(usage)
}

fn usage_from_metadata(metadata: &Value) -> GeminiUsage {
    let prompt = metadata
        .get("promptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let cached = metadata
        .get("cachedContentTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .min(prompt);
    let tool_prompt = metadata
        .get("toolUsePromptTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let audio_prompt = count_modality(&metadata["promptTokensDetails"], "AUDIO").min(prompt);
    let cached_audio = count_modality(&metadata["cacheTokensDetails"], "AUDIO")
        .min(cached)
        .min(audio_prompt);
    let audio_input = audio_prompt.saturating_sub(cached_audio);
    let cached_input = cached.saturating_sub(cached_audio);
    let input = prompt
        .saturating_sub(cached)
        .saturating_sub(audio_input)
        .saturating_add(tool_prompt);
    let candidates = metadata
        .get("candidatesTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let image_candidates =
        count_modality(&metadata["candidatesTokensDetails"], "IMAGE").min(candidates);
    let thoughts = metadata
        .get("thoughtsTokenCount")
        .and_then(Value::as_u64)
        .unwrap_or(0);

    GeminiUsage {
        input_tokens: input,
        audio_input_tokens: audio_input,
        cached_input_tokens: cached_input,
        cached_audio_input_tokens: cached_audio,
        output_tokens: candidates
            .saturating_sub(image_candidates)
            .saturating_add(thoughts),
        image_output_tokens: image_candidates,
        ..GeminiUsage::default()
    }
}

fn grounding_usage(value: &Value) -> (u64, bool) {
    let mut search_queries = 0u64;
    let mut grounded = false;
    for candidate in value
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(grounding) = candidate.get("groundingMetadata") else {
            continue;
        };
        let queries = grounding
            .get("webSearchQueries")
            .and_then(Value::as_array)
            .map_or(0, |items| items.len() as u64);
        search_queries = search_queries.saturating_add(queries);
        grounded |= queries > 0
            || grounding
                .get("groundingChunks")
                .and_then(Value::as_array)
                .is_some_and(|chunks| !chunks.is_empty());
    }

    (search_queries, grounded)
}

fn merge_grounding_usage(usage: &mut GeminiUsage, value: &Value) {
    let (queries, grounded) = grounding_usage(value);
    // Streaming responses can repeat cumulative candidate metadata. `max` retains a grounding frame
    // that precedes the final token-only usage frame without double-charging a replayed query list.
    usage.search_queries = usage.search_queries.max(queries);
    usage.grounded_search_prompts = usage.grounded_search_prompts.max(u64::from(grounded));
}

/// Merge one streaming GenerateContentResponse into the last authoritative usage snapshot. Token
/// counts replace older cumulative counts; grounding facts survive when Google emits them in a
/// different SSE frame from the final `usageMetadata`.
pub fn merge_stream_response_value(usage: &mut GeminiUsage, value: &Value) {
    let search_queries = usage.search_queries;
    let grounded_search_prompts = usage.grounded_search_prompts;
    if let Some(metadata) = value.get("usageMetadata") {
        *usage = usage_from_metadata(metadata);
        usage.search_queries = search_queries;
        usage.grounded_search_prompts = grounded_search_prompts;
    }
    merge_grounding_usage(usage, value);
}

pub fn usage_from_response_json(bytes: &[u8]) -> GeminiUsage {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|value| usage_from_response_value(&value))
        .unwrap_or_default()
}

/// Parse the last authoritative `usageMetadata` snapshot from a Gemini `alt=sse` stream. Partial
/// or malformed frames are ignored and never panic.
pub fn usage_from_sse(bytes: &[u8]) -> GeminiUsage {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return GeminiUsage::default();
    };
    let mut usage = GeminiUsage::default();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(data.trim()) else {
            continue;
        };
        merge_stream_response_value(&mut usage, &value);
    }
    usage
}

pub fn cost_nanodollars(usage: &GeminiUsage, prices: &GeminiPrices) -> i128 {
    let prompt_tokens = usage
        .input_tokens
        .saturating_add(usage.audio_input_tokens)
        .saturating_add(usage.cached_input_tokens)
        .saturating_add(usage.cached_audio_input_tokens);
    let long = prompt_tokens > prices.long_context_threshold;
    let (input, audio, cached, cached_audio, output) = if long {
        (
            prices.long_input,
            prices.long_audio_input,
            prices.long_cached_input,
            prices.long_cached_audio_input,
            prices.long_output,
        )
    } else {
        (
            prices.input,
            prices.audio_input,
            prices.cached_input,
            prices.cached_audio_input,
            prices.output,
        )
    };
    let search = match prices.search {
        GeminiSearchBilling::PerQuery { nano } => {
            (usage.search_queries as i128).saturating_mul(nano)
        }
        GeminiSearchBilling::PerGroundedPrompt { nano } => {
            (usage.grounded_search_prompts as i128).saturating_mul(nano)
        }
    };
    (usage.input_tokens as i128)
        .saturating_mul(input)
        .saturating_add((usage.audio_input_tokens as i128).saturating_mul(audio))
        .saturating_add((usage.cached_input_tokens as i128).saturating_mul(cached))
        .saturating_add((usage.cached_audio_input_tokens as i128).saturating_mul(cached_audio))
        .saturating_add((usage.output_tokens as i128).saturating_mul(output))
        .saturating_add((usage.image_output_tokens as i128).saturating_mul(prices.image_output))
        .saturating_add(search)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_paid_catalog_matches_the_official_rates() {
        let cases = [
            (
                "gemini-3.1-flash-image",
                GeminiPrices {
                    input: 500,
                    audio_input: 500,
                    cached_input: 500,
                    cached_audio_input: 500,
                    output: 3_000,
                    image_output: 60_000,
                    long_context_threshold: u64::MAX,
                    long_input: 500,
                    long_audio_input: 500,
                    long_cached_input: 500,
                    long_cached_audio_input: 500,
                    long_output: 3_000,
                    search: GeminiSearchBilling::PerQuery { nano: 14_000_000 },
                },
            ),
            (
                "gemini-3.6-flash",
                GeminiPrices {
                    input: 1_500,
                    audio_input: 1_500,
                    cached_input: 150,
                    cached_audio_input: 150,
                    output: 7_500,
                    image_output: 0,
                    long_context_threshold: u64::MAX,
                    long_input: 1_500,
                    long_audio_input: 1_500,
                    long_cached_input: 150,
                    long_cached_audio_input: 150,
                    long_output: 7_500,
                    search: GeminiSearchBilling::PerQuery { nano: 14_000_000 },
                },
            ),
            (
                "gemini-3.5-flash",
                GeminiPrices {
                    input: 1_500,
                    audio_input: 1_500,
                    cached_input: 150,
                    cached_audio_input: 150,
                    output: 9_000,
                    image_output: 0,
                    long_context_threshold: u64::MAX,
                    long_input: 1_500,
                    long_audio_input: 1_500,
                    long_cached_input: 150,
                    long_cached_audio_input: 150,
                    long_output: 9_000,
                    search: GeminiSearchBilling::PerQuery { nano: 14_000_000 },
                },
            ),
            (
                "gemini-3.1-flash-lite",
                GeminiPrices {
                    input: 250,
                    audio_input: 500,
                    cached_input: 25,
                    cached_audio_input: 50,
                    output: 1_500,
                    image_output: 0,
                    long_context_threshold: u64::MAX,
                    long_input: 250,
                    long_audio_input: 500,
                    long_cached_input: 25,
                    long_cached_audio_input: 50,
                    long_output: 1_500,
                    search: GeminiSearchBilling::PerQuery { nano: 14_000_000 },
                },
            ),
            (
                "gemini-3.1-pro-preview",
                GeminiPrices {
                    input: 2_000,
                    audio_input: 2_000,
                    cached_input: 200,
                    cached_audio_input: 200,
                    output: 12_000,
                    image_output: 0,
                    long_context_threshold: 200_000,
                    long_input: 4_000,
                    long_audio_input: 4_000,
                    long_cached_input: 400,
                    long_cached_audio_input: 400,
                    long_output: 18_000,
                    search: GeminiSearchBilling::PerQuery { nano: 14_000_000 },
                },
            ),
            (
                "gemini-2.5-pro",
                GeminiPrices {
                    input: 1_250,
                    audio_input: 1_250,
                    cached_input: 125,
                    cached_audio_input: 125,
                    output: 10_000,
                    image_output: 0,
                    long_context_threshold: 200_000,
                    long_input: 2_500,
                    long_audio_input: 2_500,
                    long_cached_input: 250,
                    long_cached_audio_input: 250,
                    long_output: 15_000,
                    search: GeminiSearchBilling::PerGroundedPrompt { nano: 35_000_000 },
                },
            ),
            (
                "gemini-2.5-flash",
                GeminiPrices {
                    input: 300,
                    audio_input: 1_000,
                    cached_input: 30,
                    cached_audio_input: 100,
                    output: 2_500,
                    image_output: 0,
                    long_context_threshold: u64::MAX,
                    long_input: 300,
                    long_audio_input: 1_000,
                    long_cached_input: 30,
                    long_cached_audio_input: 100,
                    long_output: 2_500,
                    search: GeminiSearchBilling::PerGroundedPrompt { nano: 35_000_000 },
                },
            ),
            (
                "gemini-2.5-flash-lite",
                GeminiPrices {
                    input: 100,
                    audio_input: 300,
                    cached_input: 10,
                    cached_audio_input: 30,
                    output: 400,
                    image_output: 0,
                    long_context_threshold: u64::MAX,
                    long_input: 100,
                    long_audio_input: 300,
                    long_cached_input: 10,
                    long_cached_audio_input: 30,
                    long_output: 400,
                    search: GeminiSearchBilling::PerGroundedPrompt { nano: 35_000_000 },
                },
            ),
        ];
        let catalog = gemini_catalog_at(0);
        assert_eq!(catalog.len(), cases.len());
        for (model, expected) in cases {
            assert_eq!(gemini_prices_at(model, 0), Some(expected), "{model}");
            let spec = catalog
                .iter()
                .find(|spec| spec.id == model)
                .expect("official model must be catalogued");
            let (input_limit, output_limit) = if model == "gemini-3.1-flash-image" {
                (131_072, 32_768)
            } else {
                (1_048_576, 65_536)
            };
            assert_eq!(spec.input_token_limit, input_limit, "{model} input limit");
            assert_eq!(
                spec.output_token_limit, output_limit,
                "{model} output limit"
            );
        }
    }

    #[test]
    fn one_million_tokens_uses_the_pinned_rate_exactly() {
        for model in gemini_catalog_at(0) {
            let one_million = GeminiUsage {
                input_tokens: 1_000_000,
                ..GeminiUsage::default()
            };
            assert_eq!(
                cost_nanodollars(&one_million, &model.prices),
                if 1_000_000 > model.prices.long_context_threshold {
                    model.prices.long_input * 1_000_000
                } else {
                    model.prices.input * 1_000_000
                },
                "{} input",
                model.id
            );
            let one_million = GeminiUsage {
                output_tokens: 1_000_000,
                ..GeminiUsage::default()
            };
            assert_eq!(
                cost_nanodollars(&one_million, &model.prices),
                model.prices.output * 1_000_000,
                "{} output",
                model.id
            );
        }
    }

    #[test]
    fn usage_separates_cache_audio_tool_and_thinking_without_double_counting() {
        let response = serde_json::json!({
            "usageMetadata": {
                "promptTokenCount": 1_000,
                "cachedContentTokenCount": 400,
                "candidatesTokenCount": 30,
                "thoughtsTokenCount": 20,
                "toolUsePromptTokenCount": 10,
                "promptTokensDetails": [
                    {"modality": "TEXT", "tokenCount": 700},
                    {"modality": "AUDIO", "tokenCount": 300}
                ],
                "cacheTokensDetails": [
                    {"modality": "TEXT", "tokenCount": 250},
                    {"modality": "AUDIO", "tokenCount": 150}
                ]
            },
            "candidates": [{
                "groundingMetadata": {"webSearchQueries": ["one", "two"]}
            }]
        });
        assert_eq!(
            usage_from_response_value(&response),
            Some(GeminiUsage {
                input_tokens: 460,
                audio_input_tokens: 150,
                cached_input_tokens: 250,
                cached_audio_input_tokens: 150,
                output_tokens: 50,
                image_output_tokens: 0,
                search_queries: 2,
                grounded_search_prompts: 1,
            })
        );
    }

    #[test]
    fn image_candidate_tokens_are_split_and_priced_at_the_media_rate() {
        let response = serde_json::json!({
            "usageMetadata": {
                "promptTokenCount": 100,
                "candidatesTokenCount": 1_140,
                "thoughtsTokenCount": 20,
                "candidatesTokensDetails": [
                    {"modality": "TEXT", "tokenCount": 20},
                    {"modality": "IMAGE", "tokenCount": 1_120}
                ]
            }
        });
        let usage = usage_from_response_value(&response).unwrap();
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 40);
        assert_eq!(usage.image_output_tokens, 1_120);

        let prices = gemini_prices_at("gemini-3.1-flash-image", 0).unwrap();
        assert_eq!(
            cost_nanodollars(&usage, &prices),
            100 * 500 + 40 * 3_000 + 1_120 * 60_000
        );
    }

    #[test]
    fn sse_uses_the_last_cumulative_snapshot_and_ignores_damage() {
        let stream = b"data: {\"usageMetadata\":{\"promptTokenCount\":10}}\n\n\
: ping\n\n\
data: broken\n\n\
data: {\"candidates\":[{\"groundingMetadata\":{\"webSearchQueries\":[\"one\",\"two\"]}}]}\n\n\
data: {\"usageMetadata\":{\"promptTokenCount\":20,\"candidatesTokenCount\":7,\"thoughtsTokenCount\":3}}\n\n";
        assert_eq!(
            usage_from_sse(stream),
            GeminiUsage {
                input_tokens: 20,
                output_tokens: 10,
                search_queries: 2,
                grounded_search_prompts: 1,
                ..GeminiUsage::default()
            }
        );
    }

    #[test]
    fn long_context_and_search_modes_are_exact() {
        let pro = gemini_prices_at("gemini-2.5-pro", 0).unwrap();
        let usage = GeminiUsage {
            input_tokens: 200_001,
            output_tokens: 10,
            search_queries: 9,
            grounded_search_prompts: 1,
            ..GeminiUsage::default()
        };
        assert_eq!(
            cost_nanodollars(&usage, &pro),
            200_001 * 2_500 + 10 * 15_000 + 35_000_000
        );

        let flash = gemini_prices_at("gemini-3.6-flash", 0).unwrap();
        assert_eq!(
            cost_nanodollars(&usage, &flash),
            200_001 * 1_500 + 10 * 7_500 + 9 * 14_000_000
        );

        let preview = gemini_prices_at("gemini-3.1-pro-preview", 0).unwrap();
        assert_eq!(
            cost_nanodollars(&usage, &preview),
            200_001 * 4_000 + 10 * 18_000 + 9 * 14_000_000
        );
    }

    #[test]
    fn malformed_input_is_zero_and_hostile_counts_saturate() {
        assert!(usage_from_response_json(b"not json").is_zero());
        assert!(usage_from_sse(&[0xff]).is_zero());
        let prices = gemini_prices_at("gemini-3.6-flash", 0).unwrap();
        let usage = GeminiUsage {
            input_tokens: u64::MAX,
            audio_input_tokens: u64::MAX,
            cached_input_tokens: u64::MAX,
            cached_audio_input_tokens: u64::MAX,
            output_tokens: u64::MAX,
            image_output_tokens: u64::MAX,
            search_queries: u64::MAX,
            grounded_search_prompts: u64::MAX,
        };
        assert!(cost_nanodollars(&usage, &prices) > 0);
    }
}
