-- Durable evidence for OpenAI/Codex subscription-capacity calibration.
--
-- The gateway records only real metered spend and provider-reported window snapshots. There is no
-- configured capacity prior in this schema. Runtime code can therefore recover calibration across
-- restart and blue/green overlap without inventing a dollar limit or conflating windows that have
-- different provider-reported durations/resets.

CREATE TABLE IF NOT EXISTS codex_home_spend (
    home_id text PRIMARY KEY,
    spent_nano bigint NOT NULL DEFAULT 0 CHECK (spent_nano >= 0),
    updated_ts bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS codex_window_calibrations (
    home_id text NOT NULL,
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    anchor_used_percent bigint NOT NULL CHECK (anchor_used_percent BETWEEN 0 AND 100),
    anchor_spend_nano bigint NOT NULL CHECK (anchor_spend_nano >= 0),
    used_percent bigint NOT NULL CHECK (used_percent BETWEEN 0 AND 100),
    observed_at bigint NOT NULL CHECK (observed_at > 0),
    sum_used_sq bigint NOT NULL DEFAULT 0 CHECK (sum_used_sq >= 0),
    sum_used_spend_nano bigint NOT NULL DEFAULT 0 CHECK (sum_used_spend_nano >= 0),
    observed_points bigint NOT NULL DEFAULT 0 CHECK (observed_points >= 0),
    samples bigint NOT NULL DEFAULT 0 CHECK (samples >= 0),
    current_capacity_nano bigint CHECK (current_capacity_nano IS NULL OR current_capacity_nano >= 0),
    current_low_nano bigint CHECK (current_low_nano IS NULL OR current_low_nano >= 0),
    current_high_nano bigint CHECK (current_high_nano IS NULL OR current_high_nano >= 0),
    current_confidence_bp bigint NOT NULL DEFAULT 0
        CHECK (current_confidence_bp BETWEEN 0 AND 10000),
    last_capacity_nano bigint CHECK (last_capacity_nano IS NULL OR last_capacity_nano >= 0),
    last_low_nano bigint CHECK (last_low_nano IS NULL OR last_low_nano >= 0),
    last_high_nano bigint CHECK (last_high_nano IS NULL OR last_high_nano >= 0),
    last_confidence_bp bigint NOT NULL DEFAULT 0
        CHECK (last_confidence_bp BETWEEN 0 AND 10000),
    last_measured_at bigint CHECK (last_measured_at IS NULL OR last_measured_at > 0),
    estimator_version bigint NOT NULL DEFAULT 1 CHECK (estimator_version > 0),
    version bigint NOT NULL DEFAULT 0 CHECK (version >= 0),
    updated_ts bigint NOT NULL,
    PRIMARY KEY (home_id, window_duration_mins),
    CHECK (current_low_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (current_high_nano IS NULL OR current_capacity_nano IS NOT NULL),
    CHECK (last_low_nano IS NULL OR last_capacity_nano IS NOT NULL),
    CHECK (last_high_nano IS NULL OR last_capacity_nano IS NOT NULL)
);

CREATE TABLE IF NOT EXISTS codex_window_observations (
    id bigserial PRIMARY KEY,
    home_id text NOT NULL,
    window_duration_mins bigint NOT NULL CHECK (window_duration_mins > 0),
    resets_at bigint NOT NULL CHECK (resets_at > 0),
    observed_at bigint NOT NULL CHECK (observed_at > 0),
    used_percent bigint NOT NULL CHECK (used_percent BETWEEN 0 AND 100),
    gateway_spend_nano bigint NOT NULL CHECK (gateway_spend_nano >= 0),
    UNIQUE (
        home_id,
        window_duration_mins,
        resets_at,
        observed_at,
        used_percent,
        gateway_spend_nano
    )
);
CREATE INDEX IF NOT EXISTS codex_window_observations_window
    ON codex_window_observations(home_id, window_duration_mins, resets_at, observed_at);

INSERT INTO engine_schema_migrations(version) VALUES (10)
ON CONFLICT (version) DO NOTHING;
