//! Suno reviewed derived credit schedule and nanoUSD conversion.
//!
//! **There is no official Suno API rate card** — Suno has no public API at all
//! (`docs/engine/SUNO_PROVIDER.md` §0/§5.1, reviewed 2026-08-12). This module pins a **reviewed
//! derived schedule**: the per-credit replacement value is the worst (highest) subscription unit
//! economics, Pro $10 / 2 500 credits = **$0.004 per credit** (Premier is $0.003; the higher
//! value is conservative). If top-up pricing or an official API price list becomes available, a
//! new dated epoch replaces the derivation — history is never rewritten.
//!
//! Every lookup fails closed: an unknown operation or model id returns `None`, never a guessed
//! price. All arithmetic is checked integer math.

use crate::TariffScheduleId;

/// Reviewed identity of the derived credit schedule below (`docs/engine/SUNO_PROVIDER.md` §5.1,
/// reviewed 2026-08-12). Dated like every schedule; a new money anchor (top-up pricing, an
/// official API rate card) is a new epoch, not an edit of this one.
pub const SUNO_TARIFF_SCHEDULE_ID: &str = "suno/derived-subscription/2026-08-12";

/// Hot-override tariff family of the Suno schedule. Deliberately NOT the schedule id minus its
/// date: `suno/credits` is the stable name of Suno's native credit unit, chosen so that a future
/// epoch anchored to an official price list keeps the same override family.
pub const SUNO_TARIFF_FAMILY: &str = "suno/credits";

/// Derived per-credit replacement value: Pro $10 / 2 500 credits = $0.004 = 4 000 000 nanoUSD
/// (`docs/engine/SUNO_PROVIDER.md` §5.1). Conservative against Premier ($0.003/credit).
pub const SUNO_NANOUSD_PER_CREDIT: i128 = 4_000_000;

/// Official implied song price: 5 credits per song ("50 credits = 10 songs", "2 500 credits =
/// up to 500 songs"; manifest §5.1). **No per-model price differentiation is published** — the
/// rate is flat across the paid model list.
pub const SUNO_CREDITS_PER_SONG: i64 = 5;

/// Paid (Pro/Premier) model ids, reviewed 2026-08-12 (`docs/engine/SUNO_PROVIDER.md` §3).
///
/// All of these models are announced for retirement when the industry-partnership generation
/// launches (new ToS effective 2026-09-03); that event is handled as a new reviewed epoch, not
/// an edit of this list. The free-tier `v4.5-all` and deprecated v2/v3/v3.5 are deliberately
/// absent: an unlisted id fails closed. Exact wire spellings (`mv` values) are `unknown` until
/// the live matrix pins them — this list is the reviewed catalog identity, not a wire claim.
pub const SUNO_PAID_MODELS: &[&str] = &["v4", "v4.5", "v4.5+", "v5", "v5.5"];

/// The hot-override tariff family a Suno operation price resolves against.
pub fn suno_tariff_family() -> &'static str {
    SUNO_TARIFF_FAMILY
}

/// The pinned schedule identity of this derived schedule, for durable admission snapshots.
pub fn suno_tariff_schedule_id() -> TariffScheduleId {
    TariffScheduleId::from_static(SUNO_TARIFF_SCHEDULE_ID)
}

/// Suno operations with a published credit cost (`docs/engine/SUNO_PROVIDER.md` §5.1, reviewed
/// 2026-08-12). Operations with unpublished costs (Extend, Remaster, Covers post-free-batch,
/// Voices, Custom Models, video/image, Studio operations other than MIDI) are deliberately not
/// represented: an unknown price fails closed before reserve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunoOperation {
    /// Song generation: 5 credits (official implied, flat across paid models).
    Song,
    /// Stems Auto Split: 50 credits per extraction.
    StemsAutoSplit,
    /// Stems Split from Mix: 10 credits.
    StemsSplitFromMix,
    /// Stems Advanced Split (Premier): 10 credits **per stem** — priced by
    /// [`suno_stems_advanced_split_credits`], not by [`suno_operation_credits`].
    StemsAdvancedSplit,
    /// MIDI from a stem (Studio): 10 credits.
    MidiFromStem,
}

/// Why a credit → nanoUSD conversion cannot be performed. Every variant fails closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunoMeteringError {
    /// Credits are a non-negative counter; a negative value means broken upstream accounting.
    NegativeCredits,
    /// Checked integer arithmetic overflowed.
    Overflow,
}

/// Exact credit cost of a flat-priced operation, or `None` for the per-stem operation
/// (`StemsAdvancedSplit` — use [`suno_stems_advanced_split_credits`]).
pub fn suno_operation_credits(operation: SunoOperation) -> Option<i64> {
    match operation {
        SunoOperation::Song => Some(SUNO_CREDITS_PER_SONG),
        SunoOperation::StemsAutoSplit => Some(50),
        SunoOperation::StemsSplitFromMix => Some(10),
        SunoOperation::StemsAdvancedSplit => None,
        SunoOperation::MidiFromStem => Some(10),
    }
}

/// Stems Advanced Split: **10 credits per stem** (manifest §5.1). A zero stem count is not a
/// priceable request and fails closed; the multiplication is checked.
pub fn suno_stems_advanced_split_credits(stems: u64) -> Option<i64> {
    if stems == 0 {
        return None;
    }
    10_i64.checked_mul(i64::try_from(stems).ok()?)
}

/// Resolve a paid model id to its canonical catalog entry, or `None` for unknown, free-tier
/// (`v4.5-all`), deprecated (v2/v3/v3.5), or malformed ids — all fail closed.
pub fn suno_paid_model(model: &str) -> Option<&'static str> {
    SUNO_PAID_MODELS.iter().copied().find(|known| *known == model)
}

/// Credit cost of one song on a paid model: the official implied flat 5 credits. No per-model
/// differentiation is published, so every admitted model prices identically; an unknown id fails
/// closed rather than borrowing the flat rate.
pub fn suno_song_credits_for_model(model: &str) -> Option<i64> {
    suno_paid_model(model)?;
    Some(SUNO_CREDITS_PER_SONG)
}

/// One song at the derived schedule: 5 credits × $0.004 = $0.02 = 20 000 000 nanoUSD.
pub fn suno_song_cost_nanodollars() -> i128 {
    i128::from(SUNO_CREDITS_PER_SONG) * SUNO_NANOUSD_PER_CREDIT
}

/// Convert exact credits to nanoUSD at the derived rate ($0.004/credit). Checked integer math;
/// negative credits and overflow are typed errors, never silently clamped. The input is i128 so
/// callers aggregating many settled turns can pass a running total directly — overflow is then
/// reachable and reported, not wrapped.
pub fn suno_cost_nanodollars(credits: i128) -> Result<i128, SunoMeteringError> {
    if credits < 0 {
        return Err(SunoMeteringError::NegativeCredits);
    }
    credits
        .checked_mul(SUNO_NANOUSD_PER_CREDIT)
        .ok_or(SunoMeteringError::Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operation_credit_costs_are_exact() {
        assert_eq!(suno_operation_credits(SunoOperation::Song), Some(5));
        assert_eq!(suno_operation_credits(SunoOperation::StemsAutoSplit), Some(50));
        assert_eq!(suno_operation_credits(SunoOperation::StemsSplitFromMix), Some(10));
        assert_eq!(suno_operation_credits(SunoOperation::MidiFromStem), Some(10));
        // Per-stem pricing has its own counted helper; the flat entry refuses it.
        assert_eq!(suno_operation_credits(SunoOperation::StemsAdvancedSplit), None);
    }

    #[test]
    fn advanced_split_is_priced_per_stem() {
        assert_eq!(suno_stems_advanced_split_credits(1), Some(10));
        assert_eq!(suno_stems_advanced_split_credits(4), Some(40));
        assert_eq!(suno_stems_advanced_split_credits(0), None);
        assert_eq!(suno_stems_advanced_split_credits(u64::MAX), None);
    }

    #[test]
    fn song_cost_is_exactly_two_cents() {
        assert_eq!(suno_song_cost_nanodollars(), 20_000_000);
        assert_eq!(
            suno_song_cost_nanodollars(),
            suno_cost_nanodollars(SUNO_CREDITS_PER_SONG as i128).unwrap()
        );
    }

    #[test]
    fn nanodollar_conversion_is_exact() {
        assert_eq!(suno_cost_nanodollars(1), Ok(4_000_000));
        assert_eq!(suno_cost_nanodollars(5), Ok(20_000_000));
        // Pro plan: 2 500 credits = exactly $10.00; Premier: 10 000 credits = $40.00.
        assert_eq!(suno_cost_nanodollars(2_500), Ok(10 * crate::NANO_PER_USD));
        assert_eq!(suno_cost_nanodollars(10_000), Ok(40 * crate::NANO_PER_USD));
    }

    #[test]
    fn conversion_errors_are_typed() {
        assert_eq!(
            suno_cost_nanodollars(-1),
            Err(SunoMeteringError::NegativeCredits)
        );
        assert_eq!(
            suno_cost_nanodollars(i128::MAX),
            Err(SunoMeteringError::Overflow)
        );
    }

    #[test]
    fn paid_model_catalog_is_closed() {
        for model in SUNO_PAID_MODELS {
            assert_eq!(suno_paid_model(model), Some(*model));
            // Flat 5 credits/song: no per-model differentiation.
            assert_eq!(suno_song_credits_for_model(model), Some(5), "{model}");
        }
        // Free-tier, deprecated, future and malformed ids all fail closed.
        for unknown in ["v4.5-all", "v3.5", "v3", "v2", "v6", "V5", "v5.5 ", ""] {
            assert_eq!(suno_paid_model(unknown), None, "{unknown}");
            assert_eq!(suno_song_credits_for_model(unknown), None, "{unknown}");
        }
    }

    #[test]
    fn tariff_identity_is_stable() {
        assert_eq!(suno_tariff_family(), "suno/credits");
        assert!(
            SUNO_TARIFF_SCHEDULE_ID.starts_with("suno/"),
            "the schedule id must stay under the suno/ namespace"
        );
        assert_eq!(suno_tariff_schedule_id().as_str(), SUNO_TARIFF_SCHEDULE_ID);
        // The derived schedule pins the conservative Pro unit economics, not Premier's.
        assert_eq!(SUNO_NANOUSD_PER_CREDIT, 4_000_000);
        assert_eq!(SUNO_CREDITS_PER_SONG, 5);
    }
}
