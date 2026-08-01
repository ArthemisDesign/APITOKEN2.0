-- Stage 9 policy-capable runtime floor.
--
-- Strict pricing makes a scalar-only rollback unsafe even though the expand schema remains
-- readable by an older binary. Runtime manifest pins are therefore published on the durable
-- owner lease. Before the first strict binding, every still-live owner must carry the new shape;
-- after strict activation an older binary can neither claim nor heartbeat an owner epoch.

ALTER TABLE engine_instances
    ADD COLUMN IF NOT EXISTS pricing_schema_version bigint,
    ADD COLUMN IF NOT EXISTS pricing_runtime_manifest_generation bigint,
    ADD COLUMN IF NOT EXISTS pricing_runtime_manifest_digest text;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'engine_instances_pricing_runtime_shape'
          AND conrelid = 'engine_instances'::regclass
    ) THEN
        ALTER TABLE engine_instances
            ADD CONSTRAINT engine_instances_pricing_runtime_shape CHECK (
                (
                    pricing_schema_version IS NULL
                    AND pricing_runtime_manifest_generation IS NULL
                    AND pricing_runtime_manifest_digest IS NULL
                )
                OR (
                    pricing_schema_version IS NOT NULL
                    AND pricing_schema_version > 0
                    AND pricing_runtime_manifest_generation IS NOT NULL
                    AND pricing_runtime_manifest_generation > 0
                    AND pricing_runtime_manifest_digest IS NOT NULL
                    AND pricing_runtime_manifest_digest <> ''
                )
            ) NOT VALID;
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_policy_capable_engine_instance()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF EXISTS (
        SELECT 1
        FROM account_policy_bindings
        WHERE policy_enforcement = 'strict'
           OR funding_enforcement = 'strict'
    ) AND (
        NEW.pricing_schema_version IS NULL
        OR NEW.pricing_runtime_manifest_generation IS NULL
        OR NEW.pricing_runtime_manifest_digest IS NULL
        OR NEW.pricing_runtime_manifest_digest = ''
    ) THEN
        RAISE EXCEPTION 'strict pricing requires a policy-capable engine runtime manifest'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'engine_instances_policy_runtime_floor'
          AND tgrelid = 'engine_instances'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER engine_instances_policy_runtime_floor
        BEFORE INSERT OR UPDATE OF owner_epoch, lease_until,
            pricing_schema_version, pricing_runtime_manifest_generation,
            pricing_runtime_manifest_digest
        ON engine_instances
        FOR EACH ROW EXECUTE FUNCTION enforce_policy_capable_engine_instance();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION enforce_strict_binding_runtime_floor()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    incapable_live_instances bigint;
BEGIN
    IF NEW.policy_enforcement <> 'strict' AND NEW.funding_enforcement <> 'strict' THEN
        RETURN NEW;
    END IF;

    SELECT count(*) INTO incapable_live_instances
    FROM engine_instances
    WHERE lease_until >= floor(EXTRACT(EPOCH FROM clock_timestamp()))::bigint
      AND (
          pricing_schema_version IS NULL
          OR pricing_runtime_manifest_generation IS NULL
          OR pricing_runtime_manifest_digest IS NULL
          OR pricing_runtime_manifest_digest = ''
      );

    IF incapable_live_instances <> 0 THEN
        RAISE EXCEPTION 'strict pricing activation requires policy-incapable engine instances to drain'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'account_policy_bindings_runtime_floor'
          AND tgrelid = 'account_policy_bindings'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER account_policy_bindings_runtime_floor
        BEFORE INSERT OR UPDATE OF active_effective_version, policy_enforcement,
            funding_enforcement, reconciliation_state
        ON account_policy_bindings
        FOR EACH ROW EXECUTE FUNCTION enforce_strict_binding_runtime_floor();
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (17)
ON CONFLICT (version) DO NOTHING;
