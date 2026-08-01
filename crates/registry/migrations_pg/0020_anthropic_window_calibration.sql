-- Exact fixed-point authority for Claude subscription-window calibration.
--
-- Anthropic exposes two independent unified-quota fractions but no native credit counter. The
-- only honest dollar capacity is therefore the realized official-API-price workload blend paired
-- with those fractions. Every accepted interval is recoverable from immutable observations and
-- exact cumulative provider subject spend introduced by migration 0019. There is deliberately no
-- subscription-price nominal, configured prior, EMA or floating-point money in this schema.
--
-- A plan is part of the durable identity. Changing a subscription from Pro to Max (or correcting
-- a previously wrong plan) starts a fresh anchor instead of contaminating a same-plan cohort.

CREATE TABLE IF NOT EXISTS anthropic_window_calibrations (
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    window_kind text NOT NULL CHECK (window_kind IN ('5h', '7d')),
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

    PRIMARY KEY (subject_id, plan, window_kind),
    CHECK (
        (window_kind = '5h' AND window_duration_mins = 300)
        OR (window_kind = '7d' AND window_duration_mins = 10080)
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

CREATE INDEX IF NOT EXISTS anthropic_window_calibrations_cohort
    ON anthropic_window_calibrations(plan, window_kind, window_duration_mins);

CREATE TABLE IF NOT EXISTS anthropic_window_observations (
    id bigserial PRIMARY KEY,
    subject_id text NOT NULL CHECK (subject_id <> ''),
    plan text NOT NULL CHECK (plan <> ''),
    window_kind text NOT NULL CHECK (window_kind IN ('5h', '7d')),
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
        (window_kind = '5h' AND window_duration_mins = 300)
        OR (window_kind = '7d' AND window_duration_mins = 10080)
    ),
    CHECK (
        (observation_source = 'response' AND source_request_id IS NOT NULL
            AND source_request_id <> '')
        OR (observation_source = 'poll' AND source_request_id IS NULL)
    ),
    UNIQUE (subject_id, plan, window_kind, source_request_id),
    UNIQUE (
        subject_id,
        plan,
        window_kind,
        resets_at,
        observed_at,
        used_fraction_units,
        measurement_resolution_fraction_units,
        gateway_spend_nano,
        observation_source
    )
);

CREATE INDEX IF NOT EXISTS anthropic_window_observations_window
    ON anthropic_window_observations(
        subject_id,
        plan,
        window_kind,
        resets_at,
        observed_at
    );

INSERT INTO engine_schema_migrations(version) VALUES (20)
ON CONFLICT (version) DO NOTHING;
