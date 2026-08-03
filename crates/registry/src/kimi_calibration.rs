//! Exact plan-scoped KIMI (Kimi Code) subscription calibration types.
//!
//! Shape and rationale: `docs/engine/KIMI_PROVIDER.md` §5, migration
//! `0027_kimi_window_calibration.sql`. Two differences from the Anthropic and Gemini equivalents
//! are load-bearing:
//!
//! * Quota arrives as integer `used`/`limit` counters, not a percentage. Both raw integers are
//!   the authority; the fraction and its measurement resolution are *derived* from them, so
//!   resolution follows the real limit instead of being assumed.
//! * The window's native size needs no estimation, because `limit` IS the window in native
//!   units. Only the official API replacement cost that fits inside it is estimated.

use anyhow::bail;

/// Fixed-point scale shared with the other providers: 100% == 100_000_000 units.
pub const KIMI_FRACTION_SCALE: i64 = 100_000_000;

/// One immutable quota observation for a single KIMI window.
///
/// KIMI serves quota only from `/usages`; a generation response never carries it. Every
/// observation is therefore a poll, and no request id is invented for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiWindowObservation {
    pub subject_id: String,
    /// Authoritative paid plan (`user_level_name` from `/me`). The cohort key.
    pub plan: String,
    /// Exact native window duration in seconds. The 5-hour window arrives as 300 minutes.
    pub window_duration_secs: i64,
    /// Provider-supplied label, audit metadata only. The duration is the identity.
    pub window_name: Option<String>,
    pub resets_at: i64,
    pub observed_at: i64,
    /// Raw provider integers. Their unit is not normatively documented, so they are preserved
    /// verbatim and never divided by a token price.
    pub native_used_units: i64,
    pub native_limit_units: i64,
    /// Derived from the two integers above via [`kimi_fraction_from_native`].
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    /// Cumulative official API replacement cost for this subject at observation time.
    pub cumulative_api_spend_nano: i64,
}

/// Estimator state for one subject + paid plan + exact native window duration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KimiCalibrationRow {
    pub subject_id: String,
    pub plan: String,
    pub window_duration_secs: i64,
    pub window_name: Option<String>,
    pub resets_at: i64,

    pub anchor_used_fraction_units: i64,
    pub anchor_resolution_fraction_units: i64,
    pub anchor_spend_nano: i64,

    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
    pub observed_at: i64,

    /// Published directly by the provider, never estimated.
    pub native_limit_units: i64,
    pub native_used_units: i64,

    pub observed_fraction_units: i64,
    pub observed_spend_nano: i64,
    pub samples: i64,
    pub unattributed_fraction_units: i64,

    /// Unknown stays `None`. An unbounded high stays `None` rather than a guessed ceiling.
    pub current_capacity_nano: Option<i64>,
    pub current_low_nano: Option<i64>,
    pub current_high_nano: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,

    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

impl KimiCalibrationRow {
    /// Exact native remaining. This needs no estimation and no bounds: the provider publishes
    /// both halves. Returns `None` only if the row is malformed.
    pub fn native_remaining_units(&self) -> Option<i64> {
        self.native_limit_units.checked_sub(self.native_used_units)
    }

    /// Current API-dollar remaining, derived from measured capacity and the *current* exact
    /// unused fraction. `None` while capacity is unknown — never zero.
    pub fn current_remaining_nano(&self) -> Option<i64> {
        let capacity = self.current_capacity_nano?;
        let unused = KIMI_FRACTION_SCALE.checked_sub(self.used_fraction_units)?;
        let remaining = i128::from(capacity)
            .checked_mul(i128::from(unused))?
            .checked_div(i128::from(KIMI_FRACTION_SCALE))?;
        i64::try_from(remaining).ok()
    }
}

/// Derived fraction and measurement resolution for one raw `used`/`limit` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KimiDerivedFraction {
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
}

/// Convert the provider's integer quota counters into fixed-point fraction units.
///
/// The measurement resolution is the width of one native unit expressed in fraction units, which
/// is `ceil(SCALE / limit)`: a limit of 1000 resolves to 0.1%, a limit of 100 to 1%. This is the
/// endpoint's *actual* precision and must not be confused with the fixed-point scale — storing a
/// coarse snapshot in a wide integer does not make it precise.
///
/// A limit finer than one fraction unit clamps to 1, because the estimator's uncertainty
/// envelope needs a strictly positive width.
pub fn kimi_fraction_from_native(used: i64, limit: i64) -> anyhow::Result<KimiDerivedFraction> {
    if limit <= 0 {
        bail!("KIMI quota limit must be positive");
    }
    if used < 0 {
        bail!("KIMI quota used must not be negative");
    }
    if used > limit {
        bail!("KIMI quota used exceeds its limit");
    }
    let used_fraction_units = i128::from(used)
        .checked_mul(i128::from(KIMI_FRACTION_SCALE))
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("KIMI quota fraction overflow"))?;
    let resolution = i128::from(KIMI_FRACTION_SCALE)
        .checked_add(i128::from(limit) - 1)
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("KIMI quota resolution overflow"))?;
    Ok(KimiDerivedFraction {
        used_fraction_units,
        measurement_resolution_fraction_units: resolution.max(1),
    })
}

/// Normalize a provider window into exact seconds.
///
/// The platform expresses sub-day windows in minutes — the 5-hour limit arrives as
/// `duration: 300, timeUnit: TIME_UNIT_MINUTE` — and the weekly summary omits the window
/// entirely. Callers supply the documented weekly duration explicitly rather than having this
/// function guess one.
pub fn kimi_window_duration_secs(duration: i64, time_unit: &str) -> anyhow::Result<i64> {
    if duration <= 0 {
        bail!("KIMI quota window duration must be positive");
    }
    let multiplier = match time_unit {
        "TIME_UNIT_MINUTE" => 60,
        "TIME_UNIT_HOUR" => 3_600,
        "TIME_UNIT_DAY" => 86_400,
        "TIME_UNIT_WEEK" => 604_800,
        // An unknown unit is not coerced into a neighbour: a wrong duration would merge two
        // independent windows into one calibration row.
        _ => bail!("unsupported KIMI quota window unit"),
    };
    duration
        .checked_mul(multiplier)
        .ok_or_else(|| anyhow::anyhow!("KIMI quota window duration overflow"))
}

/// Documented duration of the plan's weekly quota, which the backend omits from `/usages`.
pub const KIMI_WEEKLY_WINDOW_SECS: i64 = 604_800;
/// Documented duration of the rolling short-term rate window.
pub const KIMI_ROLLING_WINDOW_SECS: i64 = 18_000;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_follows_the_real_limit_not_the_fixed_point_scale() {
        // limit=1000 means one native unit is 0.1%, far finer than Claude's whole percent.
        let fine = kimi_fraction_from_native(40, 1_000).unwrap();
        assert_eq!(fine.used_fraction_units, 4_000_000);
        assert_eq!(fine.measurement_resolution_fraction_units, 100_000);

        // limit=100 means one unit is a whole percent.
        let coarse = kimi_fraction_from_native(40, 100).unwrap();
        assert_eq!(coarse.used_fraction_units, 40_000_000);
        assert_eq!(coarse.measurement_resolution_fraction_units, 1_000_000);
    }

    #[test]
    fn resolution_is_rounded_up_so_the_envelope_is_never_understated() {
        // 3 does not divide the scale evenly; rounding down would claim more precision than the
        // endpoint has.
        let derived = kimi_fraction_from_native(1, 3).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 33_333_334);
    }

    #[test]
    fn a_limit_finer_than_one_unit_still_has_positive_width() {
        let derived = kimi_fraction_from_native(0, KIMI_FRACTION_SCALE * 4).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 1);
    }

    #[test]
    fn full_and_empty_windows_map_to_the_scale_bounds() {
        assert_eq!(
            kimi_fraction_from_native(0, 500).unwrap().used_fraction_units,
            0
        );
        assert_eq!(
            kimi_fraction_from_native(500, 500)
                .unwrap()
                .used_fraction_units,
            KIMI_FRACTION_SCALE
        );
    }

    #[test]
    fn invalid_quota_counters_fail_closed() {
        assert!(kimi_fraction_from_native(1, 0).is_err());
        assert!(kimi_fraction_from_native(1, -5).is_err());
        assert!(kimi_fraction_from_native(-1, 10).is_err());
        assert!(kimi_fraction_from_native(11, 10).is_err());
    }

    #[test]
    fn window_units_normalize_to_exact_seconds() {
        assert_eq!(
            kimi_window_duration_secs(300, "TIME_UNIT_MINUTE").unwrap(),
            KIMI_ROLLING_WINDOW_SECS
        );
        assert_eq!(kimi_window_duration_secs(5, "TIME_UNIT_HOUR").unwrap(), 18_000);
        assert_eq!(
            kimi_window_duration_secs(7, "TIME_UNIT_DAY").unwrap(),
            KIMI_WEEKLY_WINDOW_SECS
        );
        assert_eq!(
            kimi_window_duration_secs(1, "TIME_UNIT_WEEK").unwrap(),
            KIMI_WEEKLY_WINDOW_SECS
        );
    }

    #[test]
    fn an_unknown_window_unit_is_refused_rather_than_guessed() {
        // Coercing an unknown unit would silently merge two independent windows.
        assert!(kimi_window_duration_secs(1, "TIME_UNIT_MONTH").is_err());
        assert!(kimi_window_duration_secs(1, "").is_err());
        assert!(kimi_window_duration_secs(0, "TIME_UNIT_HOUR").is_err());
    }

    fn row() -> KimiCalibrationRow {
        KimiCalibrationRow {
            subject_id: "u_1".into(),
            plan: "Vivace".into(),
            window_duration_secs: KIMI_WEEKLY_WINDOW_SECS,
            window_name: None,
            resets_at: 1_800_000_000,
            anchor_used_fraction_units: 0,
            anchor_resolution_fraction_units: 100_000,
            anchor_spend_nano: 0,
            used_fraction_units: 25_000_000,
            measurement_resolution_fraction_units: 100_000,
            observed_at: 1_799_000_000,
            native_limit_units: 1_000,
            native_used_units: 250,
            observed_fraction_units: 25_000_000,
            observed_spend_nano: 500_000_000,
            samples: 1,
            unattributed_fraction_units: 0,
            current_capacity_nano: Some(2_000_000_000),
            current_low_nano: Some(1_900_000_000),
            current_high_nano: Some(2_100_000_000),
            current_confidence_bp: 5_000,
            last_measured_at: Some(1_799_000_000),
            estimator_version: 1,
            version: 3,
            updated_ts: 1_799_000_000,
        }
    }

    #[test]
    fn native_remaining_is_exact_because_the_provider_publishes_both_halves() {
        assert_eq!(row().native_remaining_units(), Some(750));
    }

    #[test]
    fn api_remaining_follows_the_current_exact_unused_fraction() {
        // 75% of a $2.00 window remains.
        assert_eq!(row().current_remaining_nano(), Some(1_500_000_000));
    }

    #[test]
    fn unknown_capacity_stays_none_and_never_becomes_zero() {
        let mut cold = row();
        cold.current_capacity_nano = None;
        assert_eq!(cold.current_remaining_nano(), None);
    }
}
