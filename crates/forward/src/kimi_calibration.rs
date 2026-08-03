//! Evidence-only calibration of one KIMI (Kimi Code) subscription window.
//!
//! KIMI reports quota as integer `used`/`limit` counters per independent window, plus a reset
//! timestamp. It does **not** report native consumption per turn — only the window aggregate —
//! so there is no independent native ledger to maintain. What the provider does give for free is
//! the window's exact native size: `limit` IS the window, and `limit - used` is the exact native
//! remaining. Neither is estimated.
//!
//! What must be estimated is how much official API replacement cost fits in the window for the
//! observed workload:
//!
//! `capacity_nano = 100_000_000 * ΣΔspend_nano / ΣΔused_fraction_units`.
//!
//! There is no plan prior, subscription-price nominal, EMA, WLS, floating-point money or hidden
//! fallback here. The one structural advantage over the Anthropic estimator is resolution: a
//! quota limit of 1000 measures to 0.1% instead of a whole percent, which narrows the
//! quantisation envelope and lets a finite high bound be proved far sooner.

// Dormant estimator: the durable read/write path and the /usages poller that drive it land in a
// later step of this series, so nothing calls these entry points yet. The module is fully covered
// by its own deterministic tests in the meantime.
#![allow(dead_code)]

use anyhow::Context as _;
use registry::{KimiCalibrationRow, KimiWindowObservation, KIMI_FRACTION_SCALE};

pub(crate) const FRACTION_SCALE: i64 = KIMI_FRACTION_SCALE;
pub(crate) const ESTIMATOR_VERSION: i64 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct KimiWindowCalibration {
    row: KimiCalibrationRow,
}

impl KimiWindowCalibration {
    fn anchor(observation: &KimiWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        Ok(Self {
            row: KimiCalibrationRow {
                subject_id: observation.subject_id.clone(),
                plan: observation.plan.clone(),
                window_duration_secs: observation.window_duration_secs,
                window_name: observation.window_name.clone(),
                resets_at: observation.resets_at,
                anchor_used_fraction_units: observation.used_fraction_units,
                anchor_resolution_fraction_units: observation.measurement_resolution_fraction_units,
                anchor_spend_nano: observation.cumulative_api_spend_nano,
                used_fraction_units: observation.used_fraction_units,
                measurement_resolution_fraction_units: observation
                    .measurement_resolution_fraction_units,
                observed_at: observation.observed_at,
                native_limit_units: observation.native_limit_units,
                native_used_units: observation.native_used_units,
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

    pub(crate) fn from_row(row: KimiCalibrationRow) -> anyhow::Result<Self> {
        validate_row(&row)?;
        Ok(Self { row })
    }

    pub(crate) fn into_row(self) -> KimiCalibrationRow {
        self.row
    }

    fn observe(&mut self, observation: &KimiWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        // Identity is subject + paid plan + exact native duration. Two windows of different
        // length are independent evidence and must never be folded together.
        if observation.subject_id != self.row.subject_id
            || observation.plan != self.row.plan
            || observation.window_duration_secs != self.row.window_duration_secs
        {
            anyhow::bail!("KIMI calibration observation identity mismatch");
        }
        if observation.observed_at < self.row.observed_at {
            // Strictly older polls are stale and never mutate state.
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("KIMI calibration estimator version mismatch");
        }

        let reset_delta = i128::from(observation.resets_at) - i128::from(self.row.resets_at);
        let duration_secs = i128::from(self.row.window_duration_secs);
        let reset_boundary = (duration_secs / 2).max(1);
        let jitter_tolerance = (duration_secs / 200).clamp(1, 3_600);
        // A rolling window rolls over when utilisation falls back AND the reset advances
        // materially. Bounded timestamp jitter alone must not fork the window.
        let rolling_window_rollover = observation.used_fraction_units
            < self.row.anchor_used_fraction_units
            && reset_delta >= jitter_tolerance;
        if reset_delta >= reset_boundary || rolling_window_rollover {
            self.begin_window(observation);
            return Ok(());
        }
        if reset_delta <= -reset_boundary {
            // The reset moved backwards by more than half a window: this is a stale or
            // out-of-order snapshot, not new evidence.
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
        // The published window size is refreshed on every poll: a plan change can resize it, and
        // native remaining must stay exact rather than drift against a stale limit.
        self.row.native_limit_units = observation.native_limit_units;
        self.row.native_used_units = observation.native_used_units;

        let delta_used = observation.used_fraction_units - self.row.anchor_used_fraction_units;
        // A rollback and return to the old high-water is not new spend. Only a strictly higher
        // utilisation endpoint can delimit an interval.
        if delta_used <= 0 || observation.cumulative_api_spend_nano < self.row.anchor_spend_nano {
            return Ok(());
        }
        let delta_spend = observation.cumulative_api_spend_nano - self.row.anchor_spend_nano;

        // Quota can move before the turn FIFO has advanced cumulative spend. Hold the anchor
        // once. Seeing the same higher quota point again with still no spend means the movement
        // was not ours to attribute, so it is recorded as unattributed and can never inflate
        // measured capacity.
        if delta_spend == 0 {
            if previous_seen_used == observation.used_fraction_units
                && previous_seen_at < observation.observed_at
            {
                self.row.unattributed_fraction_units = self
                    .row
                    .unattributed_fraction_units
                    .checked_add(delta_used)
                    .context("KIMI unattributed fraction overflow")?;
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
            .context("KIMI observed fraction overflow")?;
        self.row.observed_spend_nano = self
            .row
            .observed_spend_nano
            .checked_add(delta_spend)
            .context("KIMI observed spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("KIMI calibration sample overflow")?;
        self.advance_anchor(observation);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &KimiWindowObservation) {
        self.row.anchor_used_fraction_units = observation.used_fraction_units;
        self.row.anchor_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.anchor_spend_nano = observation.cumulative_api_spend_nano;
    }

    /// Start a new interval. History is not erased: completed samples stay, because they remain
    /// valid evidence about how much cost fits in a window of this size.
    fn begin_window(&mut self, observation: &KimiWindowObservation) {
        self.row.resets_at = observation.resets_at;
        self.advance_anchor(observation);
        self.row.used_fraction_units = observation.used_fraction_units;
        self.row.measurement_resolution_fraction_units =
            observation.measurement_resolution_fraction_units;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
        self.row.native_limit_units = observation.native_limit_units;
        self.row.native_used_units = observation.native_used_units;
    }

    /// Widen the interval denominator by half the resolution of both endpoints, then keep the
    /// most conservative low and high seen so far.
    fn update_workload_envelope(
        &mut self,
        delta_used: i64,
        uncertainty: i64,
        delta_spend: i64,
    ) -> anyhow::Result<()> {
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(i128::from(delta_spend))
            .context("KIMI workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + i128::from(uncertainty))
                .context("KIMI workload low bound overflow")?,
        )?;
        self.row.current_low_nano = Some(
            self.row
                .current_low_nano
                .map_or(low, |existing| existing.min(low)),
        );
        // If movement did not exceed the quantisation envelope, a finite high is not
        // mathematically proved. Publish nothing rather than a guessed ceiling.
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
            .context("KIMI capacity numerator overflow")?;
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
                    .context("KIMI confidence resolution overflow")?,
            )
            .context("KIMI confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(denominator, quantisation_denominator)?;
        // Deterministic maturity x envelope stability x quantisation quality. This is a quality
        // score, not a probability.
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("KIMI confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

/// Fold one observation into existing state, or anchor a fresh window.
///
/// Reads never create evidence: this is called only by the writer path.
pub(crate) fn apply_observation(
    existing: Option<KimiCalibrationRow>,
    observation: &KimiWindowObservation,
) -> anyhow::Result<KimiCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        // A stored row from a different estimator version is not authority: rebuild from the
        // immutable observation history instead of trusting a derived value.
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            KimiWindowCalibration::from_row(row)?
        }
        Some(_) | None => KimiWindowCalibration::anchor(observation)?,
    };
    calibration.observe(observation)?;
    let mut row = calibration.into_row();
    row.version = version;
    Ok(row)
}

/// Deterministically rebuild state from immutable observations, in order.
pub(crate) fn rebuild_from_history(
    observations: &[KimiWindowObservation],
) -> anyhow::Result<Option<KimiCalibrationRow>> {
    let mut current: Option<KimiCalibrationRow> = None;
    for observation in observations {
        current = Some(apply_observation(current, observation)?);
    }
    Ok(current)
}

fn interval_fraction_uncertainty(anchor_resolution: i64, current_resolution: i64) -> i64 {
    // Half the resolution at each endpoint of the interval.
    anchor_resolution / 2 + current_resolution / 2
}

fn validate_observation(observation: &KimiWindowObservation) -> anyhow::Result<()> {
    if observation.subject_id.is_empty() {
        anyhow::bail!("KIMI calibration observation has no subject");
    }
    if observation.plan.is_empty() {
        anyhow::bail!("KIMI calibration observation has no paid plan");
    }
    if observation.window_duration_secs <= 0 {
        anyhow::bail!("KIMI calibration observation has an invalid window duration");
    }
    if observation.resets_at <= 0 || observation.observed_at <= 0 {
        anyhow::bail!("KIMI calibration observation has an invalid timestamp");
    }
    if observation.native_limit_units <= 0 {
        anyhow::bail!("KIMI calibration observation has an invalid quota limit");
    }
    if observation.native_used_units < 0
        || observation.native_used_units > observation.native_limit_units
    {
        anyhow::bail!("KIMI calibration observation has an invalid quota usage");
    }
    if !(0..=FRACTION_SCALE).contains(&observation.used_fraction_units) {
        anyhow::bail!("KIMI calibration observation fraction is out of range");
    }
    if !(1..=FRACTION_SCALE).contains(&observation.measurement_resolution_fraction_units) {
        anyhow::bail!("KIMI calibration observation resolution is out of range");
    }
    if observation.cumulative_api_spend_nano < 0 {
        anyhow::bail!("KIMI calibration observation spend is negative");
    }
    Ok(())
}

fn validate_row(row: &KimiCalibrationRow) -> anyhow::Result<()> {
    if row.subject_id.is_empty() || row.plan.is_empty() {
        anyhow::bail!("KIMI calibration row has no identity");
    }
    if row.window_duration_secs <= 0 {
        anyhow::bail!("KIMI calibration row has an invalid window duration");
    }
    if row.native_limit_units <= 0 || row.native_used_units > row.native_limit_units {
        anyhow::bail!("KIMI calibration row has an invalid quota window");
    }
    if !(0..=FRACTION_SCALE).contains(&row.used_fraction_units) {
        anyhow::bail!("KIMI calibration row fraction is out of range");
    }
    if let (Some(low), Some(high)) = (row.current_low_nano, row.current_high_nano) {
        if low > high {
            anyhow::bail!("KIMI calibration row bounds are inverted");
        }
    }
    Ok(())
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("KIMI calibration value overflow"))
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("KIMI calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("KIMI calibration rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("KIMI calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("KIMI calibration ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("KIMI calibration ratio overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::kimi_fraction_from_native;

    const WEEK: i64 = 604_800;
    const ROLLING: i64 = 18_000;
    const T0: i64 = 1_800_000_000;

    /// Build an observation from raw provider counters, exactly as the poller would.
    fn obs(
        duration: i64,
        resets_at: i64,
        observed_at: i64,
        used: i64,
        limit: i64,
        spend: i64,
    ) -> KimiWindowObservation {
        let derived = kimi_fraction_from_native(used, limit).expect("valid counters");
        KimiWindowObservation {
            subject_id: "u_1".into(),
            plan: "Vivace".into(),
            window_duration_secs: duration,
            window_name: None,
            resets_at,
            observed_at,
            native_used_units: used,
            native_limit_units: limit,
            used_fraction_units: derived.used_fraction_units,
            measurement_resolution_fraction_units: derived.measurement_resolution_fraction_units,
            cumulative_api_spend_nano: spend,
        }
    }

    #[test]
    fn the_first_snapshot_is_an_anchor_not_a_sample() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 100, 40, 1_000, 500)).unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nano, None);
        assert_eq!(row.current_low_nano, None);
        assert_eq!(row.anchor_used_fraction_units, 4_000_000);
        // Native remaining is exact from the very first poll, with no estimation at all.
        assert_eq!(row.native_remaining_units(), Some(960));
    }

    #[test]
    fn the_first_complete_interval_publishes_an_estimate() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        // 1% of the window consumed $1.00 of official API cost.
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000))
                .unwrap();
        assert_eq!(row.samples, 1);
        // capacity = 1e8 * 1e9 / 1e6 = 1e11 nanoUSD = $100.
        assert_eq!(row.current_capacity_nano, Some(100_000_000_000));
        assert!(row.current_low_nano.is_some());
        assert!(row.last_measured_at.is_some());
    }

    #[test]
    fn fine_resolution_proves_a_finite_high_that_whole_percent_could_not() {
        // limit=1000 -> resolution 0.1%, so a 1% move is 10x the quantisation width.
        let fine = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        let fine =
            apply_observation(Some(fine), &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000))
                .unwrap();
        assert!(
            fine.current_high_nano.is_some(),
            "a move far beyond the envelope must bound the high side"
        );

        // limit=100 -> resolution 1%, so a 1% move is exactly the envelope width and a finite
        // high is not mathematically proved.
        let coarse = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 100, 0)).unwrap();
        let coarse =
            apply_observation(Some(coarse), &obs(WEEK, T0, T0 - 100, 1, 100, 1_000_000_000))
                .unwrap();
        assert_eq!(
            coarse.current_high_nano, None,
            "an unbounded high must stay null, never a guessed ceiling"
        );
        assert!(coarse.current_capacity_nano.is_some());
    }

    #[test]
    fn quota_before_settlement_holds_the_anchor() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 300, 0, 1_000, 0)).unwrap();
        // Quota moved but the turn FIFO has not advanced spend yet.
        let held =
            apply_observation(Some(row.clone()), &obs(WEEK, T0, T0 - 200, 10, 1_000, 0)).unwrap();
        assert_eq!(held.samples, 0);
        assert_eq!(held.anchor_used_fraction_units, row.anchor_used_fraction_units);

        // Settlement lands: the interval completes against the original anchor.
        let settled =
            apply_observation(Some(held), &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000))
                .unwrap();
        assert_eq!(settled.samples, 1);
        assert_eq!(settled.current_capacity_nano, Some(100_000_000_000));
    }

    #[test]
    fn repeated_quota_only_movement_is_recorded_as_unattributed() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 400, 0, 1_000, 0)).unwrap();
        let row = apply_observation(Some(row), &obs(WEEK, T0, T0 - 300, 10, 1_000, 0)).unwrap();
        // The same higher point again, still with no spend: someone else's traffic.
        let row = apply_observation(Some(row), &obs(WEEK, T0, T0 - 200, 10, 1_000, 0)).unwrap();
        assert_eq!(row.unattributed_fraction_units, 1_000_000);
        assert_eq!(row.samples, 0);
        assert_eq!(
            row.current_capacity_nano, None,
            "unattributed movement must never inflate capacity"
        );
    }

    #[test]
    fn a_reset_starts_a_new_interval_without_erasing_history() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000))
                .unwrap();
        assert_eq!(row.samples, 1);

        // Window resets: utilisation falls back and the reset advances a full window.
        let row = apply_observation(
            Some(row),
            &obs(WEEK, T0 + WEEK, T0 + 10, 0, 1_000, 1_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives a reset");
        assert_eq!(row.anchor_used_fraction_units, 0);
        assert_eq!(row.resets_at, T0 + WEEK);
        assert_eq!(row.current_capacity_nano, Some(100_000_000_000));
    }

    #[test]
    fn bounded_reset_jitter_does_not_fork_the_window() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        // Reset drifts by a few seconds while utilisation keeps climbing: same interval.
        let row = apply_observation(
            Some(row),
            &obs(WEEK, T0 + 3, T0 - 100, 10, 1_000, 1_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nano, Some(100_000_000_000));
    }

    #[test]
    fn a_rollback_to_an_old_high_water_is_not_new_spend() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 300, 0, 1_000, 0)).unwrap();
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 200, 20, 1_000, 2_000_000_000))
                .unwrap();
        assert_eq!(row.samples, 1);
        // A lower reading with no material reset advance: not a new window, not new evidence.
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 100, 15, 1_000, 2_000_000_000))
                .unwrap();
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn stale_observations_do_not_mutate_state() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 100, 10, 1_000, 500)).unwrap();
        let after =
            apply_observation(Some(row.clone()), &obs(WEEK, T0, T0 - 500, 90, 1_000, 9_000))
                .unwrap();
        assert_eq!(after.used_fraction_units, row.used_fraction_units);
        assert_eq!(after.observed_at, row.observed_at);
    }

    #[test]
    fn independent_durations_never_share_a_row() {
        let weekly = apply_observation(None, &obs(WEEK, T0, T0 - 100, 10, 1_000, 500)).unwrap();
        // A rolling-window observation must be refused against weekly state, not folded in.
        let err = apply_observation(
            Some(weekly),
            &obs(ROLLING, T0, T0 - 50, 5, 100, 900),
        )
        .unwrap_err();
        assert!(err.to_string().contains("identity mismatch"));
    }

    #[test]
    fn a_different_plan_is_a_different_cohort() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 100, 10, 1_000, 500)).unwrap();
        let mut upgraded = obs(WEEK, T0, T0 - 50, 20, 1_000, 900);
        upgraded.plan = "Moderato".into();
        assert!(apply_observation(Some(row), &upgraded).is_err());
    }

    #[test]
    fn mixed_workload_intervals_accumulate_into_one_blend() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 400, 0, 1_000, 0)).unwrap();
        // 1% for $1.00 (a cheap cached turn), then 1% for $3.00 (an expensive k3 turn).
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 300, 10, 1_000, 1_000_000_000))
                .unwrap();
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 200, 20, 1_000, 4_000_000_000))
                .unwrap();
        assert_eq!(row.samples, 2);
        // Blend: 1e8 * 4e9 / 2e6 = 2e11 = $200 per window for this observed mix.
        assert_eq!(row.current_capacity_nano, Some(200_000_000_000));
        // The envelope must span both samples conservatively.
        let low = row.current_low_nano.unwrap();
        let high = row.current_high_nano.unwrap();
        assert!(low <= 200_000_000_000 && high >= 200_000_000_000);
    }

    #[test]
    fn history_rebuild_is_deterministic() {
        let history = vec![
            obs(WEEK, T0, T0 - 400, 0, 1_000, 0),
            obs(WEEK, T0, T0 - 300, 10, 1_000, 1_000_000_000),
            obs(WEEK, T0, T0 - 200, 20, 1_000, 4_000_000_000),
        ];
        let rebuilt = rebuild_from_history(&history).unwrap().unwrap();
        let mut folded: Option<KimiCalibrationRow> = None;
        for observation in &history {
            folded = Some(apply_observation(folded, observation).unwrap());
        }
        assert_eq!(rebuilt, folded.unwrap());
        assert_eq!(rebuilt.current_capacity_nano, Some(200_000_000_000));
    }

    #[test]
    fn an_estimator_version_change_rebuilds_instead_of_trusting_stored_values() {
        let mut legacy = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        legacy.current_capacity_nano = Some(999_999_999);
        legacy.samples = 42;
        let rebuilt = apply_observation(
            Some(legacy),
            &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000),
        )
        .unwrap();
        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        // The stale derived values are gone: this row re-anchored from the observation.
        assert_eq!(rebuilt.samples, 0);
        assert_ne!(rebuilt.current_capacity_nano, Some(999_999_999));
    }

    #[test]
    fn remaining_uses_the_current_exact_fraction() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 100, 10, 1_000, 1_000_000_000))
                .unwrap();
        // 99% of a $100 window remains.
        assert_eq!(row.current_remaining_nano(), Some(99_000_000_000));
        assert_eq!(row.native_remaining_units(), Some(990));
    }

    #[test]
    fn invalid_observations_fail_closed() {
        let mut no_plan = obs(WEEK, T0, T0 - 100, 10, 1_000, 500);
        no_plan.plan = String::new();
        assert!(apply_observation(None, &no_plan).is_err());

        let mut bad_window = obs(WEEK, T0, T0 - 100, 10, 1_000, 500);
        bad_window.window_duration_secs = 0;
        assert!(apply_observation(None, &bad_window).is_err());

        let mut bad_quota = obs(WEEK, T0, T0 - 100, 10, 1_000, 500);
        bad_quota.native_used_units = 2_000;
        assert!(apply_observation(None, &bad_quota).is_err());

        let mut bad_resolution = obs(WEEK, T0, T0 - 100, 10, 1_000, 500);
        bad_resolution.measurement_resolution_fraction_units = 0;
        assert!(apply_observation(None, &bad_resolution).is_err());

        let mut negative_spend = obs(WEEK, T0, T0 - 100, 10, 1_000, 500);
        negative_spend.cumulative_api_spend_nano = -1;
        assert!(apply_observation(None, &negative_spend).is_err());
    }

    #[test]
    fn overflowing_spend_fails_closed_rather_than_wrapping() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 0, 1_000, 0)).unwrap();
        let huge = obs(WEEK, T0, T0 - 100, 1, 1_000, i64::MAX);
        // 1e8 * i64::MAX exceeds i64 once divided back into a capacity.
        assert!(apply_observation(Some(row), &huge).is_err());
    }

    #[test]
    fn a_resized_window_updates_native_capacity_exactly() {
        let row = apply_observation(None, &obs(WEEK, T0, T0 - 200, 100, 1_000, 0)).unwrap();
        assert_eq!(row.native_remaining_units(), Some(900));
        // The plan is upgraded and the window grows; native remaining must follow the new limit
        // rather than drift against the stale one.
        let row =
            apply_observation(Some(row), &obs(WEEK, T0, T0 - 100, 100, 5_000, 0)).unwrap();
        assert_eq!(row.native_limit_units, 5_000);
        assert_eq!(row.native_remaining_units(), Some(4_900));
    }
}
