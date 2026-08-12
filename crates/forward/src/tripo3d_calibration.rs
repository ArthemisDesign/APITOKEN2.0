//! Evidence-only calibration of one Tripo3D (VAST / Holymolly) prepaid balance track.
//!
//! Tripo3D has NO quota window: purchased credits never expire and never reset
//! (`docs/engine/TRIPO3D_PROVIDER.md` §5.3), so the KIMI/GLM interval machine degenerates to a
//! single balance track per subject + declared top-up cohort. Calibration answers "how much
//! sellable capacity remains on the balance", not "how much fits in the window":
//!
//! `capacity_nanoUSD = round_half_up(remaining_micro_units * ΣΔapi_nano / ΣΔbalance_units)`
//!
//! where the remaining scale is the current `balance − frozen` in micro-units of the proven
//! native unit and `ΣΔbalance_units` is the exact settled drawdown from the native millicredit
//! ledger (× 1 000 to micro-units). Because the schema fixes the API leg as the exact
//! fixed-rate image of the native leg ($0.01/credit), the measured ratio can only corroborate
//! the published rate — the balance endpoint's drawdown is the independent proof that the
//! parsed unit really is credits, and a disagreement beyond the quantisation envelope is a
//! typed anomaly that fails closed (migration 0049).
//!
//! Unit discipline: the endpoint's `balance`/`frozen` floats have unproven units (manifest
//! §5.2/§6). While the parsed micro-units are `None` the track stays in its cold branch —
//! nothing is published and the cumulative anchors stay put, so a later proven reading
//! measures the full span instead of losing the blind interval's spend. Unknown is `None`,
//! never `0`; an unbounded high stays `None` rather than a guessed ceiling.
//!
//! There is no plan prior, top-up-price nominal, EMA, WLS, floating-point money or hidden
//! fallback here. All arithmetic is checked i64/i128 integer math with round-half-up; overflow
//! fails closed.

use anyhow::Context as _;
use registry::{Tripo3dBalanceObservation, Tripo3dCalibrationRow};

pub(crate) const ESTIMATOR_VERSION: i64 = 1;

/// Micro-units per whole balance unit: the parsed halves are fixed-point at 1e-6 of the
/// proven native unit (a credit), so one settled millicredit is 1 000 micro-units.
const MICRO_UNITS_PER_MILLICREDIT: i64 = 1_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Tripo3dCalibration {
    row: Tripo3dCalibrationRow,
}

impl Tripo3dCalibration {
    fn anchor(observation: &Tripo3dBalanceObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        // The first observation of the track is an anchor, not a sample. The balance halves
        // anchor at whatever the endpoint proved so far (both stay `None` while the unit is
        // unproven); the cumulative spend legs are always exact.
        Ok(Self {
            row: Tripo3dCalibrationRow {
                subject_id: observation.subject_id.clone(),
                cohort: observation.cohort.clone(),
                anchor_balance_micro_units: observation.balance_micro_units,
                anchor_frozen_micro_units: observation.frozen_micro_units,
                anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
                anchor_spend_native_millicredits: observation.cumulative_native_millicredits,
                latest_balance_raw: observation.balance_raw.clone(),
                latest_frozen_raw: observation.frozen_raw.clone(),
                latest_balance_micro_units: observation.balance_micro_units,
                latest_frozen_micro_units: observation.frozen_micro_units,
                observed_at: observation.observed_at,
                observed_spend_api_nanousd: 0,
                observed_spend_native_millicredits: 0,
                samples: 0,
                current_capacity_nanousd: None,
                current_low_nanousd: None,
                current_high_nanousd: None,
                current_confidence_bp: 0,
                last_measured_at: None,
                estimator_version: ESTIMATOR_VERSION,
                version: 0,
                updated_ts: observation.observed_at,
            },
        })
    }

    pub(crate) fn from_row(row: Tripo3dCalibrationRow) -> anyhow::Result<Self> {
        registry::validate_tripo3d_calibration_row(&row)?;
        Ok(Self { row })
    }

    pub(crate) fn into_row(self) -> Tripo3dCalibrationRow {
        self.row
    }

    fn observe(&mut self, observation: &Tripo3dBalanceObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        // Identity is subject + declared top-up cohort: the only cohort axis the provider
        // exposes. Two cohorts never fold into one track.
        if observation.subject_id != self.row.subject_id || observation.cohort != self.row.cohort
        {
            anyhow::bail!("Tripo3D calibration observation identity mismatch");
        }
        if observation.observed_at < self.row.observed_at {
            // Strictly older polls are stale and never mutate state.
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Tripo3D calibration estimator version mismatch");
        }
        // Both cumulative ledgers are monotone. A regression is corruption, not a top-up:
        // fail closed rather than reinterpret it.
        if observation.cumulative_api_nanousd < self.row.anchor_spend_api_nanousd
            || observation.cumulative_native_millicredits
                < self.row.anchor_spend_native_millicredits
        {
            anyhow::bail!("Tripo3D cumulative calibration ledger regressed");
        }

        // The previous reading delimits the interval about to close; capture it before the
        // latest-halves refresh overwrites it.
        let previous_available = match (
            self.row.latest_balance_micro_units,
            self.row.latest_frozen_micro_units,
        ) {
            (Some(balance), Some(frozen)) => Some(balance - frozen),
            _ => None,
        };
        let previous_seen_at = self.row.observed_at;
        let previous_resolution = endpoint_resolution(
            &self.row.latest_balance_raw,
            &self.row.latest_frozen_raw,
        );

        self.row.latest_balance_raw = observation.balance_raw.clone();
        self.row.latest_frozen_raw = observation.frozen_raw.clone();
        self.row.latest_balance_micro_units = observation.balance_micro_units;
        self.row.latest_frozen_micro_units = observation.frozen_micro_units;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;

        let (Some(balance), Some(frozen)) = (
            observation.balance_micro_units,
            observation.frozen_micro_units,
        ) else {
            // Cold branch: the unit is still unproven, so nothing can be measured or
            // published. The anchors stay put so the first proven reading baselines the
            // track instead of losing the blind interval's spend.
            if self.row.anchor_balance_micro_units.is_some()
                || self.row.anchor_frozen_micro_units.is_some()
            {
                // A live proven→unproven regression is corruption, not a cold track.
                anyhow::bail!("Tripo3D balance track regressed from proven to unproven units");
            }
            return Ok(());
        };
        let current_available = balance - frozen;
        let (Some(anchor_balance), Some(anchor_frozen)) = (
            self.row.anchor_balance_micro_units,
            self.row.anchor_frozen_micro_units,
        ) else {
            // The track just became readable: this reading is its anchor, not a sample.
            self.advance_anchor(observation);
            return Ok(());
        };
        let anchor_available = anchor_balance
            .checked_sub(anchor_frozen)
            .context("Tripo3D anchor balance overflow")?;
        if anchor_available < 0 {
            anyhow::bail!("Tripo3D anchor has frozen above balance");
        }

        if current_available > anchor_available {
            // A prepaid balance only grows by a top-up (or a refund release): the rise carries
            // no spend and must never be measured. Re-anchor on the new level; completed
            // history stays.
            self.advance_anchor(observation);
            return Ok(());
        }
        let drawdown_endpoint = anchor_available - current_available;
        if drawdown_endpoint == 0 {
            return Ok(());
        }
        let delta_api = observation.cumulative_api_nanousd - self.row.anchor_spend_api_nanousd;
        let delta_native =
            observation.cumulative_native_millicredits - self.row.anchor_spend_native_millicredits;

        // The balance can move before the turn FIFO has advanced the ledgers. Hold the anchor
        // once. Seeing the same lower point again with still no ledger movement means the
        // movement was not ours to attribute: advancing the anchor excludes it from
        // measurement forever, so it can never inflate capacity. (The 0049 balance-track
        // schema carries no unattributed counter column; the exclusion IS the record.)
        if delta_api == 0 || delta_native == 0 {
            if previous_available == Some(current_available)
                && previous_seen_at < observation.observed_at
            {
                self.advance_anchor(observation);
            }
            return Ok(());
        }

        let current_resolution = endpoint_resolution(&observation.balance_raw, &observation.frozen_raw)
            .context("Tripo3D proven halves contradict their raw evidence")?;
        let previous_resolution = previous_resolution
            .context("Tripo3D stored raw evidence does not parse as a decimal")?;
        // Half the resolution at each endpoint of the interval. The anchor endpoint's
        // resolution is read from the latest stored raw — after a hold-then-settle sequence
        // that is the held point, which equals the delimiting reading in value.
        let uncertainty = previous_resolution / 2 + current_resolution / 2;

        // The unit proof: the endpoint's drawdown of `balance − frozen` must agree with the
        // exact settled ledger drawdown within the quantisation envelope. A disagreement
        // beyond the envelope means the proven-unit assumption (or the feed) is broken.
        let ledger_drawdown_micro = i128::from(delta_native)
            .checked_mul(i128::from(MICRO_UNITS_PER_MILLICREDIT))
            .context("Tripo3D ledger drawdown overflow")?;
        let gap = (i128::from(drawdown_endpoint) - ledger_drawdown_micro).abs();
        if gap > i128::from(uncertainty) {
            anyhow::bail!(
                "Tripo3D balance drawdown and settled ledger disagree beyond the quantisation envelope"
            );
        }

        self.row.observed_spend_api_nanousd = self
            .row
            .observed_spend_api_nanousd
            .checked_add(delta_api)
            .context("Tripo3D observed API spend overflow")?;
        self.row.observed_spend_native_millicredits = self
            .row
            .observed_spend_native_millicredits
            .checked_add(delta_native)
            .context("Tripo3D observed native spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Tripo3D calibration sample overflow")?;
        // If the movement did not exceed the quantisation envelope, a finite high is not
        // mathematically proved. Publish nothing rather than a guessed ceiling.
        let interval_high_proven = i128::from(drawdown_endpoint) > i128::from(uncertainty);
        self.advance_anchor(observation);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute(uncertainty, interval_high_proven)
    }

    fn advance_anchor(&mut self, observation: &Tripo3dBalanceObservation) {
        self.row.anchor_balance_micro_units = observation.balance_micro_units;
        self.row.anchor_frozen_micro_units = observation.frozen_micro_units;
        self.row.anchor_spend_api_nanousd = observation.cumulative_api_nanousd;
        self.row.anchor_spend_native_millicredits = observation.cumulative_native_millicredits;
    }

    /// Recompute capacity and bounds from the cumulative exact ledgers against the CURRENT
    /// remaining balance. All three derive from the same scale at the same time, so
    /// `low ≤ capacity ≤ high` holds by denominator monotonicity (the state table CHECKs
    /// require it on every write). The low denominator is widened and the high narrowed by the
    /// completing interval's quantisation envelope; once any interval failed to prove its
    /// high, the high stays `None` — a guessed ceiling is never published.
    fn recompute(&mut self, uncertainty: i64, interval_high_proven: bool) -> anyhow::Result<()> {
        let drawdown_micro = i128::from(self.row.observed_spend_native_millicredits)
            .checked_mul(i128::from(MICRO_UNITS_PER_MILLICREDIT))
            .context("Tripo3D cumulative drawdown overflow")?;
        if drawdown_micro <= 0 {
            return Ok(());
        }
        let (Some(balance), Some(frozen)) = (
            self.row.latest_balance_micro_units,
            self.row.latest_frozen_micro_units,
        ) else {
            // A measured sample implies proven halves; anything else is corruption.
            anyhow::bail!("Tripo3D measured state without proven balance halves");
        };
        let remaining = i128::from(balance - frozen);
        let spend = i128::from(self.row.observed_spend_api_nanousd);
        let numerator = remaining
            .checked_mul(spend)
            .context("Tripo3D capacity numerator overflow")?;
        let u = i128::from(uncertainty);

        self.row.current_capacity_nanousd =
            Some(checked_i64(round_nonnegative(numerator, drawdown_micro)?)?);
        self.row.current_low_nanousd = Some(checked_i64(
            numerator
                .checked_div(drawdown_micro.checked_add(u).context("Tripo3D low overflow")?)
                .context("Tripo3D low bound overflow")?,
        )?);
        let high_allowed = interval_high_proven
            && drawdown_micro > u
            && (self.row.samples == 1 || self.row.current_high_nanousd.is_some());
        self.row.current_high_nanousd = if high_allowed {
            Some(checked_i64(ceil_nonnegative(numerator, drawdown_micro - u)?)?)
        } else {
            None
        };

        let samples = i128::from(self.row.samples);
        let maturity_bp = ratio_bp(samples, samples + 2)?;
        let stability_bp = match (self.row.current_low_nanousd, self.row.current_high_nanousd) {
            (Some(low), Some(high)) if high > 0 => ratio_bp(i128::from(low), i128::from(high))?,
            _ => 0,
        };
        // The uncertainty already spans both endpoints of the completing interval, so the
        // quantisation quality scales it by the sample count, not by two again.
        let quantisation_denominator = drawdown_micro
            .checked_add(
                u.checked_mul(samples)
                    .context("Tripo3D confidence resolution overflow")?,
            )
            .context("Tripo3D confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(drawdown_micro, quantisation_denominator)?;
        // Deterministic maturity x envelope stability x quantisation quality. This is a quality
        // score, not a probability.
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("Tripo3D confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

/// Fold one observation into existing state, or anchor a fresh balance track.
///
/// Reads never create evidence: this is called only by the writer path.
pub(crate) fn apply_observation(
    existing: Option<Tripo3dCalibrationRow>,
    observation: &Tripo3dBalanceObservation,
) -> anyhow::Result<Tripo3dCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        // A stored row from a different estimator version is not authority: rebuild from the
        // immutable observation history instead of trusting a derived value.
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            Tripo3dCalibration::from_row(row)?
        }
        Some(_) | None => Tripo3dCalibration::anchor(observation)?,
    };
    calibration.observe(observation)?;
    let mut row = calibration.into_row();
    row.version = version;
    Ok(row)
}

/// Deterministically rebuild state from immutable observations, in order.
pub(crate) fn rebuild_from_history(
    observations: &[Tripo3dBalanceObservation],
) -> anyhow::Result<Option<Tripo3dCalibrationRow>> {
    let mut current: Option<Tripo3dCalibrationRow> = None;
    for observation in observations {
        current = Some(apply_observation(current, observation)?);
    }
    Ok(current)
}

/// Apply one observation, rebuilding a stale estimator version only from immutable raw history.
pub(crate) fn apply_observation_with_history(
    existing: Option<Tripo3dCalibrationRow>,
    history: &[Tripo3dBalanceObservation],
    observation: &Tripo3dBalanceObservation,
) -> anyhow::Result<Tripo3dCalibrationRow> {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut rebuilt = rebuild_from_history(history)?
        .context("missing immutable history for Tripo3D estimator rebuild")?;
    rebuilt.version = version;
    apply_observation(Some(rebuilt), observation)
}

/// The full-width reading resolution of `balance − frozen` in micro-units, from the decimal
/// places of the raw authority strings. `None` when either raw half does not parse as a
/// strict decimal — the caller fails closed.
fn endpoint_resolution(balance_raw: &str, frozen_raw: &str) -> Option<i64> {
    let (_, balance_resolution) = parse_decimal_micro_units(balance_raw)?;
    let (_, frozen_resolution) = parse_decimal_micro_units(frozen_raw)?;
    balance_resolution.checked_add(frozen_resolution)
}

/// Parse a raw provider value into fixed-point micro-units and its decimal resolution, without
/// ever touching binary float. Strict: digits with at most one `.`, no sign, no exponent, at
/// most six decimal places (finer than one micro-unit is unrepresentable and fails closed).
/// The resolution is `10^(6 − decimals)` micro-units — the width of one raw least-significant
/// digit — clamped to a minimum of 1 so the quantisation envelope is strictly positive.
fn parse_decimal_micro_units(raw: &str) -> Option<(i64, i64)> {
    let (integer, fraction) = match raw.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (raw, ""),
    };
    if integer.is_empty() || !integer.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    if !fraction.bytes().all(|byte| byte.is_ascii_digit()) || fraction.len() > 6 {
        return None;
    }
    let decimals = fraction.len() as u32;
    let integer: i64 = integer.parse().ok()?;
    let fraction_value: i64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    let scale = 10i64.checked_pow(6 - decimals)?;
    let value = integer
        .checked_mul(1_000_000)?
        .checked_add(fraction_value.checked_mul(scale)?)?;
    Some((value, scale))
}

fn validate_observation(observation: &Tripo3dBalanceObservation) -> anyhow::Result<()> {
    if observation.subject_id.is_empty() {
        anyhow::bail!("Tripo3D calibration observation has no subject");
    }
    if observation.cohort.is_empty() {
        anyhow::bail!("Tripo3D calibration observation has no cohort");
    }
    if observation.observed_at <= 0 {
        anyhow::bail!("Tripo3D calibration observation has an invalid timestamp");
    }
    if observation.balance_raw.is_empty() || observation.frozen_raw.is_empty() {
        anyhow::bail!("Tripo3D calibration observation lost its raw balance evidence");
    }
    if observation
        .balance_micro_units
        .is_some_and(|balance| balance < 0)
        || observation
            .frozen_micro_units
            .is_some_and(|frozen| frozen < 0)
    {
        anyhow::bail!("Tripo3D calibration observation has a negative balance half");
    }
    if let (Some(frozen), Some(balance)) = (
        observation.frozen_micro_units,
        observation.balance_micro_units,
    ) {
        if frozen > balance {
            anyhow::bail!("Tripo3D calibration observation has frozen above balance");
        }
    }
    // A parsed half must be the exact fixed-point image of its raw authority string; a
    // disagreement is corruption, fail closed.
    for (raw, parsed) in [
        (&observation.balance_raw, observation.balance_micro_units),
        (&observation.frozen_raw, observation.frozen_micro_units),
    ] {
        if let Some(value) = parsed {
            match parse_decimal_micro_units(raw) {
                Some((parsed_value, _)) if parsed_value == value => {}
                _ => anyhow::bail!("Tripo3D parsed balance half contradicts its raw evidence"),
            }
        }
    }
    if observation.cumulative_api_nanousd < 0 || observation.cumulative_native_millicredits < 0 {
        anyhow::bail!("Tripo3D calibration observation has a negative cumulative ledger");
    }
    match observation.observation_source.as_str() {
        "poll" if observation.source_request_id.is_none() => {}
        "response" if observation.source_request_id.is_some() => {}
        _ => anyhow::bail!("Tripo3D calibration observation has an invalid source"),
    }
    Ok(())
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("Tripo3D calibration value overflow"))
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("Tripo3D calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("Tripo3D calibration rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("Tripo3D calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("Tripo3D calibration ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("Tripo3D calibration ratio overflow")
}

#[cfg(test)]
mod tests {
    use super::*;

    const T0: i64 = 1_800_000_000;

    /// A poll observation exactly as the writer builds it once the balance unit is proven:
    /// raw values verbatim, parsed micro-units derived from them (never through float).
    fn obs(
        balance_raw: &str,
        frozen_raw: &str,
        cum_api: i64,
        cum_native: i64,
        observed_at: i64,
    ) -> Tripo3dBalanceObservation {
        Tripo3dBalanceObservation {
            subject_id: "u_1".into(),
            cohort: "tripo3d-api-50".into(),
            observed_at,
            balance_raw: balance_raw.into(),
            frozen_raw: frozen_raw.into(),
            balance_micro_units: parse_decimal_micro_units(balance_raw).map(|(value, _)| value),
            frozen_micro_units: parse_decimal_micro_units(frozen_raw).map(|(value, _)| value),
            cumulative_api_nanousd: cum_api,
            cumulative_native_millicredits: cum_native,
            observation_source: "poll".into(),
            source_request_id: None,
        }
    }

    /// A cold observation: the unit is unproven, so both parsed halves stay `None` and only
    /// the raw text and the exact cumulative ledgers carry information.
    fn cold_obs(
        balance_raw: &str,
        frozen_raw: &str,
        cum_api: i64,
        cum_native: i64,
        observed_at: i64,
    ) -> Tripo3dBalanceObservation {
        Tripo3dBalanceObservation {
            balance_micro_units: None,
            frozen_micro_units: None,
            ..obs(balance_raw, frozen_raw, cum_api, cum_native, observed_at)
        }
    }

    #[test]
    fn the_first_snapshot_is_an_anchor_not_a_sample() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 100)).unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nanousd, None);
        assert_eq!(row.current_low_nanousd, None);
        assert_eq!(row.anchor_balance_micro_units, Some(5_000_000_000));
        // Native remaining is exact from the very first proven poll, with no estimation.
        assert_eq!(row.native_remaining_micro_units(), Some(5_000_000_000));
    }

    #[test]
    fn unproven_units_keep_the_track_cold_and_the_anchors_put() {
        let row = apply_observation(None, &cold_obs("5000.5", "0", 0, 0, T0 - 300)).unwrap();
        assert_eq!(row.anchor_balance_micro_units, None);
        // Spend settles while the unit is unproven: the ledgers are visible on the
        // observation, but the anchors must NOT advance — the first proven reading baselines
        // the track over the full span.
        let row = apply_observation(
            Some(row),
            &cold_obs("4980.5", "0", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nanousd, None);
        assert_eq!(row.anchor_spend_api_nanousd, 0);
        assert_eq!(row.latest_balance_raw, "4980.5");
        assert_eq!(row.latest_balance_micro_units, None);
    }

    #[test]
    fn the_first_proven_reading_is_an_anchor_not_a_sample() {
        let row = apply_observation(None, &cold_obs("5000.5", "0", 0, 0, T0 - 300)).unwrap();
        let row = apply_observation(
            Some(row),
            &cold_obs("4980.5", "0", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        // The unit is proven from this epoch on: the reading anchors the track, still no
        // sample — the blind interval's spend is NOT attributed to a drawdown nobody measured.
        let row = apply_observation(
            Some(row),
            &obs("4980.500000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nanousd, None);
        assert_eq!(row.anchor_balance_micro_units, Some(4_980_500_000));
        assert_eq!(row.anchor_spend_api_nanousd, 200_000_000);
    }

    #[test]
    fn the_first_complete_interval_publishes_capacity_at_the_proven_rate() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        // 20 credits settled: $0.20 of official API cost.
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // capacity = 4 980 credits × $0.01 = $49.80, from the measured ratio.
        assert_eq!(row.current_capacity_nanousd, Some(49_800_000_000));
        assert!(row.current_low_nanousd.is_some());
        assert!(row.last_measured_at.is_some());
        let low = row.current_low_nanousd.unwrap();
        let high = row.current_high_nanousd.unwrap();
        assert!(low <= 49_800_000_000 && high >= 49_800_000_000);
    }

    #[test]
    fn capacity_tracks_the_current_remaining_balance() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 300)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        assert_eq!(row.current_capacity_nanousd, Some(49_800_000_000));
        let row = apply_observation(
            Some(row),
            &obs("4950.000000", "0.000000", 500_000_000, 50_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        // 4 950 credits remain × $0.01 = $49.50.
        assert_eq!(row.current_capacity_nanousd, Some(49_500_000_000));
        assert_eq!(row.native_remaining_micro_units(), Some(4_950_000_000));
    }

    #[test]
    fn movement_beyond_the_envelope_proves_a_finite_high() {
        // Coarse two-decimal readings: resolution 10 000 micro per half, so the interval
        // uncertainty is 20 000 micro and a 20-credit drawdown dwarfs it.
        let row = apply_observation(None, &obs("5000.00", "0.00", 0, 0, T0 - 200)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.00", "0.00", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert!(
            row.current_high_nanousd.is_some(),
            "a move far beyond the envelope must bound the high side"
        );
    }

    #[test]
    fn movement_within_the_envelope_leaves_the_high_unbounded() {
        // 0.01 credit of drawdown against a 20 000-micro envelope: not mathematically provable.
        let row = apply_observation(None, &obs("5000.00", "0.00", 0, 0, T0 - 200)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4999.99", "0.00", 100_000, 10, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "the exact ledger still counts the interval");
        assert_eq!(
            row.current_high_nanousd, None,
            "an unbounded high must stay null, never a guessed ceiling"
        );
        assert_eq!(row.current_capacity_nanousd, Some(49_999_900_000));
        assert_eq!(
            row.current_confidence_bp, 0,
            "without a high bound there is no envelope stability"
        );
    }

    #[test]
    fn bounds_wrap_capacity_on_every_sample() {
        // The state table requires low ≤ capacity ≤ high on every write; drain in uneven
        // steps and check the invariant holds throughout.
        let row = apply_observation(None, &obs("5000.00", "0.00", 0, 0, T0 - 400)).unwrap();
        let mut row = row;
        for (balance, cum_api, cum_native, at) in [
            ("4980.00", 200_000_000, 20_000, T0 - 300),
            ("4930.00", 700_000_000, 70_000, T0 - 200),
            ("4900.00", 1_000_000_000, 100_000, T0 - 100),
        ] {
            row = apply_observation(Some(row), &obs(balance, "0.00", cum_api, cum_native, at)).unwrap();
            let capacity = row.current_capacity_nanousd.unwrap();
            assert!(row.current_low_nanousd.unwrap() <= capacity);
            assert!(capacity <= row.current_high_nanousd.unwrap());
        }
        assert_eq!(row.samples, 3);
        assert_eq!(row.current_capacity_nanousd, Some(49_000_000_000));
    }

    #[test]
    fn quota_before_settlement_holds_the_anchor() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 300)).unwrap();
        // The balance dropped but the turn FIFO has not advanced the ledgers yet.
        let held = apply_observation(
            Some(row.clone()),
            &obs("4980.000000", "0.000000", 0, 0, T0 - 200),
        )
        .unwrap();
        assert_eq!(held.samples, 0);
        assert_eq!(held.anchor_balance_micro_units, row.anchor_balance_micro_units);

        // Settlement lands: the interval completes against the original anchor.
        let settled = apply_observation(
            Some(held),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(settled.samples, 1);
        assert_eq!(settled.current_capacity_nanousd, Some(49_800_000_000));
    }

    #[test]
    fn repeated_balance_only_movement_is_excluded_forever() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 400)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 0, 0, T0 - 300),
        )
        .unwrap();
        // The same lower point again, still with no spend: not ours to attribute. The anchor
        // advances past it so the movement can never inflate measured capacity.
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 0, 0, T0 - 200),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.anchor_balance_micro_units, Some(4_980_000_000));
        assert_eq!(row.current_capacity_nanousd, None);
        // The next real spend measures from the advanced anchor only.
        let row = apply_observation(
            Some(row),
            &obs("4960.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(49_600_000_000));
    }

    #[test]
    fn a_rollback_to_an_old_high_water_is_not_new_spend() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 500)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 400),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // A refund release lifts the available balance above the anchor: re-anchor, no sample.
        let row = apply_observation(
            Some(row),
            &obs("4990.000000", "0.000000", 200_000_000, 20_000, T0 - 300),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.anchor_balance_micro_units, Some(4_990_000_000));
        // Falling back to the old low point with no new spend is held, then excluded.
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "returning to an old low-water is not new spend");
        assert_eq!(row.current_capacity_nanousd, Some(49_800_000_000));
    }

    #[test]
    fn a_balance_increase_reanchors_without_erasing_history() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 300)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        // Top-up: the balance jumps above the anchor. New anchor, completed history stays.
        let row = apply_observation(
            Some(row),
            &obs("6000.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives a top-up");
        assert_eq!(row.anchor_balance_micro_units, Some(6_000_000_000));
        assert_eq!(row.anchor_spend_api_nanousd, 200_000_000);
        // Measurement continues from the new level.
        let row = apply_observation(
            Some(row),
            &obs("5980.000000", "0.000000", 400_000_000, 40_000, T0),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        assert_eq!(row.current_capacity_nanousd, Some(59_800_000_000));
    }

    #[test]
    fn an_in_flight_hold_is_not_mismeasured() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 300)).unwrap();
        // A running task holds 20 credits: frozen rises, no settled spend yet.
        let row = apply_observation(
            Some(row),
            &obs("5000.000000", "20.000000", 0, 0, T0 - 200),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.native_remaining_micro_units(), Some(4_980_000_000));
        // The task settles: the hold releases into a real drawdown measured from the anchor.
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(49_800_000_000));
    }

    #[test]
    fn stale_observations_do_not_mutate_state() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 100)).unwrap();
        let after = apply_observation(
            Some(row.clone()),
            &obs("4999.000000", "0.000000", 900, 90, T0 - 500),
        )
        .unwrap();
        assert_eq!(after, row);
    }

    #[test]
    fn a_duplicate_observation_is_a_no_op() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        let again = apply_observation(
            Some(row.clone()),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(again.samples, 1);
        assert_eq!(again.current_capacity_nanousd, row.current_capacity_nanousd);
    }

    #[test]
    fn a_different_cohort_is_a_different_track() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 100)).unwrap();
        let mut other = obs("4990.000000", "0.000000", 100_000_000, 10_000, T0 - 50);
        other.cohort = "tripo3d-api-100".into();
        assert!(apply_observation(Some(row), &other).is_err());
    }

    #[test]
    fn a_different_subject_is_a_different_track() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 100)).unwrap();
        let mut other = obs("4990.000000", "0.000000", 100_000_000, 10_000, T0 - 50);
        other.subject_id = "u_2".into();
        let err = apply_observation(Some(row), &other).unwrap_err();
        assert!(err.to_string().contains("identity mismatch"));
    }

    #[test]
    fn regressing_cumulative_ledgers_fail_closed() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert!(
            apply_observation(Some(row), &obs("4970.000000", "0.000000", 100_000_000, 20_000, T0))
                .is_err()
        );
    }

    #[test]
    fn a_proven_track_never_goes_back_to_unproven() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 100)).unwrap();
        let err =
            apply_observation(Some(row), &cold_obs("4990.5", "0", 100_000_000, 10_000, T0 - 50))
                .unwrap_err();
        assert!(err.to_string().contains("regressed from proven to unproven"));
    }

    #[test]
    fn a_drawdown_the_ledger_cannot_explain_fails_closed() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        // The endpoint shows a 40-credit drawdown but only 10 credits settled: far beyond the
        // 2-micro envelope, so the proven-unit assumption is broken — fail closed.
        let err = apply_observation(
            Some(row),
            &obs("4960.000000", "0.000000", 100_000_000, 10_000, T0 - 100),
        )
        .unwrap_err();
        assert!(err.to_string().contains("disagree beyond the quantisation envelope"));
    }

    #[test]
    fn mixed_workload_intervals_accumulate_the_exact_ledgers() {
        // A 20-credit image_to_model, then a 30-credit text_to_model: the fixed rate keeps the
        // ratio exact, and the cumulative ledgers carry both legs independently.
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 300)).unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 200),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &obs("4950.000000", "0.000000", 500_000_000, 50_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        assert_eq!(row.observed_spend_api_nanousd, 500_000_000);
        assert_eq!(row.observed_spend_native_millicredits, 50_000);
        assert_eq!(row.current_capacity_nanousd, Some(49_500_000_000));
    }

    #[test]
    fn history_rebuild_is_deterministic() {
        let history = vec![
            obs("5000.000000", "0.000000", 0, 0, T0 - 400),
            obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 300),
            obs("6000.000000", "0.000000", 200_000_000, 20_000, T0 - 200),
            obs("5980.000000", "0.000000", 400_000_000, 40_000, T0 - 100),
        ];
        let rebuilt = rebuild_from_history(&history).unwrap().unwrap();
        let mut folded: Option<Tripo3dCalibrationRow> = None;
        for observation in &history {
            folded = Some(apply_observation(folded, observation).unwrap());
        }
        assert_eq!(rebuilt, folded.unwrap());
        assert_eq!(rebuilt.samples, 2);
        assert_eq!(rebuilt.current_capacity_nanousd, Some(59_800_000_000));
    }

    #[test]
    fn an_estimator_version_change_rebuilds_instead_of_trusting_stored_values() {
        let anchor = obs("5000.000000", "0.000000", 0, 0, T0 - 200);
        let mut legacy = apply_observation(None, &anchor).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        legacy.current_capacity_nanousd = Some(999_999_999);
        legacy.samples = 42;
        legacy.version = 7;
        let rebuilt = apply_observation_with_history(
            Some(legacy),
            &[anchor],
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .unwrap();
        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(rebuilt.version, 7, "the outer CAS generation must be preserved");
        assert_eq!(rebuilt.samples, 1);
        assert_ne!(rebuilt.current_capacity_nanousd, Some(999_999_999));
    }

    #[test]
    fn an_estimator_version_change_without_history_fails_closed() {
        let mut legacy =
            apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        assert!(apply_observation_with_history(
            Some(legacy),
            &[],
            &obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100),
        )
        .is_err());
    }

    #[test]
    fn a_row_round_trips_through_the_authority_shape() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        let calibration = Tripo3dCalibration::from_row(row.clone()).unwrap();
        let restored = calibration.into_row();
        assert_eq!(restored, row);
        // Folding continues identically from the restored row.
        let next = obs("4980.000000", "0.000000", 200_000_000, 20_000, T0 - 100);
        assert_eq!(
            apply_observation(Some(restored), &next).unwrap(),
            apply_observation(Some(row), &next).unwrap()
        );
    }

    #[test]
    fn invalid_observations_fail_closed() {
        let mut no_subject = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        no_subject.subject_id = String::new();
        assert!(apply_observation(None, &no_subject).is_err());

        let mut no_cohort = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        no_cohort.cohort = String::new();
        assert!(apply_observation(None, &no_cohort).is_err());

        let mut no_raw = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        no_raw.balance_raw = String::new();
        assert!(apply_observation(None, &no_raw).is_err());

        let mut frozen_above = obs("5000.000000", "6000.000000", 0, 0, T0 - 100);
        frozen_above.balance_micro_units = Some(5_000_000_000);
        frozen_above.frozen_micro_units = Some(6_000_000_000);
        assert!(apply_observation(None, &frozen_above).is_err());

        let mut mismatch = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        mismatch.balance_micro_units = Some(1);
        assert!(apply_observation(None, &mismatch).is_err());

        let mut negative = obs("5000.000000", "0.000000", -1, 0, T0 - 100);
        negative.balance_micro_units = None;
        negative.frozen_micro_units = None;
        assert!(apply_observation(None, &negative).is_err());

        let mut bad_source = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        bad_source.source_request_id = Some("cal_1".into());
        assert!(apply_observation(None, &bad_source).is_err());

        let mut bad_response = obs("5000.000000", "0.000000", 0, 0, T0 - 100);
        bad_response.observation_source = "response".into();
        assert!(apply_observation(None, &bad_response).is_err());
    }

    #[test]
    fn overflowing_spend_fails_closed_rather_than_wrapping() {
        let row = apply_observation(None, &obs("5000.000000", "0.000000", 0, 0, T0 - 200)).unwrap();
        // remaining × i64::MAX nanoUSD over a 20-credit drawdown exceeds i64 capacity.
        assert!(apply_observation(
            Some(row),
            &obs("4980.000000", "0.000000", i64::MAX, 20_000, T0 - 100),
        )
        .is_err());
    }

    #[test]
    fn raw_decimal_parsing_is_strict_and_float_free() {
        assert_eq!(parse_decimal_micro_units("4850.5"), Some((4_850_500_000, 100_000)));
        assert_eq!(parse_decimal_micro_units("4850"), Some((4_850_000_000, 1_000_000)));
        assert_eq!(parse_decimal_micro_units("0.000001"), Some((1, 1)));
        assert_eq!(parse_decimal_micro_units("0.0000001"), None, "finer than a micro-unit");
        assert_eq!(parse_decimal_micro_units("-5"), None);
        assert_eq!(parse_decimal_micro_units("1e3"), None, "no exponent notation");
        assert_eq!(parse_decimal_micro_units(""), None);
        assert_eq!(parse_decimal_micro_units("4 850"), None);
    }
}
