ALTER TABLE "pricing_funding_normalizations_v2" DROP CONSTRAINT "pricing_funding_normalizations_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_funding_normalizations_v2" ALTER COLUMN "funding_generation" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_funding_normalizations_v2" ALTER COLUMN "target_funding_digest" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_funding_normalizations_v2" ADD COLUMN "normalization_source" text;--> statement-breakpoint
ALTER TABLE "pricing_funding_normalizations_v2" ADD COLUMN "blockers" jsonb;--> statement-breakpoint
ALTER TABLE "pricing_funding_normalizations_v2" ADD CONSTRAINT "pricing_funding_normalizations_v2_shape_check" CHECK (
    "pricing_funding_normalizations_v2"."engine_account_id" <> ''
    AND ("pricing_funding_normalizations_v2"."funding_generation" IS NULL OR "pricing_funding_normalizations_v2"."funding_generation" > 0)
    AND "pricing_funding_normalizations_v2"."expected_source_digest" <> ''
    AND ("pricing_funding_normalizations_v2"."target_funding_digest" IS NULL OR "pricing_funding_normalizations_v2"."target_funding_digest" <> '')
    AND (
      "pricing_funding_normalizations_v2"."normalization_source" IS NULL
      OR "pricing_funding_normalizations_v2"."normalization_source" IN (
        'aggregate_paid_only', 'ledger_replay', 'legacy_buckets', 'stored_generation'
      )
    )
    AND ("pricing_funding_normalizations_v2"."blockers" IS NULL OR jsonb_typeof("pricing_funding_normalizations_v2"."blockers") = 'array')
    AND "pricing_funding_normalizations_v2"."attempts" >= 0
    AND "pricing_funding_normalizations_v2"."status" IN ('pending', 'processing', 'retry', 'ready', 'blocker')
    AND (
      (
        "pricing_funding_normalizations_v2"."status" = 'ready'
        AND "pricing_funding_normalizations_v2"."funding_generation" IS NOT NULL
        AND "pricing_funding_normalizations_v2"."target_funding_digest" IS NOT NULL
        AND "pricing_funding_normalizations_v2"."applied_funding_digest" = "pricing_funding_normalizations_v2"."target_funding_digest"
        AND "pricing_funding_normalizations_v2"."blockers" IS NULL
      )
      OR ("pricing_funding_normalizations_v2"."status" <> 'ready' AND "pricing_funding_normalizations_v2"."applied_funding_digest" IS NULL)
    )
    AND (
      "pricing_funding_normalizations_v2"."status" <> 'blocker'
      OR "pricing_funding_normalizations_v2"."blockers" IS NULL
      OR jsonb_array_length("pricing_funding_normalizations_v2"."blockers") > 0
    )
  );
