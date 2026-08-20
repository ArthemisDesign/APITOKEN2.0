-- Expand Gemini Batch profile capacity without changing the existing slot-1 table that schema-57
-- binaries read and write during blue-green overlap. Slot 2 is a separate, shape-identical table;
-- the dependent runtime claims across both tables under the existing profile advisory lock.
CREATE TABLE IF NOT EXISTS gemini_batch_profile_leases_slot2 (
    profile_id text PRIMARY KEY CHECK (profile_id <> ''),
    job_id text NOT NULL,
    item_index bigint NOT NULL,
    worker_instance text NOT NULL CHECK (worker_instance <> ''),
    worker_epoch bigint NOT NULL,
    claim_generation bigint NOT NULL CHECK (claim_generation > 0),
    lease_until bigint NOT NULL,
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    FOREIGN KEY (job_id, item_index)
        REFERENCES gemini_batch_items(job_id, item_index) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX IF NOT EXISTS gemini_batch_profile_leases_slot2_item
    ON gemini_batch_profile_leases_slot2(job_id, item_index);
CREATE INDEX IF NOT EXISTS gemini_batch_profile_leases_slot2_expiry
    ON gemini_batch_profile_leases_slot2(lease_until, profile_id);

-- A schema-57 binary knows only slot 1. If it releases slot 1 while slot 2 remains occupied, promote
-- the slot-2 row atomically so rollback code can renew, settle, or reconcile that surviving claim.
-- Insert-before-delete is deliberate: a concurrent slot-1 claimant can win the PK, in which case the
-- slot-2 lease must remain in place rather than being lost.
CREATE OR REPLACE FUNCTION gemini_batch_promote_profile_slot2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    promoted_job_id text;
    promoted_item_index bigint;
BEGIN
    INSERT INTO gemini_batch_profile_leases(
        profile_id,job_id,item_index,worker_instance,worker_epoch,
        claim_generation,lease_until,created_ts,updated_ts)
    SELECT profile_id,job_id,item_index,worker_instance,worker_epoch,
           claim_generation,lease_until,created_ts,updated_ts
      FROM gemini_batch_profile_leases_slot2
     WHERE profile_id=OLD.profile_id
    ON CONFLICT (profile_id) DO NOTHING
    RETURNING job_id,item_index INTO promoted_job_id,promoted_item_index;

    IF promoted_job_id IS NOT NULL THEN
        DELETE FROM gemini_batch_profile_leases_slot2
         WHERE profile_id=OLD.profile_id
           AND job_id=promoted_job_id
           AND item_index=promoted_item_index;
    END IF;
    RETURN OLD;
END;
$$;

DROP TRIGGER IF EXISTS gemini_batch_profile_slot2_promote ON gemini_batch_profile_leases;
CREATE TRIGGER gemini_batch_profile_slot2_promote
AFTER DELETE ON gemini_batch_profile_leases
FOR EACH ROW EXECUTE FUNCTION gemini_batch_promote_profile_slot2();

INSERT INTO engine_schema_migrations(version) VALUES (58)
ON CONFLICT (version) DO NOTHING;
