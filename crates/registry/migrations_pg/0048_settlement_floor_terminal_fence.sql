-- Mixed-version fence for the account-wide settlement floor runtime.
--
-- During a blue-green handoff the retired slot drains the shared settlement outbox after the new
-- slot is admitted. Migration 0047 deliberately left its new evidence columns nullable for the
-- old writer, so without a database fence that retired binary could settle a reservation created
-- by the new runtime: either crossing the shared account floor or terminalizing priced usage
-- without collected/uncollected evidence. These guards make such an old transaction fail
-- atomically and leave its durable outbox row for the new runtime to retry.
--
-- An account can already be below -$1 only because an explicit adjustment recorded deeper debt.
-- Settlement may not worsen that debt: for those rows the pre-update balance is the floor. For all
-- other accounts the shared -$1 floor applies. Top-ups and adjustments do not increase spent_nano,
-- so they retain their existing semantics.

CREATE OR REPLACE FUNCTION enforce_account_settlement_floor_fence()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    allowed_floor bigint;
BEGIN
    IF NEW.spent_nano > OLD.spent_nano THEN
        allowed_floor := CASE
            WHEN OLD.balance_nano < -1000000000 THEN OLD.balance_nano
            ELSE -1000000000
        END;

        IF NEW.balance_nano < allowed_floor THEN
            RAISE EXCEPTION 'settlement would cross the account-wide collection floor'
                -- The already-deployed outbox actor classifies 40001 as retryable. The old slot
                -- therefore leaves the intent pending for the new runtime instead of quarantining
                -- it as a permanent invariant failure.
                USING ERRCODE = '40001',
                      CONSTRAINT = 'accounts_settlement_floor_fence';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_trigger
         WHERE tgname = 'accounts_settlement_floor_fence'
           AND tgrelid = 'accounts'::regclass
           AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER accounts_settlement_floor_fence
        BEFORE UPDATE OF balance_nano, spent_nano ON accounts
        FOR EACH ROW
        EXECUTE FUNCTION enforce_account_settlement_floor_fence();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_priced_terminal_collection_fence()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.state IN ('settled', 'canceled')
       AND NEW.provider IS NOT NULL
       AND (NEW.collected_nano IS NULL OR NEW.uncollected_nano IS NULL) THEN
        RAISE EXCEPTION 'priced terminal reservation requires collection evidence'
            -- As above, the draining old outbox actor must keep this row retryable.
            USING ERRCODE = '40001',
                  CONSTRAINT = 'reservations_priced_terminal_collection_evidence';
    END IF;

    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
          FROM pg_trigger
         WHERE tgname = 'reservations_priced_terminal_collection_fence'
           AND tgrelid = 'reservations'::regclass
           AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER reservations_priced_terminal_collection_fence
        BEFORE INSERT OR UPDATE ON reservations
        FOR EACH ROW
        EXECUTE FUNCTION enforce_priced_terminal_collection_fence();
    END IF;
END $$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_scalar_pricing_pair'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_scalar_pricing_pair
            CHECK ((provider IS NULL) = (payable_multiplier_bp IS NULL)) NOT VALID;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservations_priced_terminal_collection_evidence'
          AND conrelid = 'reservations'::regclass
    ) THEN
        ALTER TABLE reservations
            ADD CONSTRAINT reservations_priced_terminal_collection_evidence
            CHECK (
                state NOT IN ('settled', 'canceled')
                OR provider IS NULL
                OR (collected_nano IS NOT NULL AND uncollected_nano IS NOT NULL)
            ) NOT VALID;
    END IF;
END $$;

ALTER TABLE reservations VALIDATE CONSTRAINT reservations_scalar_pricing_pair;
ALTER TABLE reservations VALIDATE CONSTRAINT reservations_priced_terminal_collection_evidence;

INSERT INTO engine_schema_migrations(version) VALUES (48)
ON CONFLICT (version) DO NOTHING;
