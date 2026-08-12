//! Exact subject-scoped Tripo3D (VAST / Holymolly) prepaid API calibration types.
//!
//! Shape and rationale: `docs/engine/TRIPO3D_PROVIDER.md` §5, migration
//! `0049_tripo3d_calibration.sql`. Two differences from the KIMI/GLM equivalents are
//! load-bearing:
//!
//! * Tripo3D is a task-based media API, not a chat protocol. The money identity of a turn is
//!   the provider-reported `consumed_credit` of a finished task, carried as two exact legs
//!   that agree at the published fixed rate: native millicredits (credits × 1e3) and API
//!   nanoUSD (millicredits × 10 000). Documented free tasks (`animate_prerigcheck`,
//!   `import_model`) and refunded failed/expired tasks settle at zero, so a zero pair is legal
//!   evidence — but the two totals are always zero together or positive together.
//! * A prepaid balance has no quota window: purchased credits never expire and never reset.
//!   There is no window duration, no fraction and no reset anywhere in this authority.
//!   Calibration answers "how much sellable capacity remains": `balance − frozen`, exact once
//!   the balance unit is proven, `None` until then. The endpoint's raw float values are
//!   preserved verbatim as text — money is never parsed through binary float — and the parsed
//!   fixed-point halves stay `None` while the unit is unproven: unknown is `None`, never `0`.

/// nanoUSD per millicredit at the published fixed rate: 1 credit = $0.01 = 10 000 000
/// nanoUSD, and a millicredit is 1/1000 of a credit.
pub const TRIPO3D_NANOUSD_PER_MILLICREDIT: i64 = 10_000;

/// One immutable balance observation for a single Tripo3D subject.
///
/// Balance is served from `GET /v2/openapi/user/balance` and may also be read in the wake of
/// a task response, so the source is explicit: a response-carried observation names the
/// request that carried it, a poll invents no request id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dBalanceObservation {
    pub subject_id: String,
    /// Declared top-up cohort of the offer product. No machine-readable plan identity exists,
    /// so the declared cohort is the cohort key, corroborated by the balance anchor at
    /// admission.
    pub cohort: String,
    pub observed_at: i64,
    /// Raw provider values, verbatim. The endpoint returns floats; money is never parsed
    /// through binary float, so the raw text is the authority.
    pub balance_raw: String,
    pub frozen_raw: String,
    /// Parsed fixed-point micro-units (units × 1e6) of the proven native unit. The unit of
    /// `balance`/`frozen` (credits vs dollars, decimal semantics) is unproven, so both halves
    /// stay `None` until a live run proves it — unknown is `None`, never `0`.
    pub balance_micro_units: Option<i64>,
    pub frozen_micro_units: Option<i64>,
    /// Cumulative dual ledgers for this subject at observation time.
    pub cumulative_api_nanousd: i64,
    pub cumulative_native_millicredits: i64,
    /// `poll` or `response`.
    pub observation_source: String,
    /// The request that carried a response-source observation. Never invented for a poll.
    pub source_request_id: Option<String>,
}

/// One immutable, priced Tripo3D turn: a finalized upstream task.
///
/// The money authority is the task's provider-reported `consumed_credit`, stored as native
/// millicredits; the API nanoUSD leg is its exact image at the fixed published rate, and the
/// schema enforces the equality. Requested and resolved model versions are separate fields:
/// the manifest records no silent re-routing, but a future one can only fail the admission
/// cross-check, never misprice.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dTurnCalibrationEvent {
    /// Internal CSPRNG id, stable across every pre-byte retry. Never an upstream id.
    pub request_id: String,
    pub subject_id: String,
    /// Declared top-up cohort of the offer product.
    pub cohort: String,
    /// The upstream task kind (`text_to_model`, `image_to_model`, …).
    pub task_type: String,
    /// The `model_version` the customer asked for. `None` for version-independent task kinds
    /// (`animate_retarget`, `convert_model`, `import_model`, …) — never a fabricated value.
    pub requested_model_version: Option<String>,
    /// The `model_version` the task actually ran with, under the same nullability rule.
    pub resolved_model_version: Option<String>,
    /// The effective-dated rate card that priced the reserve and cross-checked settlement.
    pub tariff_schedule_id: String,
    pub priced_ts: i64,
    pub completed_at: i64,
    /// Audit metadata only: tasks are queryable solely by the creating key, and the provider
    /// task id is never the money identity — `request_id` is.
    pub upstream_task_id: String,
    /// Authoritative native consumption from the task's `consumed_credit`, in millicredits
    /// (credits × 1e3). Zero for a documented free task or a refunded failed/expired task.
    pub native_total_millicredits: i64,
    /// Exact API replacement cost: `native_total_millicredits` × 10 000 nanoUSD.
    pub api_total_nanousd: i64,
}

/// Why a turn event cannot be persisted. Every variant refuses before money or evidence moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tripo3dEventError {
    MissingIdentity,
    InvalidTimestamp,
    NegativeCounter,
    /// The API nanoUSD total is not the exact fixed-rate image of the native millicredit
    /// total — including a partial zero (one ledger zero, the other positive).
    LegsDisagree,
}

impl std::fmt::Display for Tripo3dEventError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::MissingIdentity => "Tripo3D turn event is missing an identity field",
            Self::InvalidTimestamp => "Tripo3D turn event has an invalid timestamp",
            Self::NegativeCounter => "Tripo3D turn event has a negative counter",
            Self::LegsDisagree => {
                "Tripo3D API cost is not the fixed-rate image of the native credits"
            }
        };
        formatter.write_str(text)
    }
}

impl std::error::Error for Tripo3dEventError {}

/// A request id already names a different immutable Tripo3D turn. This is permanent corruption
/// of the proposed event, not a transient database failure, so the runtime FIFO quarantines
/// exactly that row and continues with its tail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tripo3dTurnReplayConflict;

impl std::fmt::Display for Tripo3dTurnReplayConflict {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Tripo3D turn calibration replay conflict for the same request id")
    }
}

impl std::error::Error for Tripo3dTurnReplayConflict {}

pub fn is_tripo3d_turn_replay_conflict(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.is::<Tripo3dTurnReplayConflict>())
}

impl Tripo3dTurnCalibrationEvent {
    /// Validate before persistence. These mirror the migration's CHECK constraints so a bad
    /// event is refused in Rust with a typed reason rather than as an opaque database error.
    pub fn validate(&self) -> std::result::Result<(), Tripo3dEventError> {
        for field in [
            &self.request_id,
            &self.subject_id,
            &self.cohort,
            &self.task_type,
            &self.tariff_schedule_id,
            &self.upstream_task_id,
        ] {
            if field.is_empty() {
                return Err(Tripo3dEventError::MissingIdentity);
            }
        }
        for version in [&self.requested_model_version, &self.resolved_model_version] {
            if version.as_deref().is_some_and(str::is_empty) {
                return Err(Tripo3dEventError::MissingIdentity);
            }
        }
        if self.priced_ts <= 0 || self.completed_at <= 0 {
            return Err(Tripo3dEventError::InvalidTimestamp);
        }
        if self.native_total_millicredits < 0 || self.api_total_nanousd < 0 {
            return Err(Tripo3dEventError::NegativeCounter);
        }
        // The fixed rate is part of the schema: the two legs agree exactly, or the row is not
        // evidence. A partial zero (one ledger zero, the other positive) cannot satisfy this
        // equality, so a documented free task carries the legal zero pair and a paid task can
        // never carry a zero total.
        let expected = i128::from(self.native_total_millicredits)
            .checked_mul(i128::from(TRIPO3D_NANOUSD_PER_MILLICREDIT))
            .ok_or(Tripo3dEventError::LegsDisagree)?;
        if expected != i128::from(self.api_total_nanousd) {
            return Err(Tripo3dEventError::LegsDisagree);
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

/// Cumulative dual ledgers for one subject. The API leg is the fixed-rate image of the native
/// leg, but both are exact sums over the immutable turn events — neither is back-computed from
/// the other at read time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tripo3dSubjectSpend {
    pub spent_api_nanousd: i64,
    pub spent_native_millicredits: i64,
}

/// Estimator state for one subject + declared top-up cohort. There is exactly one balance
/// track per subject: prepaid credits never reset, so the KIMI/GLM window dimension
/// degenerates to nothing and cohort replaces plan+window as the cohort axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tripo3dCalibrationRow {
    pub subject_id: String,
    pub cohort: String,

    /// The first observation of the track is an anchor, not a sample. Anchor balance halves
    /// stay `None` while the endpoint's units are unproven; the cumulative spend legs are
    /// always exact.
    pub anchor_balance_micro_units: Option<i64>,
    pub anchor_frozen_micro_units: Option<i64>,
    pub anchor_spend_api_nanousd: i64,
    pub anchor_spend_native_millicredits: i64,

    /// Latest raw halves, verbatim and parsed. The state row is always written together with
    /// an observation, so the raw text is always present; the parsed halves stay `None` until
    /// the unit is proven.
    pub latest_balance_raw: String,
    pub latest_frozen_raw: String,
    pub latest_balance_micro_units: Option<i64>,
    pub latest_frozen_micro_units: Option<i64>,
    pub observed_at: i64,

    pub observed_spend_api_nanousd: i64,
    pub observed_spend_native_millicredits: i64,
    pub samples: i64,

    /// Remaining sellable capacity in API nanoUSD, driven by the balance track
    /// (`balance − frozen` at the fixed rate) once the unit is proven — not by a window
    /// fraction. Unknown stays `None`; an unbounded high stays `None` rather than a guessed
    /// ceiling.
    pub current_capacity_nanousd: Option<i64>,
    pub current_low_nanousd: Option<i64>,
    pub current_high_nanousd: Option<i64>,
    pub current_confidence_bp: i64,
    pub last_measured_at: Option<i64>,

    pub estimator_version: i64,
    pub version: i64,
    pub updated_ts: i64,
}

impl Tripo3dCalibrationRow {
    /// Exact native remaining in micro-units of the proven unit. This needs no estimation and
    /// no bounds once both halves are known. Returns `None` while either half is unknown —
    /// never zero.
    pub fn native_remaining_micro_units(&self) -> Option<i64> {
        self.latest_balance_micro_units?
            .checked_sub(self.latest_frozen_micro_units?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Tripo3dTurnCalibrationEvent {
        Tripo3dTurnCalibrationEvent {
            request_id: "cal_1".into(),
            subject_id: "u_1".into(),
            cohort: "tripo3d-api-50".into(),
            task_type: "image_to_model".into(),
            requested_model_version: Some("v2.5-20250123".into()),
            resolved_model_version: Some("v2.5-20250123".into()),
            tariff_schedule_id: "tripo3d/openapi-billing/2026-08-12".into(),
            priced_ts: 1_800_000_000,
            completed_at: 1_800_000_120,
            upstream_task_id: "task_abc123".into(),
            native_total_millicredits: 20_000,
            api_total_nanousd: 200_000_000,
        }
    }

    #[test]
    fn a_well_formed_turn_event_validates() {
        event().validate().unwrap();
    }

    #[test]
    fn fixed_rate_identity_is_exact() {
        // 20 credits = 20 000 millicredits = 200 000 000 nanoUSD at $0.01/credit.
        assert_eq!(
            TRIPO3D_NANOUSD_PER_MILLICREDIT * 1_000,
            10_000_000,
            "one credit must be exactly $0.01"
        );
        event().validate().unwrap();
    }

    #[test]
    fn version_independent_task_kinds_keep_null_versions() {
        // animate_retarget/convert_model/import_model take no model_version; absence stays
        // None rather than a fabricated value.
        let mut task = event();
        task.task_type = "convert_model".into();
        task.requested_model_version = None;
        task.resolved_model_version = None;
        task.native_total_millicredits = 5_000;
        task.api_total_nanousd = 50_000_000;
        task.validate().unwrap();
    }

    #[test]
    fn a_zero_cost_free_task_is_legal_evidence() {
        // animate_prerigcheck/import_model are officially free, and a failed/expired task
        // refunds to zero: the zero pair is allowed by the joint invariant.
        let mut free = event();
        free.task_type = "animate_prerigcheck".into();
        free.native_total_millicredits = 0;
        free.api_total_nanousd = 0;
        free.validate().unwrap();
    }

    #[test]
    fn a_paid_task_with_a_zero_total_fails_closed() {
        // The schema cannot know which kinds are free, but a partial zero is impossible and a
        // paid-kind zero contradicts the tariff — the metering catalog refuses it upstream.
        let mut broken = event();
        broken.native_total_millicredits = 0;
        assert_eq!(broken.validate(), Err(Tripo3dEventError::LegsDisagree));
        let mut broken = event();
        broken.api_total_nanousd = 0;
        assert_eq!(broken.validate(), Err(Tripo3dEventError::LegsDisagree));
    }

    #[test]
    fn legs_that_disagree_with_the_fixed_rate_fail_closed() {
        let mut broken = event();
        broken.api_total_nanousd += 1;
        assert_eq!(broken.validate(), Err(Tripo3dEventError::LegsDisagree));
    }

    #[test]
    fn missing_identity_or_bad_timestamps_fail_closed() {
        for mutate in [
            (|e: &mut Tripo3dTurnCalibrationEvent| e.request_id = String::new()) as fn(&mut _),
            |e: &mut Tripo3dTurnCalibrationEvent| e.subject_id = String::new(),
            |e: &mut Tripo3dTurnCalibrationEvent| e.cohort = String::new(),
            |e: &mut Tripo3dTurnCalibrationEvent| e.task_type = String::new(),
            |e: &mut Tripo3dTurnCalibrationEvent| e.tariff_schedule_id = String::new(),
            |e: &mut Tripo3dTurnCalibrationEvent| e.upstream_task_id = String::new(),
            |e: &mut Tripo3dTurnCalibrationEvent| e.requested_model_version = Some(String::new()),
        ] {
            let mut broken = event();
            mutate(&mut broken);
            assert_eq!(broken.validate(), Err(Tripo3dEventError::MissingIdentity));
        }
        let mut bad_ts = event();
        bad_ts.priced_ts = 0;
        assert_eq!(bad_ts.validate(), Err(Tripo3dEventError::InvalidTimestamp));
        let mut bad_ts = event();
        bad_ts.completed_at = -1;
        assert_eq!(bad_ts.validate(), Err(Tripo3dEventError::InvalidTimestamp));
        let mut negative = event();
        negative.native_total_millicredits = -1;
        assert_eq!(negative.validate(), Err(Tripo3dEventError::NegativeCounter));
    }

    #[test]
    fn exact_replay_is_recognised_and_a_changed_payload_is_not() {
        let stored = event();
        assert!(event().is_exact_replay_of(&stored));
        // A different payload under the same request id must be a typed conflict upstream,
        // never an update: overwriting would silently rewrite priced history.
        let mut changed = event();
        changed.native_total_millicredits = 30_000;
        changed.api_total_nanousd = 300_000_000;
        assert!(!changed.is_exact_replay_of(&stored));
        assert_eq!(changed.request_id, stored.request_id);
    }

    #[test]
    fn the_typed_conflict_is_detectable_through_anyhow_chains() {
        let error: anyhow::Error = Tripo3dTurnReplayConflict.into();
        let wrapped = error.context("persisting the turn");
        assert!(is_tripo3d_turn_replay_conflict(&wrapped));
        let other = anyhow::anyhow!("unrelated");
        assert!(!is_tripo3d_turn_replay_conflict(&other));
    }

    fn row() -> Tripo3dCalibrationRow {
        Tripo3dCalibrationRow {
            subject_id: "u_1".into(),
            cohort: "tripo3d-api-50".into(),
            anchor_balance_micro_units: Some(5_000_000_000),
            anchor_frozen_micro_units: Some(0),
            anchor_spend_api_nanousd: 0,
            anchor_spend_native_millicredits: 0,
            latest_balance_raw: "4850.5".into(),
            latest_frozen_raw: "20.0".into(),
            latest_balance_micro_units: Some(4_850_500_000),
            latest_frozen_micro_units: Some(20_000_000),
            observed_at: 1_800_000_300,
            observed_spend_api_nanousd: 1_495_000_000,
            observed_spend_native_millicredits: 149_500,
            samples: 3,
            current_capacity_nanousd: Some(48_305_000_000),
            current_low_nanousd: Some(48_305_000_000),
            current_high_nanousd: None,
            current_confidence_bp: 9_000,
            last_measured_at: Some(1_800_000_300),
            estimator_version: 1,
            version: 3,
            updated_ts: 1_800_000_300,
        }
    }

    #[test]
    fn native_remaining_is_exact_once_both_halves_are_known() {
        // 4850.5 − 20.0 = 4830.5 proven units.
        assert_eq!(row().native_remaining_micro_units(), Some(4_830_500_000));
    }

    #[test]
    fn unproven_balance_units_stay_none_and_never_become_zero() {
        // The endpoint's unit is unproven: both parsed halves stay None, so nothing can be
        // derived — and None is never collapsed to 0.
        let mut unproven = row();
        unproven.latest_balance_micro_units = None;
        unproven.latest_frozen_micro_units = None;
        assert_eq!(unproven.native_remaining_micro_units(), None);
        let mut half = row();
        half.latest_frozen_micro_units = None;
        assert_eq!(half.native_remaining_micro_units(), None);
        // The raw text is still always present as the authority.
        assert!(!unproven.latest_balance_raw.is_empty());
    }
}
