CREATE TABLE "pricing_stage5_blockers_v2" (
	"run_id" uuid NOT NULL,
	"blocker_digest" text NOT NULL,
	"blocker_code" text NOT NULL,
	"blocker_context" text NOT NULL,
	"subject_id" text NOT NULL,
	"detail" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_stage5_blockers_v2_pk" PRIMARY KEY("run_id","blocker_digest"),
	CONSTRAINT "pricing_stage5_blockers_v2_shape_check" CHECK (
    "pricing_stage5_blockers_v2"."blocker_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_blockers_v2"."blocker_code" <> ''
    AND "pricing_stage5_blockers_v2"."blocker_context" IN ('commerce', 'engine', 'openkeys', 'service', 'funding', 'release')
    AND "pricing_stage5_blockers_v2"."subject_id" <> ''
    AND "pricing_stage5_blockers_v2"."detail" <> ''
  )
);
--> statement-breakpoint
CREATE TABLE "pricing_stage5_prepare_acks_v2" (
	"run_id" uuid NOT NULL,
	"artifact_kind" text NOT NULL,
	"artifact_id" text NOT NULL,
	"artifact_version" bigint NOT NULL,
	"expected_digest" text NOT NULL,
	"mutation_result" text NOT NULL,
	"readback_digest" text NOT NULL,
	"ack_digest" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_stage5_prepare_acks_v2_pk" PRIMARY KEY("run_id","artifact_kind","artifact_id","artifact_version"),
	CONSTRAINT "pricing_stage5_prepare_acks_v2_ack_digest_unique" UNIQUE("ack_digest"),
	CONSTRAINT "pricing_stage5_prepare_acks_v2_shape_check" CHECK (
    "pricing_stage5_prepare_acks_v2"."artifact_kind" IN (
      'capability', 'main_catalog', 'openkeys_catalog', 'switches', 'policy',
      'target_release', 'recovery_release', 'recovery_link'
    )
    AND "pricing_stage5_prepare_acks_v2"."artifact_id" <> ''
    AND "pricing_stage5_prepare_acks_v2"."artifact_version" > 0
    AND "pricing_stage5_prepare_acks_v2"."expected_digest" ~ '^sha256:v[12]:[0-9a-f]{64}$'
    AND "pricing_stage5_prepare_acks_v2"."mutation_result" IN ('stored', 'unchanged')
    AND "pricing_stage5_prepare_acks_v2"."readback_digest" = "pricing_stage5_prepare_acks_v2"."expected_digest"
    AND "pricing_stage5_prepare_acks_v2"."ack_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
  )
);
--> statement-breakpoint
CREATE TABLE "pricing_stage5_runs_v2" (
	"run_id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"schema_version" bigint NOT NULL,
	"plan_digest" text NOT NULL,
	"commerce_inventory_digest" text NOT NULL,
	"engine_scan_first_digest" text NOT NULL,
	"engine_scan_second_digest" text NOT NULL,
	"openkeys_scan_first_digest" text NOT NULL,
	"openkeys_scan_second_digest" text NOT NULL,
	"service_inventory_digest" text NOT NULL,
	"funding_plan_digest" text NOT NULL,
	"target_generation" bigint NOT NULL,
	"target_digest" text NOT NULL,
	"recovery_generation" bigint NOT NULL,
	"recovery_digest" text NOT NULL,
	"inventory_artifact" jsonb NOT NULL,
	"plan_artifact" jsonb NOT NULL,
	"blocker_count" bigint NOT NULL,
	"status" text DEFAULT 'planned' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_stage5_runs_v2_plan_digest_unique" UNIQUE("plan_digest"),
	CONSTRAINT "pricing_stage5_runs_v2_target_unique" UNIQUE("target_generation","target_digest"),
	CONSTRAINT "pricing_stage5_runs_v2_recovery_unique" UNIQUE("recovery_generation","recovery_digest"),
	CONSTRAINT "pricing_stage5_runs_v2_shape_check" CHECK (
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
    AND "pricing_stage5_runs_v2"."target_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage5_runs_v2"."recovery_generation" > "pricing_stage5_runs_v2"."target_generation"
    AND "pricing_stage5_runs_v2"."recovery_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND jsonb_typeof("pricing_stage5_runs_v2"."inventory_artifact") = 'object'
    AND jsonb_typeof("pricing_stage5_runs_v2"."plan_artifact") = 'object'
    AND "pricing_stage5_runs_v2"."blocker_count" >= 0
    AND "pricing_stage5_runs_v2"."status" IN ('blocked', 'planned', 'materializing', 'prepared', 'failed')
    AND (
      ("pricing_stage5_runs_v2"."status" = 'blocked' AND "pricing_stage5_runs_v2"."blocker_count" > 0)
      OR ("pricing_stage5_runs_v2"."status" <> 'blocked' AND "pricing_stage5_runs_v2"."blocker_count" = 0)
    )
  )
);
--> statement-breakpoint
ALTER TABLE "pricing_stage5_blockers_v2" ADD CONSTRAINT "pricing_stage5_blockers_v2_run_fk" FOREIGN KEY ("run_id") REFERENCES "public"."pricing_stage5_runs_v2"("run_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_stage5_prepare_acks_v2" ADD CONSTRAINT "pricing_stage5_prepare_acks_v2_run_fk" FOREIGN KEY ("run_id") REFERENCES "public"."pricing_stage5_runs_v2"("run_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "pricing_stage5_blockers_v2_subject_idx" ON "pricing_stage5_blockers_v2" USING btree ("run_id","blocker_context","subject_id");