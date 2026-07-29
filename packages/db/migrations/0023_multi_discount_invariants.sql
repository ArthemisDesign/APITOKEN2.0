-- The 0022 tables have no writers yet. Refuse an out-of-order rollout before adding the
-- mandatory relational pins; legacy scalar pricing tables are intentionally untouched.
DO $block$
DECLARE
	table_name text;
	has_rows boolean;
BEGIN
	FOREACH table_name IN ARRAY ARRAY[
		'account_policy_bindings',
		'account_policy_reconciliations',
		'account_policy_rules',
		'account_policy_versions',
		'business_invite_policy_bindings',
		'engine_catalog_jobs',
		'engine_policy_jobs',
		'engine_switch_jobs',
		'pricing_policies',
		'pricing_policy_heads',
		'pricing_policy_rules',
		'pricing_policy_versions',
		'pricing_usage_attributions',
		'pricing_usage_funding_allocations',
		'product_catalog_entries',
		'product_catalog_heads',
		'product_catalog_versions',
		'provider_capability_aliases',
		'provider_capability_entries',
		'provider_capability_head',
		'provider_capability_versions',
		'provider_switch_entries',
		'provider_switch_head',
		'provider_switch_versions'
	]
	LOOP
		EXECUTE format(
			'LOCK TABLE public.%I IN SHARE ROW EXCLUSIVE MODE NOWAIT',
			table_name
		);
		EXECUTE format(
			'SELECT EXISTS (SELECT 1 FROM public.%I LIMIT 1)',
			table_name
		)
		INTO has_rows;

		IF has_rows THEN
			RAISE EXCEPTION USING
				ERRCODE = '23514',
				CONSTRAINT = 'multi_discount_invariants_empty_preflight',
				MESSAGE = format(
					'0023 requires empty pre-writer table public.%I; manual audit required',
					table_name
				);
		END IF;
	END LOOP;
END;
$block$;
--> statement-breakpoint

ALTER TABLE "account_policy_bindings" DROP CONSTRAINT "account_policy_bindings_enforcement_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_funding_allocations" DROP CONSTRAINT "pricing_usage_funding_allocations_shape_check";--> statement-breakpoint
ALTER TABLE "provider_switch_entries" DROP CONSTRAINT "provider_switch_entries_scope_check";--> statement-breakpoint
ALTER TABLE "provider_switch_versions" DROP CONSTRAINT "provider_switch_versions_identity_check";--> statement-breakpoint
ALTER TABLE "pricing_usage_funding_allocations" ALTER COLUMN "engine_bucket_id" SET NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "binding_id" uuid;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD COLUMN "effective_policy_digest" text;--> statement-breakpoint
ALTER TABLE "provider_switch_entries" ADD COLUMN "catalog_generation" bigint;--> statement-breakpoint
ALTER TABLE "provider_switch_versions" ADD COLUMN "capability_generation" bigint NOT NULL;--> statement-breakpoint
ALTER TABLE "provider_switch_versions" ADD COLUMN "capability_digest" text NOT NULL;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_effective_fk" FOREIGN KEY ("binding_id","effective_policy_version","effective_policy_digest") REFERENCES "public"."account_policy_versions"("binding_id","effective_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_switch_entries" ADD CONSTRAINT "provider_switch_entries_catalog_fk" FOREIGN KEY ("product_id","catalog_generation") REFERENCES "public"."product_catalog_versions"("product_id","generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_switch_versions" ADD CONSTRAINT "provider_switch_versions_capability_fk" FOREIGN KEY ("capability_generation","capability_digest") REFERENCES "public"."provider_capability_versions"("generation","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_enforcement_check" CHECK (
    "account_policy_bindings"."policy_enforcement" IN ('legacy_scalar', 'shadow', 'strict')
    AND "account_policy_bindings"."funding_enforcement" IN ('legacy_single', 'shadow', 'strict')
    AND "account_policy_bindings"."reconciliation_state" IN ('pending', 'verified', 'exception')
    AND "account_policy_bindings"."sync_state" IN ('legacy', 'pending', 'confirmed', 'failed')
    AND (
      "account_policy_bindings"."policy_enforcement" = 'legacy_scalar'
      OR "account_policy_bindings"."desired_effective_version" IS NOT NULL
    )
    AND (
      "account_policy_bindings"."policy_enforcement" <> 'strict'
      OR (
        "account_policy_bindings"."applied_effective_version" IS NOT NULL
        AND "account_policy_bindings"."reconciliation_state" = 'verified'
      )
    )
    AND (
      "account_policy_bindings"."funding_enforcement" <> 'strict'
      OR "account_policy_bindings"."reconciliation_state" = 'verified'
    )
    AND (
      "account_policy_bindings"."applied_effective_version" IS NULL
      OR "account_policy_bindings"."desired_effective_version" IS NULL
      OR "account_policy_bindings"."applied_effective_version" <= "account_policy_bindings"."desired_effective_version"
    )
    AND (
      "account_policy_bindings"."sync_state" <> 'confirmed'
      OR (
        "account_policy_bindings"."desired_effective_version" IS NOT NULL
        AND "account_policy_bindings"."applied_effective_version" IS NOT NULL
        AND "account_policy_bindings"."applied_effective_version" = "account_policy_bindings"."desired_effective_version"
        AND "account_policy_bindings"."desired_digest" IS NOT NULL
        AND "account_policy_bindings"."applied_digest" IS NOT NULL
        AND "account_policy_bindings"."applied_digest" = "account_policy_bindings"."desired_digest"
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
      "pricing_usage_attributions"."snapshot_kind" IN ('legacy_scalar', 'legacy_b2c_track')
      AND "pricing_usage_attributions"."binding_id" IS NULL
      AND "pricing_usage_attributions"."effective_policy_digest" IS NULL
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_policy_funding_check" CHECK (
    "pricing_usage_attributions"."snapshot_kind" <> 'policy_v1'
    OR (
      "pricing_usage_attributions"."paid_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."bonus_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."other_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."funding_allocation_json" IS NOT NULL
    )
  );--> statement-breakpoint
ALTER TABLE "pricing_usage_funding_allocations" ADD CONSTRAINT "pricing_usage_funding_allocations_shape_check" CHECK (
    "pricing_usage_funding_allocations"."ordinal" >= 0
    AND "pricing_usage_funding_allocations"."bucket_version" > 0
    AND "pricing_usage_funding_allocations"."source_type" <> ''
    AND "pricing_usage_funding_allocations"."amount_nano" > 0
    AND "pricing_usage_funding_allocations"."engine_bucket_id" <> ''
  );--> statement-breakpoint
ALTER TABLE "provider_switch_entries" ADD CONSTRAINT "provider_switch_entries_scope_check" CHECK (
    (
      "provider_switch_entries"."scope_type" = 'master'
      AND "provider_switch_entries"."product_id" = ''
      AND "provider_switch_entries"."segment" = ''
      AND "provider_switch_entries"."catalog_generation" IS NULL
    )
    OR (
      "provider_switch_entries"."scope_type" = 'product'
      AND "provider_switch_entries"."product_id" <> ''
      AND "provider_switch_entries"."segment" = ''
      AND "provider_switch_entries"."catalog_generation" IS NOT NULL
      AND "provider_switch_entries"."catalog_generation" > 0
    )
    OR (
      "provider_switch_entries"."scope_type" = 'segment'
      AND "provider_switch_entries"."product_id" = 'main'
      AND "provider_switch_entries"."segment" IN ('b2c', 'b2b')
      AND "provider_switch_entries"."catalog_generation" IS NOT NULL
      AND "provider_switch_entries"."catalog_generation" > 0
    )
  );--> statement-breakpoint
ALTER TABLE "provider_switch_versions" ADD CONSTRAINT "provider_switch_versions_identity_check" CHECK (
    "provider_switch_versions"."generation" > 0
    AND "provider_switch_versions"."schema_version" > 0
    AND "provider_switch_versions"."capability_generation" > 0
    AND "provider_switch_versions"."capability_digest" <> ''
    AND "provider_switch_versions"."content_digest" <> ''
    AND "provider_switch_versions"."actor_type" <> ''
    AND "provider_switch_versions"."reason" <> ''
  );
