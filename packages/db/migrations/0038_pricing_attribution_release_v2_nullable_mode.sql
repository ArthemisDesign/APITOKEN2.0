ALTER TABLE "pricing_usage_attributions" DROP CONSTRAINT "pricing_usage_attributions_base_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ALTER COLUMN "pricing_mode" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ALTER COLUMN "rule_origin" DROP NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_base_check" CHECK (
    "pricing_usage_attributions"."attribution_schema_version" > 0
    AND "pricing_usage_attributions"."snapshot_kind" IN ('policy_v1', 'legacy_scalar', 'legacy_b2c_track', 'release_v2')
    AND ("pricing_usage_attributions"."pricing_mode" IS NULL OR "pricing_usage_attributions"."pricing_mode" IN ('track', 'discount', 'legacy_scalar'))
    AND ("pricing_usage_attributions"."rule_origin" IS NULL OR "pricing_usage_attributions"."rule_origin" IN ('managed', 'legacy'))
    AND "pricing_usage_attributions"."charged_nano" > 0
    AND "pricing_usage_attributions"."snapshot_digest" <> ''
    AND ("pricing_usage_attributions"."engine_request_id" IS NULL OR "pricing_usage_attributions"."engine_request_id" <> '')
    AND ("pricing_usage_attributions"."served_model_id" IS NULL OR "pricing_usage_attributions"."served_model_id" <> '')
    AND (
      "pricing_usage_attributions"."served_canonical_model_id" IS NULL
      OR "pricing_usage_attributions"."served_canonical_model_id" <> ''
    )
    AND (
      "pricing_usage_attributions"."billing_invariant_code" IS NULL
      OR "pricing_usage_attributions"."billing_invariant_code" <> ''
    )
    AND ("pricing_usage_attributions"."official_nano" IS NULL OR "pricing_usage_attributions"."official_nano" >= 0)
    AND (
      "pricing_usage_attributions"."payable_multiplier_bp" IS NULL
      OR "pricing_usage_attributions"."payable_multiplier_bp" BETWEEN 0 AND 10000
    )
    AND (
      "pricing_usage_attributions"."discount_bps" IS NULL
      OR (
        "pricing_usage_attributions"."discount_bps" BETWEEN 0 AND 9500
        AND "pricing_usage_attributions"."discount_bps" % 100 = 0
      )
    )
    AND (
      "pricing_usage_attributions"."official_cost_json" IS NULL
      OR jsonb_typeof("pricing_usage_attributions"."official_cost_json") = 'object'
    )
    AND (NOT "pricing_usage_attributions"."commission_eligible" OR "pricing_usage_attributions"."track_eligible"
      OR "pricing_usage_attributions"."snapshot_kind" = 'release_v2')
  );