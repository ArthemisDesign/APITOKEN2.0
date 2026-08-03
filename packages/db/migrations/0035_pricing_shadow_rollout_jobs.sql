CREATE TABLE "pricing_shadow_policy_jobs_v2" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"rollout_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"account_status" text NOT NULL,
	"account_class" text NOT NULL,
	"owner_context" text NOT NULL,
	"release_policy_id" text NOT NULL,
	"release_policy_version" bigint NOT NULL,
	"release_policy_digest" text NOT NULL,
	"effective_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"expected_active_version" bigint,
	"expected_active_digest" text,
	"request_digest" text NOT NULL,
	"request_payload" jsonb NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"ack_digest" text,
	"ack_payload" jsonb,
	"confirmed_at" timestamp with time zone,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_shadow_policy_jobs_v2_account_unique" UNIQUE("rollout_id","engine_account_id"),
	CONSTRAINT "pricing_shadow_policy_jobs_v2_request_unique" UNIQUE("rollout_id","request_digest"),
	CONSTRAINT "pricing_shadow_policy_jobs_v2_shape_check" CHECK (
    "pricing_shadow_policy_jobs_v2"."engine_account_id" <> ''
    AND "pricing_shadow_policy_jobs_v2"."account_status" IN ('active', 'disabled')
    AND "pricing_shadow_policy_jobs_v2"."account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
    AND "pricing_shadow_policy_jobs_v2"."owner_context" IN ('commerce', 'openkeys', 'service')
    AND "pricing_shadow_policy_jobs_v2"."release_policy_id" <> ''
    AND "pricing_shadow_policy_jobs_v2"."release_policy_version" > 0
    AND "pricing_shadow_policy_jobs_v2"."release_policy_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_policy_jobs_v2"."effective_version" > 0
    AND "pricing_shadow_policy_jobs_v2"."content_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND (("pricing_shadow_policy_jobs_v2"."expected_active_version" IS NULL) = ("pricing_shadow_policy_jobs_v2"."expected_active_digest" IS NULL))
    AND ("pricing_shadow_policy_jobs_v2"."expected_active_version" IS NULL OR "pricing_shadow_policy_jobs_v2"."expected_active_version" > 0)
    AND ("pricing_shadow_policy_jobs_v2"."expected_active_digest" IS NULL OR "pricing_shadow_policy_jobs_v2"."expected_active_digest" ~ '^sha256:v[12]:[0-9a-f]{64}$')
    AND "pricing_shadow_policy_jobs_v2"."request_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND jsonb_typeof("pricing_shadow_policy_jobs_v2"."request_payload") = 'object'
    AND "pricing_shadow_policy_jobs_v2"."attempts" >= 0
    AND "pricing_shadow_policy_jobs_v2"."status" IN ('pending', 'processing', 'retry', 'confirmed', 'blocked', 'dead')
    AND (
      ("pricing_shadow_policy_jobs_v2"."status" = 'processing' AND "pricing_shadow_policy_jobs_v2"."locked_at" IS NOT NULL AND "pricing_shadow_policy_jobs_v2"."locked_by" IS NOT NULL)
      OR ("pricing_shadow_policy_jobs_v2"."status" <> 'processing' AND "pricing_shadow_policy_jobs_v2"."locked_at" IS NULL AND "pricing_shadow_policy_jobs_v2"."locked_by" IS NULL)
    )
    AND (
      ("pricing_shadow_policy_jobs_v2"."status" = 'confirmed'
        AND "pricing_shadow_policy_jobs_v2"."ack_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
        AND "pricing_shadow_policy_jobs_v2"."ack_payload" IS NOT NULL
        AND jsonb_typeof("pricing_shadow_policy_jobs_v2"."ack_payload") = 'object'
        AND "pricing_shadow_policy_jobs_v2"."confirmed_at" IS NOT NULL
        AND "pricing_shadow_policy_jobs_v2"."completed_at" IS NOT NULL
        AND "pricing_shadow_policy_jobs_v2"."last_error" IS NULL)
      OR ("pricing_shadow_policy_jobs_v2"."status" IN ('blocked', 'dead')
        AND "pricing_shadow_policy_jobs_v2"."ack_digest" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."ack_payload" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."confirmed_at" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."completed_at" IS NOT NULL
        AND "pricing_shadow_policy_jobs_v2"."last_error" IS NOT NULL)
      OR ("pricing_shadow_policy_jobs_v2"."status" IN ('pending', 'processing', 'retry')
        AND "pricing_shadow_policy_jobs_v2"."ack_digest" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."ack_payload" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."confirmed_at" IS NULL
        AND "pricing_shadow_policy_jobs_v2"."completed_at" IS NULL)
    )
  )
);
--> statement-breakpoint
CREATE TABLE "pricing_shadow_rollouts_v2" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"idempotency_key" uuid NOT NULL,
	"stage5_run_id" uuid NOT NULL,
	"target_generation" bigint NOT NULL,
	"target_digest" text NOT NULL,
	"recovery_generation" bigint NOT NULL,
	"recovery_digest" text NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"main_catalog_digest" text NOT NULL,
	"openkeys_catalog_digest" text NOT NULL,
	"switch_generation" bigint NOT NULL,
	"switch_digest" text NOT NULL,
	"engine_inventory_digest" text NOT NULL,
	"assignment_manifest_digest" text NOT NULL,
	"policy_manifest_digest" text NOT NULL,
	"rollout_digest" text NOT NULL,
	"assignment_count" bigint NOT NULL,
	"job_count" bigint NOT NULL,
	"actor_id" text NOT NULL,
	"reason" text NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"last_error" text,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_shadow_rollouts_v2_idempotency_key_unique" UNIQUE("idempotency_key"),
	CONSTRAINT "pricing_shadow_rollouts_v2_rollout_digest_unique" UNIQUE("rollout_digest"),
	CONSTRAINT "pricing_shadow_rollouts_v2_shape_check" CHECK (
    "pricing_shadow_rollouts_v2"."target_generation" > 0
    AND "pricing_shadow_rollouts_v2"."recovery_generation" > "pricing_shadow_rollouts_v2"."target_generation"
    AND "pricing_shadow_rollouts_v2"."target_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."recovery_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."catalog_generation" > 0
    AND "pricing_shadow_rollouts_v2"."main_catalog_digest" ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."openkeys_catalog_digest" ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."switch_generation" > 0
    AND "pricing_shadow_rollouts_v2"."switch_digest" ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."engine_inventory_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."assignment_manifest_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."policy_manifest_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."rollout_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_shadow_rollouts_v2"."assignment_count" > 0
    AND "pricing_shadow_rollouts_v2"."job_count" >= 0
    AND "pricing_shadow_rollouts_v2"."job_count" <= "pricing_shadow_rollouts_v2"."assignment_count"
    AND "pricing_shadow_rollouts_v2"."actor_id" <> ''
    AND "pricing_shadow_rollouts_v2"."reason" <> ''
    AND "pricing_shadow_rollouts_v2"."status" IN ('pending', 'processing', 'confirmed', 'blocked', 'dead')
    AND (
      ("pricing_shadow_rollouts_v2"."status" IN ('pending', 'processing') AND "pricing_shadow_rollouts_v2"."completed_at" IS NULL)
      OR ("pricing_shadow_rollouts_v2"."status" = 'confirmed' AND "pricing_shadow_rollouts_v2"."completed_at" IS NOT NULL AND "pricing_shadow_rollouts_v2"."last_error" IS NULL)
      OR ("pricing_shadow_rollouts_v2"."status" IN ('blocked', 'dead') AND "pricing_shadow_rollouts_v2"."completed_at" IS NOT NULL AND "pricing_shadow_rollouts_v2"."last_error" IS NOT NULL)
    )
  )
);
--> statement-breakpoint
ALTER TABLE "pricing_shadow_policy_jobs_v2" ADD CONSTRAINT "pricing_shadow_policy_jobs_v2_rollout_fk" FOREIGN KEY ("rollout_id") REFERENCES "public"."pricing_shadow_rollouts_v2"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_shadow_rollouts_v2" ADD CONSTRAINT "pricing_shadow_rollouts_v2_stage5_run_fk" FOREIGN KEY ("stage5_run_id") REFERENCES "public"."pricing_stage5_runs_v2"("run_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_shadow_rollouts_v2" ADD CONSTRAINT "pricing_shadow_rollouts_v2_target_fk" FOREIGN KEY ("target_generation","target_digest") REFERENCES "public"."pricing_release_plans_v2"("generation","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_shadow_rollouts_v2" ADD CONSTRAINT "pricing_shadow_rollouts_v2_recovery_fk" FOREIGN KEY ("recovery_generation","recovery_digest") REFERENCES "public"."pricing_release_plans_v2"("generation","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "pricing_shadow_policy_jobs_v2_claim_idx" ON "pricing_shadow_policy_jobs_v2" USING btree ("status","next_attempt_at","created_at") WHERE "pricing_shadow_policy_jobs_v2"."status" IN ('pending', 'retry');--> statement-breakpoint
CREATE INDEX "pricing_shadow_rollouts_v2_status_idx" ON "pricing_shadow_rollouts_v2" USING btree ("status","created_at");
