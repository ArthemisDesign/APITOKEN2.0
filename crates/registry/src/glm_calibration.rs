//! Exact plan-scoped GLM (Zhipu AI / Z.ai) Coding Plan subscription calibration types.
//!
//! Shape and rationale: `docs/engine/GLM_PROVIDER.md` §5, migration
//! `0029_glm_window_calibration.sql`. Two differences from the KIMI equivalent are
//! load-bearing:
//!
//! * GLM is a GPT-like dual-ledger provider: every turn carries the official API replacement
//!   cost (nanoUSD) AND the native credits consumption (microcredits, credits × 1e6) computed
//!   from the provider's published formula. The two ledgers are independent — one is never
//!   reconstructed from the other.
//! * The quota endpoint's raw counters have unproven units, so observations store them
//!   verbatim with `None` for unknown — never `0`. The derived fraction and its measurement
//!   resolution exist only once the unit semantics are proven live.

use anyhow::bail;

/// Fixed-point scale shared with the other providers: 100% == 100_000_000 units.
pub const GLM_FRACTION_SCALE: i64 = 100_000_000;

/// Documented duration of the rolling short-term window: the 5-hour credits reset five hours
/// after consumption.
pub const GLM_5H_WINDOW_SECS: i64 = 18_000;
/// Documented duration of the weekly window: seven days from the order timestamp.
pub const GLM_WEEKLY_WINDOW_SECS: i64 = 604_800;

/// One immutable quota observation for a single GLM window.
///
/// Quota is served from `GET /api/monitor/usage/quota/limit` and may also arrive on a
/// generation response, so the source is explicit: a response-carried observation names the
/// request that carried it, a poll invents no request id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmWindowObservation {
    pub subject_id: String,
    /// Declared paid plan (`Lite`/`Pro`/`Max`). No machine-readable plan identity exists, so
    /// the declared plan is the cohort key.
    pub plan: String,
    /// Exact native window duration in seconds. Independent durations never share a row.
    pub window_duration_secs: i64,
    /// `nextResetTime` when the provider supplied it; a rolling window may not name one.
    pub reset_at: Option<i64>,
    pub observed_at: i64,
    /// Raw provider counters. Their unit is unproven, so they are preserved verbatim and
    /// unknown stays `None` — never `0`.
    pub native_used_units: Option<i64>,
    pub native_limit_units: Option<i64>,
    pub native_remaining_units: Option<i64>,
    pub percentage_raw: Option<i64>,
    /// Derived from the raw counters via [`glm_fraction_from_native`], only once the unit
    /// semantics are proven. Until then both stay `None` together.
    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
    /// Cumulative dual ledgers for this subject at observation time.
    pub cumulative_api_nanousd: i64,
    pub cumulative_native_microcredits: i64,
    /// `poll` or `response`.
    pub observation_source: String,
    /// The request that carried a response-source observation. Never invented for a poll.
    pub source_request_id: Option<String>,
}

/// One immutable, priced GLM turn.
///
/// Requested and served model are separate fields on purpose: the provider silently re-routes
/// glm-5.1/glm-5 onto glm-5.2, which carries a different rate card, so billing follows
/// `served_model` while `requested_model` preserves what the customer actually asked for.
///
/// Every turn carries two independent exact ledgers: the official API replacement cost
/// (nanoUSD) and the native credits consumption (microcredits) from the provider's published
/// formula.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmTurnCalibrationEvent {
    /// Internal CSPRNG id, stable across every pre-byte retry. Never an upstream id.
    pub request_id: String,
    pub subject_id: String,
    pub plan: String,
    pub requested_model: String,
    pub served_model: String,
    /// `200k` or `1m`.
    pub context_mode: String,
    /// `off`, `high` or `max` after the provider's mapping (low/medium→high, xhigh→max,
    /// none/minimal→off). `None` for models that take no reasoning effort at all.
    pub reasoning_effort: Option<String>,
    pub api_tariff_schedule_id: String,
    pub credit_schedule_id: String,
    pub priced_ts: i64,
    pub completed_at: i64,

    pub fresh_input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    /// Subset of `output_tokens`.
    pub reasoning_tokens: i64,

    pub api_fresh_input_nanousd: i64,
    pub api_cached_input_nanousd: i64,
    pub api_output_nanousd: i64,
    pub api_total_nanousd: i64,

    pub native_fresh_input_microcredits: i64,
    pub native_cached_input_microcredits: i64,
    pub native_output_microcredits: i64,
    pub native_total_microcredits: i64,
    /// Whether the off-peak ×0.5 schedule (outside Mon–Fri 14:00–18:00 UTC+8) was applied.
    pub off_peak: bool,
}

/// Why a turn event cannot be persisted. Every variant refuses before money or evidence moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlmEventError {
    MissingIdentity,
    InvalidTimestamp,
    NegativeCounter,
    ReasoningExceedsOutput,
    /// API-dollar legs do not sum to the recorded total.
    LegsDoNotSum,
    /// Native credit legs do not sum to the recorded total.
    NativeLegsDoNotSum,
    /// A turn with no usage at all is not evidence of anything.
    EmptyUsage,
    UnsupportedContextMode,
    UnsupportedReasoningEffort,
}

impl std::fmt::Display for GlmEventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::MissingIdentity => "GLM turn event is missing an identity field",
            Self::InvalidTimestamp => "GLM turn event has an invalid timestamp",
            Self::NegativeCounter => "GLM turn event has a negative counter",
            Self::ReasoningExceedsOutput => "GLM reasoning tokens exceed output tokens",
            Self::LegsDoNotSum => "GLM API cost legs do not sum to the recorded total",
            Self::NativeLegsDoNotSum => "GLM native credit legs do not sum to the recorded total",
            Self::EmptyUsage => "GLM turn event carries no usage",
            Self::UnsupportedContextMode => "GLM turn event has an unsupported context mode",
            Self::UnsupportedReasoningEffort => {
                "GLM turn event has an unsupported reasoning effort"
            }
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for GlmEventError {}

/// A request id already names a different immutable GLM turn. This is permanent corruption of
/// the proposed event, not a transient database failure, so the runtime FIFO quarantines
/// exactly that row and continues with its tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmTurnReplayConflict;

impl std::fmt::Display for GlmTurnReplayConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("GLM turn calibration replay conflict for the same request id")
    }
}

impl std::error::Error for GlmTurnReplayConflict {}

pub fn is_glm_turn_replay_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<GlmTurnReplayConflict>())
}

impl GlmTurnCalibrationEvent {
    /// Validate before persistence. These mirror the migration's CHECK constraints so a bad
    /// event is refused in Rust with a typed reason rather than as an opaque database error.
    pub fn validate(&self) -> std::result::Result<(), GlmEventError> {
        for field in [
            &self.request_id,
            &self.subject_id,
            &self.plan,
            &self.requested_model,
            &self.served_model,
            &self.api_tariff_schedule_id,
            &self.credit_schedule_id,
        ] {
            if field.is_empty() {
                return Err(GlmEventError::MissingIdentity);
            }
        }
        if !matches!(self.context_mode.as_str(), "200k" | "1m") {
            return Err(GlmEventError::UnsupportedContextMode);
        }
        if let Some(effort) = &self.reasoning_effort {
            if !matches!(effort.as_str(), "off" | "high" | "max") {
                return Err(GlmEventError::UnsupportedReasoningEffort);
            }
        }
        if self.priced_ts <= 0 || self.completed_at <= 0 {
            return Err(GlmEventError::InvalidTimestamp);
        }
        let counters = [
            self.fresh_input_tokens,
            self.cached_input_tokens,
            self.cache_write_tokens,
            self.output_tokens,
            self.reasoning_tokens,
            self.api_fresh_input_nanousd,
            self.api_cached_input_nanousd,
            self.api_output_nanousd,
            self.native_fresh_input_microcredits,
            self.native_cached_input_microcredits,
            self.native_output_microcredits,
        ];
        if counters.iter().any(|value| *value < 0)
            || self.api_total_nanousd <= 0
            || self.native_total_microcredits <= 0
        {
            return Err(GlmEventError::NegativeCounter);
        }
        if self.reasoning_tokens > self.output_tokens {
            return Err(GlmEventError::ReasoningExceedsOutput);
        }
        if self.fresh_input_tokens == 0
            && self.cached_input_tokens == 0
            && self.cache_write_tokens == 0
            && self.output_tokens == 0
        {
            return Err(GlmEventError::EmptyUsage);
        }
        let api_sum = self
            .api_fresh_input_nanousd
            .checked_add(self.api_cached_input_nanousd)
            .and_then(|value| value.checked_add(self.api_output_nanousd))
            .ok_or(GlmEventError::LegsDoNotSum)?;
        if api_sum != self.api_total_nanousd {
            return Err(GlmEventError::LegsDoNotSum);
        }
        let native_sum = self
            .native_fresh_input_microcredits
            .checked_add(self.native_cached_input_microcredits)
            .and_then(|value| value.checked_add(self.native_output_microcredits))
            .ok_or(GlmEventError::NativeLegsDoNotSum)?;
        if native_sum != self.native_total_microcredits {
            return Err(GlmEventError::NativeLegsDoNotSum);
        }
        Ok(())
    }

    /// Whether a stored row is the exact same semantic event, for idempotent replay.
    ///
    /// A retry of the same internal request id must be a no-op. A *different* payload under
    /// that id is a typed conflict, never an update: overwriting would silently rewrite priced
    /// history.
    pub fn is_exact_replay_of(&self, stored: &Self) -> bool {
        self == stored
    }
}

/// Cumulative dual ledgers for one subject. The two totals are independent exact sums over the
/// immutable turn events; one is never derived from the other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GlmSubjectSpend {
    pub spent_api_nanousd: i64,
    pub spent_native_microcredits: i64,
}

/// Estimator state for one subject + declared plan + exact native window duration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlmCalibrationRow {
    pub subject_id: String,
    pub plan: String,
    pub window_duration_secs: i64,
    pub reset_at: Option<i64>,

    /// Anchor fraction legs stay `None` while the endpoint's units are unproven; the
    /// cumulative ledgers are always exact.
    pub anchor_used_fraction_units: Option<i64>,
    pub anchor_resolution_fraction_units: Option<i64>,
    pub anchor_spend_api_nanousd: i64,
    pub anchor_spend_native_microcredits: i64,

    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
    pub observed_at: i64,

    /// The window's native size in microcredits, from the plan's published limits corroborated
    /// by observations — never estimated. `None` until either source names it.
    pub native_limit_microcredits: Option<i64>,
    pub native_used_microcredits: Option<i64>,

    pub observed_fraction_units: i64,
    pub observed_spend_api_nanousd: i64,
    pub observed_spend_native_microcredits: i64,
    pub samples: i64,
    pub unattributed_fraction_units: i64,

    /// Unknown stays `None`. An unbounded high stays `None` rather than a guessed ceiling.
    pub current_capacity_nanousd: Option<i64>,
    pub current_low_nanousd: Option<i64>,
    pub current_high_nanousd: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,

    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

impl GlmCalibrationRow {
    /// Exact native remaining in microcredits. This needs no estimation and no bounds once both
    /// halves are known. Returns `None` while either half is unknown — never zero.
    pub fn native_remaining_units(&self) -> Option<i64> {
        self.native_limit_microcredits?
            .checked_sub(self.native_used_microcredits?)
    }

    /// Current API-dollar remaining, derived from measured capacity and the *current* exact
    /// unused fraction. `None` while capacity or the fraction is unknown — never zero.
    pub fn current_remaining_nano(&self) -> Option<i64> {
        let capacity = self.current_capacity_nanousd?;
        let unused = GLM_FRACTION_SCALE.checked_sub(self.used_fraction_units?)?;
        let remaining = i128::from(capacity)
            .checked_mul(i128::from(unused))?
            .checked_div(i128::from(GLM_FRACTION_SCALE))?;
        i64::try_from(remaining).ok()
    }
}

/// Derived fraction and measurement resolution for one raw `used`/`limit` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlmDerivedFraction {
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
}

/// Convert the provider's integer quota counters into fixed-point fraction units.
///
/// The measurement resolution is the width of one native unit expressed in fraction units,
/// which is `ceil(SCALE / limit)`: a limit of 1000 resolves to 0.1%, a limit of 100 to 1%.
/// This is the endpoint's *actual* precision and must not be confused with the fixed-point
/// scale — storing a coarse snapshot in a wide integer does not make it precise.
///
/// A limit finer than one fraction unit clamps to 1, because the estimator's uncertainty
/// envelope needs a strictly positive width.
///
/// Callers must run this only once the raw counters' unit semantics are proven; until then the
/// derived fields of an observation stay `None`.
pub fn glm_fraction_from_native(used: i64, limit: i64) -> anyhow::Result<GlmDerivedFraction> {
    if limit <= 0 {
        bail!("GLM quota limit must be positive");
    }
    if used < 0 {
        bail!("GLM quota used must not be negative");
    }
    if used > limit {
        bail!("GLM quota used exceeds its limit");
    }
    let used_fraction_units = i128::from(used)
        .checked_mul(i128::from(GLM_FRACTION_SCALE))
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("GLM quota fraction overflow"))?;
    let resolution = i128::from(GLM_FRACTION_SCALE)
        .checked_add(i128::from(limit) - 1)
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("GLM quota resolution overflow"))?;
    Ok(GlmDerivedFraction {
        used_fraction_units,
        measurement_resolution_fraction_units: resolution.max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> GlmTurnCalibrationEvent {
        GlmTurnCalibrationEvent {
            request_id: "cal_1".into(),
            subject_id: "u_1".into(),
            plan: "Pro".into(),
            requested_model: "glm-5".into(),
            served_model: "glm-5.2".into(),
            context_mode: "1m".into(),
            reasoning_effort: Some("high".into()),
            api_tariff_schedule_id: "zhipu/z.ai-open-platform/2026-08-03".into(),
            credit_schedule_id: "zhipu/glm-coding-plan-credits/2026-07-30".into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_001,
            fresh_input_tokens: 1_000,
            cached_input_tokens: 500,
            cache_write_tokens: 50,
            output_tokens: 100,
            reasoning_tokens: 40,
            api_fresh_input_nanousd: 1_400_000,
            api_cached_input_nanousd: 130_000,
            api_output_nanousd: 440_000,
            api_total_nanousd: 1_970_000,
            native_fresh_input_microcredits: 690_000,
            native_cached_input_microcredits: 85_000,
            native_output_microcredits: 240_000,
            native_total_microcredits: 1_015_000,
            off_peak: false,
        }
    }

    #[test]
    fn a_well_formed_turn_event_validates() {
        event().validate().unwrap();
    }

    #[test]
    fn requested_and_served_model_are_both_preserved() {
        // The provider silently re-routes glm-5.1/glm-5 onto glm-5.2, which is priced
        // differently; losing either side would make the charge unauditable.
        let rerouted = event();
        rerouted.validate().unwrap();
        assert_ne!(rerouted.requested_model, rerouted.served_model);
    }

    #[test]
    fn api_legs_that_do_not_sum_to_the_total_fail_closed() {
        // A total that does not equal its parts means the charge cannot be reconstructed.
        let mut broken = event();
        broken.api_total_nanousd += 1;
        assert_eq!(broken.validate(), Err(GlmEventError::LegsDoNotSum));
    }

    #[test]
    fn native_legs_that_do_not_sum_to_the_total_fail_closed() {
        // The native ledger is independent: its legs must reconstruct its own total, never
        // borrow plausibility from the API-dollar side.
        let mut broken = event();
        broken.native_total_microcredits += 1;
        assert_eq!(broken.validate(), Err(GlmEventError::NativeLegsDoNotSum));
    }

    #[test]
    fn reasoning_above_output_fails_closed() {
        let mut broken = event();
        broken.reasoning_tokens = broken.output_tokens + 1;
        assert_eq!(
            broken.validate(),
            Err(GlmEventError::ReasoningExceedsOutput)
        );
    }

    #[test]
    fn a_turn_with_no_usage_is_not_evidence() {
        let mut empty = event();
        empty.fresh_input_tokens = 0;
        empty.cached_input_tokens = 0;
        empty.cache_write_tokens = 0;
        empty.output_tokens = 0;
        empty.reasoning_tokens = 0;
        assert_eq!(empty.validate(), Err(GlmEventError::EmptyUsage));
    }

    #[test]
    fn missing_identity_or_bad_enums_fail_closed() {
        for mutate in [
            (|e: &mut GlmTurnCalibrationEvent| e.plan = String::new()) as fn(&mut _),
            |e: &mut GlmTurnCalibrationEvent| e.served_model = String::new(),
            |e: &mut GlmTurnCalibrationEvent| e.request_id = String::new(),
            |e: &mut GlmTurnCalibrationEvent| e.api_tariff_schedule_id = String::new(),
            |e: &mut GlmTurnCalibrationEvent| e.credit_schedule_id = String::new(),
        ] {
            let mut broken = event();
            mutate(&mut broken);
            assert_eq!(broken.validate(), Err(GlmEventError::MissingIdentity));
        }
        let mut bad_mode = event();
        bad_mode.context_mode = "512k".into();
        assert_eq!(
            bad_mode.validate(),
            Err(GlmEventError::UnsupportedContextMode)
        );
        let mut bad_effort = event();
        bad_effort.reasoning_effort = Some("ultra".into());
        assert_eq!(
            bad_effort.validate(),
            Err(GlmEventError::UnsupportedReasoningEffort)
        );
    }

    #[test]
    fn reasoning_effort_is_nullable_because_only_glm_5_2_takes_one() {
        let mut no_effort = event();
        no_effort.served_model = "glm-5-turbo".into();
        no_effort.reasoning_effort = None;
        no_effort.validate().unwrap();
        for canonical in ["off", "high", "max"] {
            let mut with_effort = event();
            with_effort.reasoning_effort = Some(canonical.into());
            with_effort.validate().unwrap();
        }
    }

    #[test]
    fn a_zero_cost_turn_is_refused_so_free_traffic_never_becomes_evidence() {
        let mut free = event();
        free.api_fresh_input_nanousd = 0;
        free.api_cached_input_nanousd = 0;
        free.api_output_nanousd = 0;
        free.api_total_nanousd = 0;
        assert_eq!(free.validate(), Err(GlmEventError::NegativeCounter));

        let mut free_native = event();
        free_native.native_fresh_input_microcredits = 0;
        free_native.native_cached_input_microcredits = 0;
        free_native.native_output_microcredits = 0;
        free_native.native_total_microcredits = 0;
        assert_eq!(free_native.validate(), Err(GlmEventError::NegativeCounter));
    }

    #[test]
    fn exact_replay_is_recognised_and_a_changed_payload_is_not() {
        let stored = event();
        assert!(event().is_exact_replay_of(&stored));
        // A different payload under the same request id must be a typed conflict upstream,
        // never an update: overwriting would silently rewrite priced history.
        let mut changed = event();
        changed.api_total_nanousd += 1_000;
        changed.api_output_nanousd += 1_000;
        assert!(!changed.is_exact_replay_of(&stored));
        assert_eq!(changed.request_id, stored.request_id);
    }

    #[test]
    fn resolution_follows_the_real_limit_not_the_fixed_point_scale() {
        // limit=1000 means one native unit is 0.1%, far finer than Claude's whole percent.
        let fine = glm_fraction_from_native(40, 1_000).unwrap();
        assert_eq!(fine.used_fraction_units, 4_000_000);
        assert_eq!(fine.measurement_resolution_fraction_units, 100_000);

        // limit=100 means one unit is a whole percent.
        let coarse = glm_fraction_from_native(40, 100).unwrap();
        assert_eq!(coarse.used_fraction_units, 40_000_000);
        assert_eq!(coarse.measurement_resolution_fraction_units, 1_000_000);
    }

    #[test]
    fn resolution_is_rounded_up_so_the_envelope_is_never_understated() {
        // 3 does not divide the scale evenly; rounding down would claim more precision than
        // the endpoint has.
        let derived = glm_fraction_from_native(1, 3).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 33_333_334);
    }

    #[test]
    fn a_limit_finer_than_one_unit_still_has_positive_width() {
        let derived = glm_fraction_from_native(0, GLM_FRACTION_SCALE * 4).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 1);
    }

    #[test]
    fn full_and_empty_windows_map_to_the_scale_bounds() {
        assert_eq!(
            glm_fraction_from_native(0, 500)
                .unwrap()
                .used_fraction_units,
            0
        );
        assert_eq!(
            glm_fraction_from_native(500, 500)
                .unwrap()
                .used_fraction_units,
            GLM_FRACTION_SCALE
        );
    }

    #[test]
    fn invalid_quota_counters_fail_closed() {
        assert!(glm_fraction_from_native(1, 0).is_err());
        assert!(glm_fraction_from_native(1, -5).is_err());
        assert!(glm_fraction_from_native(-1, 10).is_err());
        assert!(glm_fraction_from_native(11, 10).is_err());
    }

    #[test]
    fn window_durations_are_the_documented_independent_lengths() {
        // The rolling 5-hour window and the 7-day weekly window are separate evidence and must
        // never collapse into one row.
        assert_eq!(GLM_5H_WINDOW_SECS, 5 * 3_600);
        assert_eq!(GLM_WEEKLY_WINDOW_SECS, 7 * 86_400);
        assert_ne!(GLM_5H_WINDOW_SECS, GLM_WEEKLY_WINDOW_SECS);
    }

    fn row() -> GlmCalibrationRow {
        GlmCalibrationRow {
            subject_id: "u_1".into(),
            plan: "Max".into(),
            window_duration_secs: GLM_WEEKLY_WINDOW_SECS,
            reset_at: Some(1_800_604_800),
            anchor_used_fraction_units: Some(0),
            anchor_resolution_fraction_units: Some(100_000),
            anchor_spend_api_nanousd: 0,
            anchor_spend_native_microcredits: 0,
            used_fraction_units: Some(25_000_000),
            measurement_resolution_fraction_units: Some(100_000),
            observed_at: 1_799_000_000,
            native_limit_microcredits: Some(140_000_000_000),
            native_used_microcredits: Some(35_000_000_000),
            observed_fraction_units: 25_000_000,
            observed_spend_api_nanousd: 500_000_000,
            observed_spend_native_microcredits: 35_000_000_000,
            samples: 1,
            unattributed_fraction_units: 0,
            current_capacity_nanousd: Some(2_000_000_000),
            current_low_nanousd: Some(1_900_000_000),
            current_high_nanousd: Some(2_100_000_000),
            current_confidence_bp: 5_000,
            last_measured_at: Some(1_799_000_000),
            estimator_version: 1,
            version: 3,
            updated_ts: 1_799_000_000,
        }
    }

    #[test]
    fn native_remaining_is_exact_once_both_halves_are_known() {
        // 140 000 published weekly credits on Max, 35 000 consumed.
        assert_eq!(row().native_remaining_units(), Some(105_000_000_000));
    }

    #[test]
    fn native_remaining_stays_unknown_while_either_half_is_unknown() {
        let mut unknown_limit = row();
        unknown_limit.native_limit_microcredits = None;
        assert_eq!(unknown_limit.native_remaining_units(), None);
        let mut unknown_used = row();
        unknown_used.native_used_microcredits = None;
        assert_eq!(unknown_used.native_remaining_units(), None);
    }

    #[test]
    fn api_remaining_follows_the_current_exact_unused_fraction() {
        // 75% of a $2.00 window remains.
        assert_eq!(row().current_remaining_nano(), Some(1_500_000_000));
    }

    #[test]
    fn unknown_capacity_or_fraction_stays_none_and_never_becomes_zero() {
        let mut cold = row();
        cold.current_capacity_nanousd = None;
        assert_eq!(cold.current_remaining_nano(), None);
        // Unproven quota units mean no fraction, hence no remaining estimate either.
        let mut unproven = row();
        unproven.used_fraction_units = None;
        assert_eq!(unproven.current_remaining_nano(), None);
    }
}
