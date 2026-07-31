//! Evidence-only calibration of one provider-reported Codex subscription window.
//!
//! The provider gives integer utilisation and an explicit duration/reset; the gateway gives exact
//! official API-price spend in integer nanodollars. One snapshot is only an anchor and therefore
//! cannot reveal an absolute capacity. The first movement after a cold start or real reset is also
//! censored because that anchor can have arrived midway through an integer percentage bucket.
//! Every later high-water movement with settled positive spend contributes to one cumulative
//! weighted least-squares estimate through the origin:
//!
//! `capacity_nano = 100 * Σ(Δused_percent * Δgateway_spend_nano) / Σ(Δused_percent²)`.
//!
//! There is deliberately no configured prior, EMA, jump clamp, minimum delta, or foreign-usage
//! rejection. Evidence survives real resets, restarts and estimator upgrades; lower provider
//! snapshots stay in the raw log but cannot erase or duplicate accepted intervals.

use registry::{CodexCalibrationRow, CodexWindowObservation};

pub const ESTIMATOR_VERSION: i64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacitySource {
    MeasuredCumulative,
}

impl CapacitySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MeasuredCumulative => "measured_cumulative",
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
                anchor_ready: false,
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
        self.row
            .current_capacity_nano
            .map(|capacity_nano| CapacityEstimate {
                capacity_nano,
                low_nano: self.row.current_low_nano,
                high_nano: self.row.current_high_nano,
                confidence_bp: self.row.current_confidence_bp,
                measured_at: self.row.last_measured_at.unwrap_or(self.row.observed_at),
                source: CapacitySource::MeasuredCumulative,
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
        if self.row.estimator_version != ESTIMATOR_VERSION {
            return;
        }
        if resets_at > self.row.resets_at {
            self.begin_window(resets_at, used, spend, observed_at);
            return;
        }
        // A late snapshot from an older reset is useful raw evidence for diagnosis, but it cannot
        // move the active window backwards or replace its monotonic high-water anchor.
        if resets_at < self.row.resets_at {
            return;
        }

        self.row.observed_at = observed_at;
        self.row.updated_ts = observed_at;
        self.row.used_percent = used;

        let delta_used = used - self.row.anchor_used_percent;
        // Provider snapshots are empirically non-monotonic inside one reset. Only a strictly new
        // high-water can delimit an interval; a rollback and return to the same high-water must
        // neither reset cumulative evidence nor count it twice.
        if delta_used <= 0 || spend < self.row.anchor_spend_nano {
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

        // A cold anchor can be anywhere inside its integer percentage bucket. Advancing once gives
        // us the first observed bucket boundary, but the preceding spend is right-censored and is
        // not a complete interval. Persist the new anchor so restarts do not censor repeatedly.
        if !self.row.anchor_ready {
            self.row.anchor_used_percent = used;
            self.row.anchor_spend_nano = spend;
            self.row.anchor_ready = true;
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
        self.row.last_measured_at = Some(observed_at);
        self.recompute();
    }

    fn begin_window(&mut self, resets_at: i64, used: i64, spend: i64, observed_at: i64) {
        self.row.resets_at = resets_at;
        self.row.anchor_used_percent = used;
        self.row.anchor_spend_nano = spend;
        self.row.anchor_ready = false;
        self.row.used_percent = used;
        self.row.observed_at = observed_at;
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
    let existing_version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => WindowCalibration::from_row(row),
        _ => WindowCalibration::anchor(
            &observation.home_id,
            observation.window_duration_mins,
            observation.resets_at,
            observation.used_percent,
            observation.gateway_spend_nano,
            observation.observed_at,
        ),
    };
    calibration.row.version = existing_version;
    if calibration.row().observed_at < observation.observed_at {
        calibration.observe(
            observation.resets_at,
            observation.used_percent,
            observation.gateway_spend_nano,
            observation.observed_at,
        );
    }
    calibration.into_row()
}

/// Replay immutable raw evidence when the stored estimator semantics change.
pub(crate) fn replay_observations(
    observations: &[CodexWindowObservation],
    version: i64,
) -> Option<CodexCalibrationRow> {
    let first = observations.first()?;
    let mut calibration = WindowCalibration::anchor(
        &first.home_id,
        first.window_duration_mins,
        first.resets_at,
        first.used_percent,
        first.gateway_spend_nano,
        first.observed_at,
    );
    calibration.row.version = version;
    for observation in &observations[1..] {
        if observation.home_id == first.home_id
            && observation.window_duration_mins == first.window_duration_mins
        {
            calibration.observe(
                observation.resets_at,
                observation.used_percent,
                observation.gateway_spend_nano,
                observation.observed_at,
            );
        }
    }
    Some(calibration.into_row())
}

/// Increment normally, or rebuild poisoned/obsolete derived state from its raw source of truth.
pub(crate) fn apply_observation_with_history(
    existing: Option<CodexCalibrationRow>,
    history: &[CodexWindowObservation],
    observation: &CodexWindowObservation,
) -> CodexCalibrationRow {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let rebuilt = replay_observations(history, version);
    apply_observation(rebuilt, observation)
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
        observation_at(RESET, used, spend_nano, observed_at)
    }

    fn observation_at(
        resets_at: i64,
        used: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> CodexWindowObservation {
        CodexWindowObservation {
            home_id: HOME.into(),
            window_duration_mins: DURATION,
            resets_at,
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
    fn first_movement_is_censored_because_the_cold_anchor_is_partial() {
        let row = next(None, 40, 100_000_000_000, 100);
        let row = next(Some(row), 41, 120_000_000_000, 101);
        assert!(row.anchor_ready);
        assert_eq!(row.anchor_used_percent, 41);
        assert_eq!(row.samples, 0);
        assert_eq!(WindowCalibration::from_row(row).estimate(), None);
    }

    #[test]
    fn second_movement_is_the_first_complete_interval() {
        let row = next(None, 40, 100_000_000_000, 100);
        let row = next(Some(row), 41, 120_000_000_000, 101);
        let row = next(Some(row), 42, 140_000_000_000, 102);
        let estimate = WindowCalibration::from_row(row.clone()).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 2_000_000_000_000);
        assert_eq!(estimate.source, CapacitySource::MeasuredCumulative);
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn percentage_snapshot_waits_for_its_positive_settlement_evidence() {
        let row = next(None, 17, 126_000_000_000, 100);
        let row = next(Some(row), 18, 126_000_000_000, 101);
        assert_eq!(row.anchor_used_percent, 17);
        assert_eq!(row.anchor_spend_nano, 126_000_000_000);
        assert_eq!(row.samples, 0);
        assert_eq!(WindowCalibration::from_row(row.clone()).estimate(), None);

        // The percentage is unchanged, but settlement has now caught up. This crosses only the
        // censored boundary; a complete interval is still required before publishing capacity.
        let row = next(Some(row), 18, 131_000_000_000, 102);
        assert_eq!(row.anchor_used_percent, 18);
        assert_eq!(row.anchor_spend_nano, 131_000_000_000);
        assert!(row.anchor_ready);
        assert_eq!(row.samples, 0);

        let row = next(Some(row), 19, 136_000_000_000, 103);
        assert_eq!(
            WindowCalibration::from_row(row)
                .estimate()
                .unwrap()
                .capacity_nano,
            500_000_000_000
        );
    }

    #[test]
    fn weighted_regression_uses_every_complete_interval_exactly() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 12, 40_000_000_000, 101); // censored
        let row = next(Some(row), 16, 140_000_000_000, 102); // x=4, y=$100
        let row = next(Some(row), 18, 190_000_000_000, 103); // x=2, y=$50
        // 100 * (4*100 + 2*50) / (4² + 2²) = $2500.
        let estimate = WindowCalibration::from_row(row).estimate().unwrap();
        assert_eq!(estimate.capacity_nano, 2_500_000_000_000);
    }

    #[test]
    fn same_reset_rollbacks_neither_erase_nor_duplicate_evidence() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 11, 10_000_000_000, 101); // censored
        let row = next(Some(row), 12, 30_000_000_000, 102); // first sample
        let row = next(Some(row), 11, 35_000_000_000, 103); // rollback
        let row = next(Some(row), 12, 40_000_000_000, 104); // old high-water
        let row = next(Some(row), 13, 50_000_000_000, 105); // second sample
        assert_eq!(row.samples, 2);
        assert_eq!(row.sum_used_spend_nano, 40_000_000_000);
        assert_eq!(
            WindowCalibration::from_row(row)
                .estimate()
                .unwrap()
                .capacity_nano,
            2_000_000_000_000
        );
    }

    #[test]
    fn real_reset_keeps_cumulative_capacity_and_rearms_censoring() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 11, 10_000_000_000, 101);
        let row = next(Some(row), 12, 30_000_000_000, 102);
        let row = apply_observation(
            Some(row),
            &observation_at(RESET + 300, 3, 40_000_000_000, 103),
        );
        assert!(!row.anchor_ready);
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(2_000_000_000_000));

        let row = apply_observation(
            Some(row),
            &observation_at(RESET + 300, 4, 50_000_000_000, 104),
        );
        assert!(row.anchor_ready);
        assert_eq!(row.samples, 1);

        let row = apply_observation(
            Some(row),
            &observation_at(RESET + 300, 5, 70_000_000_000, 105),
        );
        assert_eq!(row.samples, 2);
        assert_eq!(row.current_capacity_nano, Some(2_000_000_000_000));
    }

    #[test]
    fn production_like_raw_history_replays_to_stable_capacity() {
        let spends = [
            (12, 0),
            (13, 5_794_907_000),
            (14, 29_799_996_000),
            (15, 53_945_708_000),
            (16, 77_952_705_000),
            (17, 101_553_253_000),
            (18, 125_449_866_000),
            (19, 152_141_317_000),
            (20, 178_380_227_000),
            (21, 201_788_240_000),
            (22, 225_680_889_000),
            (23, 250_799_095_000),
        ];
        let history: Vec<_> = spends
            .into_iter()
            .enumerate()
            .map(|(index, (used, spend))| observation(used, spend, 100 + index as i64))
            .collect();
        let row = replay_observations(&history, 7).unwrap();
        assert_eq!(row.version, 7);
        assert_eq!(row.samples, 10);
        assert_eq!(row.current_capacity_nano, Some(2_450_041_880_000));
    }

    #[test]
    fn estimator_upgrade_rebuilds_poisoned_state_from_raw_history() {
        let history = vec![
            observation(20, 178_380_227_000, 100),
            observation(21, 201_788_240_000, 101),
            observation(22, 225_680_889_000, 102),
            observation(23, 250_799_095_000, 103),
        ];
        let mut poisoned = replay_observations(&history[..1], 9).unwrap();
        poisoned.estimator_version = ESTIMATOR_VERSION - 1;
        poisoned.current_capacity_nano = Some(187_994_100_000);
        poisoned.samples = 1;

        let rebuilt =
            apply_observation_with_history(Some(poisoned), &history, history.last().unwrap());
        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(rebuilt.version, 9);
        assert_eq!(rebuilt.samples, 2);
        assert_eq!(rebuilt.current_capacity_nano, Some(2_450_542_750_000));
    }

    #[test]
    fn remaining_is_exact_integer_share_of_stable_capacity() {
        let row = next(None, 10, 0, 100);
        let row = next(Some(row), 11, 10_000_000_000, 101);
        let row = next(Some(row), 12, 30_000_000_000, 102);
        let calibration = WindowCalibration::from_row(row);
        assert_eq!(calibration.remaining_nano(50), Some(1_000_000_000_000));
        assert_eq!(calibration.remaining_nano(100), Some(0));
    }

    #[test]
    fn independent_durations_do_not_share_state() {
        let five_hour = next(None, 10, 0, 100);
        let five_hour = next(Some(five_hour), 11, 10_000_000_000, 101);
        let five_hour = next(Some(five_hour), 12, 30_000_000_000, 102);
        let mut weekly_observation = observation(70, 500_000_000_000, 102);
        weekly_observation.window_duration_mins = 10_080;
        let weekly = apply_observation(None, &weekly_observation);
        assert!(five_hour.current_capacity_nano.is_some());
        assert!(weekly.current_capacity_nano.is_none());
        assert_ne!(five_hour.window_duration_mins, weekly.window_duration_mins);
    }
}
