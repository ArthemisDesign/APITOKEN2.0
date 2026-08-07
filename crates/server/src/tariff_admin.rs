//! Hot tariff override payloads built FROM the compiled `metering` constants.
//!
//! The Control API surface in `admin.rs` (`/admin/pricing/tariffs*`) never accepts an
//! operator-typed number for seeding: this module converts the compiled `metering` price structs
//! into the `registry::pricing` mirror structs one-to-one and serializes them into the canonical
//! payload JSON the override table stores (i128 money legs as decimal strings). The registry
//! authority re-validates and re-digests every payload on write, so a converter bug cannot
//! produce a silently mispriced row — and the unit tests below prove the round trip for every
//! provider family.

use registry::pricing::{
    AnthropicTariffPrices, CodexTariffCreditRates, CodexTariffPrices, GeminiTariffPrices,
    GeminiTariffSearchBilling, GlmTariffCreditRates, GlmTariffPrices, KimiTariffPrices,
    OpenAiImageTariffPrices, TariffOverride,
};

/// One compiled tariff family with its canonical override payload JSON.
pub(crate) struct CompiledTariff {
    pub tariff_family: &'static str,
    pub payload: serde_json::Value,
}

fn anthropic_payload(prices: metering::Prices) -> AnthropicTariffPrices {
    AnthropicTariffPrices {
        input: prices.input,
        output: prices.output,
        cache_read: prices.cache_read,
        cache_write_5m: prices.cache_write_5m,
        cache_write_1h: prices.cache_write_1h,
    }
}

fn codex_payload(prices: metering::CodexPrices) -> CodexTariffPrices {
    CodexTariffPrices {
        input: prices.input,
        cached_input: prices.cached_input,
        cache_write_input: prices.cache_write_input,
        output: prices.output,
        api_fast_multiplier_basis_points: prices.api_fast_multiplier_basis_points,
        long_context_threshold: prices.long_context_threshold,
        long_input_basis_points: prices.long_input_basis_points,
        long_output_basis_points: prices.long_output_basis_points,
    }
}

fn codex_credit_payload(rates: metering::CodexCreditRates) -> CodexTariffCreditRates {
    CodexTariffCreditRates {
        input: rates.input,
        cached_input: rates.cached_input,
        output: rates.output,
    }
}

fn gemini_payload(prices: metering::GeminiPrices) -> GeminiTariffPrices {
    GeminiTariffPrices {
        input: prices.input,
        audio_input: prices.audio_input,
        cached_input: prices.cached_input,
        cached_audio_input: prices.cached_audio_input,
        output: prices.output,
        image_output: prices.image_output,
        long_context_threshold: prices.long_context_threshold,
        long_input: prices.long_input,
        long_audio_input: prices.long_audio_input,
        long_cached_input: prices.long_cached_input,
        long_cached_audio_input: prices.long_cached_audio_input,
        long_output: prices.long_output,
        search: match prices.search {
            metering::GeminiSearchBilling::PerQuery { nano } => {
                GeminiTariffSearchBilling::PerQuery { nano }
            }
            metering::GeminiSearchBilling::PerGroundedPrompt { nano } => {
                GeminiTariffSearchBilling::PerGroundedPrompt { nano }
            }
        },
    }
}

fn glm_payload(prices: metering::GlmPrices) -> GlmTariffPrices {
    GlmTariffPrices {
        cached_input: prices.cached_input,
        input: prices.input,
        cache_write: prices.cache_write,
        output: prices.output,
    }
}

fn glm_credit_payload(rates: metering::GlmCreditRates) -> GlmTariffCreditRates {
    GlmTariffCreditRates {
        input_tenths: rates.input_tenths,
        cached_input_tenths: rates.cached_input_tenths,
        output_tenths: rates.output_tenths,
    }
}

fn kimi_payload(prices: metering::KimiPrices) -> KimiTariffPrices {
    KimiTariffPrices {
        cached_input: prices.cached_input,
        input: prices.input,
        cache_write: prices.cache_write,
        output: prices.output,
    }
}

fn openai_image_payload(prices: metering::OpenAiImagePrices) -> OpenAiImageTariffPrices {
    OpenAiImageTariffPrices {
        fresh_text_input: prices.fresh_text_input,
        cached_text_input: prices.cached_text_input,
        fresh_image_input: prices.fresh_image_input,
        cached_image_input: prices.cached_image_input,
        image_output: prices.image_output,
    }
}

fn to_payload<T: serde::Serialize>(mirror: T) -> serde_json::Value {
    serde_json::to_value(mirror).expect("tariff mirror structs always serialize")
}

/// The full compiled tariff catalog as of `now_unix`: every family the hot override authority
/// can seed, sorted by family name. Built only from `metering` enumerators — never from
/// operator input.
pub(crate) fn compiled_tariff_catalog_at(now_unix: i64) -> Vec<CompiledTariff> {
    let mut catalog = Vec::new();
    for (family, prices) in metering::anthropic_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(anthropic_payload(prices)),
        });
    }
    for (family, prices) in metering::codex_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(codex_payload(prices)),
        });
    }
    for (family, rates) in metering::codex_compiled_credit_rates() {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(codex_credit_payload(rates)),
        });
    }
    for (family, prices) in metering::gemini_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(gemini_payload(prices)),
        });
    }
    for (family, prices) in metering::glm_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(glm_payload(prices)),
        });
    }
    for (family, rates) in metering::glm_compiled_credit_rates() {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(glm_credit_payload(rates)),
        });
    }
    for (family, prices) in metering::kimi_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(kimi_payload(prices)),
        });
    }
    let (family, prices) = metering::openai_image_compiled_tariff();
    catalog.push(CompiledTariff {
        tariff_family: family,
        payload: to_payload(openai_image_payload(prices)),
    });
    catalog.sort_by(|left, right| left.tariff_family.cmp(right.tariff_family));
    catalog
}

/// The newest durable version of one family in the override table, or 1 (the implicit compiled
/// constants) when the family has no rows yet.
pub(crate) fn family_head_version(rows: &[TariffOverride], family: &str) -> i64 {
    rows.iter()
        .filter(|row| row.tariff_family == family)
        .map(|row| row.version)
        .max()
        .unwrap_or(1)
}

/// An override leg may differ from its compiled baseline by at most this factor in either
/// direction (and may not cross zero) without an explicit `force`. A hot override takes effect
/// fleet-wide within seconds, so a fat-fingered number must be stopped at the API, not in a
/// postmortem. Legitimate larger repricings pass with `force: true`.
pub(crate) const SANITY_FACTOR: i128 = 4;

fn leaf_number(value: &serde_json::Value) -> Option<i128> {
    match value {
        serde_json::Value::String(raw) => raw.parse::<i128>().ok(),
        serde_json::Value::Number(number) => number
            .as_i64()
            .map(i128::from)
            .or_else(|| number.as_u64().map(|v| v as i128)),
        _ => None,
    }
}

fn collect_deviations(path: &str, baseline: &serde_json::Value, candidate: &serde_json::Value, out: &mut Vec<String>) {
    match (baseline, candidate) {
        (serde_json::Value::Object(base), serde_json::Value::Object(cand)) => {
            for (key, base_value) in base {
                if let Some(cand_value) = cand.get(key) {
                    collect_deviations(&format!("{path}.{key}"), base_value, cand_value, out);
                }
            }
        }
        _ => {
            let (Some(base), Some(cand)) = (leaf_number(baseline), leaf_number(candidate)) else {
                return;
            };
            let deviates = if base == 0 || cand == 0 {
                base != cand
            } else {
                let (lo, hi) = if base <= cand { (base, cand) } else { (cand, base) };
                hi > lo * SANITY_FACTOR
            };
            if deviates {
                out.push(format!("{path}: compiled {base} vs override {cand}"));
            }
        }
    }
}

/// Legs of `candidate` that deviate from the `baseline` payload by more than [`SANITY_FACTOR`]
/// (or cross zero), as human-readable `path: compiled X vs override Y` entries. Only leaves
/// present in BOTH payloads are compared — the payload schema is identical by construction, so
/// this is every leg. Non-numeric leaves (billing kind tags) are skipped.
pub(crate) fn payload_deviations(
    baseline: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Vec<String> {
    let mut out = Vec::new();
    collect_deviations("", baseline, candidate, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::pricing::{parse_tariff_override_payload, TariffOverridePayload};

    #[test]
    fn every_compiled_family_payload_round_trips_through_the_registry_schema() {
        for ts in [0, 1_788_220_800 - 1, 1_788_220_800, i64::MAX] {
            let catalog = compiled_tariff_catalog_at(ts);
            // Sorted by family and free of duplicates.
            for window in catalog.windows(2) {
                assert!(window[0].tariff_family < window[1].tariff_family);
            }
            for entry in &catalog {
                let parsed = parse_tariff_override_payload(entry.tariff_family, &entry.payload)
                    .unwrap_or_else(|error| {
                        panic!("{}: compiled payload rejected: {error:#}", entry.tariff_family)
                    });
                let canonical = parsed.to_canonical_value().unwrap();
                assert_eq!(
                    canonical, entry.payload,
                    "{}: canonical projection must equal the converter output",
                    entry.tariff_family
                );
            }
        }
    }

    #[test]
    fn converters_preserve_every_provider_value() {
        let now = 1_788_220_800; // after the Sonnet 5 epoch flip

        for (family, prices) in metering::anthropic_compiled_tariffs_at(now) {
            let parsed = parse_tariff_override_payload(family, &to_payload(anthropic_payload(prices)))
                .unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::Anthropic(anthropic_payload(prices)),
                "{family}"
            );
        }
        for (family, prices) in metering::codex_compiled_tariffs_at(now) {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(codex_payload(prices))).unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::Codex(codex_payload(prices)),
                "{family}"
            );
        }
        for (family, rates) in metering::codex_compiled_credit_rates() {
            let parsed = parse_tariff_override_payload(
                family,
                &to_payload(codex_credit_payload(rates)),
            )
            .unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::CodexCredits(codex_credit_payload(rates)),
                "{family}"
            );
        }
        for (family, prices) in metering::gemini_compiled_tariffs_at(now) {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(gemini_payload(prices))).unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::Gemini(gemini_payload(prices)),
                "{family}"
            );
        }
        for (family, prices) in metering::glm_compiled_tariffs_at(now) {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(glm_payload(prices))).unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::Glm(glm_payload(prices)),
                "{family}"
            );
        }
        for (family, rates) in metering::glm_compiled_credit_rates() {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(glm_credit_payload(rates)))
                    .unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::GlmCredits(glm_credit_payload(rates)),
                "{family}"
            );
        }
        for (family, prices) in metering::kimi_compiled_tariffs_at(now) {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(kimi_payload(prices))).unwrap();
            assert_eq!(
                parsed,
                TariffOverridePayload::Kimi(kimi_payload(prices)),
                "{family}"
            );
        }
        let (family, prices) = metering::openai_image_compiled_tariff();
        let parsed =
            parse_tariff_override_payload(family, &to_payload(openai_image_payload(prices)))
                .unwrap();
        assert_eq!(
            parsed,
            TariffOverridePayload::OpenAiImage(openai_image_payload(prices)),
            "{family}"
        );
    }

    #[test]
    fn family_head_version_defaults_to_the_implicit_compiled_version() {
        let row = |family: &str, version: i64| TariffOverride {
            tariff_family: family.to_owned(),
            version,
            effective_from: 0,
            payload: serde_json::json!({}),
            payload_digest: format!("sha256:v2:{}", "0".repeat(64)),
            created_ts: 1,
            created_by: "operator".to_owned(),
            reason: "unit test".to_owned(),
        };
        let rows = vec![row("a/b", 2), row("a/b", 3), row("c/d", 7)];
        assert_eq!(family_head_version(&rows, "a/b"), 3);
        assert_eq!(family_head_version(&rows, "c/d"), 7);
        assert_eq!(family_head_version(&rows, "absent/family"), 1);
        assert_eq!(family_head_version(&[], "a/b"), 1);
    }

    #[test]
    fn payload_deviations_flags_only_legs_outside_the_sanity_band() {
        let baseline = serde_json::json!({
            "input": "5000",
            "output": "25000",
            "long_context_threshold": 200000,
            "search": {"kind": "per_query", "nano": "35000000"}
        });
        // Identical, exactly 4x up and exactly 4x down all pass.
        for candidate in [
            baseline.clone(),
            serde_json::json!({
                "input": "20000",
                "output": "6250",
                "long_context_threshold": 800000,
                "search": {"kind": "per_query", "nano": "35000000"}
            }),
        ] {
            assert!(
                payload_deviations(&baseline, &candidate).is_empty(),
                "{candidate}"
            );
        }
        // Past the band, a zero crossing and a nested deviation are all flagged with their paths.
        let deviating = serde_json::json!({
            "input": "20001",
            "output": "0",
            "long_context_threshold": 200000,
            "search": {"kind": "per_query", "nano": "35000001"}
        });
        let deviations = payload_deviations(&baseline, &deviating);
        assert!(
            deviations.iter().any(|entry| entry.contains(".input")),
            "{deviations:?}"
        );
        assert!(
            deviations.iter().any(|entry| entry.contains(".output")),
            "{deviations:?}"
        );
        assert!(
            !deviations.iter().any(|entry| entry.contains("threshold")),
            "{deviations:?}"
        );
        // The nested search nano leg is 1 nano off — well inside the band.
        assert!(
            !deviations.iter().any(|entry| entry.contains("search")),
            "{deviations:?}"
        );
        // A baseline zero may only stay zero.
        let zero_base = serde_json::json!({"image_output": "0"});
        assert!(payload_deviations(&zero_base, &serde_json::json!({"image_output": "0"})).is_empty());
        assert_eq!(
            payload_deviations(&zero_base, &serde_json::json!({"image_output": "1"})).len(),
            1
        );
    }
}
