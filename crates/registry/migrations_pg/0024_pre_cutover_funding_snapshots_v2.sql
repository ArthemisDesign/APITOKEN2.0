-- Expand-only reservation funding authority for the online Stage 6 transition.
--
-- Migration 0023 intentionally ties pricing_request_funding_allocations_v2 to a fully prepared
-- pricing release. A prepared release in turn requires every referenced account funding
-- generation to exist. These separate pre-cutover tables break that ordering cycle without
-- inventing a temporary pricing release or changing the active scalar price: once an account has
-- a funding-v2 head, a legacy-priced reservation can pin its exact funding generation and lot
-- allocation here while its existing pricing_admission_snapshots row remains the pricing
-- authority. After the global release head exists, new reservations use the release-v2 snapshot
-- and allocation tables from migration 0023 instead.

CREATE OR REPLACE FUNCTION enforce_account_funding_head_step_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'account funding v2 head cannot be deleted'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.head_version <> 1 THEN
            RAISE EXCEPTION 'initial account funding v2 head version must be 1'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.account_id IS DISTINCT FROM OLD.account_id
       OR NEW.active_generation <= OLD.active_generation
       OR NEW.head_version <> OLD.head_version + 1 THEN
        RAISE EXCEPTION 'account funding v2 head must advance one version and generation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER account_funding_head_step_v2
BEFORE INSERT OR UPDATE OR DELETE ON account_funding_head_v2
FOR EACH ROW EXECUTE FUNCTION enforce_account_funding_head_step_v2();

CREATE TABLE IF NOT EXISTS funding_reservation_snapshots_v2 (
    request_id text PRIMARY KEY REFERENCES reservations(request_id) ON DELETE CASCADE,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    funding_schema_version bigint NOT NULL CHECK (funding_schema_version >= 2),
    funding_generation bigint NOT NULL CHECK (funding_generation > 0),
    funding_head_version bigint NOT NULL CHECK (funding_head_version > 0),
    hold_nano bigint NOT NULL CHECK (hold_nano >= 0),
    snapshot_digest text NOT NULL UNIQUE CHECK (snapshot_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    UNIQUE (request_id, account_id, funding_generation),
    FOREIGN KEY (account_id, funding_generation)
        REFERENCES account_funding_generations_v2(account_id, generation) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS funding_reservation_snapshots_v2_account
    ON funding_reservation_snapshots_v2(account_id, created_ts, request_id);

CREATE OR REPLACE FUNCTION enforce_funding_reservation_snapshot_v2_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM reservations reservation
        WHERE reservation.request_id = NEW.request_id
          AND reservation.account_id = NEW.account_id
          AND reservation.hold_nano = NEW.hold_nano
    ) THEN
        RAISE EXCEPTION 'pre-cutover funding v2 snapshot does not match reservation'
            USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER funding_reservation_snapshot_v2_account
BEFORE INSERT ON funding_reservation_snapshots_v2
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_snapshot_v2_account();

CREATE OR REPLACE FUNCTION reject_funding_reservation_snapshot_v2_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pre-cutover funding v2 snapshot is immutable'
        USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER funding_reservation_snapshot_v2_immutable
BEFORE UPDATE ON funding_reservation_snapshots_v2
FOR EACH ROW EXECUTE FUNCTION reject_funding_reservation_snapshot_v2_update();

CREATE TABLE IF NOT EXISTS funding_reservation_allocations_v2 (
    request_id text NOT NULL,
    account_id text NOT NULL,
    funding_generation bigint NOT NULL CHECK (funding_generation > 0),
    allocation_order bigint NOT NULL CHECK (allocation_order > 0),
    lot_id text NOT NULL,
    lot_source_type text NOT NULL CHECK (lot_source_type IN ('paid', 'welcome_bonus')),
    lot_version bigint NOT NULL CHECK (lot_version >= 0),
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    charged_nano bigint CHECK (charged_nano IS NULL OR charged_nano >= 0),
    released_nano bigint CHECK (released_nano IS NULL OR released_nano >= 0),
    PRIMARY KEY (request_id, allocation_order),
    UNIQUE (request_id, lot_id),
    FOREIGN KEY (request_id, account_id, funding_generation)
        REFERENCES funding_reservation_snapshots_v2(
            request_id,
            account_id,
            funding_generation
        ) ON DELETE CASCADE,
    FOREIGN KEY (lot_id, account_id, funding_generation, lot_source_type)
        REFERENCES funding_lots_v2(lot_id, account_id, funding_generation, source_type)
        ON DELETE RESTRICT,
    CHECK (
        (charged_nano IS NULL AND released_nano IS NULL)
        OR (
            charged_nano IS NOT NULL
            AND released_nano IS NOT NULL
            AND released_nano <= reserved_nano
            AND (
                (charged_nano <= reserved_nano
                    AND charged_nano + released_nano = reserved_nano)
                OR (
                    charged_nano > reserved_nano
                    AND lot_source_type = 'paid'
                    AND released_nano = 0
                )
            )
        )
    )
);

CREATE OR REPLACE FUNCTION enforce_funding_reservation_allocation_v2_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF (
        NEW.request_id,
        NEW.account_id,
        NEW.funding_generation,
        NEW.allocation_order,
        NEW.lot_id,
        NEW.lot_source_type,
        NEW.lot_version,
        NEW.reserved_nano
    ) IS DISTINCT FROM (
        OLD.request_id,
        OLD.account_id,
        OLD.funding_generation,
        OLD.allocation_order,
        OLD.lot_id,
        OLD.lot_source_type,
        OLD.lot_version,
        OLD.reserved_nano
    ) THEN
        RAISE EXCEPTION 'pre-cutover funding v2 allocation identity is immutable'
            USING ERRCODE = '23514';
    END IF;
    IF OLD.charged_nano IS NOT NULL
       AND (NEW.charged_nano, NEW.released_nano)
           IS DISTINCT FROM (OLD.charged_nano, OLD.released_nano) THEN
        RAISE EXCEPTION 'pre-cutover funding v2 allocation is already terminal'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER funding_reservation_allocation_v2_update
BEFORE UPDATE ON funding_reservation_allocations_v2
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_allocation_v2_update();

CREATE OR REPLACE FUNCTION assert_funding_reservation_snapshot_v2(p_request_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    reservation_account text;
    reservation_state text;
    reservation_hold bigint;
    reservation_actual bigint;
    snapshot_account text;
    snapshot_hold bigint;
    allocation_count bigint;
    min_order bigint;
    max_order bigint;
    first_paid_order bigint;
    last_bonus_order bigint;
    reserved_total numeric;
    charged_total numeric;
    released_total numeric;
    terminalized_count bigint;
    incomplete_terminal_count bigint;
    overrun_count bigint;
    invalid_overrun_count bigint;
BEGIN
    SELECT
        reservation.account_id,
        reservation.state,
        reservation.hold_nano,
        reservation.actual_nano,
        snapshot.account_id,
        snapshot.hold_nano
    INTO
        reservation_account,
        reservation_state,
        reservation_hold,
        reservation_actual,
        snapshot_account,
        snapshot_hold
    FROM reservations reservation
    JOIN funding_reservation_snapshots_v2 snapshot
      ON snapshot.request_id = reservation.request_id
    WHERE reservation.request_id = p_request_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;
    IF snapshot_account <> reservation_account OR snapshot_hold <> reservation_hold THEN
        RAISE EXCEPTION 'pre-cutover funding v2 snapshot does not match reservation'
            USING ERRCODE = '23514';
    END IF;

    SELECT
        count(*),
        min(allocation_order),
        max(allocation_order),
        min(allocation_order) FILTER (WHERE lot_source_type = 'paid'),
        max(allocation_order) FILTER (WHERE lot_source_type = 'welcome_bonus'),
        COALESCE(sum(reserved_nano), 0),
        COALESCE(sum(charged_nano), 0),
        COALESCE(sum(released_nano), 0),
        count(*) FILTER (WHERE charged_nano IS NOT NULL OR released_nano IS NOT NULL),
        count(*) FILTER (
            WHERE (charged_nano IS NULL) IS DISTINCT FROM (released_nano IS NULL)
        ),
        count(*) FILTER (WHERE charged_nano > reserved_nano),
        count(*) FILTER (
            WHERE charged_nano > reserved_nano
              AND (lot_source_type <> 'paid' OR allocation_order <> (
                  SELECT max(nested.allocation_order)
                  FROM funding_reservation_allocations_v2 nested
                  WHERE nested.request_id = p_request_id
              ))
        )
    INTO
        allocation_count,
        min_order,
        max_order,
        first_paid_order,
        last_bonus_order,
        reserved_total,
        charged_total,
        released_total,
        terminalized_count,
        incomplete_terminal_count,
        overrun_count,
        invalid_overrun_count
    FROM funding_reservation_allocations_v2
    WHERE request_id = p_request_id;

    IF reserved_total <> reservation_hold
       OR (allocation_count = 0 AND reservation_hold <> 0)
       OR (
           allocation_count > 0
           AND (min_order <> 1 OR max_order <> allocation_count)
       )
       OR (
           first_paid_order IS NOT NULL
           AND last_bonus_order IS NOT NULL
           AND last_bonus_order > first_paid_order
       ) THEN
        RAISE EXCEPTION 'pre-cutover funding v2 allocations do not cover hold bonus-first'
            USING ERRCODE = '23514';
    END IF;

    IF reservation_state IN ('settled', 'canceled') THEN
        IF reservation_actual IS NULL
           OR incomplete_terminal_count <> 0
           OR terminalized_count <> allocation_count
           OR charged_total <> reservation_actual
           OR released_total <> GREATEST(reservation_hold - reservation_actual, 0)
           OR invalid_overrun_count <> 0
           OR (reservation_actual > reservation_hold AND overrun_count <> 1)
           OR (reservation_actual <= reservation_hold AND overrun_count <> 0) THEN
            RAISE EXCEPTION 'terminal pre-cutover funding v2 allocation is inconsistent'
                USING ERRCODE = '23514';
        END IF;
    ELSIF terminalized_count <> 0 OR incomplete_terminal_count <> 0 THEN
        RAISE EXCEPTION 'active pre-cutover funding v2 allocation is terminalized'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION assert_normalized_reservation_funding_v2(p_request_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    reservation_state text;
    active_generation bigint;
    active_head_version bigint;
    snapshot_count bigint;
    compatible_snapshot_count bigint;
BEGIN
    SELECT reservation.state, head.active_generation, head.head_version
    INTO reservation_state, active_generation, active_head_version
    FROM reservations reservation
    LEFT JOIN account_funding_head_v2 head ON head.account_id = reservation.account_id
    WHERE reservation.request_id = p_request_id;

    IF NOT FOUND
       OR active_generation IS NULL
       OR reservation_state IN ('settled', 'canceled') THEN
        RETURN;
    END IF;

    SELECT
        count(*),
        count(*) FILTER (
            WHERE snapshot.funding_generation = active_generation
              AND (
                  snapshot.snapshot_source = 'pricing_release'
                  OR snapshot.funding_head_version = active_head_version
              )
        )
    INTO snapshot_count, compatible_snapshot_count
    FROM (
        SELECT
            'pre_cutover'::text AS snapshot_source,
            funding_generation,
            funding_head_version
        FROM funding_reservation_snapshots_v2
        WHERE request_id = p_request_id
        UNION ALL
        SELECT
            'pricing_release'::text,
            funding_generation,
            NULL::bigint
        FROM pricing_request_snapshots_v2
        WHERE request_id = p_request_id AND billing_mode = 'balance'
    ) snapshot;

    IF snapshot_count <> 1 OR compatible_snapshot_count <> 1 THEN
        RAISE EXCEPTION 'active normalized reservation lacks one compatible funding v2 snapshot'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_reservation_v2_from_reservation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM assert_funding_reservation_snapshot_v2(NEW.request_id);
    PERFORM assert_normalized_reservation_funding_v2(NEW.request_id);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_reservation_v2_from_snapshot()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    request_identity text;
BEGIN
    request_identity := CASE WHEN TG_OP = 'DELETE' THEN OLD.request_id ELSE NEW.request_id END;
    PERFORM assert_funding_reservation_snapshot_v2(request_identity);
    PERFORM assert_normalized_reservation_funding_v2(request_identity);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_reservation_v2_from_allocation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_funding_reservation_snapshot_v2(OLD.request_id);
    ELSE
        PERFORM assert_funding_reservation_snapshot_v2(NEW.request_id);
        IF TG_OP = 'UPDATE' AND NEW.request_id IS DISTINCT FROM OLD.request_id THEN
            PERFORM assert_funding_reservation_snapshot_v2(OLD.request_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_reservation_v2_from_head()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    account_identity text;
    reservation_identity text;
BEGIN
    account_identity := CASE WHEN TG_OP = 'DELETE' THEN OLD.account_id ELSE NEW.account_id END;
    FOR reservation_identity IN
        SELECT request_id
        FROM reservations
        WHERE account_id = account_identity
          AND state NOT IN ('settled', 'canceled')
    LOOP
        PERFORM assert_normalized_reservation_funding_v2(reservation_identity);
    END LOOP;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER reservations_funding_snapshot_v2
AFTER INSERT OR UPDATE OF account_id, hold_nano, state, actual_nano ON reservations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_v2_from_reservation();

CREATE CONSTRAINT TRIGGER funding_reservation_snapshots_v2_parity
AFTER INSERT OR DELETE ON funding_reservation_snapshots_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_v2_from_snapshot();

CREATE CONSTRAINT TRIGGER funding_reservation_allocations_v2_parity
AFTER INSERT OR UPDATE OR DELETE ON funding_reservation_allocations_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_v2_from_allocation();

CREATE CONSTRAINT TRIGGER pricing_request_snapshots_funding_v2_coverage
AFTER INSERT OR DELETE ON pricing_request_snapshots_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_v2_from_snapshot();

CREATE CONSTRAINT TRIGGER funding_head_reservation_v2_coverage
AFTER INSERT OR UPDATE OR DELETE ON account_funding_head_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_reservation_v2_from_head();

INSERT INTO engine_schema_migrations(version) VALUES (24)
ON CONFLICT (version) DO NOTHING;
