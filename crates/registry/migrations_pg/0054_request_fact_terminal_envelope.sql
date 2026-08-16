-- Crash-safe terminal envelope for dormant request observability.
--
-- Settlement enqueue and authoritative outbox apply are separate transactions. Terminal request-
-- fact evidence must therefore survive in the outbox: an in-memory payload would be lost if the
-- process crashed after enqueue and before apply. The dependent producer will copy this envelope
-- into request_facts only inside the transaction that actually applies settlement. billing_outcome
-- is deliberately absent here because apply derives it from authoritative settlement state, with
-- loser -> reconciled -> canceled -> zero_metered -> winner precedence.
--
-- Every new outbox column is nullable so the currently serving writer can keep enqueueing rows and
-- so a completely NULL envelope unambiguously represents legacy/no evidence. No index or foreign
-- key is added; this migration remains dormant and creates no runtime dependency.

-- Delivery is independent from provider termination and financial settlement. Existing dormant
-- facts have no such observation, so NULL remains honest unknown evidence.
ALTER TABLE request_facts
    ADD COLUMN IF NOT EXISTS delivery_state text;

-- Migration 0053 used false/zero defaults for evidence that can instead be absent, unparsed, or
-- unsupported. Remove both the defaults and NOT NULL requirements before any producer exists.
-- stream_flag stays unchanged because the owning request parser always knows whether streaming was
-- requested.
ALTER TABLE request_facts
    ALTER COLUMN tool_classes DROP DEFAULT,
    ALTER COLUMN tool_classes DROP NOT NULL,
    ALTER COLUMN tool_results_in_input DROP DEFAULT,
    ALTER COLUMN tool_results_in_input DROP NOT NULL,
    ALTER COLUMN tool_calls_in_output DROP DEFAULT,
    ALTER COLUMN tool_calls_in_output DROP NOT NULL,
    ALTER COLUMN structured_output_flag DROP DEFAULT,
    ALTER COLUMN structured_output_flag DROP NOT NULL,
    ALTER COLUMN reasoning_flag DROP DEFAULT,
    ALTER COLUMN reasoning_flag DROP NOT NULL,
    ALTER COLUMN input_modalities DROP DEFAULT,
    ALTER COLUMN input_modalities DROP NOT NULL,
    ALTER COLUMN output_modalities DROP DEFAULT,
    ALTER COLUMN output_modalities DROP NOT NULL;

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS request_fact_terminal_schema_version integer,
    ADD COLUMN IF NOT EXISTS request_fact_terminal_at bigint,
    ADD COLUMN IF NOT EXISTS request_fact_http_status_code integer,
    ADD COLUMN IF NOT EXISTS request_fact_provider_terminal_class text,
    ADD COLUMN IF NOT EXISTS request_fact_delivery_state text,
    ADD COLUMN IF NOT EXISTS request_fact_downstream_disconnect boolean,
    ADD COLUMN IF NOT EXISTS request_fact_upstream_request_id text,
    ADD COLUMN IF NOT EXISTS request_fact_first_public_byte_at bigint,
    ADD COLUMN IF NOT EXISTS request_fact_internal_attempt_count integer,
    ADD COLUMN IF NOT EXISTS request_fact_failure_class text,
    ADD COLUMN IF NOT EXISTS request_fact_tool_calls_in_output boolean;

-- The constraint-missing guards keep replay safe even if an operator already applied part of the
-- DDL. NOT VALID permits old-writer-compatible installation before validation of existing rows.
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'request_facts_delivery_state_valid'
          AND conrelid = 'request_facts'::regclass
    ) THEN
        ALTER TABLE request_facts
            ADD CONSTRAINT request_facts_delivery_state_valid
            CHECK (delivery_state IS NULL OR delivery_state IN
                ('not_started', 'started', 'completed', 'interrupted', 'unknown')) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_fact_schema_version_positive'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_fact_schema_version_positive
            CHECK (
                request_fact_terminal_schema_version IS NULL
                OR request_fact_terminal_schema_version > 0
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_request_fact_http_status_code_range'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_request_fact_http_status_code_range
            CHECK (
                request_fact_http_status_code IS NULL
                OR request_fact_http_status_code BETWEEN 100 AND 599
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_fact_provider_terminal_class_valid'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_fact_provider_terminal_class_valid
            CHECK (
                request_fact_provider_terminal_class IS NULL
                OR request_fact_provider_terminal_class IN (
                    'success', 'client_error', 'quota', 'auth', 'timeout', 'transport',
                    'upstream_error', 'protocol_error', 'unknown'
                )
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_request_fact_delivery_state_valid'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_request_fact_delivery_state_valid
            CHECK (
                request_fact_delivery_state IS NULL
                OR request_fact_delivery_state IN
                    ('not_started', 'started', 'completed', 'interrupted', 'unknown')
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_fact_attempt_count_nonnegative'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_fact_attempt_count_nonnegative
            CHECK (
                request_fact_internal_attempt_count IS NULL
                OR request_fact_internal_attempt_count >= 0
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_request_fact_terminal_envelope_shape'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_request_fact_terminal_envelope_shape
            CHECK (
                (
                    request_fact_terminal_schema_version IS NULL
                    AND request_fact_terminal_at IS NULL
                    AND request_fact_http_status_code IS NULL
                    AND request_fact_provider_terminal_class IS NULL
                    AND request_fact_delivery_state IS NULL
                    AND request_fact_downstream_disconnect IS NULL
                    AND request_fact_upstream_request_id IS NULL
                    AND request_fact_first_public_byte_at IS NULL
                    AND request_fact_internal_attempt_count IS NULL
                    AND request_fact_failure_class IS NULL
                    AND request_fact_tool_calls_in_output IS NULL
                )
                OR (
                    request_fact_terminal_schema_version IS NOT NULL
                    AND request_fact_terminal_at IS NOT NULL
                    AND request_fact_provider_terminal_class IS NOT NULL
                    AND request_fact_delivery_state IS NOT NULL
                )
            ) NOT VALID;
    END IF;
END $$;

ALTER TABLE request_facts
    VALIDATE CONSTRAINT request_facts_delivery_state_valid;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_fact_schema_version_positive;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_request_fact_http_status_code_range;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_fact_provider_terminal_class_valid;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_request_fact_delivery_state_valid;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_fact_attempt_count_nonnegative;
ALTER TABLE settlement_outbox
    VALIDATE CONSTRAINT settlement_outbox_request_fact_terminal_envelope_shape;

INSERT INTO engine_schema_migrations(version) VALUES (54)
ON CONFLICT (version) DO NOTHING;
