//! Evidence-only calibration of one GLM (Zhipu AI / Z.ai) Coding Plan subscription window.
//!
//! GLM is a GPT-like dual-ledger provider (`docs/engine/GLM_PROVIDER.md` §5.3): every settled
//! turn advances two independent exact cumulative ledgers — official API replacement cost
//! (nanoUSD) and native credits consumption (microcredits) from the provider's published
//! formula. One ledger is never reconstructed from the other. The window's native capacity is
//! **not** estimated: the plan's limits are published (2 000/12 000/28 000 credits per rolling
//! 5h and 10 000/60 000/140 000 weekly for Lite/Pro/Max). What is estimated is how much
//! official API replacement cost fits in the window for the observed workload:
//!
//! `capacity_nanoUSD = round_half_up(FRACTION_SCALE * ΣΔapi_nano / ΣΔused_fraction_units)`
//!
//! over complete intervals, exactly as the KIMI estimator does. The difference is where the
//! used fraction comes from, because the quota endpoint's counter units are unproven until a
//! controlled live run (manifest §6):
//!
//! * **Native-ledger path (primary while units are unproven).** The fraction is derived from
//!   our own exact credits ledger against the published window limit:
//!   `fraction_units = window_native_used_microcredits * FRACTION_SCALE / native_limit_microcredits`.
//!   The window baseline is the cumulative native total when the current window was anchored,
//!   so the delta between two observations is exact even mid-window. One microcredit of
//!   resolution makes the quantisation envelope negligible, so a finite high is provable from
//!   the first complete interval.
//! * **Quota-endpoint path (raw evidence until proven).** Once the unit semantics are proven
//!   live, the writer derives `used_fraction_units` from the provider's raw counters via
//!   `registry::glm_fraction_from_native` and the observation carries it; that provider-side
//!   window-absolute fraction then drives the state machine. Until then the nullable fields
//!   stay `None` together and the raw counters are preserved verbatim on the immutable
//!   observation — unknown is `None`, never `0`.
//!
//! Switching the fraction basis (native-ledger ↔ quota-endpoint) is a cutover: the estimator
//! sets a fresh common anchor for the window without erasing completed intervals, and old API
//! evidence stays valid history rather than being read as zero-native spend.
//!
//! There is no plan prior, subscription-price nominal, EMA, WLS, floating-point money or
//! hidden fallback here. All arithmetic is checked i64/i128 integer math with round-half-up;
//! overflow fails closed.

use anyhow::Context as _;
use registry::{
    GlmCalibrationRow, GlmWindowObservation, GLM_5H_WINDOW_SECS, GLM_FRACTION_SCALE,
    GLM_WEEKLY_WINDOW_SECS,
};

pub(crate) const FRACTION_SCALE: i64 = GLM_FRACTION_SCALE;
pub(crate) const ESTIMATOR_VERSION: i64 = 1;

/// Native credits are tracked in microcredits (credits × 1e6) so the published fractional
/// multipliers stay in integers.
const MICROCREDITS_PER_CREDIT: i64 = 1_000_000;

/// The window's published native size in credits for an exact declared plan and an exact
/// native duration (`docs/engine/GLM_PROVIDER.md` §5.3). This is provider-published fact,
/// never an estimate. A missing/legacy plan or an undocumented duration names no limit, which
/// keeps the native-ledger fraction path closed and blocks cohort aggregation.
pub(crate) fn published_window_limit_credits(plan: &str, window_duration_secs: i64) -> Option<i64> {
    match (plan, window_duration_secs) {
        ("Lite", GLM_5H_WINDOW_SECS) => Some(2_000),
        ("Pro", GLM_5H_WINDOW_SECS) => Some(12_000),
        ("Max", GLM_5H_WINDOW_SECS) => Some(28_000),
        ("Lite", GLM_WEEKLY_WINDOW_SECS) => Some(10_000),
        ("Pro", GLM_WEEKLY_WINDOW_SECS) => Some(60_000),
        ("Max", GLM_WEEKLY_WINDOW_SECS) => Some(140_000),
        _ => None,
    }
}

/// The same published limit in microcredits, checked.
pub(crate) fn published_window_limit_microcredits(
    plan: &str,
    window_duration_secs: i64,
) -> Option<i64> {
    published_window_limit_credits(plan, window_duration_secs)
        .and_then(|credits| credits.checked_mul(MICROCREDITS_PER_CREDIT))
}

/// Which evidence path produced the window's used fraction. The basis is part of the anchor's
/// identity: mixing deltas across bases would compare a tracking-relative reading against a
/// window-absolute one, so a basis change re-anchors the interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FractionSource {
    /// Provider-side, window-absolute, derived from the quota endpoint's raw counters once
    /// their units are proven live.
    QuotaEndpoint,
    /// Our own exact credits ledger against the published window limit, relative to the
    /// cumulative native total at the window's anchor.
    NativeLedger,
}

/// The fraction view of one observation, resolved through exactly one evidence path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ResolvedFraction {
    used_fraction_units: i64,
    resolution_fraction_units: i64,
    /// Native window halves in microcredits when either source can name them: the published
    /// limit on the native-ledger path, the proven raw counters × 1e6 on the quota path.
    native_limit_microcredits: Option<i64>,
    native_used_microcredits: Option<i64>,
    source: FractionSource,
}

/// Resolve an observation's used fraction. The quota-endpoint value wins when the observation
/// carries one (it is window-absolute and provider-side); otherwise the native-ledger path
/// derives the fraction from our own credits ledger against the published plan limit.
/// `native_baseline_microcredits` is the cumulative native total at the current window's
/// anchor; the window-scoped usage is the exact delta from it.
fn resolve_fraction(
    observation: &GlmWindowObservation,
    native_baseline_microcredits: Option<i64>,
) -> anyhow::Result<Option<ResolvedFraction>> {
    if let Some(quota_fraction) = observation.used_fraction_units {
        let resolution = observation
            .measurement_resolution_fraction_units
            .context("GLM quota fraction without its measurement resolution")?;
        let native_limit_microcredits = observation
            .native_limit_units
            .map(|limit| {
                limit
                    .checked_mul(MICROCREDITS_PER_CREDIT)
                    .context("GLM native limit overflow")
            })
            .transpose()?;
        let native_used_microcredits = observation
            .native_used_units
            .map(|used| {
                used.checked_mul(MICROCREDITS_PER_CREDIT)
                    .context("GLM native usage overflow")
            })
            .transpose()?;
        return Ok(Some(ResolvedFraction {
            used_fraction_units: quota_fraction,
            resolution_fraction_units: resolution,
            native_limit_microcredits,
            native_used_microcredits,
            source: FractionSource::QuotaEndpoint,
        }));
    }
    let Some(native_limit_microcredits) =
        published_window_limit_microcredits(&observation.plan, observation.window_duration_secs)
    else {
        return Ok(None);
    };
    let Some(baseline) = native_baseline_microcredits else {
        return Ok(None);
    };
    let native_used_microcredits = observation
        .cumulative_native_microcredits
        .checked_sub(baseline)
        .context("GLM native ledger regressed below the window baseline")?;
    let used_fraction_units = checked_i64(
        i128::from(native_used_microcredits)
            .checked_mul(i128::from(FRACTION_SCALE))
            .and_then(|value| value.checked_div(i128::from(native_limit_microcredits)))
            .context("GLM native fraction overflow")?,
    )?;
    // One microcredit expressed in fraction units. Any realistic window limit (billions of
    // microcredits) resolves far finer than one fraction unit, so the width clamps to 1 and
    // the quantisation envelope stays negligible.
    let resolution_fraction_units = checked_i64(ceil_nonnegative(
        i128::from(FRACTION_SCALE),
        i128::from(native_limit_microcredits),
    )?)?
    .max(1);
    Ok(Some(ResolvedFraction {
        used_fraction_units,
        resolution_fraction_units,
        native_limit_microcredits: Some(native_limit_microcredits),
        native_used_microcredits: Some(native_used_microcredits),
        source: FractionSource::NativeLedger,
    }))
}

/// The cumulative native total at the current window's anchor. The estimator maintains the
/// invariant `baseline = anchor_spend_native - native_used` across anchor advances, so the
/// baseline survives every completed interval inside the window.
fn native_baseline(row: &GlmCalibrationRow) -> Option<i64> {
    row.anchor_spend_native_microcredits
        .checked_sub(row.native_used_microcredits?)
}

/// Which basis produced the row's stored fraction, if any. On the native-ledger path the
/// stored fraction is always the exact function of the stored native halves, and its
/// resolution is the microcredit width; on the quota path the resolution is the endpoint's
/// own. When both would coincide numerically the bases are indistinguishable — and a cutover
/// between them is a no-op anyway.
fn stored_fraction_source(row: &GlmCalibrationRow) -> Option<FractionSource> {
    let used_fraction = row.used_fraction_units?;
    if let (Some(used), Some(limit)) = (row.native_used_microcredits, row.native_limit_microcredits)
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
            .context("GLM native fraction overflow")?,
    )?;
    let resolution_fraction_units = checked_i64(ceil_nonnegative(
        i128::from(FRACTION_SCALE),
        i128::from(limit),
    )?)?
    .max(1);
    Ok(Some(ResolvedFraction {
        used_fraction_units,
        resolution_fraction_units,
        native_limit_microcredits: Some(limit),
        native_used_microcredits: Some(used),
        source: FractionSource::NativeLedger,
    }))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GlmWindowCalibration {
    row: GlmCalibrationRow,
}

impl GlmWindowCalibration {
    fn anchor(observation: &GlmWindowObservation) -> anyhow::Result<Self> {
        validate_observation(observation)?;
        // The first snapshot baselines the window at the current cumulative native total:
        // exactly what our own ledger can prove, with no fabrication about consumption that
        // predates tracking.
        let resolved = resolve_fraction(
            observation,
            Some(observation.cumulative_native_microcredits),
        )?;
        Ok(Self {
            row: GlmCalibrationRow {
                subject_id: observation.subject_id.clone(),
                plan: observation.plan.clone(),
                window_duration_secs: observation.window_duration_secs,
                reset_at: observation.reset_at,
                anchor_used_fraction_units: resolved.map(|value| value.used_fraction_units),
                anchor_resolution_fraction_units: resolved
                    .map(|value| value.resolution_fraction_units),
                anchor_spend_api_nanousd: observation.cumulative_api_nanousd,
                anchor_spend_native_microcredits: observation.cumulative_native_microcredits,
                used_fraction_units: resolved.map(|value| value.used_fraction_units),
                measurement_resolution_fraction_units: resolved
                    .map(|value| value.resolution_fraction_units),
                observed_at: observation.observed_at,
                native_limit_microcredits: resolved
                    .and_then(|value| value.native_limit_microcredits),
                native_used_microcredits: resolved.and_then(|value| value.native_used_microcredits),
                observed_fraction_units: 0,
                observed_spend_api_nanousd: 0,
                observed_spend_native_microcredits: 0,
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

    pub(crate) fn from_row(row: GlmCalibrationRow) -> anyhow::Result<Self> {
        registry::validate_glm_calibration_row(&row)?;
        Ok(Self { row })
    }

    pub(crate) fn into_row(self) -> GlmCalibrationRow {
        self.row
    }

    fn observe(&mut self, observation: &GlmWindowObservation) -> anyhow::Result<()> {
        validate_observation(observation)?;
        // Identity is subject + declared paid plan + exact native duration. The rolling
        // 5-hour window and the weekly window are independent evidence and never fold into
        // one row; a different plan is a different cohort.
        if observation.subject_id != self.row.subject_id
            || observation.plan != self.row.plan
            || observation.window_duration_secs != self.row.window_duration_secs
        {
            anyhow::bail!("GLM calibration observation identity mismatch");
        }
        if observation.observed_at < self.row.observed_at {
            // Strictly older polls are stale and never mutate state.
            return Ok(());
        }
        if self.row.estimator_version != ESTIMATOR_VERSION {
            anyhow::bail!("GLM calibration estimator version mismatch");
        }
        // Both cumulative ledgers are monotone. A regression is corruption, not a reset:
        // fail closed rather than reinterpret it.
        if observation.cumulative_api_nanousd < self.row.anchor_spend_api_nanousd
            || observation.cumulative_native_microcredits
                < self.row.anchor_spend_native_microcredits
        {
            anyhow::bail!("GLM cumulative calibration ledger regressed");
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
        // A rolling window rolls over when utilisation falls back AND the reset advances
        // materially. Bounded timestamp jitter alone must not fork the window, and an unnamed
        // reset supplies no reset evidence at all.
        let rolling_window_rollover =
            fraction_fell_back && reset_delta.is_some_and(|delta| delta >= jitter_tolerance);
        // Our own ledger says the window usage outgrew the published window size: the window
        // slid under the baseline (rolling expiry) or the published limit went stale. Re-anchor
        // instead of publishing an impossible fraction above 100%.
        let window_slid = resolved.is_some_and(|value| {
            value.source == FractionSource::NativeLedger
                && match (
                    value.native_used_microcredits,
                    value.native_limit_microcredits,
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
            self.row.native_limit_microcredits = value.native_limit_microcredits;
            self.row.native_used_microcredits = value.native_used_microcredits;
        }
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;

        let Some(current) = resolved else {
            // No fraction source can read this window yet (unproven units and no published
            // limit). The cumulative anchors stay put so a later readable observation measures
            // the full span instead of losing the blind interval's spend.
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
            observation.cumulative_native_microcredits - self.row.anchor_spend_native_microcredits;

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
                    .context("GLM unattributed fraction overflow")?;
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
            .context("GLM observed fraction overflow")?;
        self.row.observed_spend_api_nanousd = self
            .row
            .observed_spend_api_nanousd
            .checked_add(delta_api)
            .context("GLM observed API spend overflow")?;
        self.row.observed_spend_native_microcredits = self
            .row
            .observed_spend_native_microcredits
            .checked_add(delta_native)
            .context("GLM observed native spend overflow")?;
        self.row.samples = self
            .row
            .samples
            .checked_add(1)
            .context("GLM calibration sample overflow")?;
        self.advance_anchor(observation, &current);
        self.row.last_measured_at = Some(observation.observed_at);
        self.recompute()
    }

    fn advance_anchor(&mut self, observation: &GlmWindowObservation, current: &ResolvedFraction) {
        // The baseline invariant holds across the advance: native_used was already refreshed
        // to `cumulative - baseline`, so `anchor_spend_native - native_used` keeps naming the
        // same window baseline.
        self.row.anchor_used_fraction_units = Some(current.used_fraction_units);
        self.row.anchor_resolution_fraction_units = Some(current.resolution_fraction_units);
        self.row.anchor_spend_api_nanousd = observation.cumulative_api_nanousd;
        self.row.anchor_spend_native_microcredits = observation.cumulative_native_microcredits;
    }

    /// Start a new interval. History is not erased: completed samples stay, because they remain
    /// valid evidence about how much cost fits in a window of this size.
    fn begin_window(&mut self, observation: &GlmWindowObservation) -> anyhow::Result<()> {
        // Re-baseline the native ledger at the current cumulative total: whatever the old
        // window consumed stays behind, and the new window measures from here.
        let resolved = resolve_fraction(
            observation,
            Some(observation.cumulative_native_microcredits),
        )?;
        self.row.reset_at = observation.reset_at;
        self.row.observed_at = observation.observed_at;
        self.row.updated_ts = observation.observed_at;
        if let Some(value) = resolved {
            self.row.used_fraction_units = Some(value.used_fraction_units);
            self.row.measurement_resolution_fraction_units = Some(value.resolution_fraction_units);
            self.row.native_limit_microcredits = value.native_limit_microcredits;
            self.row.native_used_microcredits = value.native_used_microcredits;
            self.advance_anchor(observation, &value);
        } else {
            // The new window is not readable yet: clear the old fraction legs rather than
            // anchor a new interval on the previous window's reading. The cumulative anchors
            // still advance so no spend is double-counted later.
            self.row.used_fraction_units = None;
            self.row.measurement_resolution_fraction_units = None;
            self.row.native_limit_microcredits = None;
            self.row.native_used_microcredits = None;
            self.row.anchor_used_fraction_units = None;
            self.row.anchor_resolution_fraction_units = None;
            self.row.anchor_spend_api_nanousd = observation.cumulative_api_nanousd;
            self.row.anchor_spend_native_microcredits = observation.cumulative_native_microcredits;
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
            .context("GLM workload envelope numerator overflow")?;
        let low = checked_i64(
            numerator
                .checked_div(i128::from(delta_used) + i128::from(uncertainty))
                .context("GLM workload low bound overflow")?,
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
            .context("GLM capacity numerator overflow")?;
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
                    .context("GLM confidence resolution overflow")?,
            )
            .context("GLM confidence denominator overflow")?;
        let quantisation_bp = ratio_bp(denominator, quantisation_denominator)?;
        // Deterministic maturity x envelope stability x quantisation quality. This is a quality
        // score, not a probability.
        let confidence_bp = maturity_bp
            .checked_mul(stability_bp)
            .and_then(|value| value.checked_div(10_000))
            .and_then(|value| value.checked_mul(quantisation_bp))
            .and_then(|value| value.checked_div(10_000))
            .context("GLM confidence overflow")?;
        self.row.current_confidence_bp = checked_i64(confidence_bp)?.clamp(0, 10_000);
        Ok(())
    }
}

/// Fold one observation into existing state, or anchor a fresh window.
///
/// Reads never create evidence: this is called only by the writer path.
pub(crate) fn apply_observation(
    existing: Option<GlmCalibrationRow>,
    observation: &GlmWindowObservation,
) -> anyhow::Result<GlmCalibrationRow> {
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut calibration = match existing {
        // A stored row from a different estimator version is not authority: rebuild from the
        // immutable observation history instead of trusting a derived value.
        Some(row) if row.estimator_version == ESTIMATOR_VERSION => {
            GlmWindowCalibration::from_row(row)?
        }
        Some(_) | None => GlmWindowCalibration::anchor(observation)?,
    };
    calibration.observe(observation)?;
    let mut row = calibration.into_row();
    row.version = version;
    Ok(row)
}

/// Deterministically rebuild state from immutable observations, in order.
pub(crate) fn rebuild_from_history(
    observations: &[GlmWindowObservation],
) -> anyhow::Result<Option<GlmCalibrationRow>> {
    let mut current: Option<GlmCalibrationRow> = None;
    for observation in observations {
        current = Some(apply_observation(current, observation)?);
    }
    Ok(current)
}

/// Apply one observation, rebuilding a stale estimator version only from immutable raw history.
pub(crate) fn apply_observation_with_history(
    existing: Option<GlmCalibrationRow>,
    history: &[GlmWindowObservation],
    observation: &GlmWindowObservation,
) -> anyhow::Result<GlmCalibrationRow> {
    if existing
        .as_ref()
        .is_none_or(|row| row.estimator_version == ESTIMATOR_VERSION)
    {
        return apply_observation(existing, observation);
    }
    let version = existing.as_ref().map_or(0, |row| row.version);
    let mut rebuilt = rebuild_from_history(history)?
        .context("missing immutable history for GLM estimator rebuild")?;
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
    rows: &[GlmCalibrationRow],
) -> anyhow::Result<Option<i64>> {
    let Some(first) = rows.first() else {
        return Ok(None);
    };
    if first.plan.is_empty()
        || published_window_limit_microcredits(&first.plan, first.window_duration_secs).is_none()
    {
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
            .context("GLM cohort fraction overflow")?;
        observed_spend_api_nanousd = observed_spend_api_nanousd
            .checked_add(row.observed_spend_api_nanousd)
            .context("GLM cohort spend overflow")?;
    }
    if observed_fraction_units <= 0 {
        return Ok(None);
    }
    let numerator = i128::from(FRACTION_SCALE)
        .checked_mul(i128::from(observed_spend_api_nanousd))
        .context("GLM cohort capacity numerator overflow")?;
    Ok(Some(checked_i64(round_nonnegative(
        numerator,
        i128::from(observed_fraction_units),
    )?)?))
}

/// Apply a pooled cohort capacity to one member's current exact unused fraction. `None` while
/// the member's fraction is unknown — never zero.
pub(crate) fn pooled_remaining_nanousd(
    capacity_nanousd: i64,
    row: &GlmCalibrationRow,
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

fn validate_observation(observation: &GlmWindowObservation) -> anyhow::Result<()> {
    if observation.subject_id.is_empty() {
        anyhow::bail!("GLM calibration observation has no subject");
    }
    if observation.plan.is_empty() {
        anyhow::bail!("GLM calibration observation has no declared paid plan");
    }
    if observation.window_duration_secs <= 0 {
        anyhow::bail!("GLM calibration observation has an invalid window duration");
    }
    if observation.observed_at <= 0 || observation.reset_at.is_some_and(|reset| reset <= 0) {
        anyhow::bail!("GLM calibration observation has an invalid timestamp");
    }
    if observation.cumulative_api_nanousd < 0 || observation.cumulative_native_microcredits < 0 {
        anyhow::bail!("GLM calibration observation has a negative cumulative ledger");
    }
    if let Some(limit) = observation.native_limit_units {
        if limit <= 0 {
            anyhow::bail!("GLM calibration observation has an invalid quota limit");
        }
    }
    if let Some(used) = observation.native_used_units {
        if used < 0
            || observation
                .native_limit_units
                .is_some_and(|limit| used > limit)
        {
            anyhow::bail!("GLM calibration observation has an invalid quota usage");
        }
    }
    if let Some(remaining) = observation.native_remaining_units {
        if remaining < 0
            || observation
                .native_limit_units
                .is_some_and(|limit| remaining > limit)
        {
            anyhow::bail!("GLM calibration observation has an invalid quota remaining");
        }
    }
    if observation.used_fraction_units.is_some()
        != observation.measurement_resolution_fraction_units.is_some()
    {
        anyhow::bail!("GLM calibration observation fraction and resolution must move together");
    }
    if let Some(fraction) = observation.used_fraction_units {
        if !(0..=FRACTION_SCALE).contains(&fraction) {
            anyhow::bail!("GLM calibration observation fraction is out of range");
        }
    }
    if let Some(resolution) = observation.measurement_resolution_fraction_units {
        if !(1..=FRACTION_SCALE).contains(&resolution) {
            anyhow::bail!("GLM calibration observation resolution is out of range");
        }
    }
    match observation.observation_source.as_str() {
        "poll" if observation.source_request_id.is_none() => {}
        "response" if observation.source_request_id.is_some() => {}
        _ => anyhow::bail!("GLM calibration observation has an invalid source"),
    }
    Ok(())
}

fn checked_i64(value: i128) -> anyhow::Result<i64> {
    i64::try_from(value).map_err(|_| anyhow::anyhow!("GLM calibration value overflow"))
}

fn round_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("GLM calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator / 2)
        .and_then(|value| value.checked_div(denominator))
        .context("GLM calibration rounding overflow")
}

fn ceil_nonnegative(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        anyhow::bail!("GLM calibration divides by a non-positive denominator");
    }
    numerator
        .checked_add(denominator - 1)
        .and_then(|value| value.checked_div(denominator))
        .context("GLM calibration ceiling overflow")
}

fn ratio_bp(numerator: i128, denominator: i128) -> anyhow::Result<i128> {
    if denominator <= 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(10_000)
        .and_then(|value| value.checked_div(denominator))
        .map(|value| value.clamp(0, 10_000))
        .context("GLM calibration ratio overflow")
}

#[cfg(test)]
mod tests {
    use super::*;
    use registry::glm_fraction_from_native;

    const T0: i64 = 1_800_000_000;
    const ROLLING: i64 = GLM_5H_WINDOW_SECS;
    const WEEK: i64 = GLM_WEEKLY_WINDOW_SECS;
    /// Pro rolling 5h limit: 12 000 credits.
    const PRO_5H_LIMIT_MICRO: i64 = 12_000_000_000;

    /// A poll observation on the native-ledger path: the quota endpoint's units are unproven,
    /// so every raw counter and the derived fraction stay `None` and only the exact cumulative
    /// dual ledgers move.
    fn native_obs(
        plan: &str,
        duration: i64,
        reset_at: Option<i64>,
        observed_at: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> GlmWindowObservation {
        GlmWindowObservation {
            subject_id: "u_1".into(),
            plan: plan.into(),
            window_duration_secs: duration,
            reset_at,
            observed_at,
            native_used_units: None,
            native_limit_units: None,
            native_remaining_units: None,
            percentage_raw: None,
            used_fraction_units: None,
            measurement_resolution_fraction_units: None,
            cumulative_api_nanousd: cum_api,
            cumulative_native_microcredits: cum_native,
            observation_source: "poll".into(),
            source_request_id: None,
        }
    }

    fn pro_5h(
        reset_at: Option<i64>,
        observed_at: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> GlmWindowObservation {
        native_obs("Pro", ROLLING, reset_at, observed_at, cum_api, cum_native)
    }

    fn pro_week(
        reset_at: Option<i64>,
        observed_at: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> GlmWindowObservation {
        native_obs("Pro", WEEK, reset_at, observed_at, cum_api, cum_native)
    }

    /// A poll observation on the quota-endpoint path, exactly as the writer builds it once the
    /// raw counters' units are proven: raw counters preserved verbatim and the fraction
    /// derived via `glm_fraction_from_native`.
    fn quota_obs(
        plan: &str,
        duration: i64,
        reset_at: Option<i64>,
        observed_at: i64,
        used: i64,
        limit: i64,
        cum_api: i64,
        cum_native: i64,
    ) -> GlmWindowObservation {
        let derived = glm_fraction_from_native(used, limit).expect("valid counters");
        GlmWindowObservation {
            native_used_units: Some(used),
            native_limit_units: Some(limit),
            native_remaining_units: Some(limit - used),
            used_fraction_units: Some(derived.used_fraction_units),
            measurement_resolution_fraction_units: Some(
                derived.measurement_resolution_fraction_units,
            ),
            ..native_obs(plan, duration, reset_at, observed_at, cum_api, cum_native)
        }
    }

    /// Build a measured Pro 5h row on the native-ledger path: `credits` consumed for
    /// `api_nanousd` of official replacement cost since the anchor.
    fn measured_pro_5h(api_nanousd: i64, credits_micro: i64) -> GlmCalibrationRow {
        let row = apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 200, 0, 0)).unwrap();
        apply_observation(
            Some(row),
            &pro_5h(Some(T0 + ROLLING), T0 - 100, api_nanousd, credits_micro),
        )
        .unwrap()
    }

    #[test]
    fn the_first_snapshot_is_an_anchor_not_a_sample() {
        let row = apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 100, 500, 40_000_000))
            .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.current_capacity_nanousd, None);
        assert_eq!(row.current_low_nanousd, None);
        // The native-ledger path baselines the window at the current cumulative total: our own
        // ledger proves nothing about consumption that predates tracking.
        assert_eq!(row.anchor_used_fraction_units, Some(0));
        assert_eq!(row.anchor_spend_native_microcredits, 40_000_000);
        assert_eq!(row.native_used_microcredits, Some(0));
        assert_eq!(row.native_limit_microcredits, Some(PRO_5H_LIMIT_MICRO));
        // Native remaining is exact from the very first poll, with no estimation at all.
        assert_eq!(row.native_remaining_units(), Some(PRO_5H_LIMIT_MICRO));
    }

    #[test]
    fn the_first_complete_interval_publishes_an_estimate() {
        let row = measured_pro_5h(1_000_000_000, 600_000_000);
        assert_eq!(row.samples, 1);
        // 600 of 12 000 published credits is exactly 5% of the window: the fraction comes from
        // our own ledger, not from the provider's counters.
        assert_eq!(row.used_fraction_units, Some(5_000_000));
        // capacity = 1e8 * 1e9 / 5e6 = 2e10 nanoUSD = $20 per 5h window.
        assert_eq!(row.current_capacity_nanousd, Some(20_000_000_000));
        assert!(row.current_low_nanousd.is_some());
        assert_eq!(row.observed_spend_native_microcredits, 600_000_000);
        assert!(row.last_measured_at.is_some());
    }

    #[test]
    fn the_native_ledger_fraction_is_exact_and_fine_enough_to_bound_the_high() {
        let row = measured_pro_5h(1_000_000_000, 600_000_000);
        // One microcredit of 12e9 is far finer than one fraction unit: the resolution clamps
        // to 1, so any real movement dwarfs the quantisation envelope and the high is provable
        // from the first interval.
        assert_eq!(row.measurement_resolution_fraction_units, Some(1));
        let low = row.current_low_nanousd.unwrap();
        let high = row
            .current_high_nanousd
            .expect("microcredit resolution must bound the high side");
        assert!(low <= 20_000_000_000 && high >= 20_000_000_000);
    }

    #[test]
    fn a_coarse_quota_resolution_leaves_the_high_unbounded() {
        // Quota path with whole-percent counters: a 1% move is exactly the envelope width, so
        // a finite high is not mathematically proved.
        let coarse = apply_observation(
            None,
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 200, 0, 100, 0, 0),
        )
        .unwrap();
        let coarse = apply_observation(
            Some(coarse),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 100,
                1,
                100,
                1_000_000_000,
                500_000_000,
            ),
        )
        .unwrap();
        assert_eq!(coarse.samples, 1);
        assert_eq!(coarse.current_capacity_nanousd, Some(100_000_000_000));
        assert_eq!(
            coarse.current_high_nanousd, None,
            "an unbounded high must stay null, never a guessed ceiling"
        );

        // The same 1% move measured to 0.1% is 10x the envelope width and bounds the high.
        let fine = apply_observation(
            None,
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 200, 0, 1_000, 0, 0),
        )
        .unwrap();
        let fine = apply_observation(
            Some(fine),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 100,
                10,
                1_000,
                1_000_000_000,
                500_000_000,
            ),
        )
        .unwrap();
        assert!(fine.current_high_nanousd.is_some());
    }

    #[test]
    fn mixed_workload_intervals_accumulate_into_one_blend() {
        let row = apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 400, 0, 0)).unwrap();
        // 5% of the window for $1.00 (a cheap cached turn), then 10% more for $3.00.
        let row = apply_observation(
            Some(row),
            &pro_5h(Some(T0 + ROLLING), T0 - 300, 1_000_000_000, 600_000_000),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &pro_5h(Some(T0 + ROLLING), T0 - 200, 4_000_000_000, 1_800_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        // Blend: 1e8 * 4e9 / 15e6 = 26 666 666 666.67, round-half-up.
        assert_eq!(row.current_capacity_nanousd, Some(26_666_666_667));
        let low = row.current_low_nanousd.unwrap();
        let high = row.current_high_nanousd.unwrap();
        assert!(low <= 26_666_666_667 && high >= 26_666_666_667);
    }

    #[test]
    fn quota_before_settlement_holds_the_anchor() {
        let row = apply_observation(
            None,
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 300, 0, 1_000, 0, 0),
        )
        .unwrap();
        // Provider quota moved 1% but neither ledger has advanced: the turn is not durable
        // yet. Hold the anchor once.
        let held = apply_observation(
            Some(row.clone()),
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 200, 10, 1_000, 0, 0),
        )
        .unwrap();
        assert_eq!(held.samples, 0);
        assert_eq!(
            held.anchor_used_fraction_units,
            row.anchor_used_fraction_units
        );

        // Settlement lands: the interval completes against the original anchor.
        let settled = apply_observation(
            Some(held),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 100,
                10,
                1_000,
                1_000_000_000,
                500_000_000,
            ),
        )
        .unwrap();
        assert_eq!(settled.samples, 1);
        assert_eq!(settled.current_capacity_nanousd, Some(100_000_000_000));
    }

    #[test]
    fn repeated_quota_only_movement_is_recorded_as_unattributed() {
        let row = apply_observation(
            None,
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 400, 0, 1_000, 0, 0),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 300, 10, 1_000, 0, 0),
        )
        .unwrap();
        // The same higher point again, still with no ledger movement: every supported tool
        // shares the account's quota, so this is someone else's traffic.
        let row = apply_observation(
            Some(row),
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 200, 10, 1_000, 0, 0),
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
        let row = apply_observation(None, &pro_week(Some(T0 + WEEK), T0 - 200, 0, 0)).unwrap();
        // 6 000 of 60 000 weekly credits = 10% for $1.00 -> $10 per weekly window.
        let row = apply_observation(
            Some(row),
            &pro_week(Some(T0 + WEEK), T0 - 100, 1_000_000_000, 6_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));

        // The weekly reset advances a full window.
        let row = apply_observation(
            Some(row),
            &pro_week(Some(T0 + 2 * WEEK), T0 + 10, 1_000_000_000, 6_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives a reset");
        assert_eq!(row.anchor_used_fraction_units, Some(0));
        assert_eq!(row.reset_at, Some(T0 + 2 * WEEK));
        assert_eq!(row.native_used_microcredits, Some(0));
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));
    }

    #[test]
    fn a_rolling_window_rolls_over_on_fallback_with_material_reset_advance() {
        let reset_1 = T0 + ROLLING;
        let row = apply_observation(
            None,
            &quota_obs("Pro", ROLLING, Some(reset_1), T0 - 300, 1_200, 12_000, 0, 0),
        )
        .unwrap();
        // 10% more for $2.00: one complete interval on the rolling 5h window.
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                ROLLING,
                Some(reset_1),
                T0 - 200,
                2_400,
                12_000,
                2_000_000_000,
                2_400_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(20_000_000_000));

        // Utilisation falls back to 2.5% and the reset advances materially: the rolling
        // window rolled over. History stays.
        let reset_2 = reset_1 + 1_800;
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                ROLLING,
                Some(reset_2),
                T0 - 100,
                300,
                12_000,
                2_000_000_000,
                2_400_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives the rollover");
        assert_eq!(row.anchor_used_fraction_units, Some(2_500_000));
        assert_eq!(row.reset_at, Some(reset_2));
        assert_eq!(row.current_capacity_nanousd, Some(20_000_000_000));
    }

    #[test]
    fn bounded_reset_jitter_does_not_fork_the_window() {
        let row = apply_observation(None, &pro_week(Some(T0 + WEEK), T0 - 200, 0, 0)).unwrap();
        // The reset drifts by a few seconds while utilisation keeps climbing: same interval.
        let row = apply_observation(
            Some(row),
            &pro_week(Some(T0 + WEEK + 3), T0 - 100, 1_000_000_000, 6_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));
        assert_eq!(row.reset_at, Some(T0 + WEEK + 3));
    }

    #[test]
    fn a_rollback_to_an_old_high_water_is_not_new_spend() {
        let row = apply_observation(
            None,
            &quota_obs("Pro", WEEK, Some(T0 + WEEK), T0 - 300, 0, 1_000, 0, 0),
        )
        .unwrap();
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 200,
                20,
                1_000,
                2_000_000_000,
                1_000_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        // A lower reading with no material reset advance: not a new window, not new evidence.
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 100,
                15,
                1_000,
                2_000_000_000,
                1_000_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
    }

    #[test]
    fn stale_observations_do_not_mutate_state() {
        let row = apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 100, 500, 40_000_000))
            .unwrap();
        let after = apply_observation(
            Some(row.clone()),
            &pro_5h(Some(T0 + ROLLING), T0 - 500, 9_000, 90_000_000),
        )
        .unwrap();
        assert_eq!(after.used_fraction_units, row.used_fraction_units);
        assert_eq!(after.observed_at, row.observed_at);
    }

    #[test]
    fn independent_durations_never_share_a_row() {
        let weekly =
            apply_observation(None, &pro_week(Some(T0 + WEEK), T0 - 100, 500, 40_000_000)).unwrap();
        // A rolling 5h observation must be refused against weekly state, not folded in.
        let err = apply_observation(
            Some(weekly),
            &pro_5h(Some(T0 + ROLLING), T0 - 50, 900, 50_000_000),
        )
        .unwrap_err();
        assert!(err.to_string().contains("identity mismatch"));
    }

    #[test]
    fn a_different_plan_is_a_different_cohort() {
        let row =
            apply_observation(None, &pro_week(Some(T0 + WEEK), T0 - 100, 500, 40_000_000)).unwrap();
        let upgraded = native_obs("Max", WEEK, Some(T0 + WEEK), T0 - 50, 900, 50_000_000);
        assert!(apply_observation(Some(row), &upgraded).is_err());
    }

    #[test]
    fn a_fraction_basis_cutover_reanchors_without_erasing_history() {
        // Native-ledger era: 10% of the weekly Pro window for $1.00.
        let row = apply_observation(None, &pro_week(Some(T0 + WEEK), T0 - 200, 0, 0)).unwrap();
        let row = apply_observation(
            Some(row),
            &pro_week(Some(T0 + WEEK), T0 - 100, 1_000_000_000, 6_000_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1);
        assert_eq!(row.measurement_resolution_fraction_units, Some(1));

        // Live proof lands and the writer starts deriving the provider-side fraction: the
        // basis switches native-ledger -> quota-endpoint. The new reading anchors a fresh
        // interval; completed history is not erased and old API evidence is not read as
        // zero-native spend.
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 50,
                15_000,
                60_000,
                1_000_000_000,
                6_000_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "the cutover keeps completed intervals");
        assert_eq!(row.current_capacity_nanousd, Some(10_000_000_000));
        assert_eq!(row.anchor_used_fraction_units, Some(25_000_000));
        assert_eq!(row.anchor_resolution_fraction_units, Some(1_667));
        assert_eq!(row.native_used_microcredits, Some(15_000_000_000));

        // Intervals keep completing on the new basis and blend with the preserved history:
        // fraction 10% + 5%, spend $1 + $1 -> 1e8 * 2e9 / 15e6 = 13 333 333 333.33.
        let row = apply_observation(
            Some(row),
            &quota_obs(
                "Pro",
                WEEK,
                Some(T0 + WEEK),
                T0 - 10,
                18_000,
                60_000,
                2_000_000_000,
                12_000_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 2);
        assert_eq!(row.current_capacity_nanousd, Some(13_333_333_333));
    }

    #[test]
    fn window_usage_beyond_the_published_limit_rolls_the_window() {
        let row = measured_pro_5h(1_000_000_000, 600_000_000);
        assert_eq!(row.samples, 1);
        // Our own ledger now names more consumption than the published 12 000-credit window
        // holds: the rolling window slid under the baseline. Re-anchor rather than publish a
        // fraction above 100%.
        let row = apply_observation(
            Some(row),
            &pro_5h(Some(T0 + ROLLING), T0 - 50, 2_000_000_000, 12_600_000_000),
        )
        .unwrap();
        assert_eq!(row.samples, 1, "completed evidence survives the slide");
        assert_eq!(row.anchor_used_fraction_units, Some(0));
        assert_eq!(row.native_used_microcredits, Some(0));
        assert_eq!(row.anchor_spend_native_microcredits, 12_600_000_000);
        assert_eq!(row.current_capacity_nanousd, Some(20_000_000_000));
    }

    #[test]
    fn unproven_units_without_a_published_limit_keep_the_window_cold() {
        // A legacy plan the published table cannot corroborate: raw counters are preserved as
        // evidence but name no fraction, and no capacity is ever derived from them.
        let row = apply_observation(
            None,
            &native_obs("Legacy", WEEK, Some(T0 + WEEK), T0 - 200, 0, 0),
        )
        .unwrap();
        assert_eq!(row.used_fraction_units, None);
        assert_eq!(row.native_limit_microcredits, None);
        let row = apply_observation(
            Some(row),
            &native_obs(
                "Legacy",
                WEEK,
                Some(T0 + WEEK),
                T0 - 100,
                5_000_000_000,
                2_000_000_000,
            ),
        )
        .unwrap();
        assert_eq!(row.samples, 0);
        assert_eq!(row.observed_spend_api_nanousd, 0);
        assert_eq!(row.current_capacity_nanousd, None);
    }

    #[test]
    fn history_rebuild_is_deterministic() {
        let history = vec![
            pro_5h(Some(T0 + ROLLING), T0 - 400, 0, 0),
            pro_5h(Some(T0 + ROLLING), T0 - 300, 1_000_000_000, 600_000_000),
            pro_5h(Some(T0 + ROLLING), T0 - 200, 4_000_000_000, 1_800_000_000),
        ];
        let rebuilt = rebuild_from_history(&history).unwrap().unwrap();
        let mut folded: Option<GlmCalibrationRow> = None;
        for observation in &history {
            folded = Some(apply_observation(folded, observation).unwrap());
        }
        assert_eq!(rebuilt, folded.unwrap());
        assert_eq!(rebuilt.current_capacity_nanousd, Some(26_666_666_667));
    }

    #[test]
    fn an_estimator_version_change_rebuilds_instead_of_trusting_stored_values() {
        let anchor = pro_5h(Some(T0 + ROLLING), T0 - 200, 0, 0);
        let mut legacy = apply_observation(None, &anchor).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        legacy.current_capacity_nanousd = Some(999_999_999);
        legacy.samples = 42;
        legacy.version = 7;
        let rebuilt = apply_observation_with_history(
            Some(legacy),
            std::slice::from_ref(&anchor),
            &pro_5h(Some(T0 + ROLLING), T0 - 100, 1_000_000_000, 600_000_000),
        )
        .unwrap();
        assert_eq!(rebuilt.estimator_version, ESTIMATOR_VERSION);
        assert_eq!(
            rebuilt.version, 7,
            "the outer CAS generation must be preserved"
        );
        assert_eq!(rebuilt.samples, 1);
        assert_ne!(rebuilt.current_capacity_nanousd, Some(999_999_999));
    }

    #[test]
    fn an_estimator_version_change_without_history_fails_closed() {
        let mut legacy =
            apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 200, 0, 0)).unwrap();
        legacy.estimator_version = ESTIMATOR_VERSION + 1;
        assert!(apply_observation_with_history(
            Some(legacy),
            &[],
            &pro_5h(Some(T0 + ROLLING), T0 - 100, 1_000_000_000, 600_000_000),
        )
        .is_err());
    }

    #[test]
    fn remaining_uses_the_current_exact_fraction() {
        let row = measured_pro_5h(1_000_000_000, 600_000_000);
        // 95% of a $20 window remains.
        assert_eq!(row.current_remaining_nano(), Some(19_000_000_000));
        // 11 400 of 12 000 credits remain, exactly and without any estimation.
        assert_eq!(row.native_remaining_units(), Some(11_400_000_000));
    }

    #[test]
    fn invalid_observations_fail_closed() {
        let good = pro_5h(Some(T0 + ROLLING), T0 - 100, 500, 40_000_000);

        let mut no_plan = good.clone();
        no_plan.plan = String::new();
        assert!(apply_observation(None, &no_plan).is_err());

        let mut bad_window = good.clone();
        bad_window.window_duration_secs = 0;
        assert!(apply_observation(None, &bad_window).is_err());

        let mut bad_timestamp = good.clone();
        bad_timestamp.observed_at = 0;
        assert!(apply_observation(None, &bad_timestamp).is_err());

        let mut negative_ledger = good.clone();
        negative_ledger.cumulative_api_nanousd = -1;
        assert!(apply_observation(None, &negative_ledger).is_err());

        // Fraction and resolution must move together.
        let mut split_pair = quota_obs(
            "Pro",
            ROLLING,
            Some(T0 + ROLLING),
            T0 - 100,
            10,
            1_000,
            0,
            0,
        );
        split_pair.measurement_resolution_fraction_units = None;
        assert!(apply_observation(None, &split_pair).is_err());

        let mut bad_resolution = quota_obs(
            "Pro",
            ROLLING,
            Some(T0 + ROLLING),
            T0 - 100,
            10,
            1_000,
            0,
            0,
        );
        bad_resolution.measurement_resolution_fraction_units = Some(0);
        assert!(apply_observation(None, &bad_resolution).is_err());

        // Raw counters stay sane when present.
        let mut bad_raw = quota_obs(
            "Pro",
            ROLLING,
            Some(T0 + ROLLING),
            T0 - 100,
            10,
            1_000,
            0,
            0,
        );
        bad_raw.native_used_units = Some(2_000);
        assert!(apply_observation(None, &bad_raw).is_err());

        // A poll invents no request id; a response names its carrier.
        let mut poll_with_request = good.clone();
        poll_with_request.source_request_id = Some("req_1".into());
        assert!(apply_observation(None, &poll_with_request).is_err());
        let mut response_without_request = good.clone();
        response_without_request.observation_source = "response".into();
        assert!(apply_observation(None, &response_without_request).is_err());
        let mut unknown_source = good;
        unknown_source.observation_source = "guess".into();
        assert!(apply_observation(None, &unknown_source).is_err());
    }

    #[test]
    fn regressing_cumulative_ledgers_fail_closed() {
        let row = apply_observation(
            None,
            &pro_5h(Some(T0 + ROLLING), T0 - 200, 5_000_000_000, 3_000_000_000),
        )
        .unwrap();
        // A cumulative total can never shrink; a regression is corruption, not a reset.
        let regressed_api = pro_5h(Some(T0 + ROLLING), T0 - 100, 4_999_999_999, 3_000_000_000);
        assert!(apply_observation(Some(row.clone()), &regressed_api).is_err());
        let regressed_native = pro_5h(Some(T0 + ROLLING), T0 - 100, 5_000_000_000, 2_999_999_999);
        assert!(apply_observation(Some(row), &regressed_native).is_err());
    }

    #[test]
    fn overflowing_spend_fails_closed_rather_than_wrapping() {
        let row = apply_observation(None, &pro_5h(Some(T0 + ROLLING), T0 - 200, 0, 0)).unwrap();
        let huge = pro_5h(Some(T0 + ROLLING), T0 - 100, i64::MAX, 600_000_000);
        // 1e8 * i64::MAX exceeds i64 once divided back into a capacity.
        assert!(apply_observation(Some(row), &huge).is_err());
    }

    #[test]
    fn equal_plans_pool_into_one_cohort_capacity_applied_to_current_unused_fraction() {
        // Two Pro 5h homes with different observed workloads: 5% for $1 and 15% for $6.
        let first = measured_pro_5h(1_000_000_000, 600_000_000);
        let second = measured_pro_5h(6_000_000_000, 1_800_000_000);
        let pooled = pooled_cohort_capacity_nanousd(&[first.clone(), second]).unwrap();
        // 1e8 * (1e9 + 6e9) / (5e6 + 15e6) = $35 per window for the blend of both homes.
        assert_eq!(pooled, Some(35_000_000_000));
        // Applied to the first home's current exact unused fraction: 95% of $35.
        assert_eq!(
            pooled_remaining_nanousd(35_000_000_000, &first),
            Some(33_250_000_000)
        );
    }

    #[test]
    fn different_plans_never_mix_in_a_cohort() {
        let pro = measured_pro_5h(1_000_000_000, 600_000_000);
        let mut max = measured_pro_5h(1_000_000_000, 600_000_000);
        max.plan = "Max".into();
        assert_eq!(pooled_cohort_capacity_nanousd(&[pro, max]).unwrap(), None);
    }

    #[test]
    fn a_missing_or_legacy_plan_blocks_cohort_aggregation() {
        let mut legacy = measured_pro_5h(1_000_000_000, 600_000_000);
        legacy.plan = "Legacy".into();
        assert_eq!(pooled_cohort_capacity_nanousd(&[legacy]).unwrap(), None);

        let mut missing = measured_pro_5h(1_000_000_000, 600_000_000);
        missing.plan = String::new();
        assert_eq!(pooled_cohort_capacity_nanousd(&[missing]).unwrap(), None);
    }
}
