-- Exact plan-scoped authority for Suno (suno.com) subscription session-pool calibration.
--
-- This is expand-only and stands BESIDE provider_turn_calibration_events from migration 0019,
-- beside the KIMI authority from migration 0027, beside the GLM authority from migration 0029
-- and beside the Tripo3D authority from migration 0049 rather than extending any of them. The
-- 0019 durable identity cannot carry this provider: its row models a chat turn with token
-- legs, while Suno is a task-based media API (create -> poll -> download) whose money identity
-- is a settled song generation measured in native credits. The Tripo3D tables cannot carry it
-- either: they are a windowless prepaid-balance authority, while Suno subscriptions refill
-- credits on a MONTHLY window (Pro 2 500 / Premier 10 000 credits, published per plan), so
-- the KIMI/GLM window dimension is present here in full. The KIMI/GLM tables themselves
-- cannot be reused: neither carries a per-turn derived-schedule flag, and Suno's quota
-- endpoint semantics are unproven in a way that needs its own raw evidence shape.
--
-- Three provider facts shape this schema (docs/engine/SUNO_PROVIDER.md §5):
--
--   * Dual ledger at a DERIVED fixed rate. Suno has no official API and no official rate
--     card, so the customer-facing nanoUSD tariff is a reviewed derived schedule: $0.004 per
--     credit (the worst subscription unit economics, Pro $10 / 2 500 credits). Every settled
--     turn carries two exact legs that MUST agree by construction: native millicredits
--     (credits × 1e3) and API nanoUSD (millicredits × 4 000). The legs are stored separately
--     so a future official money anchor re-keys the schedule instead of rewriting history.
--     Unlike Tripo3D, the native leg is NOT always provider truth: the provider may not
--     report per-turn credit consumption, in which case the reviewed 5-credits-per-song
--     nominal is recorded with native_schedule_derived = TRUE — a schedule-derived number is
--     never presented as provider-reported consumption (manifest §5.1/§5.3). A finalized-but-
--     failed generation with zero credit movement refunds its hold and settles at the legal
--     zero pair; a partial zero (one ledger zero, the other positive) is impossible by the
--     same equality CHECK.
--   * The window's native capacity needs no estimation: the plan's monthly limits are
--     published (2 500 / 10 000 credits) and corroborated by the quota endpoint. Only the
--     API-dollar replacement cost that fits inside the window is estimated, via the manifest
--     §10.5 fraction formula over the dual cumulative ledgers.
--   * The quota endpoint's raw counters (GET /api/billing/info/: total_credits_left, period,
--     monthly_limit, monthly_usage) are an oss-hypothesis with UNPROVEN semantics (manifest
--     §5.2/§6: is total_credits_left inclusive of top-ups? does period name the reset?). They
--     are preserved VERBATIM — unknown stays NULL, never 0 — and the derived fraction with its
--     measurement resolution is computed only when the field semantics allow it (limit > 0 and
--     usage present); until then both stay NULL together, rows remain in the cold branch of
--     the estimator-state CHECK, and no capacity is published.
--
-- Windows are identified by their exact native duration in seconds, the GLM discipline from
-- migration 0029: the monthly reset is anchored to the subscription's billing date and its
-- exact duration is UNKNOWN (manifest §1), so NO synthetic constant is published anywhere — a
-- nominal 30- or 31-day value would silently become somebody's assumption. The runtime keys a
-- window by the duration the reset evidence actually shows; an unknown duration fails closed.
-- reset_at is stored when the raw `period` field supplies a reset anchor, NULL otherwise.

CREATE TABLE IF NOT EXISTS suno_turn_calibration_events (
    request_id text NOT NULL CHECK (request_id <> ''),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    -- Declared paid plan (Pro/Premier). No machine-readable plan identity is proven yet, so
    -- the declared plan is the authoritative cohort key, corroborated by the observed native
    -- window limit at admission. The Free tier is excluded by design (manifest §1: no
    -- commercial rights, daily drip, anti-pooling clause) and is not admitted by this CHECK.
    plan text NOT NULL CHECK (plan IN ('Pro', 'Premier')),

    -- The model the customer asked for, and the model the upstream response says it served.
    -- served_model is NULLABLE until the live matrix pins the wire id spellings (manifest §3:
    -- the exact `mv` values are unknown) — absence stays NULL rather than a fabricated copy of
    -- requested_model. No per-model price differentiation is published, so billing follows the
    -- flat schedule, not the served model; a mismatch is a typed anomaly in the estimator.
    requested_model text NOT NULL CHECK (requested_model <> ''),
    served_model text CHECK (served_model IS NULL OR served_model <> ''),
    -- The effective-dated derived schedule that priced the reserve and cross-checked the
    -- settlement (e.g. suno/derived-subscription/2026-08-12).
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    completed_at bigint NOT NULL CHECK (completed_at > 0),
    -- Audit metadata only: the upstream clip/task id of the settled generation is never the
    -- money identity — request_id is.
    upstream_clip_id text NOT NULL CHECK (upstream_clip_id <> ''),

    -- Native consumption in millicredits (credits × 1e3) and the exact API replacement cost
    -- derived from it at the reviewed fixed rate (millicredits × 4 000 nanoUSD = credits ×
    -- 4 000 000 nanoUSD at $0.004/credit).
    native_total_millicredits bigint NOT NULL CHECK (native_total_millicredits >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd >= 0),
    -- TRUE when the native leg came from the reviewed 5-credits-per-song schedule rather than
    -- provider-reported consumption. A schedule-derived amount is estimate-grade evidence and
    -- is never presented as provider truth (manifest §5.3).
    native_schedule_derived boolean NOT NULL,

    PRIMARY KEY (request_id),
    -- The derived fixed rate is part of the schema: the two legs agree exactly, or the row is
    -- not evidence. A partial zero (one ledger zero, the other positive) is impossible by this
    -- same equality, so a refunded failed generation carries the legal zero pair and a paid
    -- generation can never carry a zero total.
    CHECK (api_total_nanousd = native_total_millicredits * 4000)
);

CREATE INDEX IF NOT EXISTS suno_turn_calibration_subject_time
    ON suno_turn_calibration_events(subject_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS suno_turn_calibration_model_time
    ON suno_turn_calibration_events(requested_model, completed_at DESC);
CREATE INDEX IF NOT EXISTS suno_turn_calibration_cohort
    ON suno_turn_calibration_events(plan, requested_model, completed_at DESC);

-- Cumulative dual ledgers per subject: exact API nanoUSD AND native millicredits. Advanced in
-- the same transaction that wins the immutable event insert above, so a quota observation can
-- never be paired with a stale spend total. The API leg is the fixed-rate image of the native
-- leg, but both are exact sums over immutable events — neither is back-computed from the other
-- at read time.
CREATE TABLE IF NOT EXISTS suno_calibration_subject_spend (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    spent_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (spent_api_nanousd >= 0),
    spent_native_millicredits bigint NOT NULL DEFAULT 0 CHECK (spent_native_millicredits >= 0),
    tracking_started_ts bigint NOT NULL CHECK (tracking_started_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (subject_id)
);

-- Immutable quota observations from GET /api/billing/info/. Quota can also be read in the
-- wake of a generation response, so the source is explicit: a response-carried observation
-- names the request that carried it, while a poll invents no request id.
CREATE TABLE IF NOT EXISTS suno_window_observations (
    id bigserial PRIMARY KEY,
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan IN ('Pro', 'Premier')),

    -- Exact native window duration in seconds from reset evidence. The monthly reset is
    -- billing-anchored and its duration is unknown until observed; an unknown duration fails
    -- closed instead of assuming a synthetic constant.
    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    -- The reset anchor when the raw `period` field supplies it; NULL otherwise.
    reset_at bigint CHECK (reset_at IS NULL OR reset_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- Raw provider counters, VERBATIM: monthly_limit, monthly_usage and total_credits_left.
    -- Their semantics are unproven (manifest §5.2), so they are preserved as-is and unknown
    -- stays NULL — never 0. No remaining<=limit CHECK exists on purpose: whether
    -- total_credits_left includes top-up credits is unknown, so remaining may legitimately
    -- exceed the monthly subscription limit and the schema must not reject that evidence.
    native_limit_units bigint CHECK (native_limit_units IS NULL OR native_limit_units > 0),
    native_used_units bigint CHECK (native_used_units IS NULL OR native_used_units >= 0),
    native_remaining_units bigint CHECK (native_remaining_units IS NULL OR native_remaining_units >= 0),
    -- The raw `period` value, verbatim. Its format is unproven; when it names a reset it also
    -- feeds reset_at above.
    period_raw text CHECK (period_raw IS NULL OR period_raw <> ''),

    -- Derived from the raw counters above, only when the field semantics allow it (limit > 0
    -- and usage present; resolution = ceil(SCALE / limit)). Until then both stay NULL
    -- together; storing a coarse snapshot in a wide integer does not make it precise.
    used_fraction_units bigint CHECK (used_fraction_units IS NULL OR used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint CHECK (measurement_resolution_fraction_units IS NULL OR measurement_resolution_fraction_units BETWEEN 1 AND 100000000),

    -- Cumulative dual ledgers for this subject at observation time.
    cumulative_api_nanousd bigint NOT NULL CHECK (cumulative_api_nanousd >= 0),
    cumulative_native_millicredits bigint NOT NULL CHECK (cumulative_native_millicredits >= 0),
    observation_source text NOT NULL CHECK (observation_source IN ('poll', 'response')),
    source_request_id text CHECK (source_request_id IS NULL OR source_request_id <> ''),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),

    CHECK (native_used_units IS NULL OR native_limit_units IS NULL
        OR native_used_units <= native_limit_units),
    CHECK ((used_fraction_units IS NULL) = (measurement_resolution_fraction_units IS NULL)),
    CHECK (observation_source <> 'poll' OR source_request_id IS NULL),
    CHECK (observation_source <> 'response' OR source_request_id IS NOT NULL),
    -- A duplicate poll must be a no-op rather than a new sample. The raw counters are
    -- nullable, so the dedup key treats NULLs as equal.
    UNIQUE NULLS NOT DISTINCT (
        subject_id,
        plan,
        window_duration_secs,
        reset_at,
        observed_at,
        native_used_units,
        native_limit_units,
        native_remaining_units,
        cumulative_api_nanousd,
        cumulative_native_millicredits
    )
);

CREATE INDEX IF NOT EXISTS suno_window_observations_window
    ON suno_window_observations(subject_id, plan, window_duration_secs, reset_at, observed_at);

-- Estimator state per subject + declared plan + exact native window duration. Independent
-- durations never share a row.
CREATE TABLE IF NOT EXISTS suno_window_calibrations (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan IN ('Pro', 'Premier')),
    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    reset_at bigint CHECK (reset_at IS NULL OR reset_at > 0),

    -- The first snapshot of an interval is an anchor, not a sample. Anchor fraction legs stay
    -- NULL while the endpoint's field semantics are unproven; the cumulative ledgers are
    -- always exact.
    anchor_used_fraction_units bigint CHECK (anchor_used_fraction_units IS NULL OR anchor_used_fraction_units BETWEEN 0 AND 100000000),
    anchor_resolution_fraction_units bigint CHECK (anchor_resolution_fraction_units IS NULL OR anchor_resolution_fraction_units BETWEEN 1 AND 100000000),
    anchor_spend_api_nanousd bigint NOT NULL CHECK (anchor_spend_api_nanousd >= 0),
    anchor_spend_native_millicredits bigint NOT NULL CHECK (anchor_spend_native_millicredits >= 0),

    used_fraction_units bigint CHECK (used_fraction_units IS NULL OR used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint CHECK (measurement_resolution_fraction_units IS NULL OR measurement_resolution_fraction_units BETWEEN 1 AND 100000000),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- The window's native size in millicredits, from the plan's published monthly limits
    -- (Pro 2 500 / Premier 10 000 credits) corroborated by observations — never estimated.
    -- NULL until either source names it.
    native_limit_millicredits bigint CHECK (native_limit_millicredits IS NULL OR native_limit_millicredits > 0),
    native_used_millicredits bigint CHECK (native_used_millicredits IS NULL OR native_used_millicredits >= 0),

    observed_fraction_units bigint NOT NULL DEFAULT 0 CHECK (observed_fraction_units >= 0),
    observed_spend_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (observed_spend_api_nanousd >= 0),
    observed_spend_native_millicredits bigint NOT NULL DEFAULT 0 CHECK (observed_spend_native_millicredits >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),
    unattributed_fraction_units bigint NOT NULL DEFAULT 0 CHECK (unattributed_fraction_units >= 0),

    -- Unknown stays NULL. An unbounded high stays NULL rather than becoming a guessed ceiling.
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

    PRIMARY KEY (subject_id, plan, window_duration_secs),
    CHECK (native_used_millicredits IS NULL OR native_limit_millicredits IS NULL
        OR native_used_millicredits <= native_limit_millicredits),
    CHECK ((used_fraction_units IS NULL) = (measurement_resolution_fraction_units IS NULL)),
    CHECK ((anchor_used_fraction_units IS NULL) = (anchor_resolution_fraction_units IS NULL)),
    CHECK (current_low_nanousd IS NULL OR current_capacity_nanousd IS NOT NULL),
    CHECK (current_high_nanousd IS NULL OR current_capacity_nanousd IS NOT NULL),
    CHECK (current_low_nanousd IS NULL OR current_capacity_nanousd >= current_low_nanousd),
    CHECK (current_high_nanousd IS NULL OR current_capacity_nanousd <= current_high_nanousd),
    CHECK (current_low_nanousd IS NULL OR current_high_nanousd IS NULL
        OR current_low_nanousd <= current_high_nanousd),
    -- Cold state publishes nothing; a measured state publishes a capacity and a proven low.
    -- Both ledgers must have moved, because every measured turn advances API dollars and
    -- native credits together (schedule-derived or not). The high may still be NULL when
    -- movement did not exceed the quantisation envelope.
    CHECK (
        (samples = 0
            AND observed_fraction_units = 0
            AND observed_spend_api_nanousd = 0
            AND observed_spend_native_millicredits = 0
            AND current_capacity_nanousd IS NULL
            AND current_low_nanousd IS NULL
            AND current_high_nanousd IS NULL
            AND current_confidence_bp = 0
            AND last_measured_at IS NULL)
        OR
        (samples > 0
            AND observed_fraction_units > 0
            AND observed_spend_api_nanousd > 0
            AND observed_spend_native_millicredits > 0
            AND current_capacity_nanousd IS NOT NULL
            AND current_low_nanousd IS NOT NULL
            AND last_measured_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS suno_window_calibrations_cohort
    ON suno_window_calibrations(plan, window_duration_secs);

INSERT INTO engine_schema_migrations(version) VALUES (50)
ON CONFLICT (version) DO NOTHING;
