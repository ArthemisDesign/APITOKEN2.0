CREATE TABLE "pricing_stage8_capture_artifacts_v2" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"job_id" uuid NOT NULL,
	"attempt" integer NOT NULL,
	"engine_evidence_digest" text NOT NULL,
	"engine_captured_at" timestamp with time zone NOT NULL,
	"engine_payload_json" text NOT NULL,
	"combined_evidence_digest" text,
	"combined_payload_json" text,
	"combined_passed" boolean,
	"combined_write_result" text,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_stage8_capture_artifacts_v2_job_attempt_unique" UNIQUE("job_id","attempt"),
	CONSTRAINT "pricing_stage8_capture_artifacts_v2_shape_check" CHECK (
    "pricing_stage8_capture_artifacts_v2"."attempt" > 0
    AND "pricing_stage8_capture_artifacts_v2"."engine_evidence_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage8_capture_artifacts_v2"."engine_payload_json" <> ''
    AND (
      (
        "pricing_stage8_capture_artifacts_v2"."combined_evidence_digest" IS NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_payload_json" IS NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_passed" IS NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_write_result" IS NULL
        AND "pricing_stage8_capture_artifacts_v2"."completed_at" IS NULL
      )
      OR (
        "pricing_stage8_capture_artifacts_v2"."combined_evidence_digest" IS NOT NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_evidence_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
        AND "pricing_stage8_capture_artifacts_v2"."combined_payload_json" IS NOT NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_payload_json" <> ''
        AND "pricing_stage8_capture_artifacts_v2"."combined_passed" IS NOT NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_write_result" IS NOT NULL
        AND "pricing_stage8_capture_artifacts_v2"."combined_write_result" IN ('stored', 'unchanged', 'not_persisted')
        AND "pricing_stage8_capture_artifacts_v2"."completed_at" IS NOT NULL
      )
    )
  )
);
--> statement-breakpoint
CREATE TABLE "pricing_stage8_capture_jobs_v2" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"idempotency_key" text NOT NULL,
	"request_digest" text NOT NULL,
	"target_generation" bigint NOT NULL,
	"recovery_generation" bigint NOT NULL,
	"window_start_at" timestamp with time zone NOT NULL,
	"window_end_at" timestamp with time zone NOT NULL,
	"min_samples_per_provider" bigint NOT NULL,
	"financial_sample_size" integer NOT NULL,
	"gemini_client_admissions" bigint NOT NULL,
	"operator_id" text NOT NULL,
	"reason" text NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"result_engine_evidence_digest" text,
	"result_combined_evidence_digest" text,
	"result_passed" boolean,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_stage8_capture_jobs_v2_idempotency_key_unique" UNIQUE("idempotency_key"),
	CONSTRAINT "pricing_stage8_capture_jobs_v2_shape_check" CHECK (
    "pricing_stage8_capture_jobs_v2"."idempotency_key" <> ''
    AND "pricing_stage8_capture_jobs_v2"."request_digest" ~ '^sha256:v2:[0-9a-f]{64}$'
    AND "pricing_stage8_capture_jobs_v2"."target_generation" > 0
    AND "pricing_stage8_capture_jobs_v2"."recovery_generation" > "pricing_stage8_capture_jobs_v2"."target_generation"
    AND "pricing_stage8_capture_jobs_v2"."window_end_at" > "pricing_stage8_capture_jobs_v2"."window_start_at"
    AND "pricing_stage8_capture_jobs_v2"."min_samples_per_provider" BETWEEN 1 AND 1000000
    AND "pricing_stage8_capture_jobs_v2"."financial_sample_size" BETWEEN 1 AND 1000
    AND "pricing_stage8_capture_jobs_v2"."gemini_client_admissions" >= 0
    AND "pricing_stage8_capture_jobs_v2"."operator_id" <> ''
    AND "pricing_stage8_capture_jobs_v2"."reason" <> ''
    AND "pricing_stage8_capture_jobs_v2"."attempts" >= 0
    AND "pricing_stage8_capture_jobs_v2"."status" IN ('pending', 'processing', 'retry', 'passed', 'blocked', 'dead')
    AND (
      (
        "pricing_stage8_capture_jobs_v2"."result_engine_evidence_digest" IS NULL
        AND "pricing_stage8_capture_jobs_v2"."result_combined_evidence_digest" IS NULL
        AND "pricing_stage8_capture_jobs_v2"."result_passed" IS NULL
      )
      OR (
        "pricing_stage8_capture_jobs_v2"."result_engine_evidence_digest" IS NOT NULL
        AND "pricing_stage8_capture_jobs_v2"."result_engine_evidence_digest" <> ''
        AND "pricing_stage8_capture_jobs_v2"."result_combined_evidence_digest" IS NOT NULL
        AND "pricing_stage8_capture_jobs_v2"."result_combined_evidence_digest" <> ''
        AND "pricing_stage8_capture_jobs_v2"."result_passed" IS NOT NULL
      )
    )
    AND (
      (
        "pricing_stage8_capture_jobs_v2"."status" IN ('pending', 'processing', 'retry')
        AND "pricing_stage8_capture_jobs_v2"."completed_at" IS NULL
        AND "pricing_stage8_capture_jobs_v2"."result_engine_evidence_digest" IS NULL
      )
      OR (
        "pricing_stage8_capture_jobs_v2"."status" = 'passed'
        AND "pricing_stage8_capture_jobs_v2"."completed_at" IS NOT NULL
        AND "pricing_stage8_capture_jobs_v2"."result_passed" = true
      )
      OR (
        "pricing_stage8_capture_jobs_v2"."status" = 'blocked'
        AND "pricing_stage8_capture_jobs_v2"."completed_at" IS NOT NULL
        AND "pricing_stage8_capture_jobs_v2"."result_passed" = false
      )
      OR (
        "pricing_stage8_capture_jobs_v2"."status" = 'dead'
        AND "pricing_stage8_capture_jobs_v2"."completed_at" IS NOT NULL
        AND "pricing_stage8_capture_jobs_v2"."last_error" IS NOT NULL
      )
    )
  )
);
--> statement-breakpoint
ALTER TABLE "pricing_stage8_capture_artifacts_v2" ADD CONSTRAINT "pricing_stage8_capture_artifacts_v2_job_fk" FOREIGN KEY ("job_id") REFERENCES "public"."pricing_stage8_capture_jobs_v2"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE INDEX "pricing_stage8_capture_artifacts_v2_job_idx" ON "pricing_stage8_capture_artifacts_v2" USING btree ("job_id","created_at");--> statement-breakpoint
CREATE INDEX "pricing_stage8_capture_jobs_v2_claim_idx" ON "pricing_stage8_capture_jobs_v2" USING btree ("status","next_attempt_at","created_at") WHERE "pricing_stage8_capture_jobs_v2"."status" IN ('pending', 'retry');