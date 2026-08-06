ALTER TABLE "pricing_release_activation_receipts_v2" DROP CONSTRAINT "pricing_release_activation_receipts_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_release_control_jobs_v2" DROP CONSTRAINT "pricing_release_control_jobs_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_release_activation_receipts_v2" ADD CONSTRAINT "pricing_release_activation_receipts_v2_shape_check" CHECK (
    "pricing_release_activation_receipts_v2"."activation_id" <> ''
    AND "pricing_release_activation_receipts_v2"."activation_kind" IN ('cutover', 'recovery', 'successor')
    AND "pricing_release_activation_receipts_v2"."head_version" > 0
    AND "pricing_release_activation_receipts_v2"."receipt_digest" <> ''
  );--> statement-breakpoint
ALTER TABLE "pricing_release_control_jobs_v2" ADD CONSTRAINT "pricing_release_control_jobs_v2_shape_check" CHECK (
    "pricing_release_control_jobs_v2"."release_digest" <> ''
    AND "pricing_release_control_jobs_v2"."idempotency_key" <> ''
    AND "pricing_release_control_jobs_v2"."payload_digest" <> ''
    AND "pricing_release_control_jobs_v2"."attempts" >= 0
    AND ("pricing_release_control_jobs_v2"."expected_head_version" IS NULL OR "pricing_release_control_jobs_v2"."expected_head_version" >= 0)
    AND "pricing_release_control_jobs_v2"."job_kind" IN (
      'materialize_release',
      'normalize_funding',
      'collect_stage8',
      'activate_release',
      'activate_recovery',
      'activate_successor'
    )
    AND "pricing_release_control_jobs_v2"."status" IN ('pending', 'processing', 'retry', 'confirmed', 'dead')
    AND (
      "pricing_release_control_jobs_v2"."job_kind" NOT IN ('activate_release', 'activate_recovery', 'activate_successor')
      OR ("pricing_release_control_jobs_v2"."expected_head_version" IS NOT NULL AND "pricing_release_control_jobs_v2"."stage8_evidence_digest" IS NOT NULL)
    )
    AND (
      ("pricing_release_control_jobs_v2"."status" = 'confirmed' AND "pricing_release_control_jobs_v2"."result_digest" IS NOT NULL AND "pricing_release_control_jobs_v2"."confirmed_at" IS NOT NULL)
      OR ("pricing_release_control_jobs_v2"."status" <> 'confirmed' AND "pricing_release_control_jobs_v2"."confirmed_at" IS NULL)
    )
  );