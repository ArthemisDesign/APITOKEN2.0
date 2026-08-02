-- Expand-only owner-epoch fence for the Stage 9 pricing-release runtime claim.
--
-- Old binaries remain valid while the global release head is absent: they may continue to insert
-- and heartbeat engine_instances rows with all release-v2 claim fields NULL. The dependent Stage 9
-- runtime later stamps pricing_release_claim_epoch together with the existing release/funding
-- schema identity. Once a release head exists, the trigger rejects every insert or update whose
-- claim is not bound to the exact owner epoch, so an old binary cannot inherit a prior process's
-- apparently compatible claim through ON CONFLICT.

ALTER TABLE engine_instances
    ADD COLUMN IF NOT EXISTS pricing_release_claim_epoch bigint;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'engine_instances_release_v2_epoch_shape'
          AND conrelid = 'engine_instances'::regclass
    ) THEN
        ALTER TABLE engine_instances
            ADD CONSTRAINT engine_instances_release_v2_epoch_shape CHECK (
                (
                    pricing_release_schema_version IS NULL
                    AND funding_schema_version IS NULL
                    AND pricing_release_runtime_digest IS NULL
                    AND pricing_release_claim_epoch IS NULL
                )
                OR (
                    pricing_release_schema_version >= 2
                    AND funding_schema_version >= 2
                    AND pricing_release_runtime_digest IS NOT NULL
                    AND pricing_release_runtime_digest <> ''
                    AND pricing_release_claim_epoch > 0
                )
            ) NOT VALID;
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_pricing_release_runtime_epoch_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1 FROM pricing_release_head_v2 WHERE singleton = 1
    ) AND (
        NEW.pricing_release_schema_version IS NULL
        OR NEW.pricing_release_schema_version < 2
        OR NEW.funding_schema_version IS NULL
        OR NEW.funding_schema_version < 2
        OR NEW.pricing_release_runtime_digest IS NULL
        OR NEW.pricing_release_runtime_digest = ''
        OR NEW.pricing_release_claim_epoch IS DISTINCT FROM NEW.owner_epoch
    ) THEN
        RAISE EXCEPTION 'active pricing v2 release requires an owner-epoch-bound runtime claim'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER engine_instances_release_v2_epoch_fence
BEFORE INSERT OR UPDATE ON engine_instances
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_runtime_epoch_v2();

INSERT INTO engine_schema_migrations(version) VALUES (25)
ON CONFLICT (version) DO NOTHING;
