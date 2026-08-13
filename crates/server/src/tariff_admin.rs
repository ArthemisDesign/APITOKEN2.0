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
    /// The compiled family contains a later effective epoch than the catalog snapshot.
    pub has_future_epoch: bool,
    /// A zero-effective-time seed can reproduce the complete compiled schedule.
    pub seed_safe: bool,
}

fn compiled_tariff(tariff_family: &'static str, payload: serde_json::Value) -> CompiledTariff {
    CompiledTariff {
        tariff_family,
        payload,
        has_future_epoch: false,
        seed_safe: true,
    }
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
        catalog.push(compiled_tariff(
            family,
            to_payload(anthropic_payload(prices)),
        ));
    }
    for (family, prices) in metering::codex_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(codex_payload(prices)),
            has_future_epoch: metering::codex_tariff_has_future_epoch_at(family, now_unix),
            seed_safe: metering::codex_tariff_seed_safe(family),
        });
    }
    for (family, rates) in metering::codex_compiled_credit_rates() {
        catalog.push(compiled_tariff(
            family,
            to_payload(codex_credit_payload(rates)),
        ));
    }
    for (family, prices) in metering::gemini_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(gemini_payload(prices)),
            has_future_epoch: metering::gemini_tariff_has_future_epoch_at(family, now_unix),
            seed_safe: metering::gemini_tariff_seed_safe(family),
        });
    }
    for (family, prices) in metering::glm_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(glm_payload(prices)),
            has_future_epoch: metering::glm_tariff_has_future_epoch_at(family, now_unix),
            seed_safe: metering::glm_tariff_seed_safe(family),
        });
    }
    for (family, rates) in metering::glm_compiled_credit_rates() {
        catalog.push(compiled_tariff(
            family,
            to_payload(glm_credit_payload(rates)),
        ));
    }
    for (family, prices) in metering::kimi_compiled_tariffs_at(now_unix) {
        catalog.push(CompiledTariff {
            tariff_family: family,
            payload: to_payload(kimi_payload(prices)),
            has_future_epoch: metering::kimi_tariff_has_future_epoch_at(family, now_unix),
            seed_safe: metering::kimi_tariff_seed_safe(family),
        });
    }
    let (family, prices) = metering::openai_image_compiled_tariff();
    catalog.push(compiled_tariff(
        family,
        to_payload(openai_image_payload(prices)),
    ));
    catalog.sort_by(|left, right| left.tariff_family.cmp(right.tariff_family));
    catalog
}

/// Reject the complete seed request before authority access if one selected schedule has multiple
/// epochs. One zero-effective-time row would collapse its historical or future price semantics.
pub(crate) fn ensure_seed_safe(targets: &[&CompiledTariff]) -> Result<(), String> {
    let unsafe_families: Vec<&str> = targets
        .iter()
        .filter(|entry| !entry.seed_safe)
        .map(|entry| entry.tariff_family)
        .collect();
    if unsafe_families.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "seed refused atomically: multi-epoch compiled schedules exist for {}; publish explicit effective-dated /override rows instead",
            unsafe_families.join(", ")
        ))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use registry::pricing::{parse_tariff_override_payload, TariffOverridePayload};

    #[test]
    fn every_compiled_family_payload_round_trips_through_the_registry_schema() {
        for ts in [
            0,
            1_788_220_800 - 1,
            1_788_220_800,
            1_798_761_600 - 1,
            1_798_761_600,
            i64::MAX,
        ] {
            let catalog = compiled_tariff_catalog_at(ts);
            // Sorted by family and free of duplicates.
            for window in catalog.windows(2) {
                assert!(window[0].tariff_family < window[1].tariff_family);
            }
            for entry in &catalog {
                let parsed = parse_tariff_override_payload(entry.tariff_family, &entry.payload)
                    .unwrap_or_else(|error| {
                        panic!(
                            "{}: compiled payload rejected: {error:#}",
                            entry.tariff_family
                        )
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
    fn gemini_36_compiled_metadata_guards_both_sides_of_the_cutoff() {
        const FAMILY: &str = "google/gemini/gemini-3.6-flash";
        const STANDARD_START: i64 = 1_798_761_600;

        let before = compiled_tariff_catalog_at(STANDARD_START - 1);
        let promo = before
            .iter()
            .find(|entry| entry.tariff_family == FAMILY)
            .unwrap();
        assert_eq!(promo.payload["input"], "750");
        assert_eq!(promo.payload["cached_input"], "75");
        assert_eq!(promo.payload["output"], "3750");
        assert_eq!(promo.payload["search"]["nano"], "14000000");
        assert!(promo.has_future_epoch);
        assert!(!promo.seed_safe);
        assert!(ensure_seed_safe(&[promo]).is_err());
        assert!(ensure_seed_safe(&before.iter().collect::<Vec<_>>()).is_err());

        let after = compiled_tariff_catalog_at(STANDARD_START);
        let standard = after
            .iter()
            .find(|entry| entry.tariff_family == FAMILY)
            .unwrap();
        assert_eq!(standard.payload["input"], "1500");
        assert_eq!(standard.payload["cached_input"], "150");
        assert_eq!(standard.payload["output"], "7500");
        assert_eq!(standard.payload["search"], promo.payload["search"]);
        assert!(!standard.has_future_epoch);
        assert!(!standard.seed_safe);
        assert!(ensure_seed_safe(&[standard]).is_err());
    }

    #[test]
    fn compiled_seed_safety_inventory_is_complete() {
        let catalog = compiled_tariff_catalog_at(i64::MAX);
        let unsafe_families: std::collections::BTreeSet<&str> = catalog
            .iter()
            .filter(|entry| !entry.seed_safe)
            .map(|entry| entry.tariff_family)
            .collect();
        assert_eq!(
            unsafe_families,
            std::collections::BTreeSet::from([
                "google/gemini/gemini-3.6-flash",
                "openai/codex/gpt-5.6-luna",
                "openai/codex/gpt-5.6-sol",
                "openai/codex/gpt-5.6-terra",
            ])
        );
        assert!(catalog
            .iter()
            .filter(|entry| !entry.seed_safe)
            .all(|entry| !entry.has_future_epoch));
    }

    #[test]
    fn converters_preserve_every_provider_value() {
        let now = 1_788_220_800; // after the Sonnet 5 epoch flip

        for (family, prices) in metering::anthropic_compiled_tariffs_at(now) {
            let parsed =
                parse_tariff_override_payload(family, &to_payload(anthropic_payload(prices)))
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
            let parsed =
                parse_tariff_override_payload(family, &to_payload(codex_credit_payload(rates)))
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
}
