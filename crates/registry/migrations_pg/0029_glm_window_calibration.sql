-- Exact plan-scoped authority for GLM (Zhipu AI / Z.ai) Coding Plan subscription calibration.
--
-- This is expand-only and stands BESIDE provider_turn_calibration_events from migration 0019
-- and beside the KIMI authority from migration 0027 rather than extending either. The 0019
-- durable identity cannot carry this provider for the same two reasons as before: its row has
-- one model_id, while GLM silently re-routes glm-5.1/glm-5 onto glm-5.2, which carries a
-- different rate card, and it has no paid plan, which calibration cohorts key on. The KIMI
-- tables cannot carry it either: they record a single API-dollar ledger and poll-only
-- observations with NOT NULL raw counters, while GLM is a GPT-like dual-ledger provider whose
-- quota endpoint evidence has unproven units.
--
-- Three provider facts shape this schema (docs/engine/GLM_PROVIDER.md §5):
--
--   * Dual ledger. GLM publishes the official API rate card (replacement cost) AND the native
--     credits formula — (input × m_in + cached × m_c + output × m_out) / 10 000, with an
--     off-peak ×0.5 outside Mon–Fri 14:00–18:00 UTC+8. Both are exact and are recorded per
--     turn as two independent ledgers: API nanoUSD and native microcredits (credits × 1e6, so
--     the published fractional multipliers stay in integers). One ledger is never
--     reconstructed from the other. Two effective-dated schedule ids pin which rate card and
--     which credit schedule priced the turn.
--   * The window's native capacity needs no estimation: the plan's limits are published
--     (2 000/12 000/28 000 credits per 5h; 10 000/60 000/140 000 weekly for Lite/Pro/Max) and
--     are corroborated by the quota endpoint. Only the official API replacement cost that fits
--     inside the window is estimated.
--   * The quota endpoint's raw counters (currentValue/remaining/number) have UNPROVEN units
--     (manifest §6). They are stored verbatim, and unknown stays NULL — never 0. The derived
--     fraction and its measurement resolution are computed only once the unit semantics are
--     proven live; until then both stay NULL together, rows remain in the cold branch of the
--     estimator-state CHECK, and no capacity is published.
--
-- Windows are identified by their exact native duration in seconds rather than a closed bucket
-- enum: the rolling 5-hour window (18 000 s, reset five hours after consumption) and the
-- weekly window (604 800 s, seven days from order) are independent evidence, and the provider
-- may publish further windows. An unexpected window is recorded as raw evidence, not rejected.

CREATE TABLE IF NOT EXISTS glm_turn_calibration_events (
    request_id text NOT NULL CHECK (request_id <> ''),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    -- Declared paid plan (Lite/Pro/Max). No machine-readable plan identity exists, so the
    -- declared plan is the authoritative cohort key, corroborated by the observed native
    -- window limit.
    plan text NOT NULL CHECK (plan <> ''),

    -- The model the customer asked for, and the model the provider says it served. They differ
    -- whenever the provider silently re-routes glm-5.1/glm-5 onto glm-5.2, and billing follows
    -- the served one.
    requested_model text NOT NULL CHECK (requested_model <> ''),
    served_model text NOT NULL CHECK (served_model <> ''),
    context_mode text NOT NULL CHECK (context_mode IN ('200k', '1m')),
    -- Only GLM-5.2 takes a reasoning effort at all, so absence stays NULL rather than a
    -- fabricated value. Canonical set after the provider's mapping (low/medium→high,
    -- xhigh→max, none/minimal→off).
    reasoning_effort text CHECK (reasoning_effort IS NULL OR reasoning_effort IN ('off', 'high', 'max')),

    -- Two effective-dated schedules priced this turn: the official API rate card and the
    -- native credit multipliers.
    api_tariff_schedule_id text NOT NULL CHECK (api_tariff_schedule_id <> ''),
    credit_schedule_id text NOT NULL CHECK (credit_schedule_id <> ''),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    completed_at bigint NOT NULL CHECK (completed_at > 0),

    -- Disjoint usage legs. GLM publishes no cache TTL split, so there is one cache-write leg.
    fresh_input_tokens bigint NOT NULL CHECK (fresh_input_tokens >= 0),
    cached_input_tokens bigint NOT NULL CHECK (cached_input_tokens >= 0),
    cache_write_tokens bigint NOT NULL CHECK (cache_write_tokens >= 0),
    output_tokens bigint NOT NULL CHECK (output_tokens >= 0),
    -- Subset of output_tokens, never billed as its own leg.
    reasoning_tokens bigint NOT NULL CHECK (reasoning_tokens >= 0),

    -- Official API replacement cost. Cache storage is documented as free, so there is no
    -- cache-write cost leg.
    api_fresh_input_nanousd bigint NOT NULL CHECK (api_fresh_input_nanousd >= 0),
    api_cached_input_nanousd bigint NOT NULL CHECK (api_cached_input_nanousd >= 0),
    api_output_nanousd bigint NOT NULL CHECK (api_output_nanousd >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd > 0),

    -- Native credits from the provider's published formula, in microcredits (credits × 1e6).
    -- Independent of the API-dollar legs above and never derived from them.
    native_fresh_input_microcredits bigint NOT NULL CHECK (native_fresh_input_microcredits >= 0),
    native_cached_input_microcredits bigint NOT NULL CHECK (native_cached_input_microcredits >= 0),
    native_output_microcredits bigint NOT NULL CHECK (native_output_microcredits >= 0),
    native_total_microcredits bigint NOT NULL CHECK (native_total_microcredits > 0),
    -- Whether the off-peak ×0.5 schedule (outside Mon–Fri 14:00–18:00 UTC+8) was applied.
    off_peak boolean NOT NULL,

    PRIMARY KEY (request_id),
    CHECK (reasoning_tokens <= output_tokens),
    CHECK (
        fresh_input_tokens > 0 OR cached_input_tokens > 0 OR cache_write_tokens > 0
        OR output_tokens > 0
    ),
    CHECK (
        api_total_nanousd = api_fresh_input_nanousd + api_cached_input_nanousd
            + api_output_nanousd
    ),
    CHECK (
        native_total_microcredits = native_fresh_input_microcredits
            + native_cached_input_microcredits + native_output_microcredits
    )
);

CREATE INDEX IF NOT EXISTS glm_turn_calibration_subject_time
    ON glm_turn_calibration_events(subject_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS glm_turn_calibration_served_model_time
    ON glm_turn_calibration_events(served_model, completed_at DESC);
CREATE INDEX IF NOT EXISTS glm_turn_calibration_cohort
    ON glm_turn_calibration_events(plan, served_model, completed_at DESC);

-- Cumulative dual ledgers per subject: official API replacement cost AND native microcredits.
-- Advanced in the same transaction that wins the immutable event insert above, so a quota
-- observation can never be paired with a stale spend total. The two ledgers are independent
-- by construction: one is never restored from the other.
CREATE TABLE IF NOT EXISTS glm_calibration_subject_spend (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    spent_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (spent_api_nanousd >= 0),
    spent_native_microcredits bigint NOT NULL DEFAULT 0 CHECK (spent_native_microcredits >= 0),
    tracking_started_ts bigint NOT NULL CHECK (tracking_started_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (subject_id)
);

-- Immutable quota observations from GET /api/monitor/usage/quota/limit. Quota can also arrive
-- on a generation response, so the source is explicit: a response-carried observation names
-- the request that carried it, while a poll invents no request id.
CREATE TABLE IF NOT EXISTS glm_window_observations (
    id bigserial PRIMARY KEY,
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),

    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    -- nextResetTime when the provider supplied it; a rolling window may not name one.
    reset_at bigint CHECK (reset_at IS NULL OR reset_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- Raw provider counters: the authority. Their unit is unproven (credits? tokens? something
    -- else?), so they are preserved verbatim and unknown stays NULL — never 0.
    native_used_units bigint CHECK (native_used_units IS NULL OR native_used_units >= 0),
    native_limit_units bigint CHECK (native_limit_units IS NULL OR native_limit_units > 0),
    native_remaining_units bigint CHECK (native_remaining_units IS NULL OR native_remaining_units >= 0),
    percentage_raw bigint,

    -- Derived from the raw counters above, only once the unit semantics are proven. Until then
    -- both stay NULL together; storing a coarse snapshot in a wide integer does not make it
    -- precise.
    used_fraction_units bigint CHECK (used_fraction_units IS NULL OR used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint CHECK (measurement_resolution_fraction_units IS NULL OR measurement_resolution_fraction_units BETWEEN 1 AND 100000000),

    -- Cumulative dual ledgers for this subject at observation time.
    cumulative_api_nanousd bigint NOT NULL CHECK (cumulative_api_nanousd >= 0),
    cumulative_native_microcredits bigint NOT NULL CHECK (cumulative_native_microcredits >= 0),
    observation_source text NOT NULL CHECK (observation_source IN ('poll', 'response')),
    source_request_id text CHECK (source_request_id IS NULL OR source_request_id <> ''),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),

    CHECK (native_used_units IS NULL OR native_limit_units IS NULL
        OR native_used_units <= native_limit_units),
    CHECK (native_remaining_units IS NULL OR native_limit_units IS NULL
        OR native_remaining_units <= native_limit_units),
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
        cumulative_native_microcredits
    )
);

CREATE INDEX IF NOT EXISTS glm_window_observations_window
    ON glm_window_observations(subject_id, plan, window_duration_secs, reset_at, observed_at);

-- Estimator state per subject + declared plan + exact native window duration. Independent
-- durations never share a row: the rolling 5-hour and the weekly window are separate evidence.
CREATE TABLE IF NOT EXISTS glm_window_calibrations (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    reset_at bigint CHECK (reset_at IS NULL OR reset_at > 0),

    -- The first snapshot of an interval is an anchor, not a sample. Anchor fraction legs stay
    -- NULL while the endpoint's units are unproven; the cumulative ledgers are always exact.
    anchor_used_fraction_units bigint CHECK (anchor_used_fraction_units IS NULL OR anchor_used_fraction_units BETWEEN 0 AND 100000000),
    anchor_resolution_fraction_units bigint CHECK (anchor_resolution_fraction_units IS NULL OR anchor_resolution_fraction_units BETWEEN 1 AND 100000000),
    anchor_spend_api_nanousd bigint NOT NULL CHECK (anchor_spend_api_nanousd >= 0),
    anchor_spend_native_microcredits bigint NOT NULL CHECK (anchor_spend_native_microcredits >= 0),

    used_fraction_units bigint CHECK (used_fraction_units IS NULL OR used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint CHECK (measurement_resolution_fraction_units IS NULL OR measurement_resolution_fraction_units BETWEEN 1 AND 100000000),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- The window's native size in microcredits, from the plan's published limits corroborated
    -- by observations — never estimated. NULL until either source names it.
    native_limit_microcredits bigint CHECK (native_limit_microcredits IS NULL OR native_limit_microcredits > 0),
    native_used_microcredits bigint CHECK (native_used_microcredits IS NULL OR native_used_microcredits >= 0),

    observed_fraction_units bigint NOT NULL DEFAULT 0 CHECK (observed_fraction_units >= 0),
    observed_spend_api_nanousd bigint NOT NULL DEFAULT 0 CHECK (observed_spend_api_nanousd >= 0),
    observed_spend_native_microcredits bigint NOT NULL DEFAULT 0 CHECK (observed_spend_native_microcredits >= 0),
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
    CHECK (native_used_microcredits IS NULL OR native_limit_microcredits IS NULL
        OR native_used_microcredits <= native_limit_microcredits),
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
    -- native credits together. The high may still be NULL when movement did not exceed the
    -- quantisation envelope.
    CHECK (
        (samples = 0
            AND observed_fraction_units = 0
            AND observed_spend_api_nanousd = 0
            AND observed_spend_native_microcredits = 0
            AND current_capacity_nanousd IS NULL
            AND current_low_nanousd IS NULL
            AND current_high_nanousd IS NULL
            AND current_confidence_bp = 0
            AND last_measured_at IS NULL)
        OR
        (samples > 0
            AND observed_fraction_units > 0
            AND observed_spend_api_nanousd > 0
            AND observed_spend_native_microcredits > 0
            AND current_capacity_nanousd IS NOT NULL
            AND current_low_nanousd IS NOT NULL
            AND last_measured_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS glm_window_calibrations_cohort
    ON glm_window_calibrations(plan, window_duration_secs);

INSERT INTO engine_schema_migrations(version) VALUES (29)
ON CONFLICT (version) DO NOTHING;
