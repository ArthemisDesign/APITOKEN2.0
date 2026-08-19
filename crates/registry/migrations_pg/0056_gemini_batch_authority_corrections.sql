-- Expand-only corrections required before the Gemini Batch registry authority starts writing.
--
-- Migration 0055 is already production history and remains immutable. Its review found four facts
-- that cannot be repaired in runtime code: non-secret key attribution must survive key deletion,
-- result retention starts at completion rather than admission, a single PostgreSQL bytea cannot
-- implement a streamable 2 GiB Files API object, and settlement must carry a complete immutable
-- Gemini calibration event into the same transaction as money/result terminalization.
--
-- The serving runtime still has no batch reader or writer. Every added column is nullable for
-- mixed-version safety; Stage 2 validates complete shapes before it writes them.

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS key_id text;
ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS key_id text;
ALTER TABLE gemini_batch_items
    ADD COLUMN IF NOT EXISTS creator_key_id text;

-- A job may spend up to 48 hours queued. The 42-day result lifetime begins only when the job becomes
-- terminal, so creation cannot truthfully populate this column.
ALTER TABLE gemini_batch_jobs
    ALTER COLUMN result_expiration_ts DROP NOT NULL;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'gemini_batch_jobs_result_expiry_from_completion'
          AND conrelid = 'gemini_batch_jobs'::regclass
    ) THEN
        ALTER TABLE gemini_batch_jobs
            ADD CONSTRAINT gemini_batch_jobs_result_expiry_from_completion
            CHECK (
                (completed_ts IS NULL AND result_expiration_ts IS NULL)
                OR (
                    completed_ts IS NOT NULL
                    AND result_expiration_ts IS NOT NULL
                    AND result_expiration_ts >= completed_ts + 3628800
                )
            ) NOT VALID;
    END IF;
END $$;
ALTER TABLE gemini_batch_jobs
    VALIDATE CONSTRAINT gemini_batch_jobs_result_expiry_from_completion;

-- Files are encrypted independently in bounded chunks. The 8 MiB plaintext bound keeps each AEAD
-- operation and PostgreSQL value bounded while still allowing a 2 GiB logical file. The 16-byte
-- XChaCha20-Poly1305 tag is part of ciphertext; nonce and digest lengths are structural facts.
CREATE TABLE IF NOT EXISTS gemini_batch_file_chunks (
    file_id text NOT NULL REFERENCES gemini_batch_files(file_id) ON DELETE RESTRICT,
    chunk_index bigint NOT NULL CHECK (chunk_index >= 0),
    key_id text NOT NULL CHECK (key_id <> '' AND octet_length(key_id) <= 128),
    nonce bytea NOT NULL CHECK (octet_length(nonce) = 24),
    ciphertext bytea NOT NULL,
    plaintext_len bigint NOT NULL CHECK (plaintext_len BETWEEN 0 AND 8388608),
    plaintext_digest bytea NOT NULL CHECK (octet_length(plaintext_digest) = 32),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (file_id, chunk_index),
    CHECK (octet_length(ciphertext)::bigint = plaintext_len + 16)
);
CREATE INDEX IF NOT EXISTS gemini_batch_file_chunks_created
    ON gemini_batch_file_chunks(created_ts, file_id, chunk_index);

-- Complete immutable ProviderTurnCalibrationEvent payload for a successful measured batch item.
-- Cancellation/error intents keep the entire group NULL. A measured success supplies every field;
-- exact replay then compares this durable payload instead of reconstructing tariff/profile facts.
ALTER TABLE gemini_batch_settlement_outbox
    ADD COLUMN IF NOT EXISTS calibration_profile_id text,
    ADD COLUMN IF NOT EXISTS calibration_model_id text,
    ADD COLUMN IF NOT EXISTS calibration_service_tier text,
    ADD COLUMN IF NOT EXISTS calibration_inference_geo text,
    ADD COLUMN IF NOT EXISTS calibration_tariff_schedule_id text,
    ADD COLUMN IF NOT EXISTS calibration_priced_ts bigint,
    ADD COLUMN IF NOT EXISTS calibration_completed_at bigint,
    ADD COLUMN IF NOT EXISTS calibration_input_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_audio_input_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_cache_read_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_cached_audio_input_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_cache_write_5m_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_cache_write_1h_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_output_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_thinking_output_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_image_output_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_tool_prompt_tokens bigint,
    ADD COLUMN IF NOT EXISTS calibration_search_queries bigint,
    ADD COLUMN IF NOT EXISTS calibration_grounded_search_prompts bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_input_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_audio_input_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_cache_read_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_cached_audio_input_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_cache_write_5m_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_cache_write_1h_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_output_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_image_output_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_search_nanousd bigint,
    ADD COLUMN IF NOT EXISTS calibration_api_total_nanousd bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'gemini_batch_outbox_calibration_shape'
          AND conrelid = 'gemini_batch_settlement_outbox'::regclass
    ) THEN
        ALTER TABLE gemini_batch_settlement_outbox
            ADD CONSTRAINT gemini_batch_outbox_calibration_shape
            CHECK (
                num_nonnulls(
                    calibration_profile_id, calibration_model_id, calibration_service_tier,
                    calibration_inference_geo, calibration_tariff_schedule_id,
                    calibration_priced_ts, calibration_completed_at, calibration_input_tokens,
                    calibration_audio_input_tokens, calibration_cache_read_tokens,
                    calibration_cached_audio_input_tokens, calibration_cache_write_5m_tokens,
                    calibration_cache_write_1h_tokens, calibration_output_tokens,
                    calibration_thinking_output_tokens, calibration_image_output_tokens,
                    calibration_tool_prompt_tokens, calibration_search_queries,
                    calibration_grounded_search_prompts, calibration_api_input_nanousd,
                    calibration_api_audio_input_nanousd, calibration_api_cache_read_nanousd,
                    calibration_api_cached_audio_input_nanousd,
                    calibration_api_cache_write_5m_nanousd,
                    calibration_api_cache_write_1h_nanousd, calibration_api_output_nanousd,
                    calibration_api_image_output_nanousd, calibration_api_search_nanousd,
                    calibration_api_total_nanousd
                ) IN (0, 29)
            ) NOT VALID;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'gemini_batch_outbox_calibration_values'
          AND conrelid = 'gemini_batch_settlement_outbox'::regclass
    ) THEN
        ALTER TABLE gemini_batch_settlement_outbox
            ADD CONSTRAINT gemini_batch_outbox_calibration_values
            CHECK (
                calibration_profile_id IS NULL
                OR (
                    calibration_profile_id <> ''
                    AND calibration_model_id <> ''
                    AND calibration_service_tier IN ('standard', 'fast')
                    AND calibration_inference_geo IN ('global', 'us')
                    AND calibration_tariff_schedule_id <> ''
                    AND calibration_priced_ts > 0
                    AND calibration_completed_at > 0
                    AND calibration_input_tokens >= 0
                    AND calibration_audio_input_tokens >= 0
                    AND calibration_cache_read_tokens >= 0
                    AND calibration_cached_audio_input_tokens >= 0
                    AND calibration_cache_write_5m_tokens >= 0
                    AND calibration_cache_write_1h_tokens >= 0
                    AND calibration_output_tokens >= 0
                    AND calibration_thinking_output_tokens >= 0
                    AND calibration_image_output_tokens >= 0
                    AND calibration_tool_prompt_tokens >= 0
                    AND calibration_search_queries >= 0
                    AND calibration_grounded_search_prompts >= 0
                    AND calibration_api_input_nanousd >= 0
                    AND calibration_api_audio_input_nanousd >= 0
                    AND calibration_api_cache_read_nanousd >= 0
                    AND calibration_api_cached_audio_input_nanousd >= 0
                    AND calibration_api_cache_write_5m_nanousd >= 0
                    AND calibration_api_cache_write_1h_nanousd >= 0
                    AND calibration_api_output_nanousd >= 0
                    AND calibration_api_image_output_nanousd >= 0
                    AND calibration_api_search_nanousd >= 0
                    AND calibration_api_total_nanousd > 0
                    AND calibration_cached_audio_input_tokens <= calibration_cache_read_tokens
                    AND calibration_thinking_output_tokens <= calibration_output_tokens
                    AND calibration_tool_prompt_tokens <= calibration_input_tokens
                    AND calibration_api_total_nanousd =
                        calibration_api_input_nanousd
                        + calibration_api_audio_input_nanousd
                        + calibration_api_cache_read_nanousd
                        + calibration_api_cached_audio_input_nanousd
                        + calibration_api_cache_write_5m_nanousd
                        + calibration_api_cache_write_1h_nanousd
                        + calibration_api_output_nanousd
                        + calibration_api_image_output_nanousd
                        + calibration_api_search_nanousd
                )
            ) NOT VALID;
    END IF;
END $$;
ALTER TABLE gemini_batch_settlement_outbox
    VALIDATE CONSTRAINT gemini_batch_outbox_calibration_shape;
ALTER TABLE gemini_batch_settlement_outbox
    VALIDATE CONSTRAINT gemini_batch_outbox_calibration_values;

CREATE INDEX IF NOT EXISTS ledger_key_id_id
    ON ledger(key_id, id) WHERE key_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS usage_events_key_id_ts
    ON usage_events(key_id, ts) WHERE key_id IS NOT NULL;

INSERT INTO engine_schema_migrations(version) VALUES (56)
ON CONFLICT (version) DO NOTHING;
