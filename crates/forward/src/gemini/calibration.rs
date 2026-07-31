//! Evidence-only calibration of the two explicit Antigravity Gemini subscription windows.
//!
//! `retrieveUserQuotaSummary` reports remaining fractions with 10^-8 resolution. The gateway
//! pairs those snapshots with cumulative exact official-API-price spend for the same opaque
//! profile. A cold snapshot is only an anchor, and the first movement is censored because that
//! anchor may have arrived part-way through one quantisation cell. Every later positive movement
//! with positive settled spend contributes to cumulative weighted least squares through origin:
//!
//! `capacity_nano = SCALE * Σ(Δused_units * Δspend_nano) / Σ(Δused_units²)`.
//!
//! There is no configured prior, subscription-price assumption, EMA or float arithmetic.

use registry::{GeminiCalibrationRow, GeminiWindowObservation};

pub const FRACTION_SCALE: i64 = 100_000_000;
pub const ESTIMATOR_VERSION: i64 = 1;
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowCalibration {
    row: GeminiCalibrationRow,
}

impl WindowCalibration {
    fn anchor(observation: &GeminiWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        Ok(Self {
            row: GeminiCalibrationRow {
                profile_id: observation.profile_id.clone(),
                bucket_id: observation.bucket_id.clone(),
                window_kind: observation.window_kind.clone(),
                window_duration_mins: observation.window_duration_mins,
                resets_at: observation.resets_at,
                anchor_used_fraction_units: observation.used_fraction_units,
                anchor_spend_nano: observation.gateway_spend_nano,
                anchor_ready: false,
                used_fraction_units: observation.used_fraction_units,
                observed_at: observation.observed_at,
                sum_used_sq: "0".to_string(),
                sum_used_spend_nano: "0".to_string(),
                observed_fraction_units: 0,
                samples: 0,
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

    pub fn from_row(row: GeminiCalibrationRow) -> anyhow::Result<Self> {
        validate_row(&row)?;
        Ok(Self { row })
    }

    pub fn into_row(self) -> GeminiCalibrationRow {
        self.row
    }

    pub fn row(&self) -> &GeminiCalibrationRow {
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

    pub fn remaining_nano(&self, used_fraction_units: i64) -> Option<i64> {
        self.estimate().and_then(|estimate| {
            let unused = i128::from(FRACTION_SCALE - used_fraction_units.clamp(0, FRACTION_SCALE));
            let remaining = i128::from(estimate.capacity_nano)
                .checked_mul(unused)
                .and_then(|value| value.checked_div(i128::from(FRACTION_SCALE)))?;
            i64::try_from(remaining).ok()
        })
    }

    fn observe(&mut self, observation: &GeminiWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        if observation.profile_id != self.row.profile_id
            || observation.bucket_id != self.row.bucket_id
            || observation.window_kind != self.row.window_kind
            || observation.window_duration_mins != self.row.window_duration_mins
        {
            anyhow::bail!("Gemini calibration observation identity mismatch");
        }
        if observation.observed_at <= self.row.observed_at {
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Gemini calibration estimator version mismatch");
        }
        if observation.resets_at > self.row.resets_at {
            self.begin_window(observation);
            return Ok(());
        }
        if observation.resets_at < self.row.resets_at {
            return Ok(());
        }

        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
        self.row.used_fraction_units = observation.used_fraction_units;
        let delta_used = observation.used_fraction_units - self.row.anchor_used_fraction_units;
        if delta_used <= 0 || observation.gateway_spend_nano < self.row.anchor_spend_nano {
            return Ok(());
        }
        let delta_spend = observation.gateway_spend_nano - self.row.anchor_spend_nano;
        if delta_spend == 0 {
            return Ok(());
        }

        if !self.row.anchor_ready {
            self.row.anchor_used_fraction_units = observation.used_fraction_units;
            self.row.anchor_spend_nano = observation.gateway_spend_nano;
            self.row.anchor_ready = true;
            return Ok(());
        }

        let x = i128::from(delta_used);
        let y = i128::from(delta_spend);
        let sum_used_sq = parse_accumulator(&self.row.sum_used_sq)?
            .checked_add(x.checked_mul(x).context("Gemini WLS x² overflow")?)
            .context("Gemini WLS sum x² overflow")?;
        let sum_used_spend = parse_accumulator(&self.row.sum_used_spend_nano)?
            .checked_add(x.checked_mul(y).context("Gemini WLS x*y overflow")?)
            .context("Gemini WLS sum x*y overflow")?;
        self.row.sum_used_sq = sum_used_sq.to_string();
        self.row.sum_used_spend_nano = sum_used_spend.to_string();
        self.row.observed_fraction_units = self
            .row
            .observed_fraction_units
            .checked_add(delta_used)
            .context("Gemini observed fraction overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Gemini calibration sample overflow")?;
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_spend_nano = observation.gateway_spend_nano;
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn begin_window(&mut self, observation: &GeminiWindowObservation) {
        self.row.resets_at = observation.resets_at;
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_spend_nano = observation.gateway_spend_nano;
        self.row.anchor_ready = false;
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
    }

    fn recompute(&mut self) -> anyhow::Result<()> {
        let denominator = parse_accumulator(&self.row.sum_used_sq)?;
        if denominator <= 0 {
            return Ok(());
        }
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(parse_accumulator(&self.row.sum_used_spend_nano)?)
            .context("Gemini capacity numerator overflow")?;
        let capacity = checked_i64(round_nonnegative(numerator, denominator)?)?;
        self.row.current_capacity_nano = Some(capacity);

        // Every delta boundary carries at most one fixed-point unit of provider quantisation.
        // Bounds therefore tighten from raw observed movement, not elapsed time or a tariff prior.
        let movement = i128::from(self.row.observed_fraction_units);
        let samples = i128::from(self.row.samples);
        let capacity_i128 = i128::from(capacity);
        let low = capacity_i128
            .checked_mul(movement)
            .and_then(|value| value.checked_div((movement + samples).max(1)))
            .context("Gemini capacity low bound overflow")?;
        self.row.current_low_nano = Some(checked_i64(low)?);
        self.row.current_high_nano = if movement > samples {
            let high = capacity_i128
                .checked_mul(movement)
                .and_then(|value| value.checked_div(movement - samples))
                .context("Gemini capacity high bound overflow")?;
            Some(checked_i64(high)?)
        } else {
            None
        };
        let confidence = 10_000i128
            .checked_mul(movement)
            .and_then(|value| value.checked_div((movement + 2 * samples).max(1)))
            .context("Gemini confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence)?.clamp(0, 10_000);
        Ok(())
    }
}

pub(crate) fn apply_observation(
    existing: Option<GeminiCalibrationRow>,
    observation: &GeminiWindowObservation,
) -> anyhow::Result<GeminiCalibrationRow> {
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
    observations: &[GeminiWindowObservation],
    version: i64,
) -> anyhow::Result<Option<GeminiCalibrationRow>> {
    let Some(first) = observations.first() else {
        return Ok(None);
    };
    let mut calibration = WindowCalibration::anchor(first)?;
    calibration.row.version = version;
    for observation in &observations[1..] {
        if observation.profile_id == first.profile_id
            && observation.bucket_id == first.bucket_id
            && observation.window_kind == first.window_kind
            && observation.window_duration_mins == first.window_duration_mins
        {
            calibration.observe(observation)?;
        }
    }
    Ok(Some(calibration.into_row()))
}

pub(crate) fn apply_observation_with_history(
    existing: Option<GeminiCalibrationRow>,
    history: &[GeminiWindowObservation],
    observation: &GeminiWindowObservation,
) -> anyhow::Result<GeminiCalibrationRow> {
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

fn validate_observation(observation: &GeminiWindowObservation) -> anyhow::Result<()> {
    let contract =
        bucket_contract(&observation.bucket_id).context("unknown Gemini calibration bucket")?;
    if observation.profile_id.is_empty()
        || observation.window_kind != contract.kind
        || observation.window_duration_mins != contract.duration_mins
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&observation.used_fraction_units)
        || observation.gateway_spend_nano < 0
    {
        anyhow::bail!("invalid Gemini calibration observation");
    }
    Ok(())
}

fn validate_row(row: &GeminiCalibrationRow) -> anyhow::Result<()> {
    let contract = bucket_contract(&row.bucket_id).context("unknown Gemini calibration bucket")?;
    if row.profile_id.is_empty()
        || row.window_kind != contract.kind
        || row.window_duration_mins != contract.duration_mins
        || row.resets_at <= 0
        || row.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&row.anchor_used_fraction_units)
        || !(0..=FRACTION_SCALE).contains(&row.used_fraction_units)
        || row.anchor_spend_nano < 0
        || row.observed_fraction_units < 0
        || row.samples < 0
        || row.estimator_version <= 0
        || row.version < 0
    {
        anyhow::bail!("invalid Gemini calibration row");
    }
    parse_accumulator(&row.sum_used_sq)?;
    parse_accumulator(&row.sum_used_spend_nano)?;
    Ok(())
}

fn parse_accumulator(value: &str) -> anyhow::Result<i128> {
    let parsed = value
        .parse::<i128>()
        .context("parse Gemini calibration accumulator")?;
    if parsed < 0 || parsed.to_string() != value {
        anyhow::bail!("Gemini calibration accumulator is not canonical");
    }
    Ok(parsed)
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

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).context("Gemini calibration result exceeds bigint")
}

use anyhow::Context as _;

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = "profile-a";
    const RESET: i64 = 2_000_000_000;

    fn observation(
        contract: BucketContract,
        used: i64,
        spend_nano: i64,
        observed_at: i64,
    ) -> GeminiWindowObservation {
        GeminiWindowObservation {
            profile_id: PROFILE.to_string(),
            bucket_id: contract.id.to_string(),
            window_kind: contract.kind.to_string(),
            window_duration_mins: contract.duration_mins,
            resets_at: RESET,
            observed_at,
            used_fraction_units: used,
            gateway_spend_nano: spend_nano,
        }
    }

    fn next(
        row: Option<GeminiCalibrationRow>,
        observation: GeminiWindowObservation,
    ) -> GeminiCalibrationRow {
        let mut row = apply_observation(row, &observation).unwrap();
        row.version += 1;
        row
    }

    #[test]
    fn cold_anchor_and_first_movement_publish_no_prior() {
        let row = next(None, observation(FIVE_HOUR, 1_000, 10_000, 100));
        assert!(WindowCalibration::from_row(row.clone())
            .unwrap()
            .estimate()
            .is_none());
        let row = next(Some(row), observation(FIVE_HOUR, 2_000, 30_000, 101));
        assert!(row.anchor_ready);
        assert_eq!(row.samples, 0);
        assert!(row.current_capacity_nano.is_none());
    }

    #[test]
    fn second_complete_interval_produces_exact_fixed_point_capacity() {
        let row = next(None, observation(FIVE_HOUR, 1_000, 10_000, 100));
        let row = next(Some(row), observation(FIVE_HOUR, 2_000, 30_000, 101));
        let row = next(Some(row), observation(FIVE_HOUR, 3_000, 50_000, 102));
        // 100,000,000 * (1,000 * 20,000) / 1,000² = 2,000,000,000 nanoUSD.
        assert_eq!(row.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(row.samples, 1);
        assert_eq!(row.sum_used_sq, "1000000");
        assert_eq!(row.sum_used_spend_nano, "20000000");
    }

    #[test]
    fn exact_probe_example_matches_the_observed_five_hour_capacity() {
        let before_used = FRACTION_SCALE - 99_998_030;
        let after_used = FRACTION_SCALE - 99_913_200;
        let row = next(None, observation(FIVE_HOUR, 0, 0, 100));
        let row = next(Some(row), observation(FIVE_HOUR, before_used, 1, 101));
        let row = next(
            Some(row),
            observation(FIVE_HOUR, after_used, 19_404_001, 102),
        );
        let estimate = WindowCalibration::from_row(row)
            .unwrap()
            .estimate()
            .unwrap();
        assert_eq!(estimate.capacity_nano, 22_873_983_261);
        assert!(estimate.confidence_bp >= 9_999);
    }

    #[test]
    fn five_hour_and_weekly_evidence_are_independent() {
        let five = next(None, observation(FIVE_HOUR, 1, 1, 100));
        let weekly = next(None, observation(WEEKLY, 1, 1, 100));
        assert_ne!(five.bucket_id, weekly.bucket_id);
        assert_ne!(five.window_duration_mins, weekly.window_duration_mins);
    }

    #[test]
    fn reset_preserves_cumulative_estimate_and_rearms_censoring() {
        let row = next(None, observation(FIVE_HOUR, 1_000, 10_000, 100));
        let row = next(Some(row), observation(FIVE_HOUR, 2_000, 30_000, 101));
        let row = next(Some(row), observation(FIVE_HOUR, 3_000, 50_000, 102));
        let mut reset = observation(FIVE_HOUR, 10, 60_000, 103);
        reset.resets_at = RESET + 300;
        let row = next(Some(row), reset);
        assert!(!row.anchor_ready);
        assert_eq!(row.current_capacity_nano, Some(2_000_000_000));
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn noncanonical_or_overflowing_state_fails_closed() {
        let mut row = next(None, observation(FIVE_HOUR, 1, 1, 100));
        row.sum_used_sq = "01".to_string();
        assert!(WindowCalibration::from_row(row).is_err());

        let mut row = next(None, observation(FIVE_HOUR, 1, 1, 100));
        row.anchor_ready = true;
        row.sum_used_sq = i128::MAX.to_string();
        assert!(apply_observation(Some(row), &observation(FIVE_HOUR, 2, 2, 101)).is_err());
    }
}
