-- Exact plan-scoped authority for KIMI (Kimi Code) subscription calibration.
--
-- This is expand-only and stands BESIDE provider_turn_calibration_events from migration 0019
-- rather than extending it. That table's durable identity cannot carry this provider: its
-- provider CHECK is a closed set, and more importantly its row has one model_id, while KIMI can
-- serve a model nobody requested — disabling thinking re-routes k3 and kimi-for-coding to K2.6,
-- which carries a different rate card. Billing follows the served model, so requested and served
-- must both be part of the immutable record. The paid plan is also part of the durable identity
-- here, because KIMI publishes it machine-readably on /me and calibration cohorts key on it.
--
-- Quota evidence differs from Claude and Gemini in two ways that the schema encodes literally:
--
--   * The provider reports quota as integer used/limit counters per window, not as a percentage.
--     Both raw integers are stored as the authority; the derived fraction and its measurement
--     resolution are stored alongside so the estimator can be rebuilt, never the other way round.
--     Resolution follows from the actual limit (limit=1000 gives 0.1%), so it is recorded per
--     observation instead of being assumed globally.
--   * The window's native capacity needs no estimation at all: `limit` IS the window's total
--     native units. What must be estimated is how much official API replacement cost fits inside
--     it. There is therefore no per-turn native ledger, because the provider reports native
--     consumption only as a window aggregate and never per turn. Inventing one would mean
--     dividing API dollars by a token price, which is exactly what is forbidden.
--
-- Windows are identified by their exact native duration in seconds rather than by a closed
-- bucket enum, because the provider returns the window shape dynamically (the 5-hour limit
-- arrives as duration=300, TIME_UNIT_MINUTE) and may publish further windows. An unexpected
-- window must be recorded as raw evidence, not rejected and lost.

CREATE TABLE IF NOT EXISTS kimi_turn_calibration_events (
    request_id text NOT NULL CHECK (request_id <> ''),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),

    -- The model the customer asked for, and the model the provider says it served. They differ
    -- whenever thinking is disabled, and billing must follow the served one.
    requested_model text NOT NULL CHECK (requested_model <> ''),
    served_model text NOT NULL CHECK (served_model <> ''),
    context_mode text NOT NULL CHECK (context_mode IN ('256k', '1m')),
    reasoning_effort text NOT NULL CHECK (reasoning_effort IN ('low', 'high', 'max', 'off')),

    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    completed_at bigint NOT NULL CHECK (completed_at > 0),

    -- Disjoint usage legs. KIMI publishes no cache TTL split, so there is one cache-write leg.
    input_tokens bigint NOT NULL CHECK (input_tokens >= 0),
    cache_read_tokens bigint NOT NULL CHECK (cache_read_tokens >= 0),
    cache_write_tokens bigint NOT NULL CHECK (cache_write_tokens >= 0),
    output_tokens bigint NOT NULL CHECK (output_tokens >= 0),
    -- Subset of output_tokens, never billed as its own leg.
    reasoning_output_tokens bigint NOT NULL CHECK (reasoning_output_tokens >= 0),

    api_input_nanousd bigint NOT NULL CHECK (api_input_nanousd >= 0),
    api_cache_read_nanousd bigint NOT NULL CHECK (api_cache_read_nanousd >= 0),
    api_cache_write_nanousd bigint NOT NULL CHECK (api_cache_write_nanousd >= 0),
    api_output_nanousd bigint NOT NULL CHECK (api_output_nanousd >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd > 0),

    PRIMARY KEY (request_id),
    CHECK (reasoning_output_tokens <= output_tokens),
    CHECK (
        input_tokens > 0 OR cache_read_tokens > 0 OR cache_write_tokens > 0
        OR output_tokens > 0
    ),
    CHECK (
        api_total_nanousd = api_input_nanousd + api_cache_read_nanousd
            + api_cache_write_nanousd + api_output_nanousd
    )
);

CREATE INDEX IF NOT EXISTS kimi_turn_calibration_subject_time
    ON kimi_turn_calibration_events(subject_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS kimi_turn_calibration_served_model_time
    ON kimi_turn_calibration_events(served_model, completed_at DESC);
CREATE INDEX IF NOT EXISTS kimi_turn_calibration_cohort
    ON kimi_turn_calibration_events(plan, served_model, completed_at DESC);

-- Cumulative official API replacement cost per subject. Advanced in the same transaction that
-- wins the immutable event insert above, so a quota observation can never be paired with a stale
-- spend total.
CREATE TABLE IF NOT EXISTS kimi_calibration_subject_spend (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    spent_nano bigint NOT NULL DEFAULT 0 CHECK (spent_nano >= 0),
    tracking_started_ts bigint NOT NULL CHECK (tracking_started_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (subject_id)
);

-- Immutable quota observations. KIMI serves quota from a separate /usages endpoint and never
-- carries it on a generation response, so every observation is a poll and no request id is
-- invented for it. The single-value CHECK documents that absence and keeps a future
-- response-carried source an explicit, expand-only change.
CREATE TABLE IF NOT EXISTS kimi_window_observations (
    id bigserial PRIMARY KEY,
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),

    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    -- Provider-supplied label, audit metadata only; the duration is the identity.
    window_name text,
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- Raw provider integers: the authority. Their unit is not normatively documented, so they
    -- are preserved verbatim and never divided by a token price to fabricate capacity.
    native_used_units bigint NOT NULL CHECK (native_used_units >= 0),
    native_limit_units bigint NOT NULL CHECK (native_limit_units > 0),

    -- Derived from the two integers above, stored so the estimator is rebuildable and auditable.
    used_fraction_units bigint NOT NULL
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint NOT NULL
        CHECK (measurement_resolution_fraction_units BETWEEN 1 AND 100000000),

    cumulative_api_spend_nano bigint NOT NULL CHECK (cumulative_api_spend_nano >= 0),
    observation_source text NOT NULL CHECK (observation_source IN ('poll')),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),

    CHECK (native_used_units <= native_limit_units),
    -- A duplicate poll must be a no-op rather than a new sample.
    UNIQUE (
        subject_id,
        plan,
        window_duration_secs,
        resets_at,
        observed_at,
        native_used_units,
        native_limit_units,
        cumulative_api_spend_nano
    )
);

CREATE INDEX IF NOT EXISTS kimi_window_observations_window
    ON kimi_window_observations(subject_id, plan, window_duration_secs, resets_at, observed_at);

-- Estimator state per subject + paid plan + exact native window duration. Independent durations
-- never share a row: the 5-hour and 7-day windows are separate evidence.
CREATE TABLE IF NOT EXISTS kimi_window_calibrations (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    window_duration_secs bigint NOT NULL CHECK (window_duration_secs > 0),
    window_name text,
    resets_at bigint NOT NULL CHECK (resets_at > 0),

    -- The first snapshot of an interval is an anchor, not a sample.
    anchor_used_fraction_units bigint NOT NULL
        CHECK (anchor_used_fraction_units BETWEEN 0 AND 100000000),
    anchor_resolution_fraction_units bigint NOT NULL
        CHECK (anchor_resolution_fraction_units BETWEEN 1 AND 100000000),
    anchor_spend_nano bigint NOT NULL CHECK (anchor_spend_nano >= 0),

    used_fraction_units bigint NOT NULL
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint NOT NULL
        CHECK (measurement_resolution_fraction_units BETWEEN 1 AND 100000000),
    observed_at bigint NOT NULL CHECK (observed_at > 0),

    -- The window's native size, published directly by the provider. This needs no estimation;
    -- it is recorded so native remaining is exact and never inferred from API dollars.
    native_limit_units bigint NOT NULL CHECK (native_limit_units > 0),
    native_used_units bigint NOT NULL CHECK (native_used_units >= 0),

    observed_fraction_units bigint NOT NULL DEFAULT 0
        CHECK (observed_fraction_units >= 0),
    observed_spend_nano bigint NOT NULL DEFAULT 0 CHECK (observed_spend_nano >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),
    unattributed_fraction_units bigint NOT NULL DEFAULT 0
        CHECK (unattributed_fraction_units >= 0),

    -- Unknown stays NULL. An unbounded high stays NULL rather than becoming a guessed ceiling.
    current_capacity_nano bigint
        CHECK (current_capacity_nano IS NULL OR current_capacity_nano >= 0),
    current_low_nano bigint CHECK (current_low_nano IS NULL OR current_low_nano >= 0),
    current_high_nano bigint CHECK (current_high_nano IS NULL OR current_high_nano >= 0),
    current_confidence_bp bigint NOT NULL DEFAULT 0
        CHECK (current_confidence_bp BETWEEN 0 AND 10000),
    last_measured_at bigint CHECK (last_measured_at IS NULL OR last_measured_at > 0),

    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),

    PRIMARY KEY (subject_id, plan, window_duration_secs),
    CHECK (native_used_units <= native_limit_units),
    CHECK (current_low_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_high_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_low_nano IS NULL OR current_capacity_nano >= current_low_nano),
    CHECK (current_high_nano IS NULL OR current_capacity_nano <= current_high_nano),
    CHECK (current_low_nano IS NULL OR current_high_nano IS NULL
        OR current_low_nano <= current_high_nano),
    -- Cold state publishes nothing; a measured state publishes a capacity and a proven low.
    -- The high may still be NULL when movement did not exceed the quantisation envelope.
    CHECK (
        (samples = 0
            AND observed_fraction_units = 0
            AND observed_spend_nano = 0
            AND current_capacity_nano IS NULL
            AND current_low_nano IS NULL
            AND current_high_nano IS NULL
            AND current_confidence_bp = 0
            AND last_measured_at IS NULL)
        OR
        (samples > 0
            AND observed_fraction_units > 0
            AND observed_spend_nano > 0
            AND current_capacity_nano IS NOT NULL
            AND current_low_nano IS NOT NULL
            AND last_measured_at IS NOT NULL)
    )
);

CREATE INDEX IF NOT EXISTS kimi_window_calibrations_cohort
    ON kimi_window_calibrations(plan, window_duration_secs);

INSERT INTO engine_schema_migrations(version) VALUES (27)
ON CONFLICT (version) DO NOTHING;
