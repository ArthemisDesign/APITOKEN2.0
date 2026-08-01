-- Exact dual-ledger evidence for Codex subscription calibration.
--
-- API replacement cost (nanoUSD) and ChatGPT subscription consumption (nanocredits) are separate
-- dimensions. Existing API-dollar calibration rows remain valid; all new credit columns are
-- nullable so the currently serving binary can continue writing the old shape and the dependent
-- release can distinguish pre-observation history from a real zero.

ALTER TABLE codex_home_spend
    ADD COLUMN IF NOT EXISTS spent_nanocredits bigint
        CHECK (spent_nanocredits IS NULL OR spent_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS credit_tracking_started_ts bigint
        CHECK (credit_tracking_started_ts IS NULL OR credit_tracking_started_ts > 0);

ALTER TABLE codex_window_calibrations
    ADD COLUMN IF NOT EXISTS anchor_spend_nanocredits bigint
        CHECK (anchor_spend_nanocredits IS NULL OR anchor_spend_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS observed_spend_nanocredits bigint
        CHECK (observed_spend_nanocredits IS NULL OR observed_spend_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS current_capacity_nanocredits bigint
        CHECK (current_capacity_nanocredits IS NULL OR current_capacity_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS current_low_nanocredits bigint
        CHECK (current_low_nanocredits IS NULL OR current_low_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS current_high_nanocredits bigint
        CHECK (current_high_nanocredits IS NULL OR current_high_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS last_capacity_nanocredits bigint
        CHECK (last_capacity_nanocredits IS NULL OR last_capacity_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS last_low_nanocredits bigint
        CHECK (last_low_nanocredits IS NULL OR last_low_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS last_high_nanocredits bigint
        CHECK (last_high_nanocredits IS NULL OR last_high_nanocredits >= 0),
    ADD COLUMN IF NOT EXISTS credit_samples bigint
        CHECK (credit_samples IS NULL OR credit_samples >= 0),
    ADD COLUMN IF NOT EXISTS credit_estimator_version bigint
        CHECK (credit_estimator_version IS NULL OR credit_estimator_version > 0),
    ADD COLUMN IF NOT EXISTS unattributed_fraction_units bigint
        CHECK (unattributed_fraction_units IS NULL OR unattributed_fraction_units >= 0),
    ADD CONSTRAINT codex_current_credit_bounds_need_capacity
        CHECK (
            (current_low_nanocredits IS NULL AND current_high_nanocredits IS NULL)
            OR current_capacity_nanocredits IS NOT NULL
        ) NOT VALID,
    ADD CONSTRAINT codex_last_credit_bounds_need_capacity
        CHECK (
            (last_low_nanocredits IS NULL AND last_high_nanocredits IS NULL)
            OR last_capacity_nanocredits IS NOT NULL
        ) NOT VALID;

ALTER TABLE codex_window_observations
    ADD COLUMN IF NOT EXISTS gateway_spend_nanocredits bigint
        CHECK (gateway_spend_nanocredits IS NULL OR gateway_spend_nanocredits >= 0);

CREATE TABLE IF NOT EXISTS codex_turn_calibration_events (
    request_id text PRIMARY KEY,
    home_id text NOT NULL CHECK (home_id <> ''),
    model_id text NOT NULL CHECK (model_id <> ''),
    service_tier text NOT NULL CHECK (service_tier IN ('standard', 'fast')),
    provider_reported_tier text,
    api_tariff_schedule_id text NOT NULL CHECK (api_tariff_schedule_id <> ''),
    credit_schedule_id text NOT NULL CHECK (credit_schedule_id <> ''),
    completed_at bigint NOT NULL CHECK (completed_at > 0),

    input_tokens bigint NOT NULL CHECK (input_tokens >= 0),
    cached_input_tokens bigint NOT NULL CHECK (cached_input_tokens >= 0),
    cache_write_input_tokens bigint NOT NULL CHECK (cache_write_input_tokens >= 0),
    output_tokens bigint NOT NULL CHECK (output_tokens >= 0),
    reasoning_output_tokens bigint NOT NULL CHECK (reasoning_output_tokens >= 0),

    api_input_nanousd bigint NOT NULL CHECK (api_input_nanousd >= 0),
    api_cached_input_nanousd bigint NOT NULL CHECK (api_cached_input_nanousd >= 0),
    api_cache_write_nanousd bigint NOT NULL CHECK (api_cache_write_nanousd >= 0),
    api_output_nanousd bigint NOT NULL CHECK (api_output_nanousd >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd >= 0),

    chatgpt_input_nanocredits bigint NOT NULL CHECK (chatgpt_input_nanocredits >= 0),
    chatgpt_cached_input_nanocredits bigint NOT NULL
        CHECK (chatgpt_cached_input_nanocredits >= 0),
    chatgpt_output_nanocredits bigint NOT NULL CHECK (chatgpt_output_nanocredits >= 0),
    chatgpt_total_nanocredits bigint NOT NULL CHECK (chatgpt_total_nanocredits >= 0),

    CHECK (cached_input_tokens + cache_write_input_tokens <= input_tokens),
    CHECK (reasoning_output_tokens <= output_tokens),
    CHECK (input_tokens > 0 OR output_tokens > 0),
    CHECK (
        api_total_nanousd = api_input_nanousd + api_cached_input_nanousd
            + api_cache_write_nanousd + api_output_nanousd
    ),
    CHECK (
        chatgpt_total_nanocredits = chatgpt_input_nanocredits
            + chatgpt_cached_input_nanocredits + chatgpt_output_nanocredits
    )
);

CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_home_time
    ON codex_turn_calibration_events(home_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_model_time
    ON codex_turn_calibration_events(model_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS codex_turn_calibration_events_time
    ON codex_turn_calibration_events(completed_at DESC);

INSERT INTO engine_schema_migrations(version) VALUES (18)
ON CONFLICT (version) DO NOTHING;
