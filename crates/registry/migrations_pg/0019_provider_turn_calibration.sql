-- Immutable provider-specific turn evidence for Claude and Gemini calibration laboratories.
--
-- This is expand-only. The currently serving binary neither reads nor writes these tables, while
-- the dependent runtime can atomically advance an exact nanoUSD subject ledger only after winning
-- the immutable event insert. Provider-native quota snapshots remain in their existing authorities;
-- these rows preserve the exact workload that can later be paired with those snapshots.

CREATE TABLE IF NOT EXISTS provider_turn_calibration_events (
    provider text NOT NULL CHECK (provider IN ('anthropic', 'google')),
    request_id text NOT NULL CHECK (request_id <> ''),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    model_id text NOT NULL CHECK (model_id <> ''),
    service_tier text NOT NULL CHECK (service_tier IN ('standard', 'fast')),
    inference_geo text NOT NULL CHECK (inference_geo IN ('global', 'us')),
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    priced_ts bigint NOT NULL CHECK (priced_ts > 0),
    completed_at bigint NOT NULL CHECK (completed_at > 0),

    input_tokens bigint NOT NULL CHECK (input_tokens >= 0),
    audio_input_tokens bigint NOT NULL CHECK (audio_input_tokens >= 0),
    cache_read_tokens bigint NOT NULL CHECK (cache_read_tokens >= 0),
    cached_audio_input_tokens bigint NOT NULL CHECK (cached_audio_input_tokens >= 0),
    cache_write_5m_tokens bigint NOT NULL CHECK (cache_write_5m_tokens >= 0),
    cache_write_1h_tokens bigint NOT NULL CHECK (cache_write_1h_tokens >= 0),
    output_tokens bigint NOT NULL CHECK (output_tokens >= 0),
    thinking_output_tokens bigint NOT NULL CHECK (thinking_output_tokens >= 0),
    image_output_tokens bigint NOT NULL CHECK (image_output_tokens >= 0),
    tool_prompt_tokens bigint NOT NULL CHECK (tool_prompt_tokens >= 0),
    search_queries bigint NOT NULL CHECK (search_queries >= 0),
    grounded_search_prompts bigint NOT NULL CHECK (grounded_search_prompts >= 0),

    api_input_nanousd bigint NOT NULL CHECK (api_input_nanousd >= 0),
    api_audio_input_nanousd bigint NOT NULL CHECK (api_audio_input_nanousd >= 0),
    api_cache_read_nanousd bigint NOT NULL CHECK (api_cache_read_nanousd >= 0),
    api_cached_audio_input_nanousd bigint NOT NULL
        CHECK (api_cached_audio_input_nanousd >= 0),
    api_cache_write_5m_nanousd bigint NOT NULL CHECK (api_cache_write_5m_nanousd >= 0),
    api_cache_write_1h_nanousd bigint NOT NULL CHECK (api_cache_write_1h_nanousd >= 0),
    api_output_nanousd bigint NOT NULL CHECK (api_output_nanousd >= 0),
    api_image_output_nanousd bigint NOT NULL CHECK (api_image_output_nanousd >= 0),
    api_search_nanousd bigint NOT NULL CHECK (api_search_nanousd >= 0),
    api_total_nanousd bigint NOT NULL CHECK (api_total_nanousd > 0),

    PRIMARY KEY (provider, request_id),
    CHECK (cached_audio_input_tokens <= cache_read_tokens),
    CHECK (thinking_output_tokens <= output_tokens),
    CHECK (tool_prompt_tokens <= input_tokens),
    CHECK (
        input_tokens > 0 OR audio_input_tokens > 0 OR cache_read_tokens > 0
        OR cache_write_5m_tokens > 0 OR cache_write_1h_tokens > 0 OR output_tokens > 0
        OR image_output_tokens > 0 OR search_queries > 0 OR grounded_search_prompts > 0
    ),
    CHECK (
        api_total_nanousd = api_input_nanousd + api_audio_input_nanousd
            + api_cache_read_nanousd + api_cached_audio_input_nanousd
            + api_cache_write_5m_nanousd + api_cache_write_1h_nanousd
            + api_output_nanousd + api_image_output_nanousd + api_search_nanousd
    )
);

CREATE INDEX IF NOT EXISTS provider_turn_calibration_subject_time
    ON provider_turn_calibration_events(provider, subject_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS provider_turn_calibration_model_time
    ON provider_turn_calibration_events(provider, model_id, completed_at DESC);
CREATE INDEX IF NOT EXISTS provider_turn_calibration_time
    ON provider_turn_calibration_events(provider, completed_at DESC);

CREATE TABLE IF NOT EXISTS provider_calibration_subject_spend (
    provider text NOT NULL CHECK (provider IN ('anthropic', 'google')),
    subject_id text NOT NULL CHECK (subject_id <> ''),
    spent_nano bigint NOT NULL DEFAULT 0 CHECK (spent_nano >= 0),
    tracking_started_ts bigint NOT NULL CHECK (tracking_started_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    PRIMARY KEY (provider, subject_id)
);

INSERT INTO engine_schema_migrations(version) VALUES (19)
ON CONFLICT (version) DO NOTHING;
