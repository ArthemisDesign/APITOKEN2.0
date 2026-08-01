ALTER TABLE "pricing_usage_attributions" ADD COLUMN "source_policy_digest" text;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "admission_catalog_generation" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "admission_catalog_digest" text;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "admission_switch_generation" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "admission_switch_digest" text;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "runtime_manifest_generation" bigint;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "runtime_manifest_digest" text;
