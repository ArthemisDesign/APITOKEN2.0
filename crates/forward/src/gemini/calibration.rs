//! Evidence-only calibration of the two explicit Antigravity Gemini subscription windows.
//!
//! Every quota snapshot is paired with cumulative exact official-API-price spend for the same
//! opaque profile and paid plan. The first complete positive interval publishes an estimate:
//!
//! `capacity_nano = SCALE * ΣΔspend_nano / ΣΔused_units`.
//!
//! Bounds use the decimal resolution of both interval endpoints. Quota movement observed before
//! its turn event settles is allowed one later snapshot to catch up; repeated quota-only movement
//! is recorded as unattributed and excluded. State upgrades replay immutable observations. There
//! is no configured prior, subscription-price assumption, EMA, WLS, or floating-point money.

use anyhow::Context as _;
use registry::{GeminiExactCalibrationRow, GeminiExactWindowObservation};

pub const FRACTION_SCALE: i64 = 100_000_000;
pub const ESTIMATOR_VERSION: i64 = 3;
pub const BUCKET_5H: &str = "gemini-5h";
pub const BUCKET_WEEKLY: &str = "gemini-weekly";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BucketContract {
    pub id: &'static str,
    pub kind: &'static str,
    pub duration_mins: i64,
}

pub(crate) const FIVE_HOUR: BucketContract = BucketContract {
    id: BUCKET_5H,
    kind: "5h",
    duration_mins: 300,
};
pub(crate) const WEEKLY: BucketContract = BucketContract {
    id: BUCKET_WEEKLY,
    kind: "weekly",
    duration_mins: 10_080,
};

pub(crate) fn bucket_contract(bucket_id: &str) -> Option<BucketContract> {
    match bucket_id {
        BUCKET_5H => Some(FIVE_HOUR),
        BUCKET_WEEKLY => Some(WEEKLY),
        _ => None,
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCalibration {
    row: GeminiExactCalibrationRow,
}

impl WindowCalibration {
    fn anchor(observation: &GeminiExactWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        Ok(Self {
            row: GeminiExactCalibrationRow {
                profile_id: observation.profile_id.clone(),
                plan: observation.plan.clone(),
                bucket_id: observation.bucket_id.clone(),
                window_kind: observation.window_kind.clone(),
                window_duration_mins: observation.window_duration_mins,
                resets_at: observation.resets_at,
                anchor_used_fraction_units: observation.used_fraction_units,
                anchor_resolution_fraction_units: observation.measurement_resolution_fraction_units,
                anchor_spend_nano: observation.gateway_spend_nano,
                used_fraction_units: observation.used_fraction_units,
                measurement_resolution_fraction_units: observation
                    .measurement_resolution_fraction_units,
                observed_at: observation.observed_at,
                observed_fraction_units: 0,
                observed_spend_nano: 0,
                samples: 0,
                unattributed_fraction_units: 0,
                current_capacity_nano: None,
                current_low_nano: None,
                current_high_nano: None,
                current_confidence_bp: 0,
                last_measured_at: None,
                estimator_version: ESTIMATOR_VERSION,
                version: 0,
                updated_ts: observation.observed_at,
            },
        })
    }

    pub fn from_row(row: GeminiExactCalibrationRow) -> anyhow::Result<Self> {
        validate_row(&row)?;
        Ok(Self { row })
    }

    pub fn into_row(self) -> GeminiExactCalibrationRow {
        self.row
    }

    pub fn row(&self) -> &GeminiExactCalibrationRow {
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

    fn observe(&mut self, observation: &GeminiExactWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        if observation.profile_id != self.row.profile_id
            || observation.plan != self.row.plan
            || observation.bucket_id != self.row.bucket_id
            || observation.window_kind != self.row.window_kind
            || observation.window_duration_mins != self.row.window_duration_mins
        {
            anyhow::bail!("Gemini calibration observation identity mismatch");
        }
        // FIFO order is authoritative. Equal-second response and poll snapshots may carry distinct
        // state; exact duplicates are filtered transactionally by the persistence layer.
        if observation.observed_at < self.row.observed_at {
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Gemini calibration estimator version mismatch");
        }
        if observation.gateway_spend_nano < self.row.anchor_spend_nano {
            anyhow::bail!("Gemini cumulative calibration spend regressed");
        }

        let reset_delta = i128::from(observation.resets_at) - i128::from(self.row.resets_at);
        let duration_secs = i128::from(self.row.window_duration_mins) * 60;
        let reset_boundary = (duration_secs / 2).max(1);
        let jitter_tolerance = (duration_secs / 200).clamp(1, 3_600);
        let rolling_window_rollover = observation.used_fraction_units
            < self.row.anchor_used_fraction_units
            && reset_delta >= jitter_tolerance;
        if reset_delta >= reset_boundary || rolling_window_rollover {
            self.begin_window(observation);
            return Ok(());
        }
        if reset_delta <= -reset_boundary {
            return Ok(());
        }

        let previous_seen_used = self.row.used_fraction_units;
        let previous_seen_at = self.row.observed_at;
        self.row.resets_at = self.row.resets_at.max(observation.resets_at);
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.measurement_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;

        // Only a strictly higher high-water delimits a new consumption interval.
        let delta_used = observation.used_fraction_units - self.row.anchor_used_fraction_units;
        if delta_used <= 0 {
            return Ok(());
        }
        let delta_spend = observation.gateway_spend_nano - self.row.anchor_spend_nano;

        // Google can expose quota movement before the successful turn is durable. Keep the anchor
        // once; seeing the same higher point again with no spend proves unattributed movement.
        if delta_spend == 0 {
            if previous_seen_used == observation.used_fraction_units
                && previous_seen_at < observation.observed_at
            {
                self.row.unattributed_fraction_units = self
                    .row
                    .unattributed_fraction_units
                    .checked_add(delta_used)
                    .context("Gemini unattributed fraction overflow")?;
                self.advance_anchor(observation);
            }
            return Ok(());
        }

        let uncertainty = interval_fraction_uncertainty(
            self.row.anchor_resolution_fraction_units,
            observation.measurement_resolution_fraction_units,
        );
        self.update_workload_envelope(delta_used, uncertainty, delta_spend)?;
        self.row.observed_fraction_units = self
            .row
            .observed_fraction_units
            .checked_add(delta_used)
            .context("Gemini observed fraction overflow")?;
        self.row.observed_spend_nano = self
            .row
            .observed_spend_nano
            .checked_add(delta_spend)
            .context("Gemini observed spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Gemini calibration sample overflow")?;
        self.advance_anchor(observation);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &GeminiExactWindowObservation) {
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.anchor_spend_nano = observation.gateway_spend_nano;
    }

    fn begin_window(&mut self, observation: &GeminiExactWindowObservation) {
        self.row.resets_at = observation.resets_at;
        self.advance_anchor(observation);
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.measurement_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
    }

    fn update_workload_envelope(
        &mut self,
        delta_used: i64,
        uncertainty: i64,
        delta_spend: i64,
    ) -> anyhow::Result<()> {
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(i128::from(delta_spend))
            .context("Gemini workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + i128::from(uncertainty))
                .context("Gemini workload low bound overflow")?,
        )?;
        self.row.current_low_nano = Some(
            self.row
                .current_low_nano
                .map_or(low, |existing| existing.min(low)),
        );
        let sample_high = if delta_used > uncertainty {
            Some(checked_i64(ceil_nonnegative(
                numerator,
                i128::from(delta_used) - i128::from(uncertainty),
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
            .context("Gemini capacity numerator overflow")?;
        self.row.current_capacity_nano =
            Some(checked_i64(round_nonnegative(numerator, denominator)?)?);

        let samples = i128::from(self.row.samples);
        let maturity_bp = ratio_bp(samples, samples + 2)?;
        let stability_bp = match (self.row.current_low_nano, self.row.current_high_nano) {
            (Some(low), Some(high)) if high > 0 => ratio_bp(i128::from(low), i128::from(high))?,
            _ => 0,
        };
        let resolution = i128::from(
            self.row
                .measurement_resolution_fraction_units
                .max(self.row.anchor_resolution_fraction_units),
        );
        let quantisation_denominator = denominator
            .checked_add(
                resolution
                    .checked_mul(2)
                    .and_then(|value| value.checked_mul(samples))
                    .context("Gemini confidence resolution overflow")?,
            )
            .context("Gemini confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(denominator, quantisation_denominator)?;
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("Gemini confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

pub(crate) fn apply_observation(
    existing: Option<GeminiExactCalibrationRow>,
    observation: &GeminiExactWindowObservation,
) -> anyhow::Result<GeminiExactCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            WindowCalibration::from_row(row)?
        }
        _ => WindowCalibration::anchor(observation)?,
    };
    calibration.row.version = version;
    if calibration.row.observed_at <= observation.observed_at {
        calibration.observe(observation)?;
    }
    Ok(calibration.into_row())
}

fn replay_observations(
    observations: &[GeminiExactWindowObservation],
    version: i64,
) -> anyhow::Result<Option<GeminiExactCalibrationRow>> {
    let Some(first) = observations.first() else {
        return Ok(None);
    };
    let mut calibration = WindowCalibration::anchor(first)?;
    calibration.row.version = version;
    for observation in &observations[1..] {
        if observation.profile_id != first.profile_id
            || observation.plan != first.plan
            || observation.bucket_id != first.bucket_id
            || observation.window_kind != first.window_kind
            || observation.window_duration_mins != first.window_duration_mins
        {
            anyhow::bail!("mixed identity in Gemini calibration history");
        }
        calibration.observe(observation)?;
    }
    Ok(Some(calibration.into_row()))
}

pub(crate) fn apply_observation_with_history(
    existing: Option<GeminiExactCalibrationRow>,
    history: &[GeminiExactWindowObservation],
    observation: &GeminiExactWindowObservation,
) -> anyhow::Result<GeminiExactCalibrationRow> {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let rebuilt = replay_observations(history, version)?
        .context("missing immutable history for Gemini estimator rebuild")?;
    apply_observation(Some(rebuilt), observation)
}

fn validate_observation(observation: &GeminiExactWindowObservation) -> anyhow::Result<()> {
    let contract =
        bucket_contract(&observation.bucket_id).context("unknown Gemini calibration bucket")?;
    let valid_source = match observation.observation_source.as_str() {
        "response" => observation
            .source_request_id
            .as_ref()
            .is_some_and(|request_id| !request_id.is_empty()),
        "poll" => observation.source_request_id.is_none(),
        _ => false,
    };
    if observation.profile_id.is_empty()
        || observation.plan.is_empty()
        || observation.window_kind != contract.kind
        || observation.window_duration_mins != contract.duration_mins
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&observation.used_fraction_units)
        || !(1..=FRACTION_SCALE).contains(&observation.measurement_resolution_fraction_units)
        || observation.gateway_spend_nano < 0
        || !valid_source
    {
        anyhow::bail!("invalid Gemini calibration observation");
    }
    Ok(())
}

fn validate_row(row: &GeminiExactCalibrationRow) -> anyhow::Result<()> {
    registry::validate_gemini_exact_calibration_row(row)?;
    if row.estimator_version != ESTIMATOR_VERSION {
        anyhow::bail!("Gemini calibration estimator version mismatch");
    }
    Ok(())
}

fn interval_fraction_uncertainty(anchor_resolution: i64, observed_resolution: i64) -> i64 {
    anchor_resolution
        .saturating_add(observed_resolution)
        .saturating_add(1)
        .saturating_div(2)
        .max(1)
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("Gemini capacity rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("Gemini capacity ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    10_000i128
        .checked_mul(numerator)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("Gemini confidence ratio overflow")
}

fn remaining_for_capacity(capacity_nano: i64, used_fraction_units: i64) -> Option<i64> {
    let unused = i128::from(FRACTION_SCALE - used_fraction_units.clamp(0, FRACTION_SCALE));
    let remaining = i128::from(capacity_nano)
        .checked_mul(unused)
        .and_then(|value| value.checked_div(i128::from(FRACTION_SCALE)))?;
    i64::try_from(remaining).ok()
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).context("Gemini calibration result exceeds bigint")
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "profile-a";
    const RESET: i64 = 2_000_000_000;

    fn observation(
        contract: BucketContract,
        used: i64,
        resolution: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> GeminiExactWindowObservation {
        GeminiExactWindowObservation {
            profile_id: PROFILE.to_owned(),
            plan: "ai-pro".to_owned(),
            bucket_id: contract.id.to_owned(),
            window_kind: contract.kind.to_owned(),
            window_duration_mins: contract.duration_mins,
            resets_at: RESET,
            observed_at,
            used_fraction_units: used,
            measurement_resolution_fraction_units: resolution,
            gateway_spend_nano: spend_nano,
            observation_source: "response".to_owned(),
            source_request_id: Some(format!("request-{observed_at}")),
        }
    }

    fn next(
        row: Option<GeminiExactCalibrationRow>,
        observation: GeminiExactWindowObservation,
    ) -> GeminiExactCalibrationRow {
        let mut row = apply_observation(row, &observation).unwrap();
        row.version += 1;
        row
    }

    #[test]
    fn first_complete_positive_interval_publishes_exact_blend_without_prior() {
        let row = next(
            None,
            observation(FIVE_HOUR, 10_000_000, 100_000, 1_000, 100),
        );
        assert!(row.current_capacity_nano.is_none());
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 20_000_000, 100_000, 2_001_000, 101),
        );
        assert_eq!(row.current_capacity_nano, Some(20_000_000));
        assert_eq!(row.observed_fraction_units, 10_000_000);
        assert_eq!(row.observed_spend_nano, 2_000_000);
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn decimal_endpoint_resolution_widens_bounds_and_can_remove_finite_high() {
        let row = next(
            None,
            observation(FIVE_HOUR, 10_000_000, 1_000_000, 1_000, 100),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 11_000_000, 1_000_000, 2_000, 101),
        );
        assert_eq!(row.current_capacity_nano, Some(100_000));
        assert_eq!(row.current_low_nano, Some(50_000));
        assert!(row.current_high_nano.is_none());
        assert_eq!(row.current_confidence_bp, 0);
    }

    #[test]
    fn resolution_envelope_contains_each_admissible_true_boundary() {
        for (anchor_used, anchor_resolution, observed_used, observed_resolution) in [
            (12_000_000, 1_000_000, 14_000_000, 1_000_000),
            (12_500_000, 100_000, 12_625_000, 1_000),
            (12_125_001, 1, 12_125_101, 1),
        ] {
            let reported_delta = observed_used - anchor_used;
            let uncertainty = interval_fraction_uncertainty(anchor_resolution, observed_resolution);
            for true_delta in [
                (reported_delta - uncertainty).max(1),
                reported_delta,
                reported_delta + uncertainty,
            ] {
                let row = next(
                    None,
                    observation(FIVE_HOUR, anchor_used, anchor_resolution, 0, 100),
                );
                let row = next(
                    Some(row),
                    observation(
                        FIVE_HOUR,
                        observed_used,
                        observed_resolution,
                        true_delta,
                        101,
                    ),
                );
                let estimate = WindowCalibration::from_row(row)
                    .unwrap()
                    .estimate()
                    .unwrap();
                assert!(
                    estimate.low_nano.unwrap() <= FRACTION_SCALE,
                    "low excluded the hidden true capacity for {anchor_used}->{observed_used}"
                );
                if let Some(high) = estimate.high_nano {
                    assert!(
                        high >= FRACTION_SCALE,
                        "high excluded the hidden true capacity for {anchor_used}->{observed_used}"
                    );
                } else {
                    assert!(reported_delta <= uncertainty);
                }
            }
        }
    }

    #[test]
    fn multiple_mixed_workload_intervals_publish_the_realized_blend_and_envelope() {
        let row = next(None, observation(FIVE_HOUR, 1_001, 1, 0, 100));
        let row = next(Some(row), observation(FIVE_HOUR, 2_001, 1, 100_000, 101));
        let row = next(Some(row), observation(FIVE_HOUR, 3_001, 1, 300_000, 102));
        let row = next(Some(row), observation(FIVE_HOUR, 3_501, 1, 700_000, 103));
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
    fn quota_before_settlement_gets_one_snapshot_lag_then_becomes_unattributed() {
        let row = next(
            None,
            observation(FIVE_HOUR, 10_000_000, 100_000, 1_000, 100),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 20_000_000, 100_000, 1_000, 101),
        );
        assert_eq!(row.unattributed_fraction_units, 0);
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 20_000_000, 100_000, 1_000, 102),
        );
        assert_eq!(row.unattributed_fraction_units, 10_000_000);
        assert_eq!(row.anchor_used_fraction_units, 20_000_000);
        assert!(row.current_capacity_nano.is_none());
    }

    #[test]
    fn equal_second_settlement_catch_up_completes_the_pending_interval() {
        let row = next(
            None,
            observation(FIVE_HOUR, 10_000_000, 100_000, 1_000, 100),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 20_000_000, 100_000, 1_000, 101),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 20_000_000, 100_000, 2_001_000, 101),
        );
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(20_000_000));
        assert_eq!(row.unattributed_fraction_units, 0);
    }

    #[test]
    fn reset_jitter_does_not_split_but_usage_drop_with_reset_advance_does() {
        let row = next(
            None,
            observation(FIVE_HOUR, 30_000_000, 100_000, 1_000, 100),
        );
        let mut jitter = observation(FIVE_HOUR, 40_000_000, 100_000, 2_000, 101);
        jitter.resets_at += 30;
        let row = next(Some(row), jitter);
        assert_eq!(row.samples, 1);

        let mut rolled = observation(FIVE_HOUR, 5_000_000, 100_000, 3_000, 102);
        rolled.resets_at += 120;
        let row = next(Some(row), rolled);
        assert_eq!(row.anchor_used_fraction_units, 5_000_000);
        assert_eq!(row.resets_at, RESET + 120);
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn rollback_and_return_to_old_high_water_do_not_duplicate_fraction_evidence() {
        let row = next(None, observation(FIVE_HOUR, 10_000_000, 1, 0, 100));
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_100, 1, 10_000, 101),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_200, 1, 30_000, 102),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_150, 1, 35_000, 103),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_200, 1, 40_000, 104),
        );
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_300, 1, 50_000, 105),
        );
        assert_eq!(row.samples, 3);
        assert_eq!(row.observed_fraction_units, 300);
        assert_eq!(row.observed_spend_nano, 50_000);
    }

    #[test]
    fn exact_remaining_and_bounds_use_the_current_fraction() {
        let row = next(None, observation(FIVE_HOUR, 10_000_001, 1, 0, 100));
        let row = next(
            Some(row),
            observation(FIVE_HOUR, 10_000_101, 1, 20_000, 101),
        );
        let calibration = WindowCalibration::from_row(row).unwrap();
        assert_eq!(calibration.remaining_nano(50_000_000), Some(10_000_000_000));
        assert_eq!(
            calibration.remaining_low_nano(50_000_000),
            Some(9_900_990_099)
        );
        assert_eq!(
            calibration.remaining_high_nano(50_000_000),
            Some(10_101_010_101)
        );
    }

    #[test]
    fn plan_and_bucket_are_independent_identity_dimensions() {
        let five = next(None, observation(FIVE_HOUR, 1, 1, 1, 100));
        let weekly = next(None, observation(WEEKLY, 1, 1, 1, 100));
        assert_ne!(five.bucket_id, weekly.bucket_id);
        let mut different_plan = observation(FIVE_HOUR, 2, 1, 2, 101);
        different_plan.plan = "ultra".to_owned();
        assert!(apply_observation(Some(five), &different_plan).is_err());
    }

    #[test]
    fn stale_and_exact_equal_second_observations_do_not_mutate_state() {
        let first = observation(FIVE_HOUR, 10_000_000, 1, 0, 100);
        let second = observation(FIVE_HOUR, 20_000_000, 1, 2_000_000, 101);
        let row = next(None, first.clone());
        let row = next(Some(row), second.clone());

        let stale = apply_observation(Some(row.clone()), &first).unwrap();
        assert_eq!(stale, row);
        let duplicate = apply_observation(Some(row.clone()), &second).unwrap();
        assert_eq!(duplicate, row);
    }

    #[test]
    fn invalid_observations_regression_and_overflow_fail_closed() {
        let valid = observation(FIVE_HOUR, 10_000_000, 1, 1_000, 100);
        for invalid in [
            GeminiExactWindowObservation {
                plan: String::new(),
                ..valid.clone()
            },
            GeminiExactWindowObservation {
                window_duration_mins: 301,
                ..valid.clone()
            },
            GeminiExactWindowObservation {
                measurement_resolution_fraction_units: 0,
                ..valid.clone()
            },
            GeminiExactWindowObservation {
                observation_source: "poll".to_owned(),
                ..valid.clone()
            },
            GeminiExactWindowObservation {
                gateway_spend_nano: -1,
                ..valid.clone()
            },
        ] {
            assert!(apply_observation(None, &invalid).is_err());
        }

        let row = next(None, valid);
        assert!(apply_observation(
            Some(row.clone()),
            &observation(FIVE_HOUR, 20_000_000, 1, 999, 101)
        )
        .is_err());
        assert!(apply_observation(
            Some(row),
            &observation(FIVE_HOUR, 10_000_001, 1, i64::MAX, 101)
        )
        .is_err());
    }

    #[test]
    fn estimator_rebuild_requires_complete_single_identity_history() {
        let first = observation(FIVE_HOUR, 10_000_000, 1, 1_000, 100);
        let mut stale = next(None, first.clone());
        stale.estimator_version = ESTIMATOR_VERSION - 1;

        assert!(apply_observation_with_history(Some(stale.clone()), &[], &first).is_err());

        let mut other_plan = observation(FIVE_HOUR, 20_000_000, 1, 2_000, 101);
        other_plan.plan = "ultra".to_owned();
        assert!(
            apply_observation_with_history(Some(stale), &[first.clone(), other_plan], &first)
                .is_err()
        );
    }

    #[test]
    fn estimator_upgrade_replays_immutable_history() {
        let first = observation(FIVE_HOUR, 10_000_000, 100_000, 1_000, 100);
        let second = observation(FIVE_HOUR, 20_000_000, 100_000, 2_001_000, 101);
        let mut stale = next(None, first.clone());
        stale = next(Some(stale), second.clone());
        stale.estimator_version = ESTIMATOR_VERSION + 100;
        stale.current_capacity_nano = Some(1);
        stale.current_low_nano = Some(1);
        stale.current_high_nano = Some(1);
        stale.version = 17;

        let rebuilt = apply_observation_with_history(
            Some(stale),
            &[first, second.clone()],
            &GeminiExactWindowObservation {
                observed_at: 102,
                source_request_id: Some("request-102".to_owned()),
                ..second
            },
        )
        .unwrap();

        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(rebuilt.version, 17);
        assert_eq!(rebuilt.current_capacity_nano, Some(20_000_000));
        assert_eq!(rebuilt.samples, 1);
    }
}
