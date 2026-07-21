-- Additive per-key policy fields. NULL limits and expirations preserve all existing keys.

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS spend_limit_nano bigint,
    ADD COLUMN IF NOT EXISTS expires_ts bigint,
    ADD COLUMN IF NOT EXISTS reserved_nano bigint NOT NULL DEFAULT 0;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'api_keys_spend_limit_positive' AND conrelid = 'api_keys'::regclass
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_spend_limit_positive
            CHECK (spend_limit_nano IS NULL OR spend_limit_nano > 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'api_keys_reserved_nonnegative' AND conrelid = 'api_keys'::regclass
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_reserved_nonnegative
            CHECK (reserved_nano >= 0);
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'api_keys_expires_positive' AND conrelid = 'api_keys'::regclass
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_expires_positive
            CHECK (expires_ts IS NULL OR expires_ts > 0);
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (2)
ON CONFLICT (version) DO NOTHING;
