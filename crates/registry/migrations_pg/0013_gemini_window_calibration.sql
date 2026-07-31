-- Durable evidence for Gemini subscription-capacity calibration.
--
-- Antigravity's retrieveUserQuotaSummary exposes two independent Gemini quota buckets:
-- `gemini-5h` and `gemini-weekly`. The gateway pairs their provider-reported remaining fraction
-- with cumulative exact official-API-price spend for the same opaque profile. There is
-- deliberately no configured capacity prior in this schema.
--
-- Fractions use fixed 10^-8 units rather than floating point. WLS sufficient statistics can
-- exceed bigint even though every input and final nanoUSD estimate fits bigint, so PostgreSQL
-- stores the two non-negative accumulators as bounded NUMERIC integers. Registry code transfers
-- them as canonical decimal strings; estimator arithmetic remains checked i128.

CREATE TABLE IF NOT EXISTS gemini_profile_spend (
    profile_id text PRIMARY KEY,
    spent_nano bigint NOT NULL DEFAULT 0 CHECK (spent_nano >= 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0)
);

CREATE TABLE IF NOT EXISTS gemini_window_calibrations (
    profile_id text NOT NULL,
    bucket_id text NOT NULL,
    window_kind text NOT NULL CHECK (window_kind IN ('5h', 'weekly')),
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    anchor_used_fraction_units bigint NOT NULL
        CHECK (anchor_used_fraction_units BETWEEN 0 AND 100000000),
    anchor_spend_nano bigint NOT NULL CHECK (anchor_spend_nano >= 0),
    anchor_ready boolean NOT NULL DEFAULT false,
    used_fraction_units bigint NOT NULL
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    observed_at bigint NOT NULL CHECK (observed_at > 0),
    sum_used_sq numeric(39, 0) NOT NULL DEFAULT 0
        CHECK (sum_used_sq >= 0 AND
               sum_used_sq <= 170141183460469231731687303715884105727),
    sum_used_spend_nano numeric(39, 0) NOT NULL DEFAULT 0
        CHECK (sum_used_spend_nano >= 0 AND
               sum_used_spend_nano <= 170141183460469231731687303715884105727),
    observed_fraction_units bigint NOT NULL DEFAULT 0
        CHECK (observed_fraction_units >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),
    current_capacity_nano bigint
        CHECK (current_capacity_nano IS NULL OR current_capacity_nano >= 0),
    current_low_nano bigint
        CHECK (current_low_nano IS NULL OR current_low_nano >= 0),
    current_high_nano bigint
        CHECK (current_high_nano IS NULL OR current_high_nano >= 0),
    current_confidence_bp bigint NOT NULL DEFAULT 0
        CHECK (current_confidence_bp BETWEEN 0 AND 10000),
    last_measured_at bigint CHECK (last_measured_at IS NULL OR last_measured_at > 0),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (profile_id, bucket_id),
    CHECK (
        (bucket_id = 'gemini-5h' AND window_kind = '5h' AND window_duration_mins = 300)
        OR
        (bucket_id = 'gemini-weekly' AND window_kind = 'weekly'
            AND window_duration_mins = 10080)
    ),
    CHECK (current_low_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_high_nano IS NULL OR current_capacity_nano IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS gemini_window_observations (
    id bigserial PRIMARY KEY,
    profile_id text NOT NULL,
    bucket_id text NOT NULL,
    window_kind text NOT NULL CHECK (window_kind IN ('5h', 'weekly')),
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),
    used_fraction_units bigint NOT NULL
        CHECK (used_fraction_units BETWEEN 0 AND 100000000),
    gateway_spend_nano bigint NOT NULL CHECK (gateway_spend_nano >= 0),
    CHECK (
        (bucket_id = 'gemini-5h' AND window_kind = '5h' AND window_duration_mins = 300)
        OR
        (bucket_id = 'gemini-weekly' AND window_kind = 'weekly'
            AND window_duration_mins = 10080)
    ),
    UNIQUE (
        profile_id,
        bucket_id,
        resets_at,
        observed_at,
        used_fraction_units,
        gateway_spend_nano
    )
);
CREATE INDEX IF NOT EXISTS gemini_window_observations_window
    ON gemini_window_observations(profile_id, bucket_id, resets_at, observed_at);

INSERT INTO engine_schema_migrations(version) VALUES (13)
ON CONFLICT (version) DO NOTHING;
