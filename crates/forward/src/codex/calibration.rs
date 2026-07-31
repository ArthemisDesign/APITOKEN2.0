//! Evidence-only calibration of one provider-reported Codex subscription window.
//!
//! The provider gives integer utilisation and an explicit duration/reset; the gateway gives exact
//! official API-price spend in integer nanodollars. One snapshot is only an anchor and therefore
//! cannot reveal an absolute capacity. Every later positive movement with settled positive spend
//! contributes to a weighted least-squares estimate through the origin:
//!
//! `capacity_nano = 100 * Σ(Δused_percent * Δgateway_spend_nano) / Σ(Δused_percent²)`.
//!
//! There is deliberately no configured prior, EMA, jump clamp, minimum delta, or foreign-usage
//! rejection. Coarse provider quantisation is represented as confidence and bounds rather than by
//! discarding real observations or restricting traffic.

use registry::{CodexCalibrationRow, CodexWindowObservation};

pub const ESTIMATOR_VERSION: i64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacitySource {
    MeasuredCurrentWindow,
    MeasuredPreviousWindow,
}

impl CapacitySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MeasuredCurrentWindow => "measured_current_window",
            Self::MeasuredPreviousWindow => "measured_previous_window",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapacityEstimate {
    pub capacity_nano: i64,
    pub low_nano: Option<i64>,
    pub high_nano: Option<i64>,
    pub confidence_bp: i64,
    pub measured_at: i64,
    pub source: CapacitySource,
}

/// Pure estimator state. Persistence and CAS retries live in the billing actor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCalibration {
    row: CodexCalibrationRow,
}

impl WindowCalibration {
    pub fn anchor(
        home_id: &str,
        window_duration_mins: i64,
        resets_at: i64,
        used_percent: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> Self {
        Self {
            row: CodexCalibrationRow {
                home_id: home_id.to_owned(),
                window_duration_mins,
                resets_at,
                anchor_used_percent: used_percent.clamp(0, 100),
                anchor_spend_nano: spend_nano.max(0),
                used_percent: used_percent.clamp(0, 100),
                observed_at,
                sum_used_sq: 0,
                sum_used_spend_nano: 0,
                observed_points: 0,
                samples: 0,
                current_capacity_nano: None,
                current_low_nano: None,
                current_high_nano: None,
                current_confidence_bp: 0,
                last_capacity_nano: None,
                last_low_nano: None,
                last_high_nano: None,
                last_confidence_bp: 0,
                last_measured_at: None,
                estimator_version: ESTIMATOR_VERSION,
                version: 0,
                updated_ts: observed_at,
            },
        }
    }

    pub fn from_row(row: CodexCalibrationRow) -> Self {
        Self { row }
    }

    pub fn into_row(self) -> CodexCalibrationRow {
        self.row
    }

    pub fn row(&self) -> &CodexCalibrationRow {
        &self.row
    }

    pub fn estimate(&self) -> Option<CapacityEstimate> {
        if let Some(capacity_nano) = self.row.current_capacity_nano {
            return Some(CapacityEstimate {
                capacity_nano,
                low_nano: self.row.current_low_nano,
                high_nano: self.row.current_high_nano,
                confidence_bp: self.row.current_confidence_bp,
                measured_at: self.row.observed_at,
                source: CapacitySource::MeasuredCurrentWindow,
            });
        }
        self.row
            .last_capacity_nano
            .map(|capacity_nano| CapacityEstimate {
                capacity_nano,
                low_nano: self.row.last_low_nano,
                high_nano: self.row.last_high_nano,
                confidence_bp: self.row.last_confidence_bp,
                measured_at: self.row.last_measured_at.unwrap_or(self.row.observed_at),
                source: CapacitySource::MeasuredPreviousWindow,
            })
    }

    pub fn remaining_nano(&self, used_percent: i64) -> Option<i64> {
        self.estimate().map(|estimate| {
            let unused = 100i128 - i128::from(used_percent.clamp(0, 100));
            ((i128::from(estimate.capacity_nano) * unused) / 100).clamp(0, i128::from(i64::MAX))
                as i64
        })
    }

    /// Apply a provider snapshot paired with the durable cumulative spend at the same home.
    pub fn observe(
        &mut self,
        resets_at: i64,
        used_percent: i64,
        spend_nano: i64,
        observed_at: i64,
    ) {
        let used = used_percent.clamp(0, 100);
        let spend = spend_nano.max(0);
        if observed_at <= self.row.observed_at {
            return;
        }
        let incompatible = self.row.estimator_version != ESTIMATOR_VERSION
            || self.row.resets_at != resets_at
            || used < self.row.anchor_used_percent
            || spend < self.row.anchor_spend_nano;
        if incompatible {
            self.roll_window(resets_at, used, spend, observed_at);
            return;
        }

        self.row.observed_at = observed_at;
        self.row.updated_ts = observed_at;
        self.row.used_percent = used;

        let delta_used = used - self.row.anchor_used_percent;
        if delta_used == 0 {
            return;
        }
        let delta_spend = spend - self.row.anchor_spend_nano;
        // The provider snapshot and gateway settlement are independent durable streams. A fresh
        // utilisation percentage can therefore arrive before the requests which caused it have
        // finished settling. Publishing that transient pair as a real sample creates a false $0
        // capacity and advances the anchor past the spend. Keep the old anchor until positive
        // official-price evidence catches up; raw observations are still persisted by the caller.
        if delta_spend == 0 {
            return;
        }
        let x = i128::from(delta_used);
        let y = i128::from(delta_spend);
        self.row.sum_used_sq = saturating_i64(i128::from(self.row.sum_used_sq) + x * x);
        self.row.sum_used_spend_nano =
            saturating_i64(i128::from(self.row.sum_used_spend_nano) + x * y);
        self.row.observed_points = self.row.observed_points.saturating_add(delta_used);
        self.row.samples = self.row.samples.saturating_add(1);
        self.row.anchor_used_percent = used;
        self.row.anchor_spend_nano = spend;
        self.recompute();
    }

    fn roll_window(&mut self, resets_at: i64, used: i64, spend: i64, observed_at: i64) {
        // Zero-capacity estimates from estimator v1 were produced by a provider-snapshot /
        // settlement race, not by positive dollar evidence. Never let an estimator upgrade turn
        // such a poisoned current value into the durable previous-window fallback.
        if let Some(capacity) = self
            .row
            .current_capacity_nano
            .filter(|capacity| *capacity > 0)
        {
            self.row.last_capacity_nano = Some(capacity);
            self.row.last_low_nano = self.row.current_low_nano;
            self.row.last_high_nano = self.row.current_high_nano;
            self.row.last_confidence_bp = self.row.current_confidence_bp;
            self.row.last_measured_at = Some(self.row.observed_at);
        }
        self.row.resets_at = resets_at;
        self.row.anchor_used_percent = used;
        self.row.anchor_spend_nano = spend;
        self.row.used_percent = used;
        self.row.observed_at = observed_at;
        self.row.sum_used_sq = 0;
        self.row.sum_used_spend_nano = 0;
        self.row.observed_points = 0;
        self.row.samples = 0;
        self.row.current_capacity_nano = None;
        self.row.current_low_nano = None;
        self.row.current_high_nano = None;
        self.row.current_confidence_bp = 0;
        self.row.estimator_version = ESTIMATOR_VERSION;
        self.row.updated_ts = observed_at;
    }

    fn recompute(&mut self) {
        let denominator = i128::from(self.row.sum_used_sq);
        if denominator <= 0 {
            return;
        }
        let numerator = 100i128 * i128::from(self.row.sum_used_spend_nano);
        let capacity = round_nonnegative(numerator, denominator);
        self.row.current_capacity_nano = Some(capacity);

        // Each reported integer delta may carry roughly one percentage point of endpoint
        // quantisation. Bounds widen accordingly; the one-point/one-sample upper bound is
        // intentionally unknown instead of pretending to precision we do not have.
        let points = i128::from(self.row.observed_points.max(0));
        let samples = i128::from(self.row.samples.max(0));
        self.row.current_low_nano = Some(saturating_i64(
            i128::from(capacity) * points / (points + samples).max(1),
        ));
        self.row.current_high_nano = (points > samples)
            .then(|| saturating_i64(i128::from(capacity) * points / (points - samples).max(1)));
        self.row.current_confidence_bp =
            saturating_i64(10_000i128 * points / (points + 2 * samples).max(1)).clamp(0, 10_000);
    }
}

pub(crate) fn apply_observation(
    existing: Option<CodexCalibrationRow>,
    observation: &CodexWindowObservation,
) -> CodexCalibrationRow {
    let mut calibration = existing.map_or_else(
        || {
            WindowCalibration::anchor(
                &observation.home_id,
                observation.window_duration_mins,
                observation.resets_at,
                observation.used_percent,
                observation.gateway_spend_nano,
                observation.observed_at,
            )
        },
        WindowCalibration::from_row,
    );
    if calibration.row().version > 0 {
        calibration.observe(
            observation.resets_at,
            observation.used_percent,
            observation.gateway_spend_nano,
            observation.observed_at,
        );
    }
    calibration.into_row()
}

fn round_nonnegative(numerator: i128, denominator: i128) -> i64 {
    if numerator <= 0 || denominator <= 0 {
        return 0;
    }
    saturating_i64((numerator + denominator / 2) / denominator)
}

fn saturating_i64(value: i128) -> i64 {
    value.clamp(0, i128::from(i64::MAX)) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "home-1";
    const DURATION: i64 = 300;
    const RESET: i64 = 2_000_000_000;

    fn observation(used: i64, spend_nano: i64, observed_at: i64) -> CodexWindowObservation {
        CodexWindowObservation {
            home_id: HOME.into(),
            window_duration_mins: DURATION,
            resets_at: RESET,
            observed_at,
            used_percent: used,
            gateway_spend_nano: spend_nano,
        }
    }

    fn next(
        row: Option<CodexCalibrationRow>,
        used: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> CodexCalibrationRow {
        let mut row = apply_observation(row, &observation(used, spend_nano, observed_at));
        // Persistence owns version increments; emulate one successful CAS for pure tests.
        row.version = row.version.saturating_add(1);
        row
    }

    #[test]
    fn first_snapshot_is_unknown_instead_of_a_dollar_prior() {
        let row = next(None, 40, 100_000_000_000, 100);
        let calibration = WindowCalibration::from_row(row);
        assert_eq!(calibration.estimate(), None);
        assert_eq!(calibration.remaining_nano(40), None);
    }

    #[test]
    fn one_percentage_point_is_real_measurement() {
        let row = next(None, 40, 100_000_000_000, 100);
        let row = next(Some(row), 41, 120_000_000_000, 101);
        let estimate = WindowCalibration::from_row(row).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 2_000_000_000_000);
        assert_eq!(estimate.source, CapacitySource::MeasuredCurrentWindow);
        assert!(estimate.high_nano.is_none());
    }

    #[test]
    fn percentage_snapshot_waits_for_its_positive_settlement_evidence() {
        let row = next(None, 17, 126_000_000_000, 100);
        let row = next(Some(row), 18, 126_000_000_000, 101);
        assert_eq!(row.anchor_used_percent, 17);
        assert_eq!(row.anchor_spend_nano, 126_000_000_000);
        assert_eq!(row.samples, 0);
        assert_eq!(WindowCalibration::from_row(row.clone()).estimate(), None);

        // The percentage is unchanged, but settlement has now caught up. Because the anchor was
        // retained, this is the real paired one-point/$5 interval rather than a zero-capacity row.
        let row = next(Some(row), 18, 131_000_000_000, 102);
        let estimate = WindowCalibration::from_row(row.clone()).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 500_000_000_000);
        assert_eq!(row.anchor_used_percent, 18);
        assert_eq!(row.anchor_spend_nano, 131_000_000_000);
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn estimator_upgrade_discards_false_zero_without_losing_previous_measurement() {
        let mut row = next(None, 17, 126_000_000_000, 100);
        row.estimator_version = ESTIMATOR_VERSION - 1;
        row.current_capacity_nano = Some(0);
        row.current_low_nano = Some(0);
        row.current_confidence_bp = 3_333;
        row.last_capacity_nano = Some(125_000_000_000);
        row.last_low_nano = Some(100_000_000_000);
        row.last_high_nano = Some(150_000_000_000);
        row.last_confidence_bp = 8_000;
        row.last_measured_at = Some(99);

        let row = next(Some(row), 18, 131_000_000_000, 101);
        assert_eq!(row.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(row.current_capacity_nano, None);
        assert_eq!(row.last_capacity_nano, Some(125_000_000_000));
        let estimate = WindowCalibration::from_row(row).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 125_000_000_000);
        assert_eq!(estimate.source, CapacitySource::MeasuredPreviousWindow);
    }

    #[test]
    fn weighted_regression_uses_every_positive_interval_exactly() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 12, 40_000_000_000, 101); // x=2, y=$40
        let row = next(Some(row), 16, 140_000_000_000, 102); // x=4, y=$100
                                                             // 100 * (2*40 + 4*100) / (2² + 4²) = $2400.
        let estimate = WindowCalibration::from_row(row).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 2_400_000_000_000);
    }

    #[test]
    fn reset_keeps_only_a_measured_previous_window() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 12, 40_000_000_000, 101);
        let mut observation = observation(3, 50_000_000_000, 102);
        observation.resets_at += 300;
        let row = apply_observation(Some(row), &observation);
        let estimate = WindowCalibration::from_row(row.clone()).estimate().unwrap();
        assert_eq!(estimate.source, CapacitySource::MeasuredPreviousWindow);
        assert_eq!(estimate.measured_at, 101);
        assert!(row.current_capacity_nano.is_none());
        assert_eq!(row.samples, 0);
        assert_eq!(row.updated_ts, 102);
    }

    #[test]
    fn reset_without_measurement_stays_unknown() {
        let row = next(None, 10, 0, 100);
        let mut observation = observation(3, 0, 101);
        observation.resets_at += 300;
        let row = apply_observation(Some(row), &observation);
        assert_eq!(WindowCalibration::from_row(row).estimate(), None);
    }

    #[test]
    fn remaining_is_exact_integer_unused_share() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 12, 40_000_000_000, 101);
        let calibration = WindowCalibration::from_row(row);
        assert_eq!(calibration.remaining_nano(50), Some(1_000_000_000_000));
        assert_eq!(calibration.remaining_nano(100), Some(0));
    }

    #[test]
    fn independent_durations_do_not_share_state() {
        let five_hour = next(None, 10, 0, 100);
        let five_hour = next(Some(five_hour), 12, 40_000_000_000, 101);
        let mut weekly_observation = observation(70, 500_000_000_000, 102);
        weekly_observation.window_duration_mins = 10_080;
        let weekly = apply_observation(None, &weekly_observation);
        assert!(five_hour.current_capacity_nano.is_some());
        assert!(weekly.current_capacity_nano.is_none());
        assert_ne!(five_hour.window_duration_mins, weekly.window_duration_mins);
    }
}
