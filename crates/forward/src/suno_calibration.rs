//! Evidence-only calibration of one Suno (suno.com) subscription monthly window.
//!
//! Suno is a dual-ledger provider with published native limits (`docs/engine/SUNO_PROVIDER.md`
//! §5.2/§5.3): every settled generation advances two independent exact cumulative ledgers —
//! API replacement cost (nanoUSD) and native credits consumption (millicredits) at the reviewed
//! derived rate. The window's native capacity is **not** estimated: the plan's monthly limits
//! are published (Pro 2 500 / Premier 10 000 credits, `suno_credential::reviewed_plan_credits`).
//! What is estimated is how much API replacement cost fits in the window for the observed
//! workload:
//!
//! `capacity_nanoUSD = round_half_up(FRACTION_SCALE * ΣΔapi_nano / ΣΔused_fraction_units)`
//!
//! over complete intervals, exactly as the KIMI/GLM estimators do. The difference from KIMI is
//! where the used fraction comes from, because the quota endpoint's field semantics are
//! unproven until a controlled live run (manifest §5.2/§6):
//!
//! * **Native-ledger path (primary while semantics are unproven).** The fraction is derived
//!   from our own exact millicredit ledger against the published plan limit:
//!   `fraction_units = window_native_used_millicredits * FRACTION_SCALE / native_limit_millicredits`.
//!   The window baseline is the cumulative native total when the current window was anchored,
//!   so the delta between two observations is exact even mid-window. One millicredit of
//!   resolution makes the quantisation envelope negligible, so a finite high is provable from
//!   the first complete interval.
//! * **Quota-endpoint path (raw evidence until proven).** Once the field semantics are proven
//!   live, the writer derives `used_fraction_units` from the provider's raw counters via
//!   `registry::suno_fraction_from_native` and the observation carries it; that provider-side
//!   window-absolute fraction then drives the state machine. Until then the nullable fields
//!   stay `None` together and the raw counters are preserved verbatim on the immutable
//!   observation — unknown is `None`, never `0`.
//!
//! Switching the fraction basis (native-ledger ↔ quota-endpoint) is a cutover: the estimator
//! sets a fresh common anchor for the window without erasing completed intervals, and old
//! evidence stays valid history rather than being read as zero spend.
//!
//! There is no plan prior, subscription-price nominal, EMA, WLS, floating-point money or
//! hidden fallback here. All arithmetic is checked i64/i128 integer math with round-half-up;
//! overflow fails closed.

use anyhow::Context as _;
use registry::{SunoCalibrationRow, SunoWindowObservation, SUNO_FRACTION_SCALE};
use suno_credential::{reviewed_plan_credits, SunoPlan};

pub(crate) const FRACTION_SCALE: i64 = SUNO_FRACTION_SCALE;
pub(crate) const ESTIMATOR_VERSION: i64 = 1;

/// Native credits are tracked in millicredits (credits × 1e3) so fractional-credit operations
/// stay exact in integers.
const MILLICREDITS_PER_CREDIT: i64 = 1_000;

/// The window's published native size in credits for an exact declared plan
/// (`suno_credential::SUNO_REVIEWED_PLANS`, reviewed 2026-08-12). This is provider-published
/// fact, never an estimate. The declared plan is the canonical label exactly as the schema
/// CHECK stores it — a legacy/unknown tier names no limit, which keeps the native-ledger
/// fraction path closed and blocks cohort aggregation.
pub(crate) fn published_window_limit_credits(plan: &str) -> Option<i64> {
    let plan = match plan {
        "Pro" => SunoPlan::Pro,
        "Premier" => SunoPlan::Premier,
        _ => return None,
    };
    reviewed_plan_credits(plan).and_then(|credits| i64::try_from(credits).ok())
}

/// The same published limit in millicredits, checked.
pub(crate) fn published_window_limit_millicredits(plan: &str) -> Option<i64> {
    published_window_limit_credits(plan).and_then(|credits| credits.checked_mul(MILLICREDITS_PER_CREDIT))
}

/// Which evidence path produced the window's used fraction. The basis is part of the anchor's
/// identity: mixing deltas across bases would compare a tracking-relative reading against a
/// window-absolute one, so a basis change re-anchors the interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FractionSource {
    /// Provider-side, window-absolute, derived from the quota endpoint's raw counters once
    /// their field semantics are proven live.
    QuotaEndpoint,
    /// Our own exact credits ledger against the published plan limit, relative to the
    /// cumulative native total at the window's anchor.
    NativeLedger,
}

/// The fraction view of one observation, resolved through exactly one evidence path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResolvedFraction {
    used_fraction_units: i64,
    resolution_fraction_units: i64,
    /// Native window halves in millicredits when either source can name them: the published
    /// limit on the native-ledger path, the proven raw counters × 1e3 on the quota path.
    native_limit_millicredits: Option<i64>,
    native_used_millicredits: Option<i64>,
    source: FractionSource,
}

/// Resolve an observation's used fraction. The quota-endpoint value wins when the observation
/// carries one (it is window-absolute and provider-side); otherwise the native-ledger path
/// derives the fraction from our own credits ledger against the published plan limit.
/// `native_baseline_millicredits` is the cumulative native total at the current window's
/// anchor; the window-scoped usage is the exact delta from it.
fn resolve_fraction(
    observation: &SunoWindowObservation,
    native_baseline_millicredits: Option<i64>,
) -> anyhow::Result<Option<ResolvedFraction>> {
    if let Some(quota_fraction) = observation.used_fraction_units {
        let resolution = observation
            .measurement_resolution_fraction_units
            .context("Suno quota fraction without its measurement resolution")?;
        let native_limit_millicredits = observation
            .native_limit_units
            .map(|limit| {
                limit
                    .checked_mul(MILLICREDITS_PER_CREDIT)
                    .context("Suno native limit overflow")
            })
            .transpose()?;
        let native_used_millicredits = observation
            .native_used_units
            .map(|used| {
                used.checked_mul(MILLICREDITS_PER_CREDIT)
                    .context("Suno native usage overflow")
            })
            .transpose()?;
        return Ok(Some(ResolvedFraction {
            used_fraction_units: quota_fraction,
            resolution_fraction_units: resolution,
            native_limit_millicredits,
            native_used_millicredits,
            source: FractionSource::QuotaEndpoint,
        }));
    }
    let Some(native_limit_millicredits) = published_window_limit_millicredits(&observation.plan)
    else {
        return Ok(None);
    };
    let Some(baseline) = native_baseline_millicredits else {
        return Ok(None);
    };
    let native_used_millicredits = observation
        .cumulative_native_millicredits
        .checked_sub(baseline)
        .context("Suno native ledger regressed below the window baseline")?;
    let used_fraction_units = checked_i64(
        i128::from(native_used_millicredits)
            .checked_mul(i128::from(FRACTION_SCALE))
            .and_then(|value| value.checked_div(i128::from(native_limit_millicredits)))
            .context("Suno native fraction overflow")?,
    )?;
    // One millicredit expressed in fraction units: 40 on Pro, 10 on Premier — far finer than
    // the quota endpoint's whole-credit resolution, so the envelope stays negligible. A
    // hypothetical limit beyond the fraction scale still clamps to a strictly positive width.
    let resolution_fraction_units = checked_i64(ceil_nonnegative(
        i128::from(FRACTION_SCALE),
        i128::from(native_limit_millicredits),
    )?)?
    .max(1);
    Ok(Some(ResolvedFraction {
        used_fraction_units,
        resolution_fraction_units,
        native_limit_millicredits: Some(native_limit_millicredits),
        native_used_millicredits: Some(native_used_millicredits),
        source: FractionSource::NativeLedger,
    }))
}

/// The cumulative native total at the current window's anchor. The estimator maintains the
/// invariant `baseline = anchor_spend_native - native_used` across anchor advances, so the
/// baseline survives every completed interval inside the window.
fn native_baseline(row: &SunoCalibrationRow) -> Option<i64> {
    row.anchor_spend_native_millicredits
        .checked_sub(row.native_used_millicredits?)
}

/// Which basis produced the row's stored fraction, if any. On the native-ledger path the
/// stored fraction is always the exact function of the stored native halves, and its
/// resolution is the millicredit width; on the quota path the resolution is the endpoint's
/// own. When both would coincide numerically the bases are indistinguishable — and a cutover
/// between them is a no-op anyway.
fn stored_fraction_source(row: &SunoCalibrationRow) -> Option<FractionSource> {
    let used_fraction = row.used_fraction_units?;
    if let (Some(used), Some(limit)) = (row.native_used_millicredits, row.native_limit_millicredits)
    {
        if let Ok(Some(native)) = resolve_native_fraction(used, limit) {
            if native.used_fraction_units == used_fraction
                && Some(native.resolution_fraction_units)
                    == row.measurement_resolution_fraction_units
            {
                return Some(FractionSource::NativeLedger);
            }
        }
    }
    Some(FractionSource::QuotaEndpoint)
}

/// The native-ledger fraction for a known window usage and limit, without an observation.
fn resolve_native_fraction(used: i64, limit: i64) -> anyhow::Result<Option<ResolvedFraction>> {
    if limit <= 0 || used < 0 || used > limit {
        return Ok(None);
    }
    let used_fraction_units = checked_i64(
        i128::from(used)
            .checked_mul(i128::from(FRACTION_SCALE))
            .and_then(|value| value.checked_div(i128::from(limit)))
            .context("Suno native fraction overflow")?,
    )?;
    let resolution_fraction_units = checked_i64(ceil_nonnegative(
        i128::from(FRACTION_SCALE),
        i128::from(limit),
    )?)?
    .max(1);
    Ok(Some(ResolvedFraction {
        used_fraction_units,
        resolution_fraction_units,
        native_limit_millicredits: Some(limit),
        native_used_millicredits: Some(used),
        source: FractionSource::NativeLedger,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SunoCalibration {
    row: SunoCalibrationRow,
}

impl SunoCalibration {
    fn anchor(observation: &SunoWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        // The first snapshot baselines the window at the current cumulative native total:
        // exactly what our own ledger can prove, with no fabrication about consumption that
        // predates tracking.
        let resolved = resolve_fraction(
            observation,
            Some(observation.cumulative_native_millicredits),
        )?;
        Ok(Self {
            row: SunoCalibrationRow {
                subject_id: observation.subject_id.clone(),
                plan: observation.plan.clone(),
                window_duration_secs: observation.window_duration_secs,
                reset_at: observation.reset_at,
                anchor_used_fraction_units: resolved.map(|value| value.used_fraction_units),
                anchor_resolution_fraction_units: resolved
                    .map(|value| value.resolution_fraction_units),
                anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
                anchor_spend_native_millicredits: observation.cumulative_native_millicredits,
                used_fraction_units: resolved.map(|value| value.used_fraction_units),
                measurement_resolution_fraction_units: resolved
                    .map(|value| value.resolution_fraction_units),
                observed_at: observation.observed_at,
                native_limit_millicredits: resolved
                    .and_then(|value| value.native_limit_millicredits),
                native_used_millicredits: resolved.and_then(|value| value.native_used_millicredits),
                observed_fraction_units: 0,
                observed_spend_api_nanousd: 0,
                observed_spend_native_millicredits: 0,
                samples: 0,
                unattributed_fraction_units: 0,
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

    pub(crate) fn from_row(row: SunoCalibrationRow) -> anyhow::Result<Self> {
        registry::validate_suno_calibration_row(&row)?;
        Ok(Self { row })
    }

    pub(crate) fn into_row(self) -> SunoCalibrationRow {
        self.row
    }

    fn observe(&mut self, observation: &SunoWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        // Identity is subject + declared paid plan + exact native duration. The monthly reset
        // is billing-anchored and its exact duration comes from reset evidence (manifest §1):
        // two windows of different length are independent evidence and never fold into one
        // row; a different plan is a different cohort.
        if observation.subject_id != self.row.subject_id
            || observation.plan != self.row.plan
            || observation.window_duration_secs != self.row.window_duration_secs
        {
            anyhow::bail!("Suno calibration observation identity mismatch");
        }
        if observation.observed_at < self.row.observed_at {
            // Strictly older polls are stale and never mutate state.
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("Suno calibration estimator version mismatch");
        }
        // Both cumulative ledgers are monotone. A regression is corruption, not a reset:
        // fail closed rather than reinterpret it.
        if observation.cumulative_api_nanousd < self.row.anchor_spend_api_nanousd
            || observation.cumulative_native_millicredits
                < self.row.anchor_spend_native_millicredits
        {
            anyhow::bail!("Suno cumulative calibration ledger regressed");
        }

        let reset_delta = match (observation.reset_at, self.row.reset_at) {
            (Some(new), Some(old)) => Some(i128::from(new) - i128::from(old)),
            _ => None,
        };
        let duration_secs = i128::from(self.row.window_duration_secs);
        let reset_boundary = (duration_secs / 2).max(1);
        let jitter_tolerance = (duration_secs / 200).clamp(1, 3_600);
        let resolved = resolve_fraction(observation, native_baseline(&self.row))?;

        let fraction_fell_back = match (
            resolved.map(|value| value.used_fraction_units),
            self.row.anchor_used_fraction_units,
        ) {
            (Some(current), Some(anchor)) => current < anchor,
            _ => false,
        };
        // A window rolls over when utilisation falls back AND the reset advances materially.
        // Bounded timestamp jitter alone must not fork the window, and an unnamed reset
        // supplies no reset evidence at all.
        let rolling_window_rollover =
            fraction_fell_back && reset_delta.is_some_and(|delta| delta >= jitter_tolerance);
        // Our own ledger says the window usage outgrew the published window size: the reset
        // landed without usable reset evidence or the published limit went stale. Re-anchor
        // instead of publishing an impossible fraction above 100%.
        let window_slid = resolved.is_some_and(|value| {
            value.source == FractionSource::NativeLedger
                && match (
                    value.native_used_millicredits,
                    value.native_limit_millicredits,
                ) {
                    (Some(used), Some(limit)) => used > limit,
                    _ => false,
                }
        });
        // A fraction-basis change (native-ledger ↔ quota-endpoint) is a cutover: the new
        // observation anchors a fresh interval and completed history stays.
        let cutover = match (
            stored_fraction_source(&self.row),
            resolved.map(|value| value.source),
        ) {
            (Some(stored), Some(new)) => stored != new,
            _ => false,
        };
        if reset_delta.is_some_and(|delta| delta >= reset_boundary)
            || rolling_window_rollover
            || window_slid
            || cutover
        {
            self.begin_window(observation)?;
            return Ok(());
        }
        if reset_delta.is_some_and(|delta| delta <= -reset_boundary) {
            // The reset moved backwards by more than half a window: this is a stale or
            // out-of-order snapshot, not new evidence.
            return Ok(());
        }

        let previous_seen_used = self.row.used_fraction_units;
        let previous_seen_at = self.row.observed_at;
        self.row.reset_at = match (self.row.reset_at, observation.reset_at) {
            (Some(old), Some(new)) => Some(old.max(new)),
            (Some(old), None) => Some(old),
            (None, new) => new,
        };
        if let Some(value) = resolved {
            self.row.used_fraction_units = Some(value.used_fraction_units);
            self.row.measurement_resolution_fraction_units = Some(value.resolution_fraction_units);
            // The published/proven native halves refresh on every resolved poll, so native
            // remaining stays exact rather than drifting against a stale limit.
            self.row.native_limit_millicredits = value.native_limit_millicredits;
            self.row.native_used_millicredits = value.native_used_millicredits;
        }
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;

        let Some(current) = resolved else {
            // No fraction source can read this window yet (unproven field semantics and no
            // published limit). The cumulative anchors stay put so a later readable
            // observation measures the full span instead of losing the blind interval's spend.
            return Ok(());
        };
        let (Some(anchor_used), Some(anchor_resolution)) = (
            self.row.anchor_used_fraction_units,
            self.row.anchor_resolution_fraction_units,
        ) else {
            // The window just became readable: this reading is its anchor, not a sample.
            self.advance_anchor(observation, &current);
            return Ok(());
        };

        let delta_used = current.used_fraction_units - anchor_used;
        // A rollback and return to the old high-water is not new spend. Only a strictly higher
        // utilisation endpoint can delimit an interval.
        if delta_used <= 0 {
            return Ok(());
        }
        let delta_api = observation.cumulative_api_nanousd - self.row.anchor_spend_api_nanousd;
        let delta_native =
            observation.cumulative_native_millicredits - self.row.anchor_spend_native_millicredits;

        // Quota can move before the turn FIFO has advanced the ledgers, and on the quota path
        // someone else's traffic moves the provider reading without touching our ledgers at
        // all. Hold the anchor once. Seeing the same higher point again with still no ledger
        // movement means the movement was not ours to attribute, so it is recorded as
        // unattributed and can never inflate measured capacity. On the native-ledger path the
        // fraction is a pure function of the native ledger, so it cannot move without
        // `delta_native > 0` — this branch is the quota path's attribution guard.
        if delta_api == 0 || delta_native == 0 {
            if previous_seen_used == Some(current.used_fraction_units)
                && previous_seen_at < observation.observed_at
            {
                self.row.unattributed_fraction_units = self
                    .row
                    .unattributed_fraction_units
                    .checked_add(delta_used)
                    .context("Suno unattributed fraction overflow")?;
                self.advance_anchor(observation, &current);
            }
            return Ok(());
        }

        let uncertainty =
            interval_fraction_uncertainty(anchor_resolution, current.resolution_fraction_units);
        self.update_workload_envelope(delta_used, uncertainty, delta_api)?;
        self.row.observed_fraction_units = self
            .row
            .observed_fraction_units
            .checked_add(delta_used)
            .context("Suno observed fraction overflow")?;
        self.row.observed_spend_api_nanousd = self
            .row
            .observed_spend_api_nanousd
            .checked_add(delta_api)
            .context("Suno observed API spend overflow")?;
        self.row.observed_spend_native_millicredits = self
            .row
            .observed_spend_native_millicredits
            .checked_add(delta_native)
            .context("Suno observed native spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("Suno calibration sample overflow")?;
        self.advance_anchor(observation, &current);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &SunoWindowObservation, current: &ResolvedFraction) {
        // The baseline invariant holds across the advance: native_used was already refreshed
        // to `cumulative - baseline`, so `anchor_spend_native - native_used` keeps naming the
        // same window baseline.
        self.row.anchor_used_fraction_units = Some(current.used_fraction_units);
        self.row.anchor_resolution_fraction_units = Some(current.resolution_fraction_units);
        self.row.anchor_spend_api_nanousd = observation.cumulative_api_nanousd;
        self.row.anchor_spend_native_millicredits = observation.cumulative_native_millicredits;
    }

    /// Start a new interval. History is not erased: completed samples stay, because they remain
    /// valid evidence about how much cost fits in a window of this size.
    fn begin_window(&mut self, observation: &SunoWindowObservation) -> anyhow::Result<()> {
        // Re-baseline the native ledger at the current cumulative total: whatever the old
        // window consumed stays behind, and the new window measures from here.
        let resolved = resolve_fraction(
            observation,
            Some(observation.cumulative_native_millicredits),
        )?;
        self.row.reset_at = observation.reset_at;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
        if let Some(value) = resolved {
            self.row.used_fraction_units = Some(value.used_fraction_units);
            self.row.measurement_resolution_fraction_units = Some(value.resolution_fraction_units);
            self.row.native_limit_millicredits = value.native_limit_millicredits;
            self.row.native_used_millicredits = value.native_used_millicredits;
            self.advance_anchor(observation, &value);
        } else {
            // The new window is not readable yet: clear the old fraction legs rather than
            // anchor a new interval on the previous window's reading. The cumulative anchors
            // still advance so no spend is double-counted later.
            self.row.used_fraction_units = None;
            self.row.measurement_resolution_fraction_units = None;
            self.row.native_limit_millicredits = None;
            self.row.native_used_millicredits = None;
            self.row.anchor_used_fraction_units = None;
            self.row.anchor_resolution_fraction_units = None;
            self.row.anchor_spend_api_nanousd = observation.cumulative_api_nanousd;
            self.row.anchor_spend_native_millicredits = observation.cumulative_native_millicredits;
        }
        Ok(())
    }

    /// Widen the interval denominator by half the resolution of both endpoints, then keep the
    /// most conservative low and high seen so far.
    fn update_workload_envelope(
        &mut self,
        delta_used: i64,
        uncertainty: i64,
        delta_api: i64,
    ) -> anyhow::Result<()> {
        let numerator = i128::from(FRACTION_SCALE)
            .checked_mul(i128::from(delta_api))
            .context("Suno workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + i128::from(uncertainty))
                .context("Suno workload low bound overflow")?,
        )?;
        self.row.current_low_nanousd = Some(
            self.row
                .current_low_nanousd
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
        self.row.current_high_nanousd = if self.row.samples == 0 {
            sample_high
        } else {
            match (self.row.current_high_nanousd, sample_high) {
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
            .checked_mul(i128::from(self.row.observed_spend_api_nanousd))
            .context("Suno capacity numerator overflow")?;
        self.row.current_capacity_nanousd =
            Some(checked_i64(round_nonnegative(numerator, denominator)?)?);

        let samples = i128::from(self.row.samples);
        let maturity_bp = ratio_bp(samples, samples + 2)?;
        let stability_bp = match (self.row.current_low_nanousd, self.row.current_high_nanousd) {
            (Some(low), Some(high)) if high > 0 => ratio_bp(i128::from(low), i128::from(high))?,
            _ => 0,
        };
        let resolution = i128::from(
            self.row
                .measurement_resolution_fraction_units
                .unwrap_or(0)
                .max(self.row.anchor_resolution_fraction_units.unwrap_or(0)),
        );
        let quantisation_denominator = denominator
            .checked_add(
                resolution
                    .checked_mul(2)
                    .and_then(|value| value.checked_mul(samples))
                    .context("Suno confidence resolution overflow")?,
            )
            .context("Suno confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(denominator, quantisation_denominator)?;
        // Deterministic maturity x envelope stability x quantisation quality. This is a quality
        // score, not a probability.
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("Suno confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

/// Fold one observation into existing state, or anchor a fresh window.
///
/// Reads never create evidence: this is called only by the writer path.
pub(crate) fn apply_observation(
    existing: Option<SunoCalibrationRow>,
    observation: &SunoWindowObservation,
) -> anyhow::Result<SunoCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        // A stored row from a different estimator version is not authority: rebuild from the
        // immutable observation history instead of trusting a derived value.
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => SunoCalibration::from_row(row)?,
        Some(_) | None => SunoCalibration::anchor(observation)?,
    };
    calibration.observe(observation)?;
    let mut row = calibration.into_row();
    row.version = version;
    Ok(row)
}

/// Deterministically rebuild state from immutable observations, in order.
pub(crate) fn rebuild_from_history(
    observations: &[SunoWindowObservation],
) -> anyhow::Result<Option<SunoCalibrationRow>> {
    let mut current: Option<SunoCalibrationRow> = None;
    for observation in observations {
        current = Some(apply_observation(current, observation)?);
    }
    Ok(current)
}

/// Apply one observation, rebuilding a stale estimator version only from immutable raw history.
pub(crate) fn apply_observation_with_history(
    existing: Option<SunoCalibrationRow>,
    history: &[SunoWindowObservation],
    observation: &SunoWindowObservation,
) -> anyhow::Result<SunoCalibrationRow> {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut rebuilt = rebuild_from_history(history)?
        .context("missing immutable history for Suno estimator rebuild")?;
    rebuilt.version = version;
    apply_observation(Some(rebuilt), observation)
}

/// Pooled cohort capacity for like-for-like subscriptions (PROVIDER_ONBOARDING §10.6): the
/// exact same declared paid plan and the exact same native window duration pool their complete
/// intervals into one shared API-dollar capacity,
///
/// `pooled = round_half_up(FRACTION_SCALE * Σ observed_api_spend / Σ observed_fraction_units)`.
///
/// Rows of different plans or durations are never mixed, and a missing/legacy plan — one the
/// published limit table cannot corroborate — blocks aggregation entirely (`None`). Per-home
/// raw evidence and bounds stay on the rows; overflow fails closed.
pub(crate) fn pooled_cohort_capacity_nanousd(
    rows: &[SunoCalibrationRow],
) -> anyhow::Result<Option<i64>> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    if first.plan.is_empty() || published_window_limit_millicredits(&first.plan).is_none() {
        return Ok(None);
    }
    let mut observed_fraction_units: i64 = 0;
    let mut observed_spend_api_nanousd: i64 = 0;
    for row in rows {
        if row.plan != first.plan || row.window_duration_secs != first.window_duration_secs {
            return Ok(None);
        }
        observed_fraction_units = observed_fraction_units
            .checked_add(row.observed_fraction_units)
            .context("Suno cohort fraction overflow")?;
        observed_spend_api_nanousd = observed_spend_api_nanousd
            .checked_add(row.observed_spend_api_nanousd)
            .context("Suno cohort spend overflow")?;
    }
    if observed_fraction_units <= 0 {
        return Ok(None);
    }
    let numerator = i128::from(FRACTION_SCALE)
        .checked_mul(i128::from(observed_spend_api_nanousd))
        .context("Suno cohort capacity numerator overflow")?;
    Ok(Some(checked_i64(round_nonnegative(
        numerator,
        i128::from(observed_fraction_units),
    )?)?))
}

/// Apply a pooled cohort capacity to one member's current exact unused fraction. `None` while
/// the member's fraction is unknown — never zero.
pub(crate) fn pooled_remaining_nanousd(
    capacity_nanousd: i64,
    row: &SunoCalibrationRow,
) -> Option<i64> {
    let unused = FRACTION_SCALE.checked_sub(row.used_fraction_units?)?;
    let remaining = i128::from(capacity_nanousd)
        .checked_mul(i128::from(unused))?
        .checked_div(i128::from(FRACTION_SCALE))?;
    i64::try_from(remaining).ok()
}

fn interval_fraction_uncertainty(anchor_resolution: i64, current_resolution: i64) -> i64 {
    // Half the resolution at each endpoint of the interval.
    anchor_resolution / 2 + current_resolution / 2
}

fn validate_observation(observation: &SunoWindowObservation) -> anyhow::Result<()> {
    if observation.subject_id.is_empty() {
        anyhow::bail!("Suno calibration observation has no subject");
    }
    // Only the paid Pro/Premier tiers are admitted, by their canonical labels exactly as the
    // schema CHECK stores them; Free and any legacy/unknown tier fail closed.
    if published_window_limit_credits(&observation.plan).is_none() {
        anyhow::bail!("Suno calibration observation has an unsupported plan");
    }
    if observation.window_duration_secs <= 0 {
        anyhow::bail!("Suno calibration observation has an invalid window duration");
    }
    if observation.observed_at <= 0 || observation.reset_at.is_some_and(|reset| reset <= 0) {
        anyhow::bail!("Suno calibration observation has an invalid timestamp");
    }
    if observation.cumulative_api_nanousd < 0 || observation.cumulative_native_millicredits < 0 {
        anyhow::bail!("Suno calibration observation has a negative cumulative ledger");
    }
    if let Some(limit) = observation.native_limit_units {
        if limit <= 0 {
            anyhow::bail!("Suno calibration observation has an invalid quota limit");
        }
    }
    if let Some(used) = observation.native_used_units {
        if used < 0
            || observation
                .native_limit_units
                .is_some_and(|limit| used > limit)
        {
            anyhow::bail!("Suno calibration observation has an invalid quota usage");
        }
    }
    if let Some(remaining) = observation.native_remaining_units {
        if remaining < 0
            || observation
                .native_limit_units
                .is_some_and(|limit| remaining > limit)
        {
            anyhow::bail!("Suno calibration observation has an invalid quota remaining");
        }
    }
    if observation.used_fraction_units.is_some()
        != observation.measurement_resolution_fraction_units.is_some()
    {
        anyhow::bail!("Suno calibration observation fraction and resolution must move together");
    }
    if let Some(fraction) = observation.used_fraction_units {
        if !(0..=FRACTION_SCALE).contains(&fraction) {
            anyhow::bail!("Suno calibration observation fraction is out of range");
        }
    }
    if let Some(resolution) = observation.measurement_resolution_fraction_units {
        if !(1..=FRACTION_SCALE).contains(&resolution) {
            anyhow::bail!("Suno calibration observation resolution is out of range");
        }
    }
    match observation.observation_source.as_str() {
        "poll" if observation.source_request_id.is_none() => {}
        "response" if observation.source_request_id.is_some() => {}
        _ => anyhow::bail!("Suno calibration observation has an invalid source"),
    }
    Ok(())
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("Suno calibration value overflow"))
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("Suno calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("Suno calibration rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("Suno calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("Suno calibration ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("Suno calibration ratio overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::suno_fraction_from_native;

    const T0: i64 = 1_800_000_000;
    /// An exact monthly duration as reset evidence would show it (the schema keys on the
    /// observed duration; no synthetic constant is published anywhere).
    const MONTH: i64 = 2_628_000;
    /// Pro published limit: 2 500 credits.
    const PRO_LIMIT_MILLI: i64 = 2_500_000;

    /// A poll observation on the native-ledger path: the quota endpoint's field semantics are
    /// unproven, so every raw counter and the derived fraction stay `None` and only the exact
    /// cumulative dual ledgers move.
    fn native_obs(
        plan: &str,
        duration: i64,
        reset_at: Option<i64>,
        observed_at: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> SunoWindowObservation {
        SunoWindowObservation {
            subject_id: "sess_1".into(),
            plan: plan.into(),
            window_duration_secs: duration,
            reset_at,
            observed_at,
            native_limit_units: None,
            native_used_units: None,
            native_remaining_units: None,
            period_raw: None,
            used_fraction_units: None,
            measurement_resolution_fraction_units: None,
            cumulative_api_nanousd: cum_api,
            cumulative_native_millicredits: cum_native,
            observation_source: "poll".into(),
            source_request_id: None,
        }
    }

    fn pro_month(
        reset_at: Option<i64>,
        observed_at: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> SunoWindowObservation {
        native_obs("Pro", MONTH, reset_at, observed_at, cum_api, cum_native)
    }

    /// A poll observation on the quota-endpoint path, exactly as the writer builds it once the
    /// raw counters' field semantics are proven: raw counters preserved verbatim and the
    /// fraction derived via `suno_fraction_from_native`.
    fn quota_obs(
        plan: &str,
        duration: i64,
        reset_at: Option<i64>,
        observed_at: i64,
        used: i64,
        limit: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> SunoWindowObservation {
        let derived = suno_fraction_from_native(used, limit).expect("valid counters");
        SunoWindowObservation {
            native_limit_units: Some(limit),
            native_used_units: Some(used),
            native_remaining_units: Some(limit - used),
            used_fraction_units: Some(derived.used_fraction_units),
            measurement_resolution_fraction_units: Some(
                derived.measurement_resolution_fraction_units,
            ),
            ..native_obs(plan, duration, reset_at, observed_at, cum_api, cum_native)
        }
    }

    fn pro_quota(
        reset_at: Option<i64>,
        observed_at: i64,
        used: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> SunoWindowObservation {
        quota_obs("Pro", MONTH, reset_at, observed_at, used, 2_500, cum_api, cum_native)
    }

    #[test]
    fn the_first_snapshot_is_an_anchor_not_a_sample() {
        let row =
            apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 0, 0)).unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nanousd, None);
        assert_eq!(row.anchor_used_fraction_units, Some(0));
        // The published limit names the native window from the very first poll — exact, no
        // estimation.
        assert_eq!(row.native_limit_millicredits, Some(PRO_LIMIT_MILLI));
        assert_eq!(row.native_remaining_units(), Some(PRO_LIMIT_MILLI));
    }

    #[test]
    fn the_first_complete_interval_publishes_an_estimate() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        // 125 credits = 5% of the Pro window consumed $0.50 of derived API cost.
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // capacity = 1e8 * 5e8 / 5e6 = 1e10 nanoUSD = $10.00 (2 500 credits × $0.004).
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));
        assert!(row.current_low_nanousd.is_some());
        assert!(row.last_measured_at.is_some());
    }

    #[test]
    fn the_native_ledger_fraction_is_exact_and_fine_enough_to_bound_the_high() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        assert_eq!(row.measurement_resolution_fraction_units, Some(40));
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 20_000_000, 5_000),
        )
        .unwrap();
        // One song: 5 credits of 2 500 = 0.2% of the window; one millicredit resolves to 40
        // fraction units.
        assert_eq!(row.used_fraction_units, Some(200_000));
        assert!(
            row.current_high_nanousd.is_some(),
            "millicredit resolution makes the quantisation envelope negligible"
        );
    }

    #[test]
    fn a_coarse_quota_resolution_leaves_the_high_unbounded() {
        // On the quota path the endpoint's own resolution rules: a 1% move against a 1%
        // resolution is exactly the envelope width, so a finite high is not proved.
        let anchor = quota_obs("Pro", MONTH, Some(T0 + MONTH), T0 - 200, 0, 100, 0, 0);
        let row = apply_observation(None, &anchor).unwrap();
        assert_eq!(row.measurement_resolution_fraction_units, Some(1_000_000));
        let row = apply_observation(
            Some(row),
            &quota_obs("Pro", MONTH, Some(T0 + MONTH), T0 - 100, 1, 100, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(
            row.current_high_nanousd, None,
            "an unbounded high must stay null, never a guessed ceiling"
        );
        assert_eq!(row.current_capacity_nanousd, Some(100_000_000_000));
    }

    #[test]
    fn the_quota_endpoint_fraction_wins_when_present() {
        // The provider counters say 1% used while our own ledger alone would say 2%: the
        // window-absolute provider reading drives the state machine once it is carried.
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 200, 0, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 100, 25, 200_000_000, 50_000),
        )
        .unwrap();
        assert_eq!(row.used_fraction_units, Some(1_000_000));
        assert_eq!(row.observed_fraction_units, 1_000_000);
        // capacity = 1e8 * 2e8 / 1e6 = 2e10 = $20.00.
        assert_eq!(row.current_capacity_nanousd, Some(20_000_000_000));
    }

    #[test]
    fn quota_counters_without_a_derived_fraction_use_the_native_ledger() {
        // Raw counters preserved verbatim, but the writer could not derive a fraction: the
        // native-ledger path drives exactly as if no counters existed.
        let mut observation = pro_quota(Some(T0 + MONTH), T0 - 100, 25, 500_000_000, 125_000);
        observation.used_fraction_units = None;
        observation.measurement_resolution_fraction_units = None;
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        let row = apply_observation(Some(row), &observation).unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.used_fraction_units, Some(5_000_000));
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));
    }

    #[test]
    fn mixed_workload_intervals_accumulate_into_one_blend() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 400, 0, 0, 0)).unwrap();
        // 1% for $1.00 (a cheap generation), then 1% for $2.00 more (an expensive one).
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 300, 25, 1_000_000_000, 250_000),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 50, 3_000_000_000, 750_000),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        // Blend: 1e8 * 3e9 / 2e6 = 1.5e11 = $150 per window for this observed mix.
        assert_eq!(row.current_capacity_nanousd, Some(150_000_000_000));
        let low = row.current_low_nanousd.unwrap();
        let high = row.current_high_nanousd.unwrap();
        assert!(low <= 150_000_000_000 && high >= 150_000_000_000);
    }

    #[test]
    fn quota_before_settlement_holds_the_anchor() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 300, 0, 0, 0)).unwrap();
        // Quota moved but the turn FIFO has not advanced the ledgers yet.
        let held = apply_observation(
            Some(row.clone()),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 25, 0, 0),
        )
        .unwrap();
        assert_eq!(held.samples, 0);
        assert_eq!(held.anchor_used_fraction_units, row.anchor_used_fraction_units);

        // Settlement lands: the interval completes against the original anchor.
        let settled = apply_observation(
            Some(held),
            &pro_quota(Some(T0 + MONTH), T0 - 100, 25, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(settled.samples, 1);
        assert_eq!(settled.current_capacity_nanousd, Some(100_000_000_000));
    }

    #[test]
    fn repeated_quota_only_movement_is_recorded_as_unattributed() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 400, 0, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 300, 25, 0, 0),
        )
        .unwrap();
        // The same higher point again, still with no ledger movement: someone else's traffic.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 25, 0, 0),
        )
        .unwrap();
        assert_eq!(row.unattributed_fraction_units, 1_000_000);
        assert_eq!(row.samples, 0);
        assert_eq!(
            row.current_capacity_nanousd, None,
            "unattributed movement must never inflate capacity"
        );
    }

    #[test]
    fn a_reset_starts_a_new_interval_without_erasing_history() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 200, 0, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 100, 25, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);

        // The monthly refill lands: utilisation falls back and the reset advances a full window.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + 2 * MONTH), T0 + 10, 0, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives a reset");
        assert_eq!(row.anchor_used_fraction_units, Some(0));
        assert_eq!(row.reset_at, Some(T0 + 2 * MONTH));
        assert_eq!(row.current_capacity_nanousd, Some(100_000_000_000));
        // The new window measures from here.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + 2 * MONTH), T0 + 20, 25, 2_000_000_000, 500_000),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
    }

    #[test]
    fn bounded_reset_jitter_does_not_fork_the_window() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 200, 0, 0, 0)).unwrap();
        // The reset anchor drifts by a few seconds while utilisation keeps climbing.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH + 3), T0 - 100, 25, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(100_000_000_000));
    }

    #[test]
    fn a_fallback_with_material_reset_advance_rolls_the_window() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 300, 0, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 50, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // Utilisation fell back AND the reset advanced materially (beyond the jitter
        // tolerance, well under half the window): the window rolled.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH + 3_600), T0 - 100, 10, 1_000_000_000, 250_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "a roll re-anchors but keeps completed history");
        assert_eq!(row.anchor_used_fraction_units, Some(400_000));
        assert_eq!(row.used_fraction_units, Some(400_000));
    }

    #[test]
    fn a_rollback_to_an_old_high_water_is_not_new_spend() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 300, 0, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 50, 2_000_000_000, 500_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // A lower reading with no material reset advance: not a new window, not new evidence.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 100, 40, 2_000_000_000, 500_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn stale_observations_do_not_mutate_state() {
        let row =
            apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000))
                .unwrap();
        let after = apply_observation(
            Some(row.clone()),
            &pro_month(Some(T0 + MONTH), T0 - 500, 900_000_000, 225_000),
        )
        .unwrap();
        assert_eq!(after.used_fraction_units, row.used_fraction_units);
        assert_eq!(after.observed_at, row.observed_at);
    }

    #[test]
    fn a_duplicate_observation_is_a_no_op() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        let again = apply_observation(
            Some(row.clone()),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        assert_eq!(again.samples, 1);
        assert_eq!(again.current_capacity_nanousd, row.current_capacity_nanousd);
    }

    #[test]
    fn independent_durations_never_share_a_row() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 0, 0)).unwrap();
        // The same plan with a different exact duration is independent evidence, never folded.
        let err = apply_observation(
            Some(row),
            &native_obs("Pro", MONTH + 86_400, Some(T0 + MONTH), T0 - 50, 0, 0),
        )
        .unwrap_err();
        assert!(err.to_string().contains("identity mismatch"));
    }

    #[test]
    fn a_different_plan_is_a_different_cohort() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 0, 0)).unwrap();
        let upgraded = native_obs("Premier", MONTH, Some(T0 + MONTH), T0 - 50, 0, 0);
        assert!(apply_observation(Some(row), &upgraded).is_err());
    }

    #[test]
    fn an_unsupported_plan_fails_closed() {
        for plan in ["Free", "pro", "Basic", ""] {
            let observation = native_obs(plan, MONTH, Some(T0 + MONTH), T0 - 100, 0, 0);
            assert!(
                apply_observation(None, &observation).is_err(),
                "plan {plan:?} must fail closed"
            );
        }
    }

    #[test]
    fn a_fraction_basis_cutover_reanchors_without_erasing_history() {
        // Two complete intervals on the native-ledger path.
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 400, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 300, 500_000_000, 125_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.measurement_resolution_fraction_units, Some(40));

        // The field semantics go live: the first quota-carrying observation is a cutover —
        // it anchors a fresh interval on the provider-side reading, history intact.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 200, 125, 500_000_000, 125_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "a cutover re-anchors, it does not sample");
        assert_eq!(row.anchor_used_fraction_units, Some(5_000_000));
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));

        // The next quota observation completes an interval on the new basis.
        let row = apply_observation(
            Some(row),
            &pro_quota(Some(T0 + MONTH), T0 - 100, 150, 700_000_000, 175_000),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        assert_eq!(row.observed_fraction_units, 6_000_000);
    }

    #[test]
    fn window_usage_beyond_the_published_limit_rolls_the_window() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        // The ledger says 2 600 credits of a 2 500-credit window: the reset landed without
        // usable reset evidence. Re-anchor instead of publishing a fraction above 100%.
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 10_400_000_000, 2_600_000),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.used_fraction_units, Some(0));
        assert_eq!(row.anchor_spend_native_millicredits, 2_600_000);
    }

    #[test]
    fn history_rebuild_is_deterministic() {
        let history = vec![
            pro_month(Some(T0 + MONTH), T0 - 400, 0, 0),
            pro_month(Some(T0 + MONTH), T0 - 300, 500_000_000, 125_000),
            pro_month(Some(T0 + MONTH), T0 - 200, 1_000_000_000, 250_000),
        ];
        let rebuilt = rebuild_from_history(&history).unwrap().unwrap();
        let mut folded: Option<SunoCalibrationRow> = None;
        for observation in &history {
            folded = Some(apply_observation(folded, observation).unwrap());
        }
        assert_eq!(rebuilt, folded.unwrap());
        assert_eq!(rebuilt.samples, 2);
        assert_eq!(rebuilt.current_capacity_nanousd, Some(10_000_000_000));
    }

    #[test]
    fn an_estimator_version_change_rebuilds_instead_of_trusting_stored_values() {
        let anchor = pro_month(Some(T0 + MONTH), T0 - 200, 0, 0);
        let mut legacy = apply_observation(None, &anchor).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        legacy.current_capacity_nanousd = Some(999_999_999);
        legacy.samples = 42;
        legacy.version = 7;
        let rebuilt = apply_observation_with_history(
            Some(legacy),
            &[anchor],
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
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
            apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        assert!(apply_observation_with_history(
            Some(legacy),
            &[],
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .is_err());
    }

    #[test]
    fn remaining_uses_the_current_exact_fraction() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        // 95% of a $10.00 window remains; native remaining is exact from the published limit.
        assert_eq!(row.current_remaining_nano(), Some(9_500_000_000));
        assert_eq!(row.native_remaining_units(), Some(2_375_000));
    }

    #[test]
    fn native_remaining_follows_the_latest_proven_counters() {
        let row = apply_observation(None, &pro_quota(Some(T0 + MONTH), T0 - 200, 100, 0, 0)).unwrap();
        assert_eq!(row.native_limit_millicredits, Some(2_500_000));
        assert_eq!(row.native_used_millicredits, Some(100_000));
        assert_eq!(row.native_remaining_units(), Some(2_400_000));
        // A plan resize shows up in the raw counters; native remaining must follow the new
        // limit rather than drift against the stale one.
        let row = apply_observation(
            Some(row),
            &quota_obs("Pro", MONTH, Some(T0 + MONTH), T0 - 100, 100, 5_000, 0, 0),
        )
        .unwrap();
        assert_eq!(row.native_limit_millicredits, Some(5_000_000));
        assert_eq!(row.native_remaining_units(), Some(4_900_000));
    }

    #[test]
    fn a_row_round_trips_through_the_authority_shape() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        let calibration = SunoCalibration::from_row(row.clone()).unwrap();
        let restored = calibration.into_row();
        assert_eq!(restored, row);
        let next = pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000);
        assert_eq!(
            apply_observation(Some(restored), &next).unwrap(),
            apply_observation(Some(row), &next).unwrap()
        );
    }

    #[test]
    fn invalid_observations_fail_closed() {
        let mut no_subject = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        no_subject.subject_id = String::new();
        assert!(apply_observation(None, &no_subject).is_err());

        let mut bad_window = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        bad_window.window_duration_secs = 0;
        assert!(apply_observation(None, &bad_window).is_err());

        let mut bad_reset = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        bad_reset.reset_at = Some(0);
        assert!(apply_observation(None, &bad_reset).is_err());

        let mut fraction_without_resolution = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        fraction_without_resolution.used_fraction_units = Some(0);
        assert!(apply_observation(None, &fraction_without_resolution).is_err());

        let mut out_of_range = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        out_of_range.used_fraction_units = Some(FRACTION_SCALE + 1);
        out_of_range.measurement_resolution_fraction_units = Some(1);
        assert!(apply_observation(None, &out_of_range).is_err());

        let mut bad_resolution = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        bad_resolution.used_fraction_units = Some(0);
        bad_resolution.measurement_resolution_fraction_units = Some(0);
        assert!(apply_observation(None, &bad_resolution).is_err());

        let mut bad_usage = pro_quota(Some(T0 + MONTH), T0 - 100, 100, 0, 0);
        bad_usage.native_used_units = Some(3_000);
        assert!(apply_observation(None, &bad_usage).is_err());

        let negative = pro_month(Some(T0 + MONTH), T0 - 100, -1, 0);
        assert!(apply_observation(None, &negative).is_err());

        let mut bad_source = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        bad_source.source_request_id = Some("cal_1".into());
        assert!(apply_observation(None, &bad_source).is_err());

        let mut bad_response = pro_month(Some(T0 + MONTH), T0 - 100, 0, 0);
        bad_response.observation_source = "response".into();
        assert!(apply_observation(None, &bad_response).is_err());
    }

    #[test]
    fn regressing_cumulative_ledgers_fail_closed() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        assert!(
            apply_observation(Some(row), &pro_month(Some(T0 + MONTH), T0, 100_000_000, 125_000))
                .is_err()
        );
    }

    #[test]
    fn overflowing_spend_fails_closed_rather_than_wrapping() {
        let row = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        // 1e8 * i64::MAX exceeds i64 once divided back into a capacity.
        assert!(
            apply_observation(Some(row), &pro_month(Some(T0 + MONTH), T0 - 100, i64::MAX, 5_000))
                .is_err()
        );
    }

    #[test]
    fn the_published_limits_match_the_reviewed_ladder() {
        assert_eq!(published_window_limit_credits("Pro"), Some(2_500));
        assert_eq!(published_window_limit_credits("Premier"), Some(10_000));
        assert_eq!(published_window_limit_credits("Free"), None);
        assert_eq!(published_window_limit_millicredits("Pro"), Some(PRO_LIMIT_MILLI));
    }

    #[test]
    fn equal_plans_pool_into_one_cohort_capacity_applied_to_current_unused_fraction() {
        let mut first =
            apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 200, 0, 0)).unwrap();
        first.subject_id = "sess_1".into();
        let first = apply_observation(
            Some(first),
            &pro_month(Some(T0 + MONTH), T0 - 100, 500_000_000, 125_000),
        )
        .unwrap();
        let mut second_anchor = pro_month(Some(T0 + MONTH), T0 - 200, 0, 0);
        second_anchor.subject_id = "sess_2".into();
        let second = apply_observation(None, &second_anchor).unwrap();
        let mut second_spend = pro_month(Some(T0 + MONTH), T0 - 100, 1_000_000_000, 250_000);
        second_spend.subject_id = "sess_2".into();
        let second = apply_observation(Some(second), &second_spend).unwrap();

        let pooled = pooled_cohort_capacity_nanousd(&[first, second.clone()])
            .unwrap()
            .unwrap();
        // 1e8 * 1.5e9 / 1.5e7 = 1e10 = $10.00.
        assert_eq!(pooled, 10_000_000_000);
        // sess_2 used 10% of its window: 90% of the pooled capacity remains.
        assert_eq!(pooled_remaining_nanousd(pooled, &second), Some(9_000_000_000));
    }

    #[test]
    fn different_plans_never_mix_in_a_cohort() {
        let pro = apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 0, 0)).unwrap();
        let premier =
            apply_observation(
                None,
                &native_obs("Premier", MONTH, Some(T0 + MONTH), T0 - 100, 0, 0),
            )
            .unwrap();
        assert_eq!(pooled_cohort_capacity_nanousd(&[pro, premier]).unwrap(), None);
    }

    #[test]
    fn a_missing_or_legacy_plan_blocks_cohort_aggregation() {
        let mut legacy =
            apply_observation(None, &pro_month(Some(T0 + MONTH), T0 - 100, 0, 0)).unwrap();
        legacy.plan = "Basic".into();
        assert_eq!(pooled_cohort_capacity_nanousd(&[legacy]).unwrap(), None);
        assert_eq!(pooled_cohort_capacity_nanousd(&[]).unwrap(), None);
    }
}
