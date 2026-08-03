ALTER TABLE "pricing_usage_attributions" DROP CONSTRAINT "pricing_usage_attributions_base_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" DROP CONSTRAINT "pricing_usage_attributions_snapshot_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" DROP CONSTRAINT "pricing_usage_attributions_effective_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" DROP CONSTRAINT "pricing_usage_attributions_policy_funding_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "release_schema_version" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "release_generation" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "release_digest" text;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "release_billing_mode" text;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "release_funding_generation" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_base_check" CHECK (
    "pricing_usage_attributions"."attribution_schema_version" > 0
    AND "pricing_usage_attributions"."snapshot_kind" IN ('policy_v1', 'legacy_scalar', 'legacy_b2c_track', 'release_v2')
    AND "pricing_usage_attributions"."pricing_mode" IN ('track', 'discount', 'legacy_scalar')
    AND "pricing_usage_attributions"."rule_origin" IN ('managed', 'legacy')
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
  );--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_snapshot_check" CHECK (
    (
      "pricing_usage_attributions"."snapshot_kind" = 'policy_v1'
      AND "pricing_usage_attributions"."engine_request_id" IS NOT NULL AND "pricing_usage_attributions"."engine_request_id" <> ''
      AND "pricing_usage_attributions"."provider_id" IS NOT NULL AND "pricing_usage_attributions"."provider_id" <> ''
      AND "pricing_usage_attributions"."product_id" IS NOT NULL AND "pricing_usage_attributions"."product_id" <> ''
      AND "pricing_usage_attributions"."account_class" IS NOT NULL
      AND "pricing_usage_attributions"."account_class" IN ('b2c', 'b2b', 'service')
      AND "pricing_usage_attributions"."requested_model_id" IS NOT NULL AND "pricing_usage_attributions"."requested_model_id" <> ''
      AND "pricing_usage_attributions"."canonical_model_id" IS NOT NULL AND "pricing_usage_attributions"."canonical_model_id" <> ''
      AND ("pricing_usage_attributions"."served_model_id" IS NULL OR "pricing_usage_attributions"."served_model_id" <> '')
      AND (
        "pricing_usage_attributions"."served_canonical_model_id" IS NULL
        OR "pricing_usage_attributions"."served_canonical_model_id" <> ''
      )
      AND (
        "pricing_usage_attributions"."billing_invariant_code" IS NULL
        OR "pricing_usage_attributions"."billing_invariant_code" <> ''
      )
      AND "pricing_usage_attributions"."alias_generation" IS NOT NULL
      AND "pricing_usage_attributions"."alias_generation" > 0
      AND "pricing_usage_attributions"."rule_id" IS NOT NULL AND "pricing_usage_attributions"."rule_id" <> ''
      AND "pricing_usage_attributions"."rule_digest" IS NOT NULL AND "pricing_usage_attributions"."rule_digest" <> ''
      AND "pricing_usage_attributions"."rule_scope" IS NOT NULL
      AND "pricing_usage_attributions"."rule_scope" IN ('provider', 'model')
      AND "pricing_usage_attributions"."policy_id" IS NOT NULL AND "pricing_usage_attributions"."policy_id" <> ''
      AND "pricing_usage_attributions"."policy_version" IS NOT NULL
      AND "pricing_usage_attributions"."policy_version" > 0
      AND "pricing_usage_attributions"."effective_policy_version" IS NOT NULL
      AND "pricing_usage_attributions"."effective_policy_version" > 0
      AND "pricing_usage_attributions"."policy_digest" IS NOT NULL AND "pricing_usage_attributions"."policy_digest" <> ''
      AND "pricing_usage_attributions"."catalog_generation" IS NOT NULL
      AND "pricing_usage_attributions"."catalog_generation" > 0
      AND "pricing_usage_attributions"."switch_generation" IS NOT NULL
      AND "pricing_usage_attributions"."switch_generation" > 0
      AND "pricing_usage_attributions"."tariff_schedule_id" IS NOT NULL AND "pricing_usage_attributions"."tariff_schedule_id" <> ''
      AND "pricing_usage_attributions"."tariff_priced_at" IS NOT NULL
      AND "pricing_usage_attributions"."official_nano" IS NOT NULL
      AND "pricing_usage_attributions"."official_cost_json" IS NOT NULL
      AND "pricing_usage_attributions"."payable_multiplier_bp" IS NOT NULL
      AND (
        (
          "pricing_usage_attributions"."pricing_mode" = 'track'
          AND "pricing_usage_attributions"."rule_origin" = 'managed'
          AND "pricing_usage_attributions"."discount_bps" IS NULL
          AND "pricing_usage_attributions"."track_eligible"
          AND "pricing_usage_attributions"."retention_eligible"
        )
        OR (
          "pricing_usage_attributions"."pricing_mode" = 'discount'
          AND "pricing_usage_attributions"."rule_origin" = 'managed'
          AND "pricing_usage_attributions"."discount_bps" IS NOT NULL
          AND "pricing_usage_attributions"."payable_multiplier_bp" = 10000 - "pricing_usage_attributions"."discount_bps"
          AND NOT "pricing_usage_attributions"."track_eligible"
          AND NOT "pricing_usage_attributions"."retention_eligible"
          AND NOT "pricing_usage_attributions"."commission_eligible"
        )
        OR (
          "pricing_usage_attributions"."pricing_mode" = 'discount'
          AND "pricing_usage_attributions"."rule_origin" = 'legacy'
          AND "pricing_usage_attributions"."discount_bps" IS NULL
          AND "pricing_usage_attributions"."payable_multiplier_bp" BETWEEN 1 AND 10000
          AND NOT "pricing_usage_attributions"."track_eligible"
          AND NOT "pricing_usage_attributions"."retention_eligible"
          AND NOT "pricing_usage_attributions"."commission_eligible"
        )
      )
    )
    OR (
      "pricing_usage_attributions"."snapshot_kind" = 'legacy_scalar'
      AND "pricing_usage_attributions"."pricing_mode" = 'legacy_scalar'
      AND "pricing_usage_attributions"."rule_origin" = 'legacy'
      AND "pricing_usage_attributions"."discount_bps" IS NULL
      AND "pricing_usage_attributions"."payable_multiplier_bp" IS NOT NULL
      AND "pricing_usage_attributions"."payable_multiplier_bp" BETWEEN 0 AND 10000
      AND "pricing_usage_attributions"."policy_id" IS NULL
      AND "pricing_usage_attributions"."policy_version" IS NULL
      AND "pricing_usage_attributions"."effective_policy_version" IS NULL
      AND "pricing_usage_attributions"."policy_digest" IS NULL
      AND "pricing_usage_attributions"."catalog_generation" IS NULL
      AND "pricing_usage_attributions"."switch_generation" IS NULL
      AND NOT "pricing_usage_attributions"."track_eligible"
      AND NOT "pricing_usage_attributions"."retention_eligible"
      AND NOT "pricing_usage_attributions"."commission_eligible"
    )
    OR (
      "pricing_usage_attributions"."snapshot_kind" = 'legacy_b2c_track'
      AND "pricing_usage_attributions"."pricing_mode" = 'track'
      AND "pricing_usage_attributions"."rule_origin" = 'legacy'
      AND "pricing_usage_attributions"."discount_bps" IS NULL
      AND "pricing_usage_attributions"."policy_id" IS NULL
      AND "pricing_usage_attributions"."policy_version" IS NULL
      AND "pricing_usage_attributions"."effective_policy_version" IS NULL
      AND "pricing_usage_attributions"."policy_digest" IS NULL
      AND "pricing_usage_attributions"."catalog_generation" IS NULL
      AND "pricing_usage_attributions"."switch_generation" IS NULL
      AND "pricing_usage_attributions"."track_eligible"
      AND "pricing_usage_attributions"."retention_eligible"
    )
    OR (
      "pricing_usage_attributions"."snapshot_kind" = 'release_v2'
      AND "pricing_usage_attributions"."engine_request_id" IS NOT NULL AND "pricing_usage_attributions"."engine_request_id" <> ''
      AND "pricing_usage_attributions"."provider_id" IS NOT NULL AND "pricing_usage_attributions"."provider_id" <> ''
      AND "pricing_usage_attributions"."account_class" IS NOT NULL
      AND "pricing_usage_attributions"."account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
      AND "pricing_usage_attributions"."requested_model_id" IS NOT NULL AND "pricing_usage_attributions"."requested_model_id" <> ''
      AND "pricing_usage_attributions"."canonical_model_id" IS NOT NULL AND "pricing_usage_attributions"."canonical_model_id" <> ''
      AND ("pricing_usage_attributions"."served_model_id" IS NULL OR "pricing_usage_attributions"."served_model_id" <> '')
      AND (
        "pricing_usage_attributions"."served_canonical_model_id" IS NULL
        OR "pricing_usage_attributions"."served_canonical_model_id" <> ''
      )
      AND (
        "pricing_usage_attributions"."rule_id" IS NULL
        OR (
          "pricing_usage_attributions"."rule_id" <> ''
          AND "pricing_usage_attributions"."rule_digest" IS NOT NULL AND "pricing_usage_attributions"."rule_digest" <> ''
          AND "pricing_usage_attributions"."rule_scope" IS NOT NULL
          AND "pricing_usage_attributions"."rule_scope" IN ('global', 'provider', 'model')
          AND "pricing_usage_attributions"."payable_multiplier_bp" IS NOT NULL
          AND (
            "pricing_usage_attributions"."discount_bps" IS NULL
            OR "pricing_usage_attributions"."payable_multiplier_bp" = 10000 - "pricing_usage_attributions"."discount_bps"
          )
        )
      )
      AND "pricing_usage_attributions"."policy_id" IS NOT NULL AND "pricing_usage_attributions"."policy_id" <> ''
      AND "pricing_usage_attributions"."policy_version" IS NOT NULL
      AND "pricing_usage_attributions"."policy_version" > 0
      AND "pricing_usage_attributions"."policy_digest" IS NOT NULL AND "pricing_usage_attributions"."policy_digest" <> ''
      AND "pricing_usage_attributions"."tariff_schedule_id" IS NOT NULL AND "pricing_usage_attributions"."tariff_schedule_id" <> ''
      AND "pricing_usage_attributions"."tariff_priced_at" IS NOT NULL
      AND "pricing_usage_attributions"."official_nano" IS NOT NULL
      AND "pricing_usage_attributions"."official_cost_json" IS NOT NULL
      AND "pricing_usage_attributions"."pricing_mode" IS NULL
      AND "pricing_usage_attributions"."rule_origin" IS NULL
      AND NOT "pricing_usage_attributions"."track_eligible"
      AND NOT "pricing_usage_attributions"."retention_eligible"
      AND "pricing_usage_attributions"."release_schema_version" IS NOT NULL
      AND "pricing_usage_attributions"."release_schema_version" >= 2
      AND "pricing_usage_attributions"."release_generation" IS NOT NULL
      AND "pricing_usage_attributions"."release_generation" > 0
      AND "pricing_usage_attributions"."release_digest" IS NOT NULL AND "pricing_usage_attributions"."release_digest" <> ''
      AND "pricing_usage_attributions"."release_billing_mode" IS NOT NULL
      AND "pricing_usage_attributions"."release_billing_mode" IN ('balance', 'meter_only')
      AND (
        (
          "pricing_usage_attributions"."release_billing_mode" = 'balance'
          AND "pricing_usage_attributions"."release_funding_generation" IS NOT NULL
          AND "pricing_usage_attributions"."release_funding_generation" > 0
        )
        OR (
          "pricing_usage_attributions"."release_billing_mode" = 'meter_only'
          AND "pricing_usage_attributions"."release_funding_generation" IS NULL
        )
      )
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_effective_check" CHECK (
    (
      "pricing_usage_attributions"."snapshot_kind" = 'policy_v1'
      AND "pricing_usage_attributions"."binding_id" IS NOT NULL
      AND "pricing_usage_attributions"."effective_policy_version" IS NOT NULL
      AND "pricing_usage_attributions"."effective_policy_digest" IS NOT NULL
      AND "pricing_usage_attributions"."effective_policy_digest" <> ''
    )
    OR (
      "pricing_usage_attributions"."snapshot_kind" IN ('legacy_scalar', 'legacy_b2c_track', 'release_v2')
      AND "pricing_usage_attributions"."binding_id" IS NULL
      AND "pricing_usage_attributions"."effective_policy_digest" IS NULL
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_policy_funding_check" CHECK (
    "pricing_usage_attributions"."snapshot_kind" NOT IN ('policy_v1', 'release_v2')
    OR (
      "pricing_usage_attributions"."paid_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."bonus_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."other_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."funding_allocation_json" IS NOT NULL
    )
  );