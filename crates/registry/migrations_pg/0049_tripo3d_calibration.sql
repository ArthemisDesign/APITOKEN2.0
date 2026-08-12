-- Exact subject-scoped authority for Tripo3D (VAST / Holymolly) prepaid API calibration.
--
-- This is expand-only and stands BESIDE provider_turn_calibration_events from migration 0019,
-- beside the KIMI authority from migration 0027 and beside the GLM authority from migration
-- 0029 rather than extending any of them. The 0019 durable identity cannot carry this provider:
-- its row models a chat turn with token legs, while Tripo3D is a task-based media API whose
-- money identity is the provider-reported consumed_credit of a finished task. The KIMI/GLM
-- tables cannot carry it either: both are quota-window authorities (native limit, reset,
-- fraction of a window), and a Tripo3D prepaid balance has NO window — purchased credits never
-- expire and never reset, so there is no duration to key on and nothing to estimate a fraction
-- of. Calibration here answers not "how much fits in the window" but "how much sellable
-- capacity remains on the balance": balance − frozen, exact once the unit is proven, NULL
-- until then.
--
-- Three provider facts shape this schema (docs/engine/TRIPO3D_PROVIDER.md §5):
--
--   * Dual ledger with a fixed exchange rate. Tripo3D publishes one prepaid price
--     ($0.01/credit) and reports an authoritative consumed_credit per finished task. Every
--     settled turn therefore carries two exact legs that MUST agree by construction: native
--     millicredits (credits × 1e3, so any fractional credit stays an integer) and API nanoUSD
--     (millicredits × 10 000, i.e. credits × 10 000 000 at the published rate). Unlike GLM the
--     API-dollar leg is derived from the native leg by the fixed rate and the schema enforces
--     it; the legs are still stored separately so a future rate-card change re-keys the
--     schedule instead of rewriting history.
--   * Documented free tasks. animate_prerigcheck and import_model are officially free, and a
--     failed/expired task settles at zero. A zero pair (0 millicredits = 0 nanoUSD) is
--     therefore legal evidence; a PAID task can never carry a zero total. The schema cannot
--     know which task kinds are free, so it enforces only the joint invariant: the two totals
--     are zero together or positive together, and a partial zero (one ledger zero, the other
--     positive) is impossible. The metering catalog (crates/metering/src/tripo3d.rs) is the
--     authority on which task kinds may settle at zero; it fails closed on unknown kinds.
--   * The balance endpoint's raw counters (balance/frozen, floats on the wire) have UNPROVEN
--     units (manifest §5.2/§6). They are preserved VERBATIM as text — money is never parsed
--     through binary float — and the parsed fixed-point halves (micro-units of the proven
--     native unit) stay NULL until a live run proves the unit. Unknown stays NULL, never 0,
--     and no capacity is published while either half is unproven.
--
-- The "window" dimension of the KIMI/GLM shape degenerates: prepaid balance has no reset, so
-- there is no window_duration_secs anywhere in these tables. Calibration state keys on
-- subject + declared top-up cohort (the Auth Bot product cohort, e.g. a declared $50 top-up),
-- which is the only cohort axis the provider exposes.

CREATE TABLE IF NOT EXISTS tripo3d_turn_calibration_events (
    request_id text NOT NULL CHECK (request_id <> ''),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    -- Declared top-up cohort of the offer product (e.g. the catalog entry "Tripo3D API $50").
    -- No machine-readable plan identity exists, so the declared cohort is the authoritative
    -- cohort key, corroborated by the balance anchor at admission.
    cohort text NOT NULL CHECK (cohort <> ''),

    -- The exact upstream task this turn settled: the task kind and the model_version the
    -- customer asked for vs the one the task actually ran with. They are recorded separately
    -- so a silent re-route cannot misprice — money follows consumed_credit, and a mismatch
    -- with the tariff's maximum for the admitted shape is a typed anomaly in the estimator.
    task_type text NOT NULL CHECK (task_type <> ''),
    requested_model_version text CHECK (
        requested_model_version IS NULL OR requested_model_version <> ''
    ),
    resolved_model_version text CHECK (
        resolved_model_version IS NULL OR resolved_model_version <> ''
    ),
    -- The effective-dated rate card that priced the reserve and cross-checked the settlement.
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    completed_at bigint NOT NULL CHECK (completed_at > 0),
    -- Audit metadata only: tasks are queryable solely by the creating key, and the provider
    -- task id is never the money identity — request_id is.
    upstream_task_id text NOT NULL CHECK (upstream_task_id <> ''),

    -- Authoritative native consumption from the task's consumed_credit, in millicredits
    -- (credits × 1e3), and the exact API replacement cost derived from it at the published
    -- fixed rate (millicredits × 10 000 nanoUSD = credits × 10 000 000).
    native_total_millicredits bigint NOT NULL CHECK (native_total_millicredits >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd >= 0),

    PRIMARY KEY (request_id),
    -- The fixed rate is part of the schema: the two legs agree exactly, or the row is not
    -- evidence. A partial zero (one ledger zero, the other positive) is impossible by this
    -- same equality, so no separate free-task flag exists.
    CHECK (api_total_nanousd = native_total_millicredits * 10000)
);

CREATE INDEX IF NOT EXISTS tripo3d_turn_calibration_subject_time
    ON tripo3d_turn_calibration_events(subject_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS tripo3d_turn_calibration_task_time
    ON tripo3d_turn_calibration_events(task_type, completed_at DESC);
CREATE INDEX IF NOT EXISTS tripo3d_turn_calibration_cohort
    ON tripo3d_turn_calibration_events(cohort, task_type, completed_at DESC);

-- Cumulative dual ledgers per subject: exact API nanoUSD AND native millicredits. Advanced in
-- the same transaction that wins the immutable event insert above, so a balance observation
-- can never be paired with a stale spend total. The API leg is the fixed-rate image of the
-- native leg, but both are exact sums over immutable events — neither is back-computed from
-- the other at read time.
CREATE TABLE IF NOT EXISTS tripo3d_calibration_subject_spend (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    spent_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (spent_api_nanousd >= 0),
    spent_native_millicredits bigint NOT NULL DEFAULT 0 CHECK (spent_native_millicredits >= 0),
    tracking_started_ts bigint NOT NULL CHECK (tracking_started_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (subject_id)
);

-- Immutable balance observations from GET /v2/openapi/user/balance. A balance can also be
-- read in the wake of a task response, so the source is explicit: a response-carried
-- observation names the request that carried it, while a poll invents no request id.
CREATE TABLE IF NOT EXISTS tripo3d_balance_observations (
    id bigserial PRIMARY KEY,
    subject_id text NOT NULL CHECK (subject_id <> ''),
    cohort text NOT NULL CHECK (cohort <> ''),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- Raw provider values VERBATIM: the authority. The endpoint returns floats; money is
    -- never parsed through binary float, so the raw text is what is preserved and compared.
    balance_raw text NOT NULL CHECK (balance_raw <> ''),
    frozen_raw text NOT NULL CHECK (frozen_raw <> ''),
    -- Parsed fixed-point micro-units (units × 1e6) of the proven native unit. The unit of
    -- balance/frozen (credits vs dollars, decimal semantics) is UNPROVEN (manifest §6), so
    -- both halves stay NULL until a live run proves it — unknown stays NULL, never 0.
    balance_micro_units bigint CHECK (balance_micro_units IS NULL OR balance_micro_units >= 0),
    frozen_micro_units bigint CHECK (frozen_micro_units IS NULL OR frozen_micro_units >= 0),

    -- Cumulative dual ledgers for this subject at observation time.
    cumulative_api_nanousd bigint NOT NULL CHECK (cumulative_api_nanousd >= 0),
    cumulative_native_millicredits bigint NOT NULL CHECK (cumulative_native_millicredits >= 0),
    observation_source text NOT NULL CHECK (observation_source IN ('poll', 'response')),
    source_request_id text CHECK (source_request_id IS NULL OR source_request_id <> ''),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),

    CHECK (frozen_micro_units IS NULL OR balance_micro_units IS NULL
        OR frozen_micro_units <= balance_micro_units),
    CHECK (observation_source <> 'poll' OR source_request_id IS NULL),
    CHECK (observation_source <> 'response' OR source_request_id IS NOT NULL),
    -- A duplicate poll must be a no-op rather than a new sample. The parsed halves are
    -- nullable, so the dedup key treats NULLs as equal.
    UNIQUE NULLS NOT DISTINCT (
        subject_id,
        cohort,
        observed_at,
        balance_raw,
        frozen_raw,
        balance_micro_units,
        frozen_micro_units,
        cumulative_api_nanousd,
        cumulative_native_millicredits
    )
);

CREATE INDEX IF NOT EXISTS tripo3d_balance_observations_subject
    ON tripo3d_balance_observations(subject_id, cohort, observed_at);

-- Estimator state per subject + declared top-up cohort. There is exactly one balance track
-- per subject, so the KIMI/GLM window dimension degenerates to nothing: prepaid credits never
-- reset, and cohort replaces plan+window as the cohort axis.
CREATE TABLE IF NOT EXISTS tripo3d_calibration_state (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    cohort text NOT NULL CHECK (cohort <> ''),

    -- The first observation of the track is an anchor, not a sample. Anchor balance halves
    -- stay NULL while the endpoint's units are unproven; the cumulative spend legs are always
    -- exact.
    anchor_balance_micro_units bigint CHECK (
        anchor_balance_micro_units IS NULL OR anchor_balance_micro_units >= 0
    ),
    anchor_frozen_micro_units bigint CHECK (
        anchor_frozen_micro_units IS NULL OR anchor_frozen_micro_units >= 0
    ),
    anchor_spend_api_nanousd bigint NOT NULL CHECK (anchor_spend_api_nanousd >= 0),
    anchor_spend_native_millicredits bigint NOT NULL CHECK (anchor_spend_native_millicredits >= 0),

    -- Latest raw halves, verbatim and parsed. The state row is always written together with an
    -- observation, so the raw text is always present; the parsed halves stay NULL until the
    -- unit is proven.
    latest_balance_raw text NOT NULL CHECK (latest_balance_raw <> ''),
    latest_frozen_raw text NOT NULL CHECK (latest_frozen_raw <> ''),
    latest_balance_micro_units bigint CHECK (
        latest_balance_micro_units IS NULL OR latest_balance_micro_units >= 0
    ),
    latest_frozen_micro_units bigint CHECK (
        latest_frozen_micro_units IS NULL OR latest_frozen_micro_units >= 0
    ),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    observed_spend_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (observed_spend_api_nanousd >= 0),
    observed_spend_native_millicredits bigint NOT NULL DEFAULT 0
        CHECK (observed_spend_native_millicredits >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),

    -- Remaining sellable capacity in API nanoUSD, driven by the balance track
    -- (balance − frozen at the fixed rate) once the unit is proven — not by a window
    -- fraction. Unknown stays NULL; an unbounded high stays NULL rather than becoming a
    -- guessed ceiling.
    current_capacity_nanousd bigint
        CHECK (current_capacity_nanousd IS NULL OR current_capacity_nanousd >= 0),
    current_low_nanousd bigint CHECK (current_low_nanousd IS NULL OR current_low_nanousd >= 0),
    current_high_nanousd bigint CHECK (current_high_nanousd IS NULL OR current_high_nanousd >= 0),
    current_confidence_bp bigint NOT NULL DEFAULT 0
        CHECK (current_confidence_bp BETWEEN 0 AND 10000),
    last_measured_at bigint CHECK (last_measured_at IS NULL OR last_measured_at > 0),

    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),

    PRIMARY KEY (subject_id, cohort),
    CHECK (latest_frozen_micro_units IS NULL OR latest_balance_micro_units IS NULL
        OR latest_frozen_micro_units <= latest_balance_micro_units),
    CHECK (current_low_nanousd IS NULL OR current_capacity_nanousd IS NOT NULL),
    CHECK (current_high_nanousd IS NULL OR current_capacity_nanousd IS NOT NULL),
    CHECK (current_low_nanousd IS NULL OR current_capacity_nanousd >= current_low_nanousd),
    CHECK (current_high_nanousd IS NULL OR current_capacity_nanousd <= current_high_nanousd),
    CHECK (current_low_nanousd IS NULL OR current_high_nanousd IS NULL
        OR current_low_nanousd <= current_high_nanousd),
    -- Cold state publishes nothing; a measured state publishes a capacity and a proven low.
    -- A measured row requires proven balance halves (unproven units can never publish) and
    -- observed spend on both ledgers, because every measured paid turn advances API dollars
    -- and native credits together. The high may still be NULL when the balance is the only
    -- capacity bound and its unit resolution does not pin an upper envelope.
    CHECK (
        (samples = 0
            AND observed_spend_api_nanousd = 0
            AND observed_spend_native_millicredits = 0
            AND current_capacity_nanousd IS NULL
            AND current_low_nanousd IS NULL
            AND current_high_nanousd IS NULL
            AND current_confidence_bp = 0
            AND last_measured_at IS NULL)
        OR
        (samples > 0
            AND observed_spend_api_nanousd > 0
            AND observed_spend_native_millicredits > 0
            AND latest_balance_micro_units IS NOT NULL
            AND latest_frozen_micro_units IS NOT NULL
            AND current_capacity_nanousd IS NOT NULL
            AND current_low_nanousd IS NOT NULL
            AND last_measured_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS tripo3d_calibration_state_cohort
    ON tripo3d_calibration_state(cohort);

INSERT INTO engine_schema_migrations(version) VALUES (49)
ON CONFLICT (version) DO NOTHING;
