ALTER TABLE "pricing_stage8_evidence_v2" DROP CONSTRAINT "pricing_stage8_evidence_v2_shape_check";--> statement-breakpoint
ALTER TABLE "pricing_stage8_evidence_v2" ADD CONSTRAINT "pricing_stage8_evidence_v2_shape_check" CHECK (
    "pricing_stage8_evidence_v2"."evidence_digest" <> ''
    AND "pricing_stage8_evidence_v2"."commerce_inventory_digest" <> ''
    AND "pricing_stage8_evidence_v2"."engine_inventory_digest" <> ''
    AND "pricing_stage8_evidence_v2"."openkeys_inventory_digest" <> ''
    AND "pricing_stage8_evidence_v2"."sales_contract_digest" <> ''
    AND "pricing_stage8_evidence_v2"."funding_digest" <> ''
    AND "pricing_stage8_evidence_v2"."shadow_digest" <> ''
    AND "pricing_stage8_evidence_v2"."runtime_floor_digest" <> ''
    AND "pricing_stage8_evidence_v2"."legacy_inflight_count" >= 0
    AND "pricing_stage8_evidence_v2"."blocker_count" >= 0
    AND "pricing_stage8_evidence_v2"."valid_until" > "pricing_stage8_evidence_v2"."observed_at"
    AND (("pricing_stage8_evidence_v2"."passed" AND "pricing_stage8_evidence_v2"."blocker_count" = 0) OR NOT "pricing_stage8_evidence_v2"."passed")
  );