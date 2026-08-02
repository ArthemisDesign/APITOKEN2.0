-- Exact plan-scoped authority for Gemini subscription-window calibration.
--
-- The legacy Gemini estimator tables remain untouched so the currently serving runtime can keep
-- writing them during migration-first rollout. The dependent runtime will switch to these tables
-- only after this schema is present. A paid plan is part of the durable identity, provider decimal
-- resolution is preserved explicitly, and every derived value can be rebuilt from immutable raw
-- observations paired with the common provider turn ledger from migration 0019.

CREATE TABLE IF NOT EXISTS gemini_exact_window_calibrations (
    profile_id text NOT NULL CHECK (profile_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    bucket_id text NOT NULL CHECK (bucket_id IN ('gemini-5h', 'gemini-weekly')),
    window_kind text NOT NULL CHECK (window_kind IN ('5h', 'weekly')),
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),

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

    observed_fraction_units bigint NOT NULL DEFAULT 0
        CHECK (observed_fraction_units >= 0),
    observed_spend_nano bigint NOT NULL DEFAULT 0 CHECK (observed_spend_nano >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),
    unattributed_fraction_units bigint NOT NULL DEFAULT 0
        CHECK (unattributed_fraction_units >= 0),

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

    PRIMARY KEY (profile_id, plan, bucket_id),
    CHECK (
        (bucket_id = 'gemini-5h' AND window_kind = '5h' AND window_duration_mins = 300)
        OR
        (bucket_id = 'gemini-weekly' AND window_kind = 'weekly'
            AND window_duration_mins = 10080)
    ),
    CHECK (current_low_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_high_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_low_nano IS NULL OR current_capacity_nano >= current_low_nano),
    CHECK (current_high_nano IS NULL OR current_capacity_nano <= current_high_nano),
    CHECK (current_low_nano IS NULL OR current_high_nano IS NULL
        OR current_low_nano <= current_high_nano),
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

CREATE INDEX IF NOT EXISTS gemini_exact_window_calibrations_cohort
    ON gemini_exact_window_calibrations(plan, bucket_id, window_duration_mins);

CREATE TABLE IF NOT EXISTS gemini_exact_window_observations (
    id bigserial PRIMARY KEY,
    profile_id text NOT NULL CHECK (profile_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    bucket_id text NOT NULL CHECK (bucket_id IN ('gemini-5h', 'gemini-weekly')),
    window_kind text NOT NULL CHECK (window_kind IN ('5h', 'weekly')),
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),
    used_fraction_units bigint NOT NULL
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    measurement_resolution_fraction_units bigint NOT NULL
        CHECK (measurement_resolution_fraction_units BETWEEN 1 AND 100000000),
    gateway_spend_nano bigint NOT NULL CHECK (gateway_spend_nano >= 0),
    observation_source text NOT NULL CHECK (observation_source IN ('response', 'poll')),
    source_request_id text,

    CHECK (
        (bucket_id = 'gemini-5h' AND window_kind = '5h' AND window_duration_mins = 300)
        OR
        (bucket_id = 'gemini-weekly' AND window_kind = 'weekly'
            AND window_duration_mins = 10080)
    ),
    CHECK (
        (observation_source = 'response' AND source_request_id IS NOT NULL
            AND source_request_id <> '')
        OR (observation_source = 'poll' AND source_request_id IS NULL)
    ),
    UNIQUE (profile_id, plan, bucket_id, source_request_id),
    UNIQUE (
        profile_id,
        plan,
        bucket_id,
        resets_at,
        observed_at,
        used_fraction_units,
        measurement_resolution_fraction_units,
        gateway_spend_nano,
        observation_source
    )
);

CREATE INDEX IF NOT EXISTS gemini_exact_window_observations_window
    ON gemini_exact_window_observations(
        profile_id,
        plan,
        bucket_id,
        resets_at,
        observed_at
    );

INSERT INTO engine_schema_migrations(version) VALUES (22)
ON CONFLICT (version) DO NOTHING;
