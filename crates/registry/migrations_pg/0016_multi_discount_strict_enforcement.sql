-- Stage 9 strict-enforcement expansion.
--
-- The columns remain nullable for pre-cutover rows and dormant policy_v1 fixtures. The strict
-- runtime constructor requires new reservations to carry the independently moving admission heads
-- and the runtime manifest that accepted them. Deferred guards make a strict account fail closed
-- even if an older scalar-only binary is accidentally started: aggregate money, funding buckets,
-- snapshots and allocations must agree at commit.

ALTER TABLE pricing_admission_snapshots
    ADD COLUMN IF NOT EXISTS source_policy_digest text,
    ADD COLUMN IF NOT EXISTS admission_catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_catalog_digest text,
    ADD COLUMN IF NOT EXISTS admission_switch_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_switch_digest text,
    ADD COLUMN IF NOT EXISTS runtime_manifest_generation bigint,
    ADD COLUMN IF NOT EXISTS runtime_manifest_digest text;

ALTER TABLE reservation_funding_allocations
    ADD COLUMN IF NOT EXISTS allocation_order bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'reservation_funding_allocations_order_positive'
          AND conrelid = 'reservation_funding_allocations'::regclass
    ) THEN
        ALTER TABLE reservation_funding_allocations
            ADD CONSTRAINT reservation_funding_allocations_order_positive CHECK (
                allocation_order IS NULL OR allocation_order > 0
            ) NOT VALID;
    END IF;
END $$;

CREATE UNIQUE INDEX IF NOT EXISTS reservation_funding_allocations_request_order
    ON reservation_funding_allocations(request_id, allocation_order)
    WHERE allocation_order IS NOT NULL;

ALTER TABLE api_keys
    ADD COLUMN IF NOT EXISTS activation_policy_effective_version bigint,
    ADD COLUMN IF NOT EXISTS activation_policy_digest text,
    ADD COLUMN IF NOT EXISTS activation_policy_ack_ts bigint;

CREATE UNIQUE INDEX IF NOT EXISTS account_policy_versions_activation_identity
    ON account_policy_versions(account_id, effective_version, content_digest);

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'api_keys_activation_policy_shape'
          AND conrelid = 'api_keys'::regclass
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_activation_policy_shape CHECK (
                (
                    activation_policy_effective_version IS NULL
                    AND activation_policy_digest IS NULL
                    AND activation_policy_ack_ts IS NULL
                )
                OR (
                    activation_policy_effective_version IS NOT NULL
                    AND activation_policy_effective_version > 0
                    AND activation_policy_digest IS NOT NULL
                    AND activation_policy_digest <> ''
                    AND activation_policy_ack_ts IS NOT NULL
                    AND activation_policy_ack_ts > 0
                )
            ) NOT VALID;
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'api_keys_activation_policy_fk'
          AND conrelid = 'api_keys'::regclass
    ) THEN
        ALTER TABLE api_keys
            ADD CONSTRAINT api_keys_activation_policy_fk FOREIGN KEY (
                account_id,
                activation_policy_effective_version,
                activation_policy_digest
            ) REFERENCES account_policy_versions(
                account_id,
                effective_version,
                content_digest
            ) ON DELETE RESTRICT NOT VALID;
    END IF;
END $$;

-- Attribution must survive request-lifecycle pruning, so repeat the admission/runtime lineage on
-- every durable history surface.
ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS source_policy_digest text,
    ADD COLUMN IF NOT EXISTS admission_catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_catalog_digest text,
    ADD COLUMN IF NOT EXISTS admission_switch_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_switch_digest text,
    ADD COLUMN IF NOT EXISTS runtime_manifest_generation bigint,
    ADD COLUMN IF NOT EXISTS runtime_manifest_digest text;

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS source_policy_digest text,
    ADD COLUMN IF NOT EXISTS admission_catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_catalog_digest text,
    ADD COLUMN IF NOT EXISTS admission_switch_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_switch_digest text,
    ADD COLUMN IF NOT EXISTS runtime_manifest_generation bigint,
    ADD COLUMN IF NOT EXISTS runtime_manifest_digest text;

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS source_policy_digest text,
    ADD COLUMN IF NOT EXISTS admission_catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_catalog_digest text,
    ADD COLUMN IF NOT EXISTS admission_switch_generation bigint,
    ADD COLUMN IF NOT EXISTS admission_switch_digest text,
    ADD COLUMN IF NOT EXISTS runtime_manifest_generation bigint,
    ADD COLUMN IF NOT EXISTS runtime_manifest_digest text;

CREATE OR REPLACE FUNCTION assert_strict_funding_account(p_account_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    account_balance bigint;
    account_reserved bigint;
    bucket_balance numeric;
    bucket_reserved numeric;
    strict_funding boolean;
BEGIN
    SELECT
        a.balance_nano,
        a.reserved_nano,
        COALESCE(b.funding_enforcement = 'strict', false)
    INTO account_balance, account_reserved, strict_funding
    FROM accounts a
    LEFT JOIN account_policy_bindings b ON b.account_id = a.id
    WHERE a.id = p_account_id;

    IF NOT FOUND OR NOT strict_funding THEN
        RETURN;
    END IF;

    SELECT COALESCE(sum(balance_nano), 0), COALESCE(sum(reserved_nano), 0)
    INTO bucket_balance, bucket_reserved
    FROM funding_buckets
    WHERE account_id = p_account_id;

    IF bucket_balance <> account_balance OR bucket_reserved <> account_reserved THEN
        RAISE EXCEPTION 'strict funding buckets do not match account aggregates'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_strict_funding_account_from_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM assert_strict_funding_account(NEW.id);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_strict_funding_account_from_bucket()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_strict_funding_account(OLD.account_id);
    ELSE
        PERFORM assert_strict_funding_account(NEW.account_id);
        IF TG_OP = 'UPDATE' AND NEW.account_id IS DISTINCT FROM OLD.account_id THEN
            PERFORM assert_strict_funding_account(OLD.account_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'accounts_strict_funding_parity'
          AND tgrelid = 'accounts'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE CONSTRAINT TRIGGER accounts_strict_funding_parity
        AFTER INSERT OR UPDATE OF balance_nano, reserved_nano ON accounts
        DEFERRABLE INITIALLY DEFERRED
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_funding_account_from_account();
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'funding_buckets_strict_account_parity'
          AND tgrelid = 'funding_buckets'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE CONSTRAINT TRIGGER funding_buckets_strict_account_parity
        AFTER INSERT OR UPDATE OR DELETE ON funding_buckets
        DEFERRABLE INITIALLY DEFERRED
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_funding_account_from_bucket();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION assert_strict_reservation(p_request_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    reservation_row reservations%ROWTYPE;
    policy_mode text;
    funding_mode text;
    reconciliation text;
    snapshot_mode text;
    snapshot_track boolean;
    allocation_count bigint;
    reserved_total numeric;
    charged_total numeric;
    released_total numeric;
    terminalized_total bigint;
    invalid_terminal_total bigint;
    ineligible_total bigint;
BEGIN
    SELECT * INTO reservation_row
    FROM reservations
    WHERE request_id = p_request_id;
    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT policy_enforcement, funding_enforcement, reconciliation_state
    INTO policy_mode, funding_mode, reconciliation
    FROM account_policy_bindings
    WHERE account_id = reservation_row.account_id;
    IF NOT FOUND OR (policy_mode <> 'strict' AND funding_mode <> 'strict') THEN
        RETURN;
    END IF;
    IF policy_mode <> 'strict' OR funding_mode <> 'strict' OR reconciliation <> 'verified' THEN
        RAISE EXCEPTION 'strict reservation has a partial or unreconciled binding'
            USING ERRCODE = '23514';
    END IF;

    SELECT snapshot_kind, track_eligible
    INTO snapshot_mode, snapshot_track
    FROM pricing_admission_snapshots
    WHERE request_id = p_request_id
      AND account_id = reservation_row.account_id;
    IF NOT FOUND OR snapshot_mode <> 'policy_v1' THEN
        RAISE EXCEPTION 'strict reservation lacks a policy_v1 admission snapshot'
            USING ERRCODE = '23514';
    END IF;

    SELECT
        count(*),
        COALESCE(sum(a.reserved_nano), 0),
        COALESCE(sum(a.charged_nano), 0),
        COALESCE(sum(a.released_nano), 0),
        count(*) FILTER (
            WHERE a.allocation_order IS NULL
               OR a.reserved_nano <= 0
               OR (snapshot_track AND b.eligibility NOT IN ('track', 'any'))
               OR (NOT snapshot_track AND b.eligibility <> 'any')
        ),
        count(*) FILTER (
            WHERE a.charged_nano IS NOT NULL OR a.released_nano IS NOT NULL
        ),
        count(*) FILTER (
            WHERE a.charged_nano IS NULL
               OR a.released_nano IS NULL
               OR a.charged_nano + a.released_nano <> a.reserved_nano
        )
    INTO
        allocation_count,
        reserved_total,
        charged_total,
        released_total,
        ineligible_total,
        terminalized_total,
        invalid_terminal_total
    FROM reservation_funding_allocations a
    JOIN funding_buckets b
      ON b.bucket_id = a.bucket_id
     AND b.account_id = a.account_id
    WHERE a.request_id = p_request_id
      AND a.account_id = reservation_row.account_id;

    IF allocation_count = 0
       OR reserved_total <> reservation_row.hold_nano
       OR ineligible_total <> 0 THEN
        RAISE EXCEPTION 'strict reservation funding allocation is incomplete or ineligible'
            USING ERRCODE = '23514';
    END IF;

    IF reservation_row.state IN ('settled', 'canceled') THEN
        IF reservation_row.actual_nano IS NULL
           OR invalid_terminal_total <> 0
           OR charged_total <> reservation_row.actual_nano
           OR released_total <> reservation_row.hold_nano - reservation_row.actual_nano THEN
            RAISE EXCEPTION 'strict terminal reservation funding settlement is inconsistent'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF terminalized_total <> 0 THEN
            RAISE EXCEPTION 'active strict reservation already has terminal funding allocation'
                USING ERRCODE = '23514';
        END IF;
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_strict_reservation_from_reservation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM assert_strict_reservation(NEW.request_id);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_strict_reservation_from_allocation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_strict_reservation(OLD.request_id);
    ELSE
        PERFORM assert_strict_reservation(NEW.request_id);
        IF TG_OP = 'UPDATE' AND NEW.request_id IS DISTINCT FROM OLD.request_id THEN
            PERFORM assert_strict_reservation(OLD.request_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'reservations_strict_policy_funding'
          AND tgrelid = 'reservations'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE CONSTRAINT TRIGGER reservations_strict_policy_funding
        AFTER INSERT OR UPDATE ON reservations
        DEFERRABLE INITIALLY DEFERRED
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_reservation_from_reservation();
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'reservation_allocations_strict_policy_funding'
          AND tgrelid = 'reservation_funding_allocations'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE CONSTRAINT TRIGGER reservation_allocations_strict_policy_funding
        AFTER INSERT OR UPDATE OR DELETE ON reservation_funding_allocations
        DEFERRABLE INITIALLY DEFERRED
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_reservation_from_allocation();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_strict_key_policy_ack()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    active_version bigint;
    active_digest text;
    enforcement text;
BEGIN
    IF COALESCE(NEW.status, 'active') <> 'active' THEN
        RETURN NEW;
    END IF;

    SELECT
        binding.active_effective_version,
        policy.content_digest,
        binding.policy_enforcement
    INTO active_version, active_digest, enforcement
    FROM account_policy_bindings binding
    LEFT JOIN account_policy_versions policy
      ON policy.account_id = binding.account_id
     AND policy.effective_version = binding.active_effective_version
    WHERE binding.account_id = NEW.account_id;

    IF NOT FOUND OR enforcement <> 'strict' THEN
        RETURN NEW;
    END IF;

    IF NEW.activation_policy_effective_version IS DISTINCT FROM active_version
       OR NEW.activation_policy_digest IS DISTINCT FROM active_digest
       OR NEW.activation_policy_ack_ts IS NULL
       OR NEW.activation_policy_ack_ts <= 0 THEN
        RAISE EXCEPTION 'strict key activation requires the exact active policy ACK'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'api_keys_strict_policy_ack'
          AND tgrelid = 'api_keys'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER api_keys_strict_policy_ack
        BEFORE INSERT OR UPDATE OF status, account_id ON api_keys
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_key_policy_ack();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_strict_binding_cutover()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    active_digest text;
    outstanding_legacy bigint;
    unstamped_keys bigint;
BEGIN
    IF NEW.policy_enforcement <> 'strict' AND NEW.funding_enforcement <> 'strict' THEN
        RETURN NEW;
    END IF;
    IF NEW.policy_enforcement <> 'strict'
       OR NEW.funding_enforcement <> 'strict'
       OR NEW.reconciliation_state <> 'verified'
       OR NEW.active_effective_version IS NULL THEN
        RAISE EXCEPTION 'strict policy and funding enforcement must activate together after reconciliation'
            USING ERRCODE = '23514';
    END IF;

    SELECT content_digest INTO active_digest
    FROM account_policy_versions
    WHERE account_id = NEW.account_id
      AND effective_version = NEW.active_effective_version;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'strict binding has no immutable active policy'
            USING ERRCODE = '23514';
    END IF;

    SELECT count(*) INTO outstanding_legacy
    FROM reservations reservation
    LEFT JOIN pricing_admission_snapshots snapshot
      ON snapshot.request_id = reservation.request_id
    WHERE reservation.account_id = NEW.account_id
      AND reservation.state IN ('reserved', 'delivering', 'settlement_pending')
      AND snapshot.snapshot_kind IS DISTINCT FROM 'policy_v1';
    IF outstanding_legacy <> 0 THEN
        RAISE EXCEPTION 'strict binding activation requires legacy reservations to drain'
            USING ERRCODE = '23514';
    END IF;

    IF TG_OP = 'INSERT' OR OLD.policy_enforcement <> 'strict' THEN
        SELECT count(*) INTO unstamped_keys
        FROM api_keys
        WHERE account_id = NEW.account_id
          AND COALESCE(status, 'active') = 'active'
          AND (
              activation_policy_effective_version IS DISTINCT FROM NEW.active_effective_version
              OR activation_policy_digest IS DISTINCT FROM active_digest
              OR activation_policy_ack_ts IS NULL
          );
        IF unstamped_keys <> 0 THEN
            RAISE EXCEPTION 'strict binding activation requires every active key to carry the exact policy ACK'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    RETURN NEW;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_strict_binding_state()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    active_request_id text;
BEGIN
    PERFORM assert_strict_funding_account(NEW.account_id);
    IF NEW.policy_enforcement = 'strict' AND NEW.funding_enforcement = 'strict' THEN
        FOR active_request_id IN
            SELECT request_id
            FROM reservations
            WHERE account_id = NEW.account_id
              AND state IN ('reserved', 'delivering', 'settlement_pending')
        LOOP
            PERFORM assert_strict_reservation(active_request_id);
        END LOOP;
    END IF;
    RETURN NULL;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'account_policy_bindings_strict_cutover'
          AND tgrelid = 'account_policy_bindings'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER account_policy_bindings_strict_cutover
        BEFORE INSERT OR UPDATE OF active_effective_version, policy_enforcement,
            funding_enforcement, reconciliation_state
        ON account_policy_bindings
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_binding_cutover();
    END IF;
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'account_policy_bindings_strict_state'
          AND tgrelid = 'account_policy_bindings'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE CONSTRAINT TRIGGER account_policy_bindings_strict_state
        AFTER INSERT OR UPDATE ON account_policy_bindings
        DEFERRABLE INITIALLY DEFERRED
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_binding_state();
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (16)
ON CONFLICT (version) DO NOTHING;
