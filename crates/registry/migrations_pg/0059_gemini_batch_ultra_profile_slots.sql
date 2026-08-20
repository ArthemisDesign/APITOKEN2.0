-- Expand Gemini Batch profile capacity for the later plan-aware runtime without changing either
-- legacy lease table understood by schema-58 binaries. Slots 3..20 live in one bounded table; the
-- dependent runtime still allocates at most two slots for every non-Ultra plan.
CREATE TABLE IF NOT EXISTS gemini_batch_profile_leases_extra (
    profile_id text NOT NULL CHECK (profile_id <> ''),
    slot_number smallint NOT NULL CHECK (slot_number BETWEEN 3 AND 20),
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    worker_instance text NOT NULL CHECK (worker_instance <> ''),
    worker_epoch bigint NOT NULL,
    claim_generation bigint NOT NULL CHECK (claim_generation > 0),
    lease_until bigint NOT NULL,
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    PRIMARY KEY (profile_id, slot_number),
    UNIQUE (job_id, item_index),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS gemini_batch_profile_leases_extra_expiry
    ON gemini_batch_profile_leases_extra(lease_until, profile_id, slot_number);

-- Fleet-wide per-profile pacing authority. The new runtime updates this row under the same profile
-- advisory lock as slot allocation, so blue-green overlap cannot start two successive Batch items
-- inside the chosen 2..5 second interval. Schema-58 binaries ignore the additive table.
CREATE TABLE IF NOT EXISTS gemini_batch_profile_dispatch_state (
    profile_id text PRIMARY KEY CHECK (profile_id <> ''),
    next_dispatch_not_before_ms bigint NOT NULL CHECK (next_dispatch_not_before_ms >= 0),
    updated_ts_ms bigint NOT NULL CHECK (updated_ts_ms > 0)
);
CREATE INDEX IF NOT EXISTS gemini_batch_profile_dispatch_state_next
    ON gemini_batch_profile_dispatch_state(next_dispatch_not_before_ms, profile_id);

-- Keep the two legacy tables populated whenever any extra Ultra lease survives. A schema-58
-- rollback can therefore reconcile at most two visible claims at a time; each legacy deletion
-- atomically promotes another extra claim until the complete set has drained. The function is also
-- safe under the new runtime's profile advisory lock and never drops a lease on conflict.
CREATE OR REPLACE FUNCTION gemini_batch_promote_extra_profile_leases()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    promoted_job_id text;
    promoted_item_index bigint;
    promoted_slot smallint;
BEGIN
    PERFORM pg_advisory_xact_lock(hashtextextended(OLD.profile_id, 552966749));

    IF NOT EXISTS (
        SELECT 1 FROM gemini_batch_profile_leases WHERE profile_id=OLD.profile_id
    ) THEN
        promoted_job_id := NULL;
        INSERT INTO gemini_batch_profile_leases(
            profile_id,job_id,item_index,worker_instance,worker_epoch,
            claim_generation,lease_until,created_ts,updated_ts)
        SELECT profile_id,job_id,item_index,worker_instance,worker_epoch,
               claim_generation,lease_until,created_ts,updated_ts
          FROM gemini_batch_profile_leases_extra
         WHERE profile_id=OLD.profile_id
         ORDER BY slot_number
         LIMIT 1
        ON CONFLICT (profile_id) DO NOTHING
        RETURNING job_id,item_index INTO promoted_job_id,promoted_item_index;

        IF promoted_job_id IS NOT NULL THEN
            DELETE FROM gemini_batch_profile_leases_extra
             WHERE profile_id=OLD.profile_id
               AND job_id=promoted_job_id
               AND item_index=promoted_item_index;
        END IF;
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM gemini_batch_profile_leases_slot2 WHERE profile_id=OLD.profile_id
    ) THEN
        promoted_job_id := NULL;
        promoted_item_index := NULL;
        promoted_slot := NULL;
        SELECT job_id,item_index,slot_number
          INTO promoted_job_id,promoted_item_index,promoted_slot
          FROM gemini_batch_profile_leases_extra
         WHERE profile_id=OLD.profile_id
         ORDER BY slot_number
         LIMIT 1;

        IF promoted_job_id IS NOT NULL THEN
            INSERT INTO gemini_batch_profile_leases_slot2(
                profile_id,job_id,item_index,worker_instance,worker_epoch,
                claim_generation,lease_until,created_ts,updated_ts)
            SELECT profile_id,job_id,item_index,worker_instance,worker_epoch,
                   claim_generation,lease_until,created_ts,updated_ts
              FROM gemini_batch_profile_leases_extra
             WHERE profile_id=OLD.profile_id AND slot_number=promoted_slot
            ON CONFLICT (profile_id) DO NOTHING
            RETURNING job_id,item_index INTO promoted_job_id,promoted_item_index;

            IF promoted_job_id IS NOT NULL THEN
                DELETE FROM gemini_batch_profile_leases_extra
                 WHERE profile_id=OLD.profile_id
                   AND slot_number=promoted_slot
                   AND job_id=promoted_job_id
                   AND item_index=promoted_item_index;
            END IF;
        END IF;
    END IF;
    RETURN OLD;
END;
$$;

DROP TRIGGER IF EXISTS zz_gemini_batch_extra_promote_from_slot1
    ON gemini_batch_profile_leases;
CREATE TRIGGER zz_gemini_batch_extra_promote_from_slot1
AFTER DELETE ON gemini_batch_profile_leases
FOR EACH ROW EXECUTE FUNCTION gemini_batch_promote_extra_profile_leases();

DROP TRIGGER IF EXISTS gemini_batch_extra_promote_from_slot2
    ON gemini_batch_profile_leases_slot2;
CREATE TRIGGER gemini_batch_extra_promote_from_slot2
AFTER DELETE ON gemini_batch_profile_leases_slot2
FOR EACH ROW EXECUTE FUNCTION gemini_batch_promote_extra_profile_leases();

INSERT INTO engine_schema_migrations(version) VALUES (59)
ON CONFLICT (version) DO NOTHING;
