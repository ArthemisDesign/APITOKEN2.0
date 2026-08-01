//! Evidence-only calibration of one provider-reported Codex subscription window.
//!
//! The provider reports decimal utilisation and an explicit duration/reset. The gateway pairs
//! that fixed-point snapshot with cumulative exact official-API-price spend for the same opaque
//! profile. A cold snapshot is only an anchor and does not publish a nominal. The first positive
//! movement with settled positive spend is already a complete interval between two fixed-point
//! snapshots, so it and every later positive movement contribute to the realized workload blend:
//!
//! `capacity_nano = FRACTION_SCALE * ΣΔspend_nano / ΣΔused_fraction_units`.
//!
//! OpenAI documents that Codex credit consumption varies by model, context, reasoning and tools.
//! API-dollar capacity is therefore a realized mix, not a fixed subscription nominal. Low/high
//! retain the observed per-interval workload envelope (including fraction quantisation), while
//! confidence combines sample maturity, workload stability and resolution. There is no configured
//! prior, subscription-price assumption, EMA or floating-point money arithmetic.

use anyhow::Context as _;
use registry::{CodexCalibrationRow, CodexWindowObservation};

pub const FRACTION_SCALE: i64 = 100_000_000;
const PERCENT_SCALE: i64 = FRACTION_SCALE / 100;
pub const ESTIMATOR_VERSION: i64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapacitySource {
    WorkloadBlend,
}

impl CapacitySource {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WorkloadBlend => "workload_blend",
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
    fn anchor(observation: &CodexWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        Ok(Self {
            row: CodexCalibrationRow {
                home_id: observation.home_id.clone(),
                window_duration_mins: observation.window_duration_mins,
                resets_at: observation.resets_at,
                anchor_used_percent: observation.used_percent,
                anchor_spend_nano: observation.gateway_spend_nano,
                used_percent: observation.used_percent,
                observed_at: observation.observed_at,
                // Legacy WLS fields stay zero in estimator v8. They remain in storage so the
                // migration-first rollout is expand-only for the previous binary.
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
                anchor_ready: true,
                anchor_used_fraction_units: observation.used_fraction_units,
                used_fraction_units: observation.used_fraction_units,
                observed_fraction_units: 0,
                observed_spend_nano: 0,
                estimator_version: ESTIMATOR_VERSION,
                version: 0,
                updated_ts: observation.observed_at,
            },
        })
    }

    pub fn from_row(row: CodexCalibrationRow) -> anyhow::Result<Self> {
        validate_row(&row)?;
        Ok(Self { row })
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
                source: CapacitySource::WorkloadBlend,
            })
    }

    pub fn remaining_nano(&self, used_fraction_units: i64) -> Option<i64> {
        self.estimate().and_then(|estimate| {
            remaining_for_capacity(estimate.capacity_nano, used_fraction_units)
        })
    }

    pub fn remaining_low_nano(&self, used_fraction_units: i64) -> Option<i64> {
        self.estimate().and_then(|estimate| {
            estimate
                .low_nano
                .and_then(|capacity| remaining_for_capacity(capacity, used_fraction_units))
        })
    }

    pub fn remaining_high_nano(&self, used_fraction_units: i64) -> Option<i64> {
        self.estimate().and_then(|estimate| {
            estimate
                .high_nano
                .and_then(|capacity| remaining_for_capacity(capacity, used_fraction_units))
        })
    }

    fn observe(&mut self, observation: &CodexWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        if observation.home_id != self.row.home_id
            || observation.window_duration_mins != self.row.window_duration_mins
        {
            anyhow::bail!("Codex calibration observation identity mismatch");
        }
        if observation.observed_at <= self.row.observed_at {
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Codex calibration estimator version mismatch");
        }

        let reset_delta = i128::from(observation.resets_at) - i128::from(self.row.resets_at);
        // The provider computes reset_at independently for every snapshot, so one concrete reset
        // can jitter. A half-window jump is independently sufficient evidence. Live weekly
        // windows can also roll over after a smaller forward reset-at move, however, because the
        // subscription window is rolling rather than calendar-aligned. In that case a material
        // reset-at advance plus utilisation dropping below the high-water is the joint signal.
        let duration_secs = i128::from(self.row.window_duration_mins) * 60;
        let boundary = (duration_secs / 2).max(1);
        let jitter_tolerance = (duration_secs / 200).clamp(1, 3_600);
        let usage_rolled_over = observation.used_fraction_units
            < self.row.anchor_used_fraction_units
            && reset_delta >= jitter_tolerance;
        if reset_delta >= boundary || usage_rolled_over {
            self.begin_window(observation);
            return Ok(());
        }
        if reset_delta <= -boundary {
            return Ok(());
        }

        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.used_percent = observation.used_percent;

        let delta_used = observation.used_fraction_units - self.row.anchor_used_fraction_units;
        // Same-window snapshots can roll back. Only a new high-water delimits an interval; a
        // rollback and return to the old high-water cannot erase or duplicate evidence.
        if delta_used <= 0 || observation.gateway_spend_nano < self.row.anchor_spend_nano {
            return Ok(());
        }
        let delta_spend = observation.gateway_spend_nano - self.row.anchor_spend_nano;
        // Snapshot and settlement are independent durable streams. Keep the old anchor until
        // positive spend catches up instead of publishing a transient zero-dollar sample.
        if delta_spend == 0 {
            return Ok(());
        }

        self.update_workload_envelope(delta_used, delta_spend)?;
        self.row.observed_fraction_units = self
            .row
            .observed_fraction_units
            .checked_add(delta_used)
            .context("Codex observed fraction overflow")?;
        self.row.observed_spend_nano = self
            .row
            .observed_spend_nano
            .checked_add(delta_spend)
            .context("Codex observed spend overflow")?;
        self.row.observed_points = self.row.observed_fraction_units / PERCENT_SCALE;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Codex calibration sample overflow")?;
        self.advance_anchor(observation);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &CodexWindowObservation) {
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_used_percent = observation.used_percent;
        self.row.anchor_spend_nano = observation.gateway_spend_nano;
    }

    fn begin_window(&mut self, observation: &CodexWindowObservation) {
        self.row.resets_at = observation.resets_at;
        self.advance_anchor(observation);
        self.row.anchor_ready = true;
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.used_percent = observation.used_percent;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
    }

    fn update_workload_envelope(
        &mut self,
        delta_used: i64,
        delta_spend: i64,
    ) -> anyhow::Result<()> {
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(i128::from(delta_spend))
            .context("Codex workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + 1)
                .context("Codex workload low bound overflow")?,
        )?;
        self.row.current_low_nano = Some(
            self.row
                .current_low_nano
                .map_or(low, |existing| existing.min(low)),
        );

        let sample_high = if delta_used > 1 {
            Some(checked_i64(ceil_nonnegative(
                numerator,
                i128::from(delta_used) - 1,
            )?)?)
        } else {
            None
        };
        self.row.current_high_nano = if self.row.samples == 0 {
            sample_high
        } else {
            match (self.row.current_high_nano, sample_high) {
                (Some(existing), Some(sample)) => Some(existing.max(sample)),
                _ => None,
            }
        };
        Ok(())
    }

    fn recompute(&mut self) -> anyhow::Result<()> {
        let denominator = i128::from(self.row.observed_fraction_units);
        if denominator <= 0 {
            return Ok(());
        }
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(i128::from(self.row.observed_spend_nano))
            .context("Codex capacity numerator overflow")?;
        self.row.current_capacity_nano =
            Some(checked_i64(round_nonnegative(numerator, denominator)?)?);

        // Confidence is evidence maturity, not a statistical probability. More samples increase
        // maturity, diverse workload regimes reduce stability, and fine movement reduces endpoint
        // quantisation doubt.
        let movement = i128::from(self.row.observed_fraction_units);
        let samples = i128::from(self.row.samples);
        let maturity_bp = ratio_bp(samples, samples + 2)?;
        let stability_bp = match (self.row.current_low_nano, self.row.current_high_nano) {
            (Some(low), Some(high)) if high > 0 => ratio_bp(i128::from(low), i128::from(high))?,
            _ => 0,
        };
        let quantisation_bp = ratio_bp(movement, movement + 2 * samples)?;
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("Codex confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

pub(crate) fn apply_observation(
    existing: Option<CodexCalibrationRow>,
    observation: &CodexWindowObservation,
) -> anyhow::Result<CodexCalibrationRow> {
    let existing_version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            WindowCalibration::from_row(row)?
        }
        _ => WindowCalibration::anchor(observation)?,
    };
    calibration.row.version = existing_version;
    if calibration.row.observed_at < observation.observed_at {
        calibration.observe(observation)?;
    }
    Ok(calibration.into_row())
}

fn replay_observations(
    observations: &[CodexWindowObservation],
    version: i64,
) -> anyhow::Result<Option<CodexCalibrationRow>> {
    let Some(first) = observations.first() else {
        return Ok(None);
    };
    let mut calibration = WindowCalibration::anchor(first)?;
    calibration.row.version = version;
    for observation in &observations[1..] {
        if observation.home_id == first.home_id
            && observation.window_duration_mins == first.window_duration_mins
        {
            calibration.observe(observation)?;
        }
    }
    Ok(Some(calibration.into_row()))
}

pub(crate) fn apply_observation_with_history(
    existing: Option<CodexCalibrationRow>,
    history: &[CodexWindowObservation],
    observation: &CodexWindowObservation,
) -> anyhow::Result<CodexCalibrationRow> {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let rebuilt = replay_observations(history, version)?;
    apply_observation(rebuilt, observation)
}

fn validate_observation(observation: &CodexWindowObservation) -> anyhow::Result<()> {
    if observation.home_id.is_empty()
        || observation.window_duration_mins <= 0
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&observation.used_fraction_units)
        || observation.used_percent != rounded_percent(observation.used_fraction_units)
        || observation.gateway_spend_nano < 0
    {
        anyhow::bail!("invalid Codex calibration observation");
    }
    Ok(())
}

fn validate_row(row: &CodexCalibrationRow) -> anyhow::Result<()> {
    if row.home_id.is_empty()
        || row.window_duration_mins <= 0
        || row.resets_at <= 0
        || row.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&row.anchor_used_fraction_units)
        || !(0..=FRACTION_SCALE).contains(&row.used_fraction_units)
        || row.anchor_used_percent != rounded_percent(row.anchor_used_fraction_units)
        || row.used_percent != rounded_percent(row.used_fraction_units)
        || row.anchor_spend_nano < 0
        || row.observed_fraction_units < 0
        || row.observed_spend_nano < 0
        || row.samples < 0
        || row.sum_used_sq != 0
        || row.sum_used_spend_nano != 0
        || row.current_capacity_nano.is_some_and(|value| value < 0)
        || row.current_low_nano.is_some_and(|value| value < 0)
        || row.current_high_nano.is_some_and(|value| value < 0)
        || row
            .current_low_nano
            .zip(row.current_capacity_nano)
            .is_some_and(|(low, capacity)| low > capacity)
        || row
            .current_high_nano
            .zip(row.current_capacity_nano)
            .is_some_and(|(high, capacity)| high < capacity)
        || row
            .current_low_nano
            .zip(row.current_high_nano)
            .is_some_and(|(low, high)| low > high)
        || !(0..=10_000).contains(&row.current_confidence_bp)
        || row.last_measured_at.is_some_and(|value| value <= 0)
        || row.estimator_version != ESTIMATOR_VERSION
        || !row.anchor_ready
        || row.version < 0
        || row.samples == 0
            && (row.observed_fraction_units != 0
                || row.observed_spend_nano != 0
                || row.current_capacity_nano.is_some()
                || row.current_low_nano.is_some()
                || row.current_high_nano.is_some()
                || row.current_confidence_bp != 0
                || row.last_measured_at.is_some())
        || row.samples > 0
            && (row.observed_fraction_units <= 0
                || row.observed_spend_nano <= 0
                || row.current_capacity_nano.is_none()
                || row.current_low_nano.is_none()
                || row.last_measured_at.is_none()
                || row.current_high_nano.is_none() && row.current_confidence_bp != 0)
    {
        anyhow::bail!("invalid Codex calibration row");
    }
    Ok(())
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("Codex capacity rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("Codex capacity ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    10_000i128
        .checked_mul(numerator)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("Codex confidence ratio overflow")
}

fn remaining_for_capacity(capacity_nano: i64, used_fraction_units: i64) -> Option<i64> {
    let unused = i128::from(FRACTION_SCALE - used_fraction_units.clamp(0, FRACTION_SCALE));
    let remaining = i128::from(capacity_nano)
        .checked_mul(unused)
        .and_then(|value| value.checked_div(i128::from(FRACTION_SCALE)))?;
    i64::try_from(remaining).ok()
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).context("Codex calibration result exceeds bigint")
}

fn rounded_percent(units: i64) -> i64 {
    units.saturating_add(PERCENT_SCALE / 2) / PERCENT_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: &str = "home-1";
    const DURATION: i64 = 300;
    const RESET: i64 = 2_000_000_000;

    fn observation(used: i64, spend_nano: i64, observed_at: i64) -> CodexWindowObservation {
        observation_at(RESET, DURATION, used, spend_nano, observed_at)
    }

    fn observation_at(
        resets_at: i64,
        duration: i64,
        used: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> CodexWindowObservation {
        CodexWindowObservation {
            home_id: HOME.into(),
            window_duration_mins: duration,
            resets_at,
            observed_at,
            used_percent: rounded_percent(used),
            used_fraction_units: used,
            gateway_spend_nano: spend_nano,
        }
    }

    fn next(
        row: Option<CodexCalibrationRow>,
        observation: CodexWindowObservation,
    ) -> CodexCalibrationRow {
        let mut row = apply_observation(row, &observation).unwrap();
        row.version += 1;
        row
    }

    #[test]
    fn cold_anchor_stays_unknown_but_first_complete_movement_publishes() {
        let row = next(None, observation(12_125_000, 0, 100));
        assert!(row.anchor_ready);
        assert!(WindowCalibration::from_row(row.clone())
            .unwrap()
            .estimate()
            .is_none());
        let row = next(Some(row), observation(12_125_100, 10_000, 101));
        assert!(row.anchor_ready);
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(10_000_000_000));
    }

    #[test]
    fn complete_intervals_use_all_exact_fractional_evidence() {
        let row = next(None, observation(12_125_000, 0, 100));
        let row = next(Some(row), observation(12_125_100, 10_000, 101));
        let row = next(Some(row), observation(12_125_300, 50_000, 102));
        let estimate = WindowCalibration::from_row(row.clone())
            .unwrap()
            .estimate()
            .unwrap();
        assert_eq!(estimate.capacity_nano, 16_666_666_667);
        assert_eq!(row.observed_fraction_units, 300);
        assert_eq!(row.observed_spend_nano, 50_000);
        assert_eq!(row.samples, 2);
        assert_eq!(row.sum_used_sq, 0);
        assert_eq!(row.sum_used_spend_nano, 0);
        assert_eq!(estimate.source, CapacitySource::WorkloadBlend);
    }

    #[test]
    fn mixed_models_publish_the_realized_api_dollar_blend_and_envelope() {
        let row = next(None, observation(1_000, 0, 100));
        let row = next(Some(row), observation(2_000, 100_000, 101)); // $10 regime
        let row = next(Some(row), observation(3_000, 300_000, 102)); // $20 regime
        let row = next(Some(row), observation(3_500, 700_000, 103)); // $80 regime
        let estimate = WindowCalibration::from_row(row.clone())
            .unwrap()
            .estimate()
            .unwrap();
        assert_eq!(estimate.capacity_nano, 28_000_000_000);
        assert!(estimate.low_nano.unwrap() < 10_100_000_000);
        assert!(estimate.high_nano.unwrap() > 79_800_000_000);
        assert_eq!(row.observed_fraction_units, 2_500);
        assert_eq!(row.observed_spend_nano, 700_000);
        assert_eq!(row.samples, 3);
        assert!(estimate.confidence_bp > 0);
    }

    #[test]
    fn percentage_waits_for_positive_settlement_evidence() {
        let row = next(None, observation(17_000_000, 126_000, 100));
        let row = next(Some(row), observation(17_000_100, 126_000, 101));
        assert_eq!(row.anchor_used_fraction_units, 17_000_000);
        let row = next(Some(row), observation(17_000_100, 131_000, 102));
        assert!(row.anchor_ready);
        assert_eq!(row.anchor_spend_nano, 131_000);
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(5_000_000_000));
        let row = next(Some(row), observation(17_000_200, 136_000, 103));
        assert_eq!(row.samples, 2);
        assert_eq!(row.current_capacity_nano, Some(5_000_000_000));
    }

    #[test]
    fn rollback_and_old_high_water_neither_erase_nor_duplicate_evidence() {
        let row = next(None, observation(10_000_000, 0, 100));
        let row = next(Some(row), observation(10_000_100, 10_000, 101));
        let row = next(Some(row), observation(10_000_200, 30_000, 102));
        let row = next(Some(row), observation(10_000_150, 35_000, 103));
        let row = next(Some(row), observation(10_000_200, 40_000, 104));
        let row = next(Some(row), observation(10_000_300, 50_000, 105));
        assert_eq!(row.samples, 3);
        assert_eq!(row.observed_fraction_units, 300);
        assert_eq!(row.observed_spend_nano, 50_000);
    }

    #[test]
    fn reset_jitter_stays_in_one_window_and_first_new_window_movement_counts() {
        let row = next(None, observation(10_000_000, 0, 100));
        let row = next(
            Some(row),
            observation_at(RESET + 3, DURATION, 10_000_100, 10_000, 101),
        );
        let row = next(
            Some(row),
            observation_at(RESET - 2, DURATION, 10_000_200, 30_000, 102),
        );
        assert_eq!(row.samples, 2);
        let row = next(
            Some(row),
            observation_at(RESET + DURATION * 60, DURATION, 100, 40_000, 103),
        );
        assert!(row.anchor_ready);
        assert_eq!(row.samples, 2);
        assert_eq!(row.current_capacity_nano, Some(15_000_000_000));
        let row = next(
            Some(row),
            observation_at(RESET + DURATION * 60, DURATION, 200, 50_000, 104),
        );
        assert_eq!(row.samples, 3);
        assert_eq!(row.current_capacity_nano, Some(13_333_333_333));
    }

    #[test]
    fn small_reset_jitter_does_not_turn_a_usage_rollback_into_a_new_window() {
        let row = next(None, observation(10_000_000, 0, 100));
        let row = next(
            Some(row),
            observation_at(RESET + 3, DURATION, 9_999_900, 5_000, 101),
        );
        assert_eq!(row.resets_at, RESET);
        assert_eq!(row.anchor_used_fraction_units, 10_000_000);
        assert_eq!(row.samples, 0);
        let row = next(
            Some(row),
            observation_at(RESET + 4, DURATION, 10_000_100, 20_000, 102),
        );
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(20_000_000_000));
    }

    #[test]
    fn estimator_upgrade_recovers_a_weekly_rollover_below_half_the_window() {
        const WEEKLY: i64 = 10_080;
        const LIVE_RESET_SHIFT: i64 = 247_336;
        assert!(LIVE_RESET_SHIFT < WEEKLY * 60 / 2);
        let new_reset = RESET + LIVE_RESET_SHIFT;
        let history = vec![
            observation_at(RESET, WEEKLY, FRACTION_SCALE, 0, 100),
            observation_at(new_reset, WEEKLY, 0, 0, 101),
            observation_at(new_reset, WEEKLY, 9_000_000, 219_960_000_000, 102),
            observation_at(new_reset, WEEKLY, 12_000_000, 277_840_000_000, 103),
        ];
        let mut poisoned = WindowCalibration::anchor(&history[0]).unwrap().into_row();
        poisoned.version = 9;
        poisoned.estimator_version = ESTIMATOR_VERSION - 1;
        poisoned.observed_at = history.last().unwrap().observed_at;
        poisoned.used_percent = history.last().unwrap().used_percent;
        poisoned.used_fraction_units = history.last().unwrap().used_fraction_units;

        let rebuilt =
            apply_observation_with_history(Some(poisoned), &history, history.last().unwrap())
                .unwrap();
        assert_eq!(rebuilt.version, 9);
        assert_eq!(rebuilt.resets_at, new_reset);
        assert_eq!(rebuilt.samples, 2);
        assert_eq!(rebuilt.observed_fraction_units, 12_000_000);
        assert_eq!(rebuilt.observed_spend_nano, 277_840_000_000);
        assert_eq!(rebuilt.current_capacity_nano, Some(2_315_333_333_333));
    }

    #[test]
    fn estimator_upgrade_replays_active_home_nine_to_twelve_percent() {
        let history = vec![
            observation(9_000_000, 219_960_000_000, 100),
            observation(12_000_000, 277_840_000_000, 101),
        ];
        let mut poisoned = replay_observations(&history[..1], 9).unwrap().unwrap();
        poisoned.estimator_version = ESTIMATOR_VERSION - 1;
        poisoned.current_capacity_nano = Some(1);
        let rebuilt =
            apply_observation_with_history(Some(poisoned), &history, history.last().unwrap())
                .unwrap();
        assert_eq!(rebuilt.version, 9);
        assert_eq!(rebuilt.samples, 1);
        assert_eq!(rebuilt.observed_fraction_units, 3_000_000);
        assert_eq!(rebuilt.observed_spend_nano, 57_880_000_000);
        assert_eq!(rebuilt.current_capacity_nano, Some(1_929_333_333_333));
        let calibration = WindowCalibration::from_row(rebuilt).unwrap();
        assert_eq!(
            calibration.remaining_nano(12_000_000),
            Some(1_697_813_333_333)
        );
    }

    #[test]
    fn remaining_and_its_bounds_use_the_exact_current_fraction() {
        let row = next(None, observation(10_000_000, 0, 100));
        let row = next(Some(row), observation(10_000_100, 20_000, 101));
        let row = next(Some(row), observation(10_000_300, 60_000, 102));
        let calibration = WindowCalibration::from_row(row).unwrap();
        assert_eq!(calibration.remaining_nano(50_000_000), Some(10_000_000_000));
        assert!(calibration.remaining_low_nano(50_000_000).is_some());
        assert!(calibration.remaining_high_nano(50_000_000).is_some());
    }

    #[test]
    fn invalid_or_overflowing_state_fails_closed() {
        let mut hostile = observation(FRACTION_SCALE + 1, 0, 100);
        hostile.used_percent = 100;
        assert!(apply_observation(None, &hostile).is_err());

        let row = next(None, observation(1, 1, 100));
        let row = next(Some(row), observation(2, 2, 101));
        let mut row = next(Some(row), observation(3, 3, 102));
        row.observed_spend_nano = i64::MAX;
        assert!(apply_observation(Some(row), &observation(4, 4, 103)).is_err());

        let mut noncanonical = next(None, observation(1, 1, 100));
        noncanonical.anchor_ready = false;
        assert!(apply_observation(Some(noncanonical), &observation(2, 2, 101)).is_err());
    }

    #[test]
    fn independent_durations_do_not_share_state() {
        let five = next(None, observation(1, 1, 100));
        let weekly = next(None, observation_at(RESET, 10_080, 1, 1, 100));
        assert_ne!(five.window_duration_mins, weekly.window_duration_mins);
    }
}
