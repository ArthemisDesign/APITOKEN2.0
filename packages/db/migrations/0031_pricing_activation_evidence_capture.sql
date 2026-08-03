ALTER TABLE "pricing_release_activation_receipts_v2" ADD COLUMN "receipt_payload" jsonb;--> statement-breakpoint
ALTER TABLE "pricing_release_control_jobs_v2" ADD COLUMN "activation_payload" jsonb;--> statement-breakpoint
ALTER TABLE "pricing_stage8_evidence_v2" ADD COLUMN "engine_evidence_digest" text;--> statement-breakpoint
ALTER TABLE "pricing_stage8_evidence_v2" ADD COLUMN "engine_captured_at" timestamp with time zone;