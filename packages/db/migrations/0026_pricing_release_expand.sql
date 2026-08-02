-- Expand-only commerce control plane for global pricing releases and online funding v2.
-- Existing legacy workers do not read these empty tables; dependent writers arrive only
-- after this migration has a green production watchdog verdict.

CREATE TABLE "pricing_policy_documents_v2" (
  "policy_id" text NOT NULL,
  "policy_version" bigint NOT NULL,
  "owner_type" text NOT NULL,
  "owner_id" text NOT NULL,
  "account_class" text NOT NULL,
  "product_id" text,
  "billing_mode" text NOT NULL,
  "schema_version" bigint NOT NULL,
  "capability_generation" bigint NOT NULL,
  "capability_digest" text NOT NULL,
  "catalog_generation" bigint,
  "catalog_digest" text,
  "switch_generation" bigint,
  "switch_digest" text,
  "content_digest" text NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_policy_documents_v2_pk" PRIMARY KEY("policy_id", "policy_version"),
  CONSTRAINT "pricing_policy_documents_v2_digest_unique"
    UNIQUE("policy_id", "policy_version", "content_digest"),
  CONSTRAINT "pricing_policy_documents_v2_identity_check" CHECK (
    "policy_id" <> ''
    AND "policy_version" > 0
    AND "owner_id" <> ''
    AND "schema_version" >= 2
    AND "capability_generation" > 0
    AND "capability_digest" <> ''
    AND "content_digest" <> ''
    AND (
      ("owner_type" = 'global_b2c' AND "account_class" = 'b2c')
      OR ("owner_type" = 'b2b_client' AND "account_class" = 'b2b')
      OR ("owner_type" = 'openkeys' AND "account_class" = 'openkeys')
      OR ("owner_type" = 'service' AND "account_class" = 'service')
    )
    AND (
      (
        "account_class" = 'service'
        AND "billing_mode" = 'meter_only'
        AND "product_id" IS NULL
        AND "catalog_generation" IS NULL
        AND "catalog_digest" IS NULL
        AND "switch_generation" IS NULL
        AND "switch_digest" IS NULL
      )
      OR (
        "account_class" <> 'service'
        AND "billing_mode" = 'balance'
        AND "product_id" IS NOT NULL AND "product_id" <> ''
        AND "catalog_generation" > 0
        AND "catalog_digest" IS NOT NULL AND "catalog_digest" <> ''
        AND "switch_generation" > 0
        AND "switch_digest" IS NOT NULL AND "switch_digest" <> ''
      )
    )
  )
);--> statement-breakpoint

CREATE TABLE "pricing_policy_rules_v2" (
  "policy_id" text NOT NULL,
  "policy_version" bigint NOT NULL,
  "rule_id" text NOT NULL,
  "rule_digest" text NOT NULL,
  "scope_type" text NOT NULL,
  "provider_id" text,
  "canonical_model_id" text,
  "discount_bps" bigint NOT NULL,
  "payable_multiplier_bp" bigint NOT NULL,
  CONSTRAINT "pricing_policy_rules_v2_pk"
    PRIMARY KEY("policy_id", "policy_version", "rule_id"),
  CONSTRAINT "pricing_policy_rules_v2_digest_unique"
    UNIQUE("policy_id", "policy_version", "rule_id", "rule_digest"),
  CONSTRAINT "pricing_policy_rules_v2_policy_fk"
    FOREIGN KEY("policy_id", "policy_version")
    REFERENCES "pricing_policy_documents_v2"("policy_id", "policy_version")
    ON DELETE restrict,
  CONSTRAINT "pricing_policy_rules_v2_shape_check" CHECK (
    "rule_id" <> ''
    AND "rule_digest" <> ''
    AND "discount_bps" BETWEEN 0 AND 10000
    AND "payable_multiplier_bp" = 10000 - "discount_bps"
    AND (
      ("scope_type" = 'global' AND "provider_id" IS NULL AND "canonical_model_id" IS NULL)
      OR (
        "scope_type" = 'provider'
        AND "provider_id" IS NOT NULL AND "provider_id" <> ''
        AND "canonical_model_id" IS NULL
      )
      OR (
        "scope_type" = 'model'
        AND "provider_id" IS NOT NULL AND "provider_id" <> ''
        AND "canonical_model_id" IS NOT NULL AND "canonical_model_id" <> ''
      )
    )
  )
);--> statement-breakpoint

CREATE UNIQUE INDEX "pricing_policy_rules_v2_global_uidx"
  ON "pricing_policy_rules_v2"("policy_id", "policy_version")
  WHERE "scope_type" = 'global';--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policy_rules_v2_provider_uidx"
  ON "pricing_policy_rules_v2"("policy_id", "policy_version", "provider_id")
  WHERE "scope_type" = 'provider';--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policy_rules_v2_model_uidx"
  ON "pricing_policy_rules_v2"(
    "policy_id", "policy_version", "provider_id", "canonical_model_id"
  ) WHERE "scope_type" = 'model';--> statement-breakpoint

CREATE FUNCTION "enforce_pricing_policy_rule_v2_owner"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NOT EXISTS (
    SELECT 1
    FROM "pricing_policy_documents_v2" policy
    WHERE policy."policy_id" = NEW."policy_id"
      AND policy."policy_version" = NEW."policy_version"
      AND policy."account_class" <> 'service'
      AND policy."billing_mode" = 'balance'
  ) THEN
    RAISE EXCEPTION 'pricing v2 rules are forbidden for meter-only service policies'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "pricing_policy_rule_v2_owner_guard"
BEFORE INSERT ON "pricing_policy_rules_v2"
FOR EACH ROW EXECUTE FUNCTION "enforce_pricing_policy_rule_v2_owner"();--> statement-breakpoint

CREATE TABLE "business_invite_policy_snapshots_v2" (
  "invite_id" uuid PRIMARY KEY,
  "policy_id" text NOT NULL,
  "policy_version" bigint NOT NULL,
  "policy_digest" text NOT NULL,
  "snapshot_digest" text NOT NULL UNIQUE,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "business_invite_policy_snapshots_v2_invite_fk"
    FOREIGN KEY("invite_id") REFERENCES "business_invites"("id") ON DELETE restrict,
  CONSTRAINT "business_invite_policy_snapshots_v2_policy_fk"
    FOREIGN KEY("policy_id", "policy_version", "policy_digest")
    REFERENCES "pricing_policy_documents_v2"("policy_id", "policy_version", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "business_invite_policy_snapshots_v2_digest_check"
    CHECK ("policy_digest" <> '' AND "snapshot_digest" <> '')
);--> statement-breakpoint

CREATE TABLE "service_account_inventory_v2" (
  "service_id" text PRIMARY KEY,
  "engine_account_id" text NOT NULL UNIQUE,
  "purpose" text NOT NULL,
  "responsible" text NOT NULL,
  "status" text NOT NULL,
  "source_version" bigint NOT NULL,
  "content_digest" text NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "service_account_inventory_v2_shape_check" CHECK (
    "service_id" <> ''
    AND "engine_account_id" <> ''
    AND "purpose" <> ''
    AND "responsible" <> ''
    AND "status" IN ('active', 'disabled')
    AND "source_version" > 0
    AND "content_digest" <> ''
  )
);--> statement-breakpoint

CREATE TABLE "pricing_release_plans_v2" (
  "generation" bigint PRIMARY KEY,
  "release_kind" text NOT NULL,
  "schema_version" bigint NOT NULL,
  "commerce_inventory_digest" text NOT NULL,
  "engine_inventory_digest" text NOT NULL,
  "openkeys_inventory_digest" text NOT NULL,
  "service_inventory_digest" text NOT NULL,
  "policy_manifest_digest" text NOT NULL,
  "assignment_manifest_digest" text NOT NULL,
  "funding_manifest_digest" text NOT NULL,
  "engine_release_digest" text NOT NULL,
  "content_digest" text NOT NULL,
  "status" text NOT NULL DEFAULT 'planned',
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_release_plans_v2_digest_unique"
    UNIQUE("generation", "content_digest"),
  CONSTRAINT "pricing_release_plans_v2_identity_check" CHECK (
    "generation" > 0
    AND "release_kind" IN ('target', 'recovery')
    AND "schema_version" >= 2
    AND "commerce_inventory_digest" <> ''
    AND "engine_inventory_digest" <> ''
    AND "openkeys_inventory_digest" <> ''
    AND "service_inventory_digest" <> ''
    AND "policy_manifest_digest" <> ''
    AND "assignment_manifest_digest" <> ''
    AND "funding_manifest_digest" <> ''
    AND "engine_release_digest" <> ''
    AND "content_digest" <> ''
    AND "status" IN ('planned', 'materializing', 'prepared', 'active', 'superseded', 'failed')
  )
);--> statement-breakpoint

CREATE TABLE "pricing_release_assignments_v2" (
  "release_generation" bigint NOT NULL,
  "engine_account_id" text NOT NULL,
  "account_class" text NOT NULL,
  "owner_context" text NOT NULL,
  "owner_id" text NOT NULL,
  "policy_id" text NOT NULL,
  "policy_version" bigint NOT NULL,
  "policy_digest" text NOT NULL,
  "billing_mode" text NOT NULL,
  "funding_generation" bigint,
  "purpose" text,
  "responsible" text,
  "assignment_digest" text NOT NULL,
  CONSTRAINT "pricing_release_assignments_v2_pk"
    PRIMARY KEY("release_generation", "engine_account_id"),
  CONSTRAINT "pricing_release_assignments_v2_digest_unique"
    UNIQUE("release_generation", "engine_account_id", "assignment_digest"),
  CONSTRAINT "pricing_release_assignments_v2_release_fk"
    FOREIGN KEY("release_generation") REFERENCES "pricing_release_plans_v2"("generation")
    ON DELETE restrict,
  CONSTRAINT "pricing_release_assignments_v2_policy_fk"
    FOREIGN KEY("policy_id", "policy_version", "policy_digest")
    REFERENCES "pricing_policy_documents_v2"("policy_id", "policy_version", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_release_assignments_v2_shape_check" CHECK (
    "engine_account_id" <> ''
    AND "owner_id" <> ''
    AND "policy_digest" <> ''
    AND "assignment_digest" <> ''
    AND "owner_context" IN ('commerce', 'openkeys', 'service')
    AND "account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
    AND (
      (
        "account_class" = 'service'
        AND "owner_context" = 'service'
        AND "billing_mode" = 'meter_only'
        AND "funding_generation" IS NULL
        AND "purpose" IS NOT NULL AND "purpose" <> ''
        AND "responsible" IS NOT NULL AND "responsible" <> ''
      )
      OR (
        "account_class" = 'openkeys'
        AND "owner_context" = 'openkeys'
        AND "billing_mode" = 'balance'
        AND "funding_generation" > 0
        AND "purpose" IS NULL
        AND "responsible" IS NULL
      )
      OR (
        "account_class" IN ('b2c', 'b2b')
        AND "owner_context" = 'commerce'
        AND "billing_mode" = 'balance'
        AND "funding_generation" > 0
        AND "purpose" IS NULL
        AND "responsible" IS NULL
      )
    )
  )
);--> statement-breakpoint

CREATE INDEX "pricing_release_assignments_v2_class_idx"
  ON "pricing_release_assignments_v2"(
    "release_generation", "account_class", "engine_account_id"
  );--> statement-breakpoint

CREATE TABLE "pricing_funding_normalizations_v2" (
  "release_generation" bigint NOT NULL,
  "engine_account_id" text NOT NULL,
  "funding_generation" bigint NOT NULL,
  "expected_source_digest" text NOT NULL,
  "target_funding_digest" text NOT NULL,
  "applied_funding_digest" text,
  "status" text NOT NULL DEFAULT 'pending',
  "attempts" integer NOT NULL DEFAULT 0,
  "next_attempt_at" timestamp with time zone NOT NULL DEFAULT now(),
  "locked_at" timestamp with time zone,
  "locked_by" text,
  "last_error" text,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_funding_normalizations_v2_pk"
    PRIMARY KEY("release_generation", "engine_account_id"),
  CONSTRAINT "pricing_funding_normalizations_v2_release_fk"
    FOREIGN KEY("release_generation") REFERENCES "pricing_release_plans_v2"("generation")
    ON DELETE restrict,
  CONSTRAINT "pricing_funding_normalizations_v2_shape_check" CHECK (
    "engine_account_id" <> ''
    AND "funding_generation" > 0
    AND "expected_source_digest" <> ''
    AND "target_funding_digest" <> ''
    AND "attempts" >= 0
    AND "status" IN ('pending', 'processing', 'retry', 'ready', 'blocker')
    AND ("status" <> 'ready' OR (
      "applied_funding_digest" IS NOT NULL
      AND "applied_funding_digest" = "target_funding_digest"
    ))
  )
);--> statement-breakpoint

CREATE INDEX "pricing_funding_normalizations_v2_claim_idx"
  ON "pricing_funding_normalizations_v2"("status", "next_attempt_at", "created_at")
  WHERE "status" IN ('pending', 'retry');--> statement-breakpoint

CREATE TABLE "pricing_release_control_jobs_v2" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid(),
  "job_kind" text NOT NULL,
  "release_generation" bigint NOT NULL,
  "release_digest" text NOT NULL,
  "idempotency_key" text NOT NULL UNIQUE,
  "payload_digest" text NOT NULL,
  "expected_head_version" bigint,
  "stage8_evidence_digest" text,
  "status" text NOT NULL DEFAULT 'pending',
  "attempts" integer NOT NULL DEFAULT 0,
  "next_attempt_at" timestamp with time zone NOT NULL DEFAULT now(),
  "locked_at" timestamp with time zone,
  "locked_by" text,
  "last_error" text,
  "result_digest" text,
  "confirmed_at" timestamp with time zone,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  "updated_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_release_control_jobs_v2_release_fk"
    FOREIGN KEY("release_generation", "release_digest")
    REFERENCES "pricing_release_plans_v2"("generation", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_release_control_jobs_v2_shape_check" CHECK (
    "release_digest" <> ''
    AND "idempotency_key" <> ''
    AND "payload_digest" <> ''
    AND "attempts" >= 0
    AND ("expected_head_version" IS NULL OR "expected_head_version" >= 0)
    AND "job_kind" IN (
      'materialize_release',
      'normalize_funding',
      'collect_stage8',
      'activate_release',
      'activate_recovery'
    )
    AND "status" IN ('pending', 'processing', 'retry', 'confirmed', 'dead')
    AND (
      "job_kind" NOT IN ('activate_release', 'activate_recovery')
      OR (
        "expected_head_version" IS NOT NULL
        AND "stage8_evidence_digest" IS NOT NULL
      )
    )
    AND (
      ("status" = 'confirmed' AND "result_digest" IS NOT NULL AND "confirmed_at" IS NOT NULL)
      OR ("status" <> 'confirmed' AND "confirmed_at" IS NULL)
    )
  )
);--> statement-breakpoint

CREATE INDEX "pricing_release_control_jobs_v2_claim_idx"
  ON "pricing_release_control_jobs_v2"("status", "next_attempt_at", "created_at")
  WHERE "status" IN ('pending', 'retry');--> statement-breakpoint

CREATE TABLE "pricing_stage8_evidence_v2" (
  "evidence_digest" text PRIMARY KEY,
  "target_generation" bigint NOT NULL,
  "target_digest" text NOT NULL,
  "recovery_generation" bigint NOT NULL,
  "recovery_digest" text NOT NULL,
  "commerce_inventory_digest" text NOT NULL,
  "engine_inventory_digest" text NOT NULL,
  "openkeys_inventory_digest" text NOT NULL,
  "sales_contract_digest" text NOT NULL,
  "funding_digest" text NOT NULL,
  "shadow_digest" text NOT NULL,
  "runtime_floor_digest" text NOT NULL,
  "legacy_inflight_count" bigint NOT NULL,
  "blocker_count" bigint NOT NULL,
  "passed" boolean NOT NULL,
  "observed_at" timestamp with time zone NOT NULL,
  "valid_until" timestamp with time zone NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_stage8_evidence_v2_target_fk"
    FOREIGN KEY("target_generation", "target_digest")
    REFERENCES "pricing_release_plans_v2"("generation", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_stage8_evidence_v2_recovery_fk"
    FOREIGN KEY("recovery_generation", "recovery_digest")
    REFERENCES "pricing_release_plans_v2"("generation", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_stage8_evidence_v2_shape_check" CHECK (
    "evidence_digest" <> ''
    AND "commerce_inventory_digest" <> ''
    AND "engine_inventory_digest" <> ''
    AND "openkeys_inventory_digest" <> ''
    AND "sales_contract_digest" <> ''
    AND "funding_digest" <> ''
    AND "shadow_digest" <> ''
    AND "runtime_floor_digest" <> ''
    AND "legacy_inflight_count" >= 0
    AND "blocker_count" >= 0
    AND "valid_until" > "observed_at"
    AND (("passed" AND "legacy_inflight_count" = 0 AND "blocker_count" = 0) OR NOT "passed")
  )
);--> statement-breakpoint

ALTER TABLE "pricing_release_control_jobs_v2"
  ADD CONSTRAINT "pricing_release_control_jobs_v2_evidence_fk"
  FOREIGN KEY("stage8_evidence_digest")
  REFERENCES "pricing_stage8_evidence_v2"("evidence_digest")
  ON DELETE restrict;--> statement-breakpoint

CREATE TABLE "pricing_release_activation_receipts_v2" (
  "activation_id" text PRIMARY KEY,
  "activation_kind" text NOT NULL,
  "release_generation" bigint NOT NULL,
  "release_digest" text NOT NULL,
  "evidence_digest" text NOT NULL,
  "head_version" bigint NOT NULL,
  "receipt_digest" text NOT NULL UNIQUE,
  "activated_at" timestamp with time zone NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pricing_release_activation_receipts_v2_release_fk"
    FOREIGN KEY("release_generation", "release_digest")
    REFERENCES "pricing_release_plans_v2"("generation", "content_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_release_activation_receipts_v2_evidence_fk"
    FOREIGN KEY("evidence_digest") REFERENCES "pricing_stage8_evidence_v2"("evidence_digest")
    ON DELETE restrict,
  CONSTRAINT "pricing_release_activation_receipts_v2_head_unique"
    UNIQUE("head_version"),
  CONSTRAINT "pricing_release_activation_receipts_v2_shape_check" CHECK (
    "activation_id" <> ''
    AND "activation_kind" IN ('cutover', 'recovery')
    AND "head_version" > 0
    AND "receipt_digest" <> ''
  )
);
