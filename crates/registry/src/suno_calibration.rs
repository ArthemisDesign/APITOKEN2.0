//! Exact plan-scoped Suno (suno.com) subscription session-pool calibration types.
//!
//! Shape and rationale: `docs/engine/SUNO_PROVIDER.md` §5, migration
//! `0050_suno_window_calibration.sql`. Suno sits between the two existing shapes:
//!
//! * Like Tripo3D it is a task-based media API, not a chat protocol: the money identity of a
//!   turn is a settled song generation measured in native credits, carried as two exact legs
//!   that agree at the reviewed fixed rate — native millicredits (credits × 1e3) and API
//!   nanoUSD (millicredits × 4 000, i.e. $0.004/credit from the DERIVED schedule — Suno has no
//!   official rate card). Unlike Tripo3D the native leg is not always provider truth: when the
//!   provider does not report per-turn consumption, the reviewed 5-credits-per-song nominal is
//!   recorded with `native_schedule_derived = true` and is never presented as provider-reported
//!   consumption. A finalized-but-failed generation with zero credit movement settles at the
//!   legal zero pair.
//! * Unlike Tripo3D and like GLM it HAS a quota window: subscription credits refill monthly
//!   (Pro 2 500 / Premier 10 000, published per plan), so no native-capacity estimation is
//!   needed. The quota endpoint's raw counters (`monthly_limit`/`monthly_usage`/
//!   `total_credits_left`/`period`) are an oss-hypothesis with unproven semantics, so they are
//!   stored verbatim with `None` for unknown — never `0` — and the derived fraction plus its
//!   measurement resolution exist only when the field semantics allow them.
//!
//! Window duration discipline: the monthly reset is anchored to the subscription's billing
//! date and its exact duration is UNKNOWN (manifest §1). Following GLM's exact-duration
//! keying, rows are keyed by the duration the reset evidence actually shows and NO synthetic
//! constant (a nominal 30- or 31-day value) is published anywhere — such a constant would
//! silently become somebody's assumption. An unknown duration fails closed.

use anyhow::bail;

/// Fixed-point scale shared with the other providers: 100% == 100_000_000 units.
pub const SUNO_FRACTION_SCALE: i64 = 100_000_000;

/// nanoUSD per millicredit at the reviewed derived rate: 1 credit = $0.004 = 4 000 000
/// nanoUSD, and a millicredit is 1/1000 of a credit.
pub const SUNO_NANOUSD_PER_MILLICREDIT: i64 = 4_000;

/// One immutable quota observation for a single Suno monthly window.
///
/// Quota is served from `GET /api/billing/info/` and may also be read in the wake of a
/// generation response, so the source is explicit: a response-carried observation names the
/// request that carried it, a poll invents no request id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoWindowObservation {
    pub subject_id: String,
    /// Declared paid plan (`Pro`/`Premier`). No machine-readable plan identity is proven yet,
    /// so the declared plan is the cohort key, corroborated by the observed native limit.
    pub plan: String,
    /// Exact native window duration in seconds from reset evidence. The monthly reset is
    /// billing-anchored and unknown until observed; an unknown duration fails closed.
    pub window_duration_secs: i64,
    /// The reset anchor when the raw `period` field supplies it; `None` otherwise.
    pub reset_at: Option<i64>,
    pub observed_at: i64,
    /// Raw provider counters, verbatim (`monthly_limit`/`monthly_usage`/`total_credits_left`).
    /// Their semantics are unproven, so unknown stays `None` — never `0`.
    pub native_limit_units: Option<i64>,
    pub native_used_units: Option<i64>,
    pub native_remaining_units: Option<i64>,
    /// The raw `period` value, verbatim. Its format is unproven; when it names a reset it also
    /// feeds `reset_at`.
    pub period_raw: Option<String>,
    /// Derived from the raw counters via [`suno_fraction_from_native`], only when the field
    /// semantics allow it. Until then both stay `None` together.
    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
    /// Cumulative dual ledgers for this subject at observation time.
    pub cumulative_api_nanousd: i64,
    pub cumulative_native_millicredits: i64,
    /// `poll` or `response`.
    pub observation_source: String,
    /// The request that carried a response-source observation. Never invented for a poll.
    pub source_request_id: Option<String>,
}

/// One immutable, priced Suno turn: a settled song generation.
///
/// The money authority is the native credit consumption in millicredits; the API nanoUSD leg
/// is its exact image at the reviewed fixed rate, and the schema enforces the equality.
/// `served_model` stays `None` until the live matrix pins the wire id spellings — never a
/// fabricated copy of `requested_model`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoTurnCalibrationEvent {
    /// Internal CSPRNG id, stable across every pre-byte retry. Never an upstream id.
    pub request_id: String,
    pub subject_id: String,
    /// Declared paid plan (`Pro`/`Premier`). The Free tier is excluded by design.
    pub plan: String,
    /// The model the customer asked for, from the reviewed paid catalog.
    pub requested_model: String,
    /// The model the upstream response says it served. `None` while the wire id spellings are
    /// unproven (manifest §3) — never a fabricated value.
    pub served_model: Option<String>,
    /// The effective-dated derived schedule that priced the reserve and cross-checked the
    /// settlement.
    pub tariff_schedule_id: String,
    pub priced_ts: i64,
    pub completed_at: i64,
    /// Audit metadata only: the upstream clip/task id is never the money identity —
    /// `request_id` is.
    pub upstream_clip_id: String,
    /// Native consumption in millicredits (credits × 1e3). Zero for a finalized-but-failed
    /// generation whose credit movement was zero (the hold is refunded).
    pub native_total_millicredits: i64,
    /// Exact API replacement cost: `native_total_millicredits` × 4 000 nanoUSD.
    pub api_total_nanousd: i64,
    /// `true` when the native leg came from the reviewed 5-credits-per-song schedule rather
    /// than provider-reported consumption. A schedule-derived amount is estimate-grade
    /// evidence and is never presented as provider truth (manifest §5.3).
    pub native_schedule_derived: bool,
}

/// Why a turn event cannot be persisted. Every variant refuses before money or evidence moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SunoEventError {
    MissingIdentity,
    InvalidTimestamp,
    NegativeCounter,
    /// The API nanoUSD total is not the exact fixed-rate image of the native millicredit
    /// total — including a partial zero (one ledger zero, the other positive).
    LegsDisagree,
    /// Only the paid Pro/Premier plans are admitted; Free is excluded by design.
    UnsupportedPlan,
}

impl std::fmt::Display for SunoEventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::MissingIdentity => "Suno turn event is missing an identity field",
            Self::InvalidTimestamp => "Suno turn event has an invalid timestamp",
            Self::NegativeCounter => "Suno turn event has a negative counter",
            Self::LegsDisagree => {
                "Suno API cost is not the fixed-rate image of the native credits"
            }
            Self::UnsupportedPlan => "Suno turn event has an unsupported plan",
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for SunoEventError {}

/// A request id already names a different immutable Suno turn. This is permanent corruption of
/// the proposed event, not a transient database failure, so the runtime FIFO quarantines
/// exactly that row and continues with its tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SunoTurnReplayConflict;

impl std::fmt::Display for SunoTurnReplayConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Suno turn calibration replay conflict for the same request id")
    }
}

impl std::error::Error for SunoTurnReplayConflict {}

pub fn is_suno_turn_replay_conflict(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| cause.is::<SunoTurnReplayConflict>())
}

impl SunoTurnCalibrationEvent {
    /// Validate before persistence. These mirror the migration's CHECK constraints so a bad
    /// event is refused in Rust with a typed reason rather than as an opaque database error.
    pub fn validate(&self) -> std::result::Result<(), SunoEventError> {
        for field in [
            &self.request_id,
            &self.subject_id,
            &self.plan,
            &self.requested_model,
            &self.tariff_schedule_id,
            &self.upstream_clip_id,
        ] {
            if field.is_empty() {
                return Err(SunoEventError::MissingIdentity);
            }
        }
        if self.served_model.as_deref().is_some_and(str::is_empty) {
            return Err(SunoEventError::MissingIdentity);
        }
        if !matches!(self.plan.as_str(), "Pro" | "Premier") {
            return Err(SunoEventError::UnsupportedPlan);
        }
        if self.priced_ts <= 0 || self.completed_at <= 0 {
            return Err(SunoEventError::InvalidTimestamp);
        }
        if self.native_total_millicredits < 0 || self.api_total_nanousd < 0 {
            return Err(SunoEventError::NegativeCounter);
        }
        // The derived fixed rate is part of the schema: the two legs agree exactly, or the row
        // is not evidence. A partial zero (one ledger zero, the other positive) cannot satisfy
        // this equality, so a refunded failed generation carries the legal zero pair and a
        // paid generation can never carry a zero total.
        let expected = i128::from(self.native_total_millicredits)
            .checked_mul(i128::from(SUNO_NANOUSD_PER_MILLICREDIT))
            .ok_or(SunoEventError::LegsDisagree)?;
        if expected != i128::from(self.api_total_nanousd) {
            return Err(SunoEventError::LegsDisagree);
        }
        Ok(())
    }

    /// Whether a stored row is the exact same semantic event, for idempotent replay.
    ///
    /// A retry of the same internal request id must be a no-op. A *different* payload under
    /// that id is a typed conflict, never an update: overwriting would silently rewrite priced
    /// history. The `native_schedule_derived` flag is part of the identity: re-grading the
    /// same turn from provider-reported to schedule-derived (or back) is a conflict, not a
    /// replay.
    pub fn is_exact_replay_of(&self, stored: &Self) -> bool {
        self == stored
    }
}

/// Cumulative dual ledgers for one subject. The API leg is the fixed-rate image of the native
/// leg, but both are exact sums over the immutable turn events — neither is back-computed from
/// the other at read time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SunoSubjectSpend {
    pub spent_api_nanousd: i64,
    pub spent_native_millicredits: i64,
}

/// Estimator state for one subject + declared plan + exact native window duration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SunoCalibrationRow {
    pub subject_id: String,
    pub plan: String,
    pub window_duration_secs: i64,
    pub reset_at: Option<i64>,

    /// Anchor fraction legs stay `None` while the endpoint's field semantics are unproven; the
    /// cumulative ledgers are always exact.
    pub anchor_used_fraction_units: Option<i64>,
    pub anchor_resolution_fraction_units: Option<i64>,
    pub anchor_spend_api_nanousd: i64,
    pub anchor_spend_native_millicredits: i64,

    pub used_fraction_units: Option<i64>,
    pub measurement_resolution_fraction_units: Option<i64>,
    pub observed_at: i64,

    /// The window's native size in millicredits, from the plan's published monthly limits
    /// (Pro 2 500 / Premier 10 000 credits) corroborated by observations — never estimated.
    /// `None` until either source names it.
    pub native_limit_millicredits: Option<i64>,
    pub native_used_millicredits: Option<i64>,

    pub observed_fraction_units: i64,
    pub observed_spend_api_nanousd: i64,
    pub observed_spend_native_millicredits: i64,
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

impl SunoCalibrationRow {
    /// Exact native remaining in millicredits. This needs no estimation and no bounds once both
    /// halves are known. Returns `None` while either half is unknown — never zero.
    pub fn native_remaining_units(&self) -> Option<i64> {
        self.native_limit_millicredits?
            .checked_sub(self.native_used_millicredits?)
    }

    /// Current API-dollar remaining, derived from measured capacity and the *current* exact
    /// unused fraction. `None` while capacity or the fraction is unknown — never zero.
    pub fn current_remaining_nano(&self) -> Option<i64> {
        let capacity = self.current_capacity_nanousd?;
        let unused = SUNO_FRACTION_SCALE.checked_sub(self.used_fraction_units?)?;
        let remaining = i128::from(capacity)
            .checked_mul(i128::from(unused))?
            .checked_div(i128::from(SUNO_FRACTION_SCALE))?;
        i64::try_from(remaining).ok()
    }
}

/// Derived fraction and measurement resolution for one raw `used`/`limit` pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SunoDerivedFraction {
    pub used_fraction_units: i64,
    pub measurement_resolution_fraction_units: i64,
}

/// Convert the provider's integer quota counters into fixed-point fraction units.
///
/// The measurement resolution is the width of one native unit expressed in fraction units,
/// which is `ceil(SCALE / limit)`: the published Pro limit of 2 500 credits resolves to 0.04%
/// per credit, Premier's 10 000 to 0.01%. This is the endpoint's *actual* precision and must
/// not be confused with the fixed-point scale — storing a coarse snapshot in a wide integer
/// does not make it precise.
///
/// A limit finer than one fraction unit clamps to 1, because the estimator's uncertainty
/// envelope needs a strictly positive width.
///
/// Callers must run this only when the raw counters' field semantics allow derivation (limit >
/// 0 and usage present); until then the derived fields of an observation stay `None`.
pub fn suno_fraction_from_native(used: i64, limit: i64) -> anyhow::Result<SunoDerivedFraction> {
    if limit <= 0 {
        bail!("Suno quota limit must be positive");
    }
    if used < 0 {
        bail!("Suno quota used must not be negative");
    }
    if used > limit {
        bail!("Suno quota used exceeds its limit");
    }
    let used_fraction_units = i128::from(used)
        .checked_mul(i128::from(SUNO_FRACTION_SCALE))
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Suno quota fraction overflow"))?;
    let resolution = i128::from(SUNO_FRACTION_SCALE)
        .checked_add(i128::from(limit) - 1)
        .and_then(|value| value.checked_div(i128::from(limit)))
        .and_then(|value| i64::try_from(value).ok())
        .ok_or_else(|| anyhow::anyhow!("Suno quota resolution overflow"))?;
    Ok(SunoDerivedFraction {
        used_fraction_units,
        measurement_resolution_fraction_units: resolution.max(1),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> SunoTurnCalibrationEvent {
        SunoTurnCalibrationEvent {
            request_id: "cal_1".into(),
            subject_id: "sess_1".into(),
            plan: "Pro".into(),
            requested_model: "v5.5".into(),
            served_model: None,
            tariff_schedule_id: "suno/derived-subscription/2026-08-12".into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_120,
            upstream_clip_id: "clip_abc123".into(),
            native_total_millicredits: 5_000,
            api_total_nanousd: 20_000_000,
            native_schedule_derived: true,
        }
    }

    #[test]
    fn a_well_formed_turn_event_validates() {
        event().validate().unwrap();
    }

    #[test]
    fn fixed_rate_identity_is_exact() {
        // 5 credits = 5 000 millicredits = 20 000 000 nanoUSD at $0.004/credit: one song.
        assert_eq!(
            SUNO_NANOUSD_PER_MILLICREDIT * 1_000,
            4_000_000,
            "one credit must be exactly $0.004"
        );
        event().validate().unwrap();
    }

    #[test]
    fn only_the_paid_plans_are_admitted() {
        for plan in ["Pro", "Premier"] {
            let mut paid = event();
            paid.plan = plan.into();
            paid.validate().unwrap();
        }
        for plan in ["Free", "pro", "Basic", ""] {
            let mut rejected = event();
            rejected.plan = plan.into();
            let error = rejected.validate().unwrap_err();
            assert!(
                matches!(
                    error,
                    SunoEventError::UnsupportedPlan | SunoEventError::MissingIdentity
                ),
                "plan {plan:?} must fail closed, got {error:?}"
            );
        }
    }

    #[test]
    fn unproven_wire_ids_keep_a_null_served_model() {
        // The exact `mv` spellings are unknown until the live matrix pins them; absence stays
        // None rather than a fabricated copy of requested_model.
        assert_eq!(event().served_model, None);
        event().validate().unwrap();
        let mut pinned = event();
        pinned.served_model = Some("chirp-v5-5".into());
        pinned.validate().unwrap();
        let mut empty = event();
        empty.served_model = Some(String::new());
        assert_eq!(empty.validate(), Err(SunoEventError::MissingIdentity));
    }

    #[test]
    fn a_zero_cost_refunded_generation_is_legal_evidence() {
        // A finalized-but-failed generation with zero credit movement refunds its hold and
        // settles at the legal zero pair.
        let mut refunded = event();
        refunded.native_total_millicredits = 0;
        refunded.api_total_nanousd = 0;
        refunded.validate().unwrap();
    }

    #[test]
    fn a_partial_zero_fails_closed() {
        let mut broken = event();
        broken.native_total_millicredits = 0;
        assert_eq!(broken.validate(), Err(SunoEventError::LegsDisagree));
        let mut broken = event();
        broken.api_total_nanousd = 0;
        assert_eq!(broken.validate(), Err(SunoEventError::LegsDisagree));
    }

    #[test]
    fn legs_that_disagree_with_the_fixed_rate_fail_closed() {
        let mut broken = event();
        broken.api_total_nanousd += 1;
        assert_eq!(broken.validate(), Err(SunoEventError::LegsDisagree));
    }

    #[test]
    fn missing_identity_or_bad_timestamps_fail_closed() {
        for mutate in [
            (|e: &mut SunoTurnCalibrationEvent| e.request_id = String::new()) as fn(&mut _),
            |e: &mut SunoTurnCalibrationEvent| e.subject_id = String::new(),
            |e: &mut SunoTurnCalibrationEvent| e.plan = String::new(),
            |e: &mut SunoTurnCalibrationEvent| e.requested_model = String::new(),
            |e: &mut SunoTurnCalibrationEvent| e.tariff_schedule_id = String::new(),
            |e: &mut SunoTurnCalibrationEvent| e.upstream_clip_id = String::new(),
        ] {
            let mut broken = event();
            mutate(&mut broken);
            assert_eq!(broken.validate(), Err(SunoEventError::MissingIdentity));
        }
        let mut bad_ts = event();
        bad_ts.priced_ts = 0;
        assert_eq!(bad_ts.validate(), Err(SunoEventError::InvalidTimestamp));
        let mut bad_ts = event();
        bad_ts.completed_at = -1;
        assert_eq!(bad_ts.validate(), Err(SunoEventError::InvalidTimestamp));
        let mut negative = event();
        negative.native_total_millicredits = -1;
        assert_eq!(negative.validate(), Err(SunoEventError::NegativeCounter));
    }

    #[test]
    fn the_schedule_derived_flag_is_part_of_the_identity() {
        // Re-grading the same turn from schedule-derived to provider-reported (or back) under
        // the same request id is a typed conflict upstream, never a silent update.
        let stored = event();
        let mut regraded = event();
        regraded.native_schedule_derived = false;
        assert!(!regraded.is_exact_replay_of(&stored));
        assert_eq!(regraded.request_id, stored.request_id);
        assert!(event().is_exact_replay_of(&stored));
    }

    #[test]
    fn the_typed_conflict_is_detectable_through_anyhow_chains() {
        let error: anyhow::Error = SunoTurnReplayConflict.into();
        let wrapped = error.context("persisting the turn");
        assert!(is_suno_turn_replay_conflict(&wrapped));
        let other = anyhow::anyhow!("unrelated");
        assert!(!is_suno_turn_replay_conflict(&other));
    }

    #[test]
    fn resolution_follows_the_published_plan_limits() {
        // Pro: 2 500 credits resolve one credit to 0.04% of the window.
        let pro = suno_fraction_from_native(100, 2_500).unwrap();
        assert_eq!(pro.used_fraction_units, 4_000_000);
        assert_eq!(pro.measurement_resolution_fraction_units, 40_000);
        // Premier: 10 000 credits resolve one credit to 0.01%.
        let premier = suno_fraction_from_native(500, 10_000).unwrap();
        assert_eq!(premier.used_fraction_units, 5_000_000);
        assert_eq!(premier.measurement_resolution_fraction_units, 10_000);
    }

    #[test]
    fn resolution_is_rounded_up_so_the_envelope_is_never_understated() {
        // 3 does not divide the scale evenly; rounding down would claim more precision than
        // the endpoint has.
        let derived = suno_fraction_from_native(1, 3).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 33_333_334);
    }

    #[test]
    fn a_limit_finer_than_one_unit_still_has_positive_width() {
        let derived = suno_fraction_from_native(0, SUNO_FRACTION_SCALE * 4).unwrap();
        assert_eq!(derived.measurement_resolution_fraction_units, 1);
    }

    #[test]
    fn full_and_empty_windows_map_to_the_scale_bounds() {
        assert_eq!(
            suno_fraction_from_native(0, 2_500).unwrap().used_fraction_units,
            0
        );
        assert_eq!(
            suno_fraction_from_native(2_500, 2_500)
                .unwrap()
                .used_fraction_units,
            SUNO_FRACTION_SCALE
        );
    }

    #[test]
    fn invalid_quota_counters_fail_closed() {
        assert!(suno_fraction_from_native(1, 0).is_err());
        assert!(suno_fraction_from_native(1, -5).is_err());
        assert!(suno_fraction_from_native(-1, 10).is_err());
        assert!(suno_fraction_from_native(11, 10).is_err());
    }

    fn row() -> SunoCalibrationRow {
        SunoCalibrationRow {
            subject_id: "sess_1".into(),
            plan: "Premier".into(),
            window_duration_secs: 2_628_000,
            reset_at: Some(1_802_628_000),
            anchor_used_fraction_units: Some(0),
            anchor_resolution_fraction_units: Some(10_000),
            anchor_spend_api_nanousd: 0,
            anchor_spend_native_millicredits: 0,
            used_fraction_units: Some(25_000_000),
            measurement_resolution_fraction_units: Some(10_000),
            observed_at: 1_799_000_000,
            native_limit_millicredits: Some(10_000_000),
            native_used_millicredits: Some(2_500_000),
            observed_fraction_units: 25_000_000,
            observed_spend_api_nanousd: 500_000_000,
            observed_spend_native_millicredits: 125_000,
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
        // 10 000 published monthly credits on Premier, 2 500 consumed.
        assert_eq!(row().native_remaining_units(), Some(7_500_000));
    }

    #[test]
    fn native_remaining_stays_unknown_while_either_half_is_unknown() {
        let mut unknown_limit = row();
        unknown_limit.native_limit_millicredits = None;
        assert_eq!(unknown_limit.native_remaining_units(), None);
        let mut unknown_used = row();
        unknown_used.native_used_millicredits = None;
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
        // Unproven quota field semantics mean no fraction, hence no remaining estimate either.
        let mut unproven = row();
        unproven.used_fraction_units = None;
        assert_eq!(unproven.current_remaining_nano(), None);
    }
}
