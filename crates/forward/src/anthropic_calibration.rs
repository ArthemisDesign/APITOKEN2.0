//! Evidence-only calibration of one Anthropic subscription window.
//!
//! Anthropic reports a quota fraction and reset, but no native credit balance. We therefore pair
//! each exact fixed-point snapshot with cumulative official API-price spend for the same
//! subscription and publish only the realized workload blend:
//!
//! `capacity_nano = 100_000_000 * ΣΔspend_nano / ΣΔused_fraction_units`.
//!
//! There is no plan prior, subscription-price nominal, EMA, WLS, or floating-point money here.

use anyhow::Context as _;
use registry::{AnthropicCalibrationRow, AnthropicWindowObservation};

pub(crate) const FRACTION_SCALE: i64 = 100_000_000;
pub(crate) const ESTIMATOR_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WindowCalibration {
    row: AnthropicCalibrationRow,
}

impl WindowCalibration {
    fn anchor(observation: &AnthropicWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        Ok(Self {
            row: AnthropicCalibrationRow {
                subject_id: observation.subject_id.clone(),
                plan: observation.plan.clone(),
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

    pub(crate) fn from_row(row: AnthropicCalibrationRow) -> anyhow::Result<Self> {
        validate_row(&row)?;
        Ok(Self { row })
    }

    pub(crate) fn into_row(self) -> AnthropicCalibrationRow {
        self.row
    }

    fn observe(&mut self, observation: &AnthropicWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        if observation.subject_id != self.row.subject_id
            || observation.plan != self.row.plan
            || observation.window_kind != self.row.window_kind
            || observation.window_duration_mins != self.row.window_duration_mins
        {
            anyhow::bail!("Anthropic calibration observation identity mismatch");
        }
        if observation.observed_at <= self.row.observed_at {
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Anthropic calibration estimator version mismatch");
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

        let delta_used = observation.used_fraction_units - self.row.anchor_used_fraction_units;
        // A rollback and return to the old high-water is not new evidence. Only a strictly higher
        // utilization endpoint can delimit an interval.
        if delta_used <= 0 || observation.gateway_spend_nano < self.row.anchor_spend_nano {
            return Ok(());
        }
        let delta_spend = observation.gateway_spend_nano - self.row.anchor_spend_nano;

        // Response headers can arrive before the successful turn event advances cumulative spend.
        // Retain the anchor once. The same higher quota point seen again without spend is excluded
        // as unattributed movement so it can never inflate measured capacity.
        if delta_spend == 0 {
            if previous_seen_used == observation.used_fraction_units
                && previous_seen_at < observation.observed_at
            {
                self.row.unattributed_fraction_units = self
                    .row
                    .unattributed_fraction_units
                    .checked_add(delta_used)
                    .context("Anthropic unattributed fraction overflow")?;
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
            .context("Anthropic observed fraction overflow")?;
        self.row.observed_spend_nano = self
            .row
            .observed_spend_nano
            .checked_add(delta_spend)
            .context("Anthropic observed spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Anthropic calibration sample overflow")?;
        self.advance_anchor(observation);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &AnthropicWindowObservation) {
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.anchor_spend_nano = observation.gateway_spend_nano;
    }

    fn begin_window(&mut self, observation: &AnthropicWindowObservation) {
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
            .context("Anthropic workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + i128::from(uncertainty))
                .context("Anthropic workload low bound overflow")?,
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
            .context("Anthropic capacity numerator overflow")?;
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
                    .context("Anthropic confidence resolution overflow")?,
            )
            .context("Anthropic confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(denominator, quantisation_denominator)?;
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("Anthropic confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

pub(crate) fn apply_observation(
    existing: Option<AnthropicCalibrationRow>,
    observation: &AnthropicWindowObservation,
) -> anyhow::Result<AnthropicCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            WindowCalibration::from_row(row)?
        }
        _ => WindowCalibration::anchor(observation)?,
    };
    calibration.row.version = version;
    if calibration.row.observed_at < observation.observed_at {
        calibration.observe(observation)?;
    }
    Ok(calibration.into_row())
}

fn replay_observations(
    observations: &[AnthropicWindowObservation],
    version: i64,
) -> anyhow::Result<Option<AnthropicCalibrationRow>> {
    let Some(first) = observations.first() else {
        return Ok(None);
    };
    let mut calibration = WindowCalibration::anchor(first)?;
    calibration.row.version = version;
    for observation in &observations[1..] {
        if observation.subject_id == first.subject_id
            && observation.plan == first.plan
            && observation.window_kind == first.window_kind
            && observation.window_duration_mins == first.window_duration_mins
        {
            calibration.observe(observation)?;
        }
    }
    Ok(Some(calibration.into_row()))
}

pub(crate) fn apply_observation_with_history(
    existing: Option<AnthropicCalibrationRow>,
    history: &[AnthropicWindowObservation],
    observation: &AnthropicWindowObservation,
) -> anyhow::Result<AnthropicCalibrationRow> {
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

fn validate_observation(observation: &AnthropicWindowObservation) -> anyhow::Result<()> {
    let valid_window = matches!(
        (
            observation.window_kind.as_str(),
            observation.window_duration_mins
        ),
        ("5h", 300) | ("7d", 10_080)
    );
    let valid_source = match observation.observation_source.as_str() {
        "response" => observation
            .source_request_id
            .as_ref()
            .is_some_and(|request_id| !request_id.is_empty()),
        "poll" => observation.source_request_id.is_none(),
        _ => false,
    };
    if observation.subject_id.is_empty()
        || observation.plan.is_empty()
        || !valid_window
        || observation.resets_at <= 0
        || observation.observed_at <= 0
        || !(0..=FRACTION_SCALE).contains(&observation.used_fraction_units)
        || !(1..=FRACTION_SCALE).contains(&observation.measurement_resolution_fraction_units)
        || observation.gateway_spend_nano < 0
        || !valid_source
    {
        anyhow::bail!("invalid Anthropic calibration observation");
    }
    Ok(())
}

fn validate_row(row: &AnthropicCalibrationRow) -> anyhow::Result<()> {
    registry::validate_anthropic_calibration_row(row)?;
    if row.estimator_version != ESTIMATOR_VERSION {
        anyhow::bail!("Anthropic calibration estimator version mismatch");
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
        .context("Anthropic capacity rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("Anthropic capacity ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if numerator <= 0 || denominator <= 0 {
        return Ok(0);
    }
    10_000i128
        .checked_mul(numerator)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("Anthropic confidence ratio overflow")
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).context("Anthropic calibration result exceeds bigint")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUBJECT: &str = "sub@example.test";
    const RESET: i64 = 2_000_000_000;

    fn observation(
        used: i64,
        resolution: i64,
        spend: i64,
        observed_at: i64,
    ) -> AnthropicWindowObservation {
        AnthropicWindowObservation {
            subject_id: SUBJECT.into(),
            plan: "max20".into(),
            window_kind: "5h".into(),
            window_duration_mins: 300,
            resets_at: RESET,
            observed_at,
            used_fraction_units: used,
            measurement_resolution_fraction_units: resolution,
            gateway_spend_nano: spend,
            observation_source: "response".into(),
            source_request_id: Some(format!("request-{observed_at}")),
        }
    }

    fn next(
        row: Option<AnthropicCalibrationRow>,
        observation: AnthropicWindowObservation,
    ) -> AnthropicCalibrationRow {
        let mut row = apply_observation(row, &observation).unwrap();
        row.version += 1;
        row
    }

    #[test]
    fn first_complete_positive_interval_publishes_exact_blend_without_prior() {
        let row = next(None, observation(10_000_000, 1_000_000, 1_000_000_000, 100));
        assert!(row.current_capacity_nano.is_none());
        let row = next(
            Some(row),
            observation(20_000_000, 1_000_000, 3_000_000_000, 101),
        );
        assert_eq!(row.current_capacity_nano, Some(20_000_000_000));
        assert_eq!(row.observed_fraction_units, 10_000_000);
        assert_eq!(row.observed_spend_nano, 2_000_000_000);
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn repeated_quota_only_movement_is_excluded_as_unattributed() {
        let row = next(None, observation(10_000_000, 1_000_000, 1_000, 100));
        let row = next(Some(row), observation(20_000_000, 1_000_000, 1_000, 101));
        assert_eq!(row.unattributed_fraction_units, 0);
        let row = next(Some(row), observation(20_000_000, 1_000_000, 1_000, 102));
        assert_eq!(row.unattributed_fraction_units, 10_000_000);
        assert!(row.current_capacity_nano.is_none());
        assert_eq!(row.anchor_used_fraction_units, 20_000_000);
    }

    #[test]
    fn reset_jitter_does_not_split_but_usage_drop_with_reset_advance_does() {
        let row = next(None, observation(30_000_000, 1_000_000, 1_000, 100));
        let mut jitter = observation(40_000_000, 1_000_000, 2_000, 101);
        jitter.resets_at += 30;
        let row = next(Some(row), jitter);
        assert_eq!(row.samples, 1);

        let mut rolled = observation(5_000_000, 1_000_000, 3_000, 102);
        rolled.resets_at += 120;
        let row = next(Some(row), rolled);
        assert_eq!(row.anchor_used_fraction_units, 5_000_000);
        assert_eq!(row.resets_at, RESET + 120);
        assert_eq!(row.samples, 1, "rollover preserves historical evidence");
    }

    #[test]
    fn explicit_endpoint_resolution_widens_the_interval_envelope() {
        let row = next(None, observation(10_000_000, 1_000_000, 1_000, 100));
        let row = next(Some(row), observation(11_000_000, 1_000_000, 2_000, 101));
        assert_eq!(row.current_capacity_nano, Some(100_000));
        assert_eq!(row.current_low_nano, Some(50_000));
        assert!(row.current_high_nano.is_none());
        assert_eq!(row.current_confidence_bp, 0);
    }

    #[test]
    fn estimator_upgrade_replays_immutable_history_instead_of_trusting_old_state() {
        let first = observation(10_000_000, 100_000, 1_000_000_000, 100);
        let second = observation(20_000_000, 100_000, 3_000_000_000, 101);
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
            &AnthropicWindowObservation {
                observed_at: 102,
                source_request_id: Some("request-102".to_owned()),
                ..second
            },
        )
        .unwrap();

        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(rebuilt.version, 17);
        assert_eq!(rebuilt.current_capacity_nano, Some(20_000_000_000));
        assert_eq!(rebuilt.observed_fraction_units, 10_000_000);
        assert_eq!(rebuilt.observed_spend_nano, 2_000_000_000);
        assert_eq!(rebuilt.samples, 1);
    }
}
