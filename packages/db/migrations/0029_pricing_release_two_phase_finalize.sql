-- Expand the dormant pricing-release authority for honest two-phase finalization.
-- Stage 5 may persist immutable ownership/policy assignments before Stage 6 observes live
-- funding identities; no row is backfilled and no release or worker is activated here.

ALTER TABLE "pricing_release_assignments_v2" DROP CONSTRAINT "pricing_release_assignments_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_release_plans_v2" DROP CONSTRAINT "pricing_release_plans_v2_identity_check";--> statement-breakpoint
ALTER TABLE "pricing_stage5_runs_v2" DROP CONSTRAINT "pricing_stage5_runs_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_release_plans_v2" ALTER COLUMN "funding_manifest_digest" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_release_plans_v2" ALTER COLUMN "engine_release_digest" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_stage5_runs_v2" ALTER COLUMN "target_digest" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_stage5_runs_v2" ALTER COLUMN "recovery_digest" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_release_assignments_v2" ADD CONSTRAINT "pricing_release_assignments_v2_shape_check" CHECK (
    "pricing_release_assignments_v2"."engine_account_id" <> ''
    AND "pricing_release_assignments_v2"."owner_id" <> ''
    AND "pricing_release_assignments_v2"."policy_digest" <> ''
    AND "pricing_release_assignments_v2"."assignment_digest" <> ''
    AND "pricing_release_assignments_v2"."owner_context" IN ('commerce', 'openkeys', 'service')
    AND "pricing_release_assignments_v2"."account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
    AND (
      (
        "pricing_release_assignments_v2"."account_class" = 'service'
        AND "pricing_release_assignments_v2"."owner_context" = 'service'
        AND "pricing_release_assignments_v2"."billing_mode" = 'meter_only'
        AND "pricing_release_assignments_v2"."funding_generation" IS NULL
        AND "pricing_release_assignments_v2"."purpose" IS NOT NULL AND "pricing_release_assignments_v2"."purpose" <> ''
        AND "pricing_release_assignments_v2"."responsible" IS NOT NULL AND "pricing_release_assignments_v2"."responsible" <> ''
      )
      OR (
        "pricing_release_assignments_v2"."account_class" = 'openkeys'
        AND "pricing_release_assignments_v2"."owner_context" = 'openkeys'
        AND "pricing_release_assignments_v2"."billing_mode" = 'balance'
        AND ("pricing_release_assignments_v2"."funding_generation" IS NULL OR "pricing_release_assignments_v2"."funding_generation" > 0)
        AND "pricing_release_assignments_v2"."purpose" IS NULL
        AND "pricing_release_assignments_v2"."responsible" IS NULL
      )
      OR (
        "pricing_release_assignments_v2"."account_class" IN ('b2c', 'b2b')
        AND "pricing_release_assignments_v2"."owner_context" = 'commerce'
        AND "pricing_release_assignments_v2"."billing_mode" = 'balance'
        AND ("pricing_release_assignments_v2"."funding_generation" IS NULL OR "pricing_release_assignments_v2"."funding_generation" > 0)
        AND "pricing_release_assignments_v2"."purpose" IS NULL
        AND "pricing_release_assignments_v2"."responsible" IS NULL
      )
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_release_plans_v2" ADD CONSTRAINT "pricing_release_plans_v2_identity_check" CHECK (
    "pricing_release_plans_v2"."generation" > 0
    AND "pricing_release_plans_v2"."release_kind" IN ('target', 'recovery')
    AND "pricing_release_plans_v2"."schema_version" >= 2
    AND "pricing_release_plans_v2"."commerce_inventory_digest" <> ''
    AND "pricing_release_plans_v2"."engine_inventory_digest" <> ''
    AND "pricing_release_plans_v2"."openkeys_inventory_digest" <> ''
    AND "pricing_release_plans_v2"."service_inventory_digest" <> ''
    AND "pricing_release_plans_v2"."policy_manifest_digest" <> ''
    AND "pricing_release_plans_v2"."assignment_manifest_digest" <> ''
    AND ("pricing_release_plans_v2"."funding_manifest_digest" IS NULL OR "pricing_release_plans_v2"."funding_manifest_digest" <> '')
    AND ("pricing_release_plans_v2"."engine_release_digest" IS NULL OR "pricing_release_plans_v2"."engine_release_digest" <> '')
    AND "pricing_release_plans_v2"."content_digest" <> ''
    AND "pricing_release_plans_v2"."status" IN ('planned', 'materializing', 'prepared', 'active', 'superseded', 'failed')
    AND (
      "pricing_release_plans_v2"."status" NOT IN ('prepared', 'active', 'superseded')
      OR ("pricing_release_plans_v2"."funding_manifest_digest" IS NOT NULL AND "pricing_release_plans_v2"."engine_release_digest" IS NOT NULL)
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_stage5_runs_v2" ADD CONSTRAINT "pricing_stage5_runs_v2_shape_check" CHECK (
    "pricing_stage5_runs_v2"."schema_version" = 2
    AND "pricing_stage5_runs_v2"."plan_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."commerce_inventory_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."engine_scan_first_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."engine_scan_second_digest" = "pricing_stage5_runs_v2"."engine_scan_first_digest"
    AND "pricing_stage5_runs_v2"."openkeys_scan_first_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."openkeys_scan_second_digest" = "pricing_stage5_runs_v2"."openkeys_scan_first_digest"
    AND "pricing_stage5_runs_v2"."service_inventory_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."funding_plan_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."target_generation" > 0
    AND "pricing_stage5_runs_v2"."recovery_generation" > "pricing_stage5_runs_v2"."target_generation"
    AND ("pricing_stage5_runs_v2"."target_digest" IS NULL OR "pricing_stage5_runs_v2"."target_digest" ~ '^sha256:v2:[0-9a-f]{64}$')
    AND ("pricing_stage5_runs_v2"."recovery_digest" IS NULL OR "pricing_stage5_runs_v2"."recovery_digest" ~ '^sha256:v2:[0-9a-f]{64}$')
    AND (("pricing_stage5_runs_v2"."target_digest" IS NULL) = ("pricing_stage5_runs_v2"."recovery_digest" IS NULL))
    AND jsonb_typeof("pricing_stage5_runs_v2"."inventory_artifact") = 'object'
    AND jsonb_typeof("pricing_stage5_runs_v2"."plan_artifact") = 'object'
    AND "pricing_stage5_runs_v2"."blocker_count" >= 0
    AND "pricing_stage5_runs_v2"."status" IN ('blocked', 'planned', 'materializing', 'prepared', 'failed')
    AND (
      ("pricing_stage5_runs_v2"."status" = 'blocked' AND "pricing_stage5_runs_v2"."blocker_count" > 0)
      OR ("pricing_stage5_runs_v2"."status" <> 'blocked' AND "pricing_stage5_runs_v2"."blocker_count" = 0)
    )
    AND (
      "pricing_stage5_runs_v2"."status" <> 'prepared'
      OR ("pricing_stage5_runs_v2"."target_digest" IS NOT NULL AND "pricing_stage5_runs_v2"."recovery_digest" IS NOT NULL)
    )
  );--> statement-breakpoint

CREATE FUNCTION "guard_pricing_release_assignment_v2"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  plan_status text;
BEGIN
  IF TG_OP = 'DELETE' THEN
    RAISE EXCEPTION 'pricing release assignments are immutable once inserted'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_release_assignments_v2_immutable_guard';
  END IF;

  SELECT plan."status"
    INTO plan_status
    FROM "pricing_release_plans_v2" plan
   WHERE plan."generation" = NEW."release_generation"
   FOR SHARE;

  IF plan_status IS NULL THEN
    RETURN NEW;
  END IF;

  IF plan_status IN ('prepared', 'active', 'superseded', 'failed') THEN
    RAISE EXCEPTION 'terminal pricing release assignments are frozen'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_release_assignments_v2_immutable_guard';
  END IF;

  IF TG_OP = 'INSERT' THEN
    RETURN NEW;
  END IF;

  IF ROW(
    NEW."release_generation", NEW."engine_account_id", NEW."account_class",
    NEW."owner_context", NEW."owner_id", NEW."policy_id", NEW."policy_version",
    NEW."policy_digest", NEW."billing_mode", NEW."purpose", NEW."responsible",
    NEW."assignment_digest"
  ) IS DISTINCT FROM ROW(
    OLD."release_generation", OLD."engine_account_id", OLD."account_class",
    OLD."owner_context", OLD."owner_id", OLD."policy_id", OLD."policy_version",
    OLD."policy_digest", OLD."billing_mode", OLD."purpose", OLD."responsible",
    OLD."assignment_digest"
  ) THEN
    RAISE EXCEPTION 'pricing release assignment identity is immutable'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_release_assignments_v2_immutable_guard';
  END IF;

  IF NEW."funding_generation" IS NOT DISTINCT FROM OLD."funding_generation" THEN
    RETURN NEW;
  END IF;

  IF OLD."funding_generation" IS NULL
     AND NEW."billing_mode" = 'balance'
     AND NEW."funding_generation" > 0 THEN
    RETURN NEW;
  END IF;

  RAISE EXCEPTION 'funding generation may only finalize from null to one positive identity'
    USING ERRCODE = '23514',
          CONSTRAINT = 'pricing_release_assignments_v2_immutable_guard';
END;
$$;--> statement-breakpoint

CREATE TRIGGER "pricing_release_assignment_v2_guard"
BEFORE INSERT OR UPDATE OR DELETE ON "pricing_release_assignments_v2"
FOR EACH ROW EXECUTE FUNCTION "guard_pricing_release_assignment_v2"();--> statement-breakpoint

CREATE FUNCTION "guard_pricing_release_plan_v2"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF TG_OP = 'UPDATE' THEN
    IF ROW(
      NEW."generation", NEW."release_kind", NEW."schema_version",
      NEW."commerce_inventory_digest", NEW."engine_inventory_digest",
      NEW."openkeys_inventory_digest", NEW."service_inventory_digest",
      NEW."policy_manifest_digest", NEW."assignment_manifest_digest",
      NEW."content_digest", NEW."created_at"
    ) IS DISTINCT FROM ROW(
      OLD."generation", OLD."release_kind", OLD."schema_version",
      OLD."commerce_inventory_digest", OLD."engine_inventory_digest",
      OLD."openkeys_inventory_digest", OLD."service_inventory_digest",
      OLD."policy_manifest_digest", OLD."assignment_manifest_digest",
      OLD."content_digest", OLD."created_at"
    ) THEN
      RAISE EXCEPTION 'pricing release source and policy identity are immutable'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF OLD."funding_manifest_digest" IS NOT NULL
       AND NEW."funding_manifest_digest" IS DISTINCT FROM OLD."funding_manifest_digest" THEN
      RAISE EXCEPTION 'finalized pricing funding manifest cannot be replaced'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF OLD."engine_release_digest" IS NOT NULL
       AND NEW."engine_release_digest" IS DISTINCT FROM OLD."engine_release_digest" THEN
      RAISE EXCEPTION 'prepared engine release digest cannot be replaced'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF OLD."status" = 'failed' AND NEW."status" <> 'failed' THEN
      RAISE EXCEPTION 'failed pricing release plan is terminal'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF NEW."status" <> 'failed'
       AND (CASE OLD."status"
         WHEN 'planned' THEN 0
         WHEN 'materializing' THEN 1
         WHEN 'prepared' THEN 2
         WHEN 'active' THEN 3
         WHEN 'superseded' THEN 4
         ELSE 5
       END) > (CASE NEW."status"
         WHEN 'planned' THEN 0
         WHEN 'materializing' THEN 1
         WHEN 'prepared' THEN 2
         WHEN 'active' THEN 3
         WHEN 'superseded' THEN 4
         ELSE 5
       END) THEN
      RAISE EXCEPTION 'pricing release status cannot move backwards'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;
  END IF;

  IF NEW."engine_release_digest" IS NOT NULL
     AND NEW."funding_manifest_digest" IS NULL THEN
    RAISE EXCEPTION 'engine release cannot be prepared before funding finalization'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
  END IF;

  IF NEW."status" IN ('prepared', 'active', 'superseded') THEN
    IF NEW."funding_manifest_digest" IS NULL OR NEW."engine_release_digest" IS NULL THEN
      RAISE EXCEPTION 'prepared pricing release requires final funding and engine identities'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF NOT EXISTS (
      SELECT 1
        FROM "pricing_release_assignments_v2" assignment
       WHERE assignment."release_generation" = NEW."generation"
    ) THEN
      RAISE EXCEPTION 'prepared pricing release requires a nonempty assignment graph'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF EXISTS (
      SELECT 1
        FROM "pricing_release_assignments_v2" assignment
        LEFT JOIN "pricing_funding_normalizations_v2" normalization
          ON normalization."release_generation" = assignment."release_generation"
         AND normalization."engine_account_id" = assignment."engine_account_id"
       WHERE assignment."release_generation" = NEW."generation"
         AND assignment."billing_mode" = 'balance'
         AND (
           assignment."funding_generation" IS NULL
           OR assignment."funding_generation" <= 0
           OR normalization."engine_account_id" IS NULL
           OR normalization."status" <> 'ready'
           OR normalization."funding_generation" IS DISTINCT FROM assignment."funding_generation"
           OR normalization."target_funding_digest" IS NULL
           OR normalization."applied_funding_digest" IS DISTINCT FROM normalization."target_funding_digest"
           OR normalization."blockers" IS NOT NULL
         )
    ) THEN
      RAISE EXCEPTION 'prepared pricing release has incomplete balance funding assignments'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;

    IF EXISTS (
      SELECT 1
        FROM "pricing_funding_normalizations_v2" normalization
        LEFT JOIN "pricing_release_assignments_v2" assignment
          ON assignment."release_generation" = normalization."release_generation"
         AND assignment."engine_account_id" = normalization."engine_account_id"
         AND assignment."billing_mode" = 'balance'
       WHERE normalization."release_generation" = NEW."generation"
         AND assignment."engine_account_id" IS NULL
    ) THEN
      RAISE EXCEPTION 'prepared pricing release has funding rows outside its assignment graph'
        USING ERRCODE = '23514',
              CONSTRAINT = 'pricing_release_plans_v2_finalize_guard';
    END IF;
  END IF;

  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "pricing_release_plan_v2_guard"
BEFORE INSERT OR UPDATE ON "pricing_release_plans_v2"
FOR EACH ROW EXECUTE FUNCTION "guard_pricing_release_plan_v2"();--> statement-breakpoint

CREATE FUNCTION "guard_pricing_stage5_run_v2"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF ROW(
    NEW."run_id", NEW."schema_version", NEW."plan_digest",
    NEW."commerce_inventory_digest", NEW."engine_scan_first_digest",
    NEW."engine_scan_second_digest", NEW."openkeys_scan_first_digest",
    NEW."openkeys_scan_second_digest", NEW."service_inventory_digest",
    NEW."funding_plan_digest", NEW."target_generation", NEW."recovery_generation",
    NEW."inventory_artifact", NEW."plan_artifact", NEW."blocker_count", NEW."created_at"
  ) IS DISTINCT FROM ROW(
    OLD."run_id", OLD."schema_version", OLD."plan_digest",
    OLD."commerce_inventory_digest", OLD."engine_scan_first_digest",
    OLD."engine_scan_second_digest", OLD."openkeys_scan_first_digest",
    OLD."openkeys_scan_second_digest", OLD."service_inventory_digest",
    OLD."funding_plan_digest", OLD."target_generation", OLD."recovery_generation",
    OLD."inventory_artifact", OLD."plan_artifact", OLD."blocker_count", OLD."created_at"
  ) THEN
    RAISE EXCEPTION 'Stage 5 source and policy plan are immutable'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_stage5_runs_v2_finalize_guard';
  END IF;

  IF OLD."target_digest" IS NOT NULL
     AND NEW."target_digest" IS DISTINCT FROM OLD."target_digest" THEN
    RAISE EXCEPTION 'Stage 5 target digest cannot be replaced after finalization'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_stage5_runs_v2_finalize_guard';
  END IF;

  IF OLD."recovery_digest" IS NOT NULL
     AND NEW."recovery_digest" IS DISTINCT FROM OLD."recovery_digest" THEN
    RAISE EXCEPTION 'Stage 5 recovery digest cannot be replaced after finalization'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_stage5_runs_v2_finalize_guard';
  END IF;

  IF OLD."status" IN ('blocked', 'failed') AND NEW."status" <> OLD."status" THEN
    RAISE EXCEPTION 'blocked or failed Stage 5 run is terminal'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_stage5_runs_v2_finalize_guard';
  END IF;

  IF NEW."status" <> 'failed'
     AND OLD."status" IN ('planned', 'materializing', 'prepared')
     AND (CASE OLD."status"
       WHEN 'planned' THEN 0
       WHEN 'materializing' THEN 1
       WHEN 'prepared' THEN 2
     END) > (CASE NEW."status"
       WHEN 'planned' THEN 0
       WHEN 'materializing' THEN 1
       WHEN 'prepared' THEN 2
       ELSE 3
     END) THEN
    RAISE EXCEPTION 'Stage 5 status cannot move backwards'
      USING ERRCODE = '23514',
            CONSTRAINT = 'pricing_stage5_runs_v2_finalize_guard';
  END IF;

  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "pricing_stage5_run_v2_guard"
BEFORE UPDATE ON "pricing_stage5_runs_v2"
FOR EACH ROW EXECUTE FUNCTION "guard_pricing_stage5_run_v2"();
