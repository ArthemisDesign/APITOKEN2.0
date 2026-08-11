-- Expand-only foundation for account-wide settlement-floor enforcement.
--
-- Admission already serializes reservations on the account row and keeps their combined post-
-- reserve balance at or above -$1. A terminal charge may legitimately exceed its request hold,
-- though, and several such settlements can otherwise collect a separate overage from the same
-- account. The dependent runtime will cap the amount moved from the balance at the shared floor
-- while preserving the full billed amount as collected + uncollected evidence.
--
-- Every new column is compatible with the currently serving binary: old settlements keep writing
-- the pre-existing fields and receive zero/default or NULL expansion values. The runtime that
-- depends on these columns is delivered only after this migration is live.

ALTER TABLE accounts
    ADD COLUMN IF NOT EXISTS uncollected_nano bigint NOT NULL DEFAULT 0;

ALTER TABLE reservations
    ADD COLUMN IF NOT EXISTS collected_nano bigint,
    ADD COLUMN IF NOT EXISTS uncollected_nano bigint,
    ADD COLUMN IF NOT EXISTS provider text,
    ADD COLUMN IF NOT EXISTS payable_multiplier_bp bigint;

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS charge_basis_nano bigint;

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS uncollected_nano bigint NOT NULL DEFAULT 0;

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS uncollected_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS charge_basis_nano bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'accounts_uncollected_nonnegative'
          AND conrelid = 'accounts'::regclass
    ) THEN
        ALTER TABLE accounts
            ADD CONSTRAINT accounts_uncollected_nonnegative
            CHECK (uncollected_nano >= 0) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_settlement_collection_shape'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_settlement_collection_shape
            CHECK (
                (collected_nano IS NULL AND uncollected_nano IS NULL)
                OR (
                    actual_nano IS NOT NULL
                    AND collected_nano >= 0
                    AND uncollected_nano >= 0
                    AND actual_nano = collected_nano + uncollected_nano
                )
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_scalar_pricing_shape'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_scalar_pricing_shape
            CHECK (
                (provider IS NULL OR provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm'))
                AND (payable_multiplier_bp IS NULL OR payable_multiplier_bp BETWEEN 0 AND 10000)
            ) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_charge_basis_nonnegative'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
        ALTER TABLE settlement_outbox
            ADD CONSTRAINT settlement_outbox_charge_basis_nonnegative
            CHECK (charge_basis_nano IS NULL OR charge_basis_nano >= 0) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'ledger_uncollected_nonnegative'
          AND conrelid = 'ledger'::regclass
    ) THEN
        ALTER TABLE ledger
            ADD CONSTRAINT ledger_uncollected_nonnegative
            CHECK (uncollected_nano >= 0) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'usage_events_settlement_collection_shape'
          AND conrelid = 'usage_events'::regclass
    ) THEN
        ALTER TABLE usage_events
            ADD CONSTRAINT usage_events_settlement_collection_shape
            CHECK (
                uncollected_nano >= 0
                AND uncollected_nano <= charge_nano
                AND (charge_basis_nano IS NULL OR charge_basis_nano >= 0)
            ) NOT VALID;
    END IF;
END $$;

ALTER TABLE accounts VALIDATE CONSTRAINT accounts_uncollected_nonnegative;
ALTER TABLE reservations VALIDATE CONSTRAINT reservations_settlement_collection_shape;
ALTER TABLE reservations VALIDATE CONSTRAINT reservations_scalar_pricing_shape;
ALTER TABLE settlement_outbox VALIDATE CONSTRAINT settlement_outbox_charge_basis_nonnegative;
ALTER TABLE ledger VALIDATE CONSTRAINT ledger_uncollected_nonnegative;
ALTER TABLE usage_events VALIDATE CONSTRAINT usage_events_settlement_collection_shape;

INSERT INTO engine_schema_migrations(version) VALUES (47)
ON CONFLICT (version) DO NOTHING;
