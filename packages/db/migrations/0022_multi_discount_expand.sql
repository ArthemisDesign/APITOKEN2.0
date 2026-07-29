CREATE TABLE "account_policy_bindings" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid,
	"engine_account_record_id" uuid,
	"engine_account_id" text,
	"account_class" text NOT NULL,
	"product_id" text NOT NULL,
	"policy_id" text NOT NULL,
	"desired_effective_version" bigint,
	"desired_digest" text,
	"applied_effective_version" bigint,
	"applied_digest" text,
	"policy_enforcement" text DEFAULT 'legacy_scalar' NOT NULL,
	"funding_enforcement" text DEFAULT 'legacy_single' NOT NULL,
	"reconciliation_state" text DEFAULT 'pending' NOT NULL,
	"sync_state" text DEFAULT 'legacy' NOT NULL,
	"last_ack_at" timestamp with time zone,
	"last_error" text,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "account_policy_bindings_engine_target_unique" UNIQUE("id","engine_account_id"),
	CONSTRAINT "account_policy_bindings_invite_copy_unique" UNIQUE("id","user_id","policy_id"),
	CONSTRAINT "account_policy_bindings_identity_check" CHECK (
    "account_policy_bindings"."product_id" <> ''
    AND "account_policy_bindings"."policy_id" <> ''
    AND (
      (
        "account_policy_bindings"."account_class" IN ('b2c', 'b2b')
        AND "account_policy_bindings"."user_id" IS NOT NULL
        AND "account_policy_bindings"."engine_account_record_id" IS NOT NULL
        AND "account_policy_bindings"."engine_account_id" IS NOT NULL
        AND "account_policy_bindings"."engine_account_id" <> ''
      )
      OR (
        "account_policy_bindings"."account_class" = 'service'
        AND "account_policy_bindings"."user_id" IS NULL
        AND "account_policy_bindings"."engine_account_record_id" IS NULL
        AND "account_policy_bindings"."engine_account_id" IS NOT NULL
        AND "account_policy_bindings"."engine_account_id" <> ''
      )
    )
  ),
	CONSTRAINT "account_policy_bindings_desired_shape_check" CHECK (
    ("account_policy_bindings"."desired_effective_version" IS NULL AND "account_policy_bindings"."desired_digest" IS NULL)
    OR (
      "account_policy_bindings"."desired_effective_version" IS NOT NULL
      AND
      "account_policy_bindings"."desired_effective_version" > 0
      AND "account_policy_bindings"."desired_digest" IS NOT NULL
      AND "account_policy_bindings"."desired_digest" <> ''
    )
  ),
	CONSTRAINT "account_policy_bindings_applied_shape_check" CHECK (
    (
      "account_policy_bindings"."applied_effective_version" IS NULL
      AND "account_policy_bindings"."applied_digest" IS NULL
      AND "account_policy_bindings"."last_ack_at" IS NULL
    )
    OR (
      "account_policy_bindings"."applied_effective_version" IS NOT NULL
      AND
      "account_policy_bindings"."applied_effective_version" > 0
      AND "account_policy_bindings"."applied_digest" IS NOT NULL
      AND "account_policy_bindings"."applied_digest" <> ''
      AND "account_policy_bindings"."last_ack_at" IS NOT NULL
    )
  ),
	CONSTRAINT "account_policy_bindings_enforcement_check" CHECK (
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
        AND "account_policy_bindings"."sync_state" = 'confirmed'
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
  )
);--> statement-breakpoint
CREATE TABLE "account_policy_reconciliations" (
	"id" uuid PRIMARY KEY NOT NULL,
	"binding_id" uuid NOT NULL,
	"effective_version" bigint,
	"scope" text NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"legacy_account_class" text,
	"legacy_multiplier_bp" integer,
	"observed_balance_nano" bigint,
	"observed_reserved_nano" bigint,
	"observed_spent_nano" bigint,
	"expected_digest" text,
	"observed_digest" text,
	"exception_code" text,
	"details" jsonb NOT NULL,
	"started_at" timestamp with time zone DEFAULT now() NOT NULL,
	"completed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "account_policy_reconciliations_shape_check" CHECK (
    "account_policy_reconciliations"."scope" IN ('classification', 'policy', 'funding', 'history')
    AND "account_policy_reconciliations"."status" IN ('pending', 'verified', 'exception')
    AND ("account_policy_reconciliations"."effective_version" IS NULL OR "account_policy_reconciliations"."effective_version" > 0)
    AND (
      "account_policy_reconciliations"."legacy_account_class" IS NULL
      OR "account_policy_reconciliations"."legacy_account_class" IN ('b2c', 'b2b', 'service')
    )
    AND (
      "account_policy_reconciliations"."legacy_multiplier_bp" IS NULL
      OR "account_policy_reconciliations"."legacy_multiplier_bp" BETWEEN 0 AND 10000
    )
    AND (
      "account_policy_reconciliations"."observed_reserved_nano" IS NULL
      OR "account_policy_reconciliations"."observed_reserved_nano" >= 0
    )
    AND (
      "account_policy_reconciliations"."observed_spent_nano" IS NULL
      OR "account_policy_reconciliations"."observed_spent_nano" >= 0
    )
    AND jsonb_typeof("account_policy_reconciliations"."details") = 'object'
    AND (
      ("account_policy_reconciliations"."status" = 'pending' AND "account_policy_reconciliations"."completed_at" IS NULL)
      OR ("account_policy_reconciliations"."status" <> 'pending' AND "account_policy_reconciliations"."completed_at" IS NOT NULL)
    )
    AND (
      (
        "account_policy_reconciliations"."status" = 'exception'
        AND "account_policy_reconciliations"."exception_code" IS NOT NULL
        AND "account_policy_reconciliations"."exception_code" <> ''
      )
      OR ("account_policy_reconciliations"."status" <> 'exception' AND "account_policy_reconciliations"."exception_code" IS NULL)
    )
  )
);--> statement-breakpoint
CREATE TABLE "account_policy_rules" (
	"binding_id" uuid NOT NULL,
	"effective_version" bigint NOT NULL,
	"product_id" text NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"rule_id" text NOT NULL,
	"rule_digest" text NOT NULL,
	"scope_type" text NOT NULL,
	"provider_id" text NOT NULL,
	"canonical_model_id" text,
	"pricing_mode" text NOT NULL,
	"rule_origin" text NOT NULL,
	"discount_bps" integer,
	"payable_multiplier_bp" integer NOT NULL,
	"track_eligible" boolean NOT NULL,
	"retention_eligible" boolean NOT NULL,
	"commission_eligible" boolean NOT NULL,
	CONSTRAINT "account_policy_rules_binding_id_effective_version_rule_id_pk" PRIMARY KEY("binding_id","effective_version","rule_id"),
	CONSTRAINT "account_policy_rules_identity_check" CHECK (
    "account_policy_rules"."product_id" <> ''
    AND "account_policy_rules"."rule_id" <> ''
    AND "account_policy_rules"."rule_digest" <> ''
    AND "account_policy_rules"."provider_id" <> ''
  ),
	CONSTRAINT "account_policy_rules_scope_check" CHECK (
    ("account_policy_rules"."scope_type" = 'provider' AND "account_policy_rules"."canonical_model_id" IS NULL)
    OR (
      "account_policy_rules"."scope_type" = 'model'
      AND "account_policy_rules"."canonical_model_id" IS NOT NULL
      AND "account_policy_rules"."canonical_model_id" <> ''
    )
  ),
	CONSTRAINT "account_policy_rules_pricing_check" CHECK (
    (
      "account_policy_rules"."pricing_mode" = 'track'
      AND "account_policy_rules"."rule_origin" = 'managed'
      AND "account_policy_rules"."discount_bps" IS NULL
      AND "account_policy_rules"."payable_multiplier_bp" BETWEEN 0 AND 10000
      AND "account_policy_rules"."track_eligible"
      AND "account_policy_rules"."retention_eligible"
    )
    OR (
      "account_policy_rules"."pricing_mode" = 'discount'
      AND "account_policy_rules"."rule_origin" = 'managed'
      AND "account_policy_rules"."discount_bps" IS NOT NULL
      AND "account_policy_rules"."discount_bps" BETWEEN 0 AND 9500
      AND "account_policy_rules"."discount_bps" % 100 = 0
      AND "account_policy_rules"."payable_multiplier_bp" = 10000 - "account_policy_rules"."discount_bps"
      AND NOT "account_policy_rules"."track_eligible"
      AND NOT "account_policy_rules"."retention_eligible"
      AND NOT "account_policy_rules"."commission_eligible"
    )
    OR (
      "account_policy_rules"."pricing_mode" = 'discount'
      AND "account_policy_rules"."rule_origin" = 'legacy'
      AND "account_policy_rules"."discount_bps" IS NULL
      AND "account_policy_rules"."payable_multiplier_bp" BETWEEN 1 AND 10000
      AND NOT "account_policy_rules"."track_eligible"
      AND NOT "account_policy_rules"."retention_eligible"
      AND NOT "account_policy_rules"."commission_eligible"
    )
  ),
	CONSTRAINT "account_policy_rules_commission_check" CHECK (
    NOT "account_policy_rules"."commission_eligible" OR "account_policy_rules"."pricing_mode" = 'track'
  )
);--> statement-breakpoint
CREATE TABLE "account_policy_versions" (
	"binding_id" uuid NOT NULL,
	"effective_version" bigint NOT NULL,
	"policy_id" text NOT NULL,
	"policy_version" bigint NOT NULL,
	"policy_digest" text NOT NULL,
	"product_id" text NOT NULL,
	"account_class" text NOT NULL,
	"schema_version" bigint NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"switch_generation" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"replacement_locked" boolean DEFAULT false NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "account_policy_versions_binding_id_effective_version_pk" PRIMARY KEY("binding_id","effective_version"),
	CONSTRAINT "account_policy_versions_binding_product_unique" UNIQUE("binding_id","effective_version","product_id"),
	CONSTRAINT "account_policy_versions_binding_catalog_unique" UNIQUE("binding_id","effective_version","product_id","catalog_generation"),
	CONSTRAINT "account_policy_versions_binding_digest_unique" UNIQUE("binding_id","effective_version","content_digest"),
	CONSTRAINT "account_policy_versions_job_target_unique" UNIQUE("binding_id","effective_version","policy_id","policy_version","catalog_generation","switch_generation","schema_version","content_digest"),
	CONSTRAINT "account_policy_versions_identity_check" CHECK (
    "account_policy_versions"."effective_version" > 0
    AND "account_policy_versions"."policy_id" <> ''
    AND "account_policy_versions"."policy_version" > 0
    AND "account_policy_versions"."policy_digest" <> ''
    AND "account_policy_versions"."product_id" <> ''
    AND "account_policy_versions"."account_class" IN ('b2c', 'b2b', 'service')
    AND "account_policy_versions"."schema_version" > 0
    AND "account_policy_versions"."catalog_generation" > 0
    AND "account_policy_versions"."switch_generation" > 0
    AND "account_policy_versions"."content_digest" <> ''
  )
);--> statement-breakpoint
CREATE TABLE "business_invite_policy_bindings" (
	"invite_id" uuid PRIMARY KEY NOT NULL,
	"invitation_policy_id" text NOT NULL,
	"current_policy_version" bigint NOT NULL,
	"current_policy_digest" text NOT NULL,
	"redeemed_source_policy_version" bigint,
	"redeemed_source_policy_digest" text,
	"copied_to_user_id" uuid,
	"copied_to_binding_id" uuid,
	"copied_client_policy_id" text,
	"copied_client_policy_version" bigint,
	"copied_client_policy_digest" text,
	"redeemed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "business_invite_policy_bindings_current_check" CHECK (
    "business_invite_policy_bindings"."invitation_policy_id" <> ''
    AND "business_invite_policy_bindings"."current_policy_version" > 0
    AND "business_invite_policy_bindings"."current_policy_digest" <> ''
  ),
	CONSTRAINT "business_invite_policy_bindings_redemption_check" CHECK (
    (
      "business_invite_policy_bindings"."redeemed_source_policy_version" IS NULL
      AND "business_invite_policy_bindings"."redeemed_source_policy_digest" IS NULL
      AND "business_invite_policy_bindings"."copied_to_user_id" IS NULL
      AND "business_invite_policy_bindings"."copied_to_binding_id" IS NULL
      AND "business_invite_policy_bindings"."copied_client_policy_id" IS NULL
      AND "business_invite_policy_bindings"."copied_client_policy_version" IS NULL
      AND "business_invite_policy_bindings"."copied_client_policy_digest" IS NULL
      AND "business_invite_policy_bindings"."redeemed_at" IS NULL
    )
    OR (
      "business_invite_policy_bindings"."redeemed_source_policy_version" IS NOT NULL
      AND
      "business_invite_policy_bindings"."redeemed_source_policy_version" > 0
      AND "business_invite_policy_bindings"."redeemed_source_policy_digest" IS NOT NULL
      AND "business_invite_policy_bindings"."redeemed_source_policy_digest" <> ''
      AND "business_invite_policy_bindings"."redeemed_source_policy_version" = "business_invite_policy_bindings"."current_policy_version"
      AND "business_invite_policy_bindings"."redeemed_source_policy_digest" = "business_invite_policy_bindings"."current_policy_digest"
      AND "business_invite_policy_bindings"."copied_to_user_id" IS NOT NULL
      AND "business_invite_policy_bindings"."copied_to_binding_id" IS NOT NULL
      AND "business_invite_policy_bindings"."copied_client_policy_id" IS NOT NULL
      AND "business_invite_policy_bindings"."copied_client_policy_id" <> ''
      AND "business_invite_policy_bindings"."copied_client_policy_version" IS NOT NULL
      AND "business_invite_policy_bindings"."copied_client_policy_version" > 0
      AND "business_invite_policy_bindings"."copied_client_policy_digest" IS NOT NULL
      AND "business_invite_policy_bindings"."copied_client_policy_digest" <> ''
      AND "business_invite_policy_bindings"."redeemed_at" IS NOT NULL
    )
  )
);--> statement-breakpoint
CREATE TABLE "engine_catalog_jobs" (
	"id" uuid PRIMARY KEY NOT NULL,
	"product_id" text NOT NULL,
	"generation" bigint NOT NULL,
	"schema_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"payload" jsonb NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"ack_generation" bigint,
	"ack_schema_version" bigint,
	"ack_content_digest" text,
	"ack_payload" jsonb,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_catalog_jobs_target_check" CHECK (
    "engine_catalog_jobs"."product_id" <> ''
    AND "engine_catalog_jobs"."generation" > 0
    AND "engine_catalog_jobs"."schema_version" > 0
    AND "engine_catalog_jobs"."content_digest" <> ''
    AND jsonb_typeof("engine_catalog_jobs"."payload") = 'object'
  ),
	CONSTRAINT "engine_catalog_jobs_state_check" CHECK (
    "engine_catalog_jobs"."status" IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND "engine_catalog_jobs"."attempts" >= 0
    AND (
      ("engine_catalog_jobs"."status" = 'processing' AND "engine_catalog_jobs"."locked_at" IS NOT NULL AND "engine_catalog_jobs"."locked_by" IS NOT NULL)
      OR ("engine_catalog_jobs"."status" <> 'processing' AND "engine_catalog_jobs"."locked_at" IS NULL AND "engine_catalog_jobs"."locked_by" IS NULL)
    )
  ),
	CONSTRAINT "engine_catalog_jobs_ack_check" CHECK (
    (
      "engine_catalog_jobs"."status" <> 'confirmed'
      AND "engine_catalog_jobs"."ack_generation" IS NULL
      AND "engine_catalog_jobs"."ack_schema_version" IS NULL
      AND "engine_catalog_jobs"."ack_content_digest" IS NULL
      AND "engine_catalog_jobs"."ack_payload" IS NULL
      AND "engine_catalog_jobs"."confirmed_at" IS NULL
    )
    OR (
      "engine_catalog_jobs"."status" = 'confirmed'
      AND "engine_catalog_jobs"."ack_generation" IS NOT NULL
      AND "engine_catalog_jobs"."ack_generation" = "engine_catalog_jobs"."generation"
      AND "engine_catalog_jobs"."ack_schema_version" IS NOT NULL
      AND "engine_catalog_jobs"."ack_schema_version" = "engine_catalog_jobs"."schema_version"
      AND "engine_catalog_jobs"."ack_content_digest" IS NOT NULL
      AND "engine_catalog_jobs"."ack_content_digest" = "engine_catalog_jobs"."content_digest"
      AND "engine_catalog_jobs"."ack_payload" IS NOT NULL
      AND jsonb_typeof("engine_catalog_jobs"."ack_payload") = 'object'
      AND "engine_catalog_jobs"."confirmed_at" IS NOT NULL
    )
  )
);--> statement-breakpoint
CREATE TABLE "engine_policy_jobs" (
	"id" uuid PRIMARY KEY NOT NULL,
	"binding_id" uuid NOT NULL,
	"effective_version" bigint NOT NULL,
	"engine_account_id" text NOT NULL,
	"policy_id" text NOT NULL,
	"policy_version" bigint NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"switch_generation" bigint NOT NULL,
	"schema_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"payload" jsonb NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"ack_effective_version" bigint,
	"ack_policy_version" bigint,
	"ack_catalog_generation" bigint,
	"ack_switch_generation" bigint,
	"ack_schema_version" bigint,
	"ack_content_digest" text,
	"ack_payload" jsonb,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_policy_jobs_target_check" CHECK (
    "engine_policy_jobs"."effective_version" > 0
    AND "engine_policy_jobs"."engine_account_id" <> ''
    AND "engine_policy_jobs"."policy_id" <> ''
    AND "engine_policy_jobs"."policy_version" > 0
    AND "engine_policy_jobs"."catalog_generation" > 0
    AND "engine_policy_jobs"."switch_generation" > 0
    AND "engine_policy_jobs"."schema_version" > 0
    AND "engine_policy_jobs"."content_digest" <> ''
    AND jsonb_typeof("engine_policy_jobs"."payload") = 'object'
  ),
	CONSTRAINT "engine_policy_jobs_state_check" CHECK (
    "engine_policy_jobs"."status" IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND "engine_policy_jobs"."attempts" >= 0
    AND (
      ("engine_policy_jobs"."status" = 'processing' AND "engine_policy_jobs"."locked_at" IS NOT NULL AND "engine_policy_jobs"."locked_by" IS NOT NULL)
      OR ("engine_policy_jobs"."status" <> 'processing' AND "engine_policy_jobs"."locked_at" IS NULL AND "engine_policy_jobs"."locked_by" IS NULL)
    )
  ),
	CONSTRAINT "engine_policy_jobs_ack_check" CHECK (
    (
      "engine_policy_jobs"."status" <> 'confirmed'
      AND "engine_policy_jobs"."ack_effective_version" IS NULL
      AND "engine_policy_jobs"."ack_policy_version" IS NULL
      AND "engine_policy_jobs"."ack_catalog_generation" IS NULL
      AND "engine_policy_jobs"."ack_switch_generation" IS NULL
      AND "engine_policy_jobs"."ack_schema_version" IS NULL
      AND "engine_policy_jobs"."ack_content_digest" IS NULL
      AND "engine_policy_jobs"."ack_payload" IS NULL
      AND "engine_policy_jobs"."confirmed_at" IS NULL
    )
    OR (
      "engine_policy_jobs"."status" = 'confirmed'
      AND "engine_policy_jobs"."ack_effective_version" IS NOT NULL
      AND "engine_policy_jobs"."ack_effective_version" = "engine_policy_jobs"."effective_version"
      AND "engine_policy_jobs"."ack_policy_version" IS NOT NULL
      AND "engine_policy_jobs"."ack_policy_version" = "engine_policy_jobs"."policy_version"
      AND "engine_policy_jobs"."ack_catalog_generation" IS NOT NULL
      AND "engine_policy_jobs"."ack_catalog_generation" = "engine_policy_jobs"."catalog_generation"
      AND "engine_policy_jobs"."ack_switch_generation" IS NOT NULL
      AND "engine_policy_jobs"."ack_switch_generation" = "engine_policy_jobs"."switch_generation"
      AND "engine_policy_jobs"."ack_schema_version" IS NOT NULL
      AND "engine_policy_jobs"."ack_schema_version" = "engine_policy_jobs"."schema_version"
      AND "engine_policy_jobs"."ack_content_digest" IS NOT NULL
      AND "engine_policy_jobs"."ack_content_digest" = "engine_policy_jobs"."content_digest"
      AND "engine_policy_jobs"."ack_payload" IS NOT NULL
      AND jsonb_typeof("engine_policy_jobs"."ack_payload") = 'object'
      AND "engine_policy_jobs"."confirmed_at" IS NOT NULL
    )
  )
);--> statement-breakpoint
CREATE TABLE "engine_switch_jobs" (
	"id" uuid PRIMARY KEY NOT NULL,
	"generation" bigint NOT NULL,
	"schema_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"payload" jsonb NOT NULL,
	"status" text DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"ack_generation" bigint,
	"ack_schema_version" bigint,
	"ack_content_digest" text,
	"ack_payload" jsonb,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_switch_jobs_target_check" CHECK (
    "engine_switch_jobs"."generation" > 0
    AND "engine_switch_jobs"."schema_version" > 0
    AND "engine_switch_jobs"."content_digest" <> ''
    AND jsonb_typeof("engine_switch_jobs"."payload") = 'object'
  ),
	CONSTRAINT "engine_switch_jobs_state_check" CHECK (
    "engine_switch_jobs"."status" IN ('pending', 'processing', 'retry', 'confirmed', 'superseded', 'dead')
    AND "engine_switch_jobs"."attempts" >= 0
    AND (
      ("engine_switch_jobs"."status" = 'processing' AND "engine_switch_jobs"."locked_at" IS NOT NULL AND "engine_switch_jobs"."locked_by" IS NOT NULL)
      OR ("engine_switch_jobs"."status" <> 'processing' AND "engine_switch_jobs"."locked_at" IS NULL AND "engine_switch_jobs"."locked_by" IS NULL)
    )
  ),
	CONSTRAINT "engine_switch_jobs_ack_check" CHECK (
    (
      "engine_switch_jobs"."status" <> 'confirmed'
      AND "engine_switch_jobs"."ack_generation" IS NULL
      AND "engine_switch_jobs"."ack_schema_version" IS NULL
      AND "engine_switch_jobs"."ack_content_digest" IS NULL
      AND "engine_switch_jobs"."ack_payload" IS NULL
      AND "engine_switch_jobs"."confirmed_at" IS NULL
    )
    OR (
      "engine_switch_jobs"."status" = 'confirmed'
      AND "engine_switch_jobs"."ack_generation" IS NOT NULL
      AND "engine_switch_jobs"."ack_generation" = "engine_switch_jobs"."generation"
      AND "engine_switch_jobs"."ack_schema_version" IS NOT NULL
      AND "engine_switch_jobs"."ack_schema_version" = "engine_switch_jobs"."schema_version"
      AND "engine_switch_jobs"."ack_content_digest" IS NOT NULL
      AND "engine_switch_jobs"."ack_content_digest" = "engine_switch_jobs"."content_digest"
      AND "engine_switch_jobs"."ack_payload" IS NOT NULL
      AND jsonb_typeof("engine_switch_jobs"."ack_payload") = 'object'
      AND "engine_switch_jobs"."confirmed_at" IS NOT NULL
    )
  )
);--> statement-breakpoint
CREATE TABLE "pricing_policies" (
	"id" text PRIMARY KEY NOT NULL,
	"owner_type" text NOT NULL,
	"owner_id" text NOT NULL,
	"product_id" text NOT NULL,
	"replacement_locked" boolean DEFAULT false NOT NULL,
	"status" text DEFAULT 'active' NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_policies_product_unique" UNIQUE("id","product_id"),
	CONSTRAINT "pricing_policies_identity_check" CHECK (
    "pricing_policies"."id" <> '' AND "pricing_policies"."owner_id" <> '' AND "pricing_policies"."product_id" <> ''
  ),
	CONSTRAINT "pricing_policies_owner_check" CHECK (
    "pricing_policies"."owner_type" IN ('global_b2c', 'b2b_client', 'b2b_invitation', 'service')
    AND (
      "pricing_policies"."owner_type" = 'service'
      OR "pricing_policies"."product_id" = 'main'
    )
  ),
	CONSTRAINT "pricing_policies_status_check" CHECK ("pricing_policies"."status" IN ('active', 'archived'))
);--> statement-breakpoint
CREATE TABLE "pricing_policy_heads" (
	"policy_id" text PRIMARY KEY NOT NULL,
	"current_version" bigint NOT NULL,
	"current_digest" text NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_policy_heads_identity_check" CHECK (
    "pricing_policy_heads"."policy_id" <> '' AND "pricing_policy_heads"."current_version" > 0 AND "pricing_policy_heads"."current_digest" <> ''
  )
);--> statement-breakpoint
CREATE TABLE "pricing_policy_rules" (
	"policy_id" text NOT NULL,
	"policy_version" bigint NOT NULL,
	"product_id" text NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"rule_id" text NOT NULL,
	"rule_digest" text NOT NULL,
	"scope_type" text NOT NULL,
	"provider_id" text NOT NULL,
	"canonical_model_id" text,
	"pricing_mode" text NOT NULL,
	"rule_origin" text NOT NULL,
	"discount_bps" integer,
	"payable_multiplier_bp" integer,
	"track_eligible" boolean NOT NULL,
	"retention_eligible" boolean NOT NULL,
	"commission_eligible" boolean NOT NULL,
	CONSTRAINT "pricing_policy_rules_policy_id_policy_version_rule_id_pk" PRIMARY KEY("policy_id","policy_version","rule_id"),
	CONSTRAINT "pricing_policy_rules_identity_check" CHECK (
    "pricing_policy_rules"."policy_id" <> ''
    AND "pricing_policy_rules"."product_id" <> ''
    AND "pricing_policy_rules"."rule_id" <> ''
    AND "pricing_policy_rules"."rule_digest" <> ''
    AND "pricing_policy_rules"."provider_id" <> ''
  ),
	CONSTRAINT "pricing_policy_rules_scope_check" CHECK (
    ("pricing_policy_rules"."scope_type" = 'provider' AND "pricing_policy_rules"."canonical_model_id" IS NULL)
    OR (
      "pricing_policy_rules"."scope_type" = 'model'
      AND "pricing_policy_rules"."canonical_model_id" IS NOT NULL
      AND "pricing_policy_rules"."canonical_model_id" <> ''
    )
  ),
	CONSTRAINT "pricing_policy_rules_pricing_check" CHECK (
    (
      "pricing_policy_rules"."pricing_mode" = 'track'
      AND "pricing_policy_rules"."rule_origin" = 'managed'
      AND "pricing_policy_rules"."discount_bps" IS NULL
      AND "pricing_policy_rules"."payable_multiplier_bp" IS NULL
      AND "pricing_policy_rules"."track_eligible"
      AND "pricing_policy_rules"."retention_eligible"
    )
    OR (
      "pricing_policy_rules"."pricing_mode" = 'discount'
      AND "pricing_policy_rules"."rule_origin" = 'managed'
      AND "pricing_policy_rules"."discount_bps" IS NOT NULL
      AND "pricing_policy_rules"."discount_bps" BETWEEN 0 AND 9500
      AND "pricing_policy_rules"."discount_bps" % 100 = 0
      AND "pricing_policy_rules"."payable_multiplier_bp" IS NOT NULL
      AND "pricing_policy_rules"."payable_multiplier_bp" = 10000 - "pricing_policy_rules"."discount_bps"
      AND NOT "pricing_policy_rules"."track_eligible"
      AND NOT "pricing_policy_rules"."retention_eligible"
      AND NOT "pricing_policy_rules"."commission_eligible"
    )
    OR (
      "pricing_policy_rules"."pricing_mode" = 'discount'
      AND "pricing_policy_rules"."rule_origin" = 'legacy'
      AND "pricing_policy_rules"."discount_bps" IS NULL
      AND "pricing_policy_rules"."payable_multiplier_bp" IS NOT NULL
      AND "pricing_policy_rules"."payable_multiplier_bp" BETWEEN 1 AND 10000
      AND NOT "pricing_policy_rules"."track_eligible"
      AND NOT "pricing_policy_rules"."retention_eligible"
      AND NOT "pricing_policy_rules"."commission_eligible"
    )
  ),
	CONSTRAINT "pricing_policy_rules_commission_check" CHECK (
    NOT "pricing_policy_rules"."commission_eligible" OR "pricing_policy_rules"."pricing_mode" = 'track'
  )
);--> statement-breakpoint
CREATE TABLE "pricing_policy_versions" (
	"policy_id" text NOT NULL,
	"version" bigint NOT NULL,
	"schema_version" bigint NOT NULL,
	"product_id" text NOT NULL,
	"catalog_generation" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"actor_type" text NOT NULL,
	"actor_id" text,
	"reason" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_policy_versions_policy_id_version_pk" PRIMARY KEY("policy_id","version"),
	CONSTRAINT "pricing_policy_versions_product_unique" UNIQUE("policy_id","version","product_id"),
	CONSTRAINT "pricing_policy_versions_catalog_unique" UNIQUE("policy_id","version","product_id","catalog_generation"),
	CONSTRAINT "pricing_policy_versions_digest_unique" UNIQUE("policy_id","version","product_id","catalog_generation","content_digest"),
	CONSTRAINT "pricing_policy_versions_head_target_unique" UNIQUE("policy_id","version","content_digest"),
	CONSTRAINT "pricing_policy_versions_identity_check" CHECK (
    "pricing_policy_versions"."policy_id" <> ''
    AND "pricing_policy_versions"."version" > 0
    AND "pricing_policy_versions"."schema_version" > 0
    AND "pricing_policy_versions"."product_id" <> ''
    AND "pricing_policy_versions"."catalog_generation" > 0
    AND "pricing_policy_versions"."content_digest" <> ''
    AND "pricing_policy_versions"."actor_type" <> ''
    AND "pricing_policy_versions"."reason" <> ''
  )
);--> statement-breakpoint
CREATE TABLE "pricing_usage_attributions" (
	"pricing_usage_event_id" uuid PRIMARY KEY NOT NULL,
	"attribution_schema_version" bigint NOT NULL,
	"snapshot_kind" text NOT NULL,
	"engine_request_id" text,
	"provider_id" text,
	"product_id" text,
	"account_class" text,
	"requested_model_id" text,
	"canonical_model_id" text,
	"served_model_id" text,
	"served_canonical_model_id" text,
	"billing_invariant_code" text,
	"alias_generation" bigint,
	"rule_id" text,
	"rule_digest" text,
	"rule_scope" text,
	"pricing_mode" text NOT NULL,
	"rule_origin" text NOT NULL,
	"discount_bps" integer,
	"payable_multiplier_bp" integer,
	"policy_id" text,
	"policy_version" bigint,
	"effective_policy_version" bigint,
	"policy_digest" text,
	"catalog_generation" bigint,
	"switch_generation" bigint,
	"tariff_schedule_id" text,
	"tariff_priced_at" timestamp with time zone,
	"official_nano" bigint,
	"charged_nano" bigint NOT NULL,
	"official_cost_json" jsonb,
	"paid_funded_nano" bigint,
	"bonus_funded_nano" bigint,
	"other_funded_nano" bigint,
	"funding_allocation_json" jsonb,
	"track_eligible" boolean NOT NULL,
	"retention_eligible" boolean NOT NULL,
	"commission_eligible" boolean NOT NULL,
	"snapshot_digest" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_usage_attributions_base_check" CHECK (
    "pricing_usage_attributions"."attribution_schema_version" > 0
    AND "pricing_usage_attributions"."snapshot_kind" IN ('policy_v1', 'legacy_scalar', 'legacy_b2c_track')
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
    AND (NOT "pricing_usage_attributions"."commission_eligible" OR "pricing_usage_attributions"."track_eligible")
  ),
	CONSTRAINT "pricing_usage_attributions_funding_check" CHECK (
    (
      "pricing_usage_attributions"."paid_funded_nano" IS NULL
      AND "pricing_usage_attributions"."bonus_funded_nano" IS NULL
      AND "pricing_usage_attributions"."other_funded_nano" IS NULL
      AND "pricing_usage_attributions"."funding_allocation_json" IS NULL
    )
    OR (
      "pricing_usage_attributions"."paid_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."paid_funded_nano" >= 0
      AND "pricing_usage_attributions"."bonus_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."bonus_funded_nano" >= 0
      AND "pricing_usage_attributions"."other_funded_nano" IS NOT NULL
      AND "pricing_usage_attributions"."other_funded_nano" >= 0
      AND "pricing_usage_attributions"."paid_funded_nano" + "pricing_usage_attributions"."bonus_funded_nano" + "pricing_usage_attributions"."other_funded_nano"
        = "pricing_usage_attributions"."charged_nano"
      AND "pricing_usage_attributions"."funding_allocation_json" IS NOT NULL
      AND jsonb_typeof("pricing_usage_attributions"."funding_allocation_json") = 'array'
    )
  ),
	CONSTRAINT "pricing_usage_attributions_snapshot_check" CHECK (
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
  )
);--> statement-breakpoint
CREATE TABLE "pricing_usage_funding_allocations" (
	"pricing_usage_event_id" uuid NOT NULL,
	"ordinal" integer NOT NULL,
	"engine_bucket_id" text,
	"bucket_version" bigint NOT NULL,
	"source_type" text NOT NULL,
	"source_ref" text DEFAULT '' NOT NULL,
	"amount_nano" bigint NOT NULL,
	CONSTRAINT "pricing_usage_funding_allocations_pk" PRIMARY KEY("pricing_usage_event_id","ordinal"),
	CONSTRAINT "pricing_usage_funding_allocations_shape_check" CHECK (
    "pricing_usage_funding_allocations"."ordinal" >= 0
    AND "pricing_usage_funding_allocations"."bucket_version" > 0
    AND "pricing_usage_funding_allocations"."source_type" <> ''
    AND "pricing_usage_funding_allocations"."amount_nano" > 0
    AND ("pricing_usage_funding_allocations"."engine_bucket_id" IS NULL OR "pricing_usage_funding_allocations"."engine_bucket_id" <> '')
  )
);--> statement-breakpoint
CREATE TABLE "product_catalog_entries" (
	"product_id" text NOT NULL,
	"generation" bigint NOT NULL,
	"capability_generation" bigint NOT NULL,
	"provider_id" text NOT NULL,
	"canonical_model_id" text NOT NULL,
	"enabled" boolean NOT NULL,
	CONSTRAINT "product_catalog_entries_pk" PRIMARY KEY("product_id","generation","provider_id","canonical_model_id"),
	CONSTRAINT "product_catalog_entries_identity_check" CHECK (
    "product_catalog_entries"."product_id" <> ''
    AND "product_catalog_entries"."provider_id" <> ''
    AND "product_catalog_entries"."canonical_model_id" <> ''
  )
);--> statement-breakpoint
CREATE TABLE "product_catalog_heads" (
	"product_id" text PRIMARY KEY NOT NULL,
	"active_generation" bigint NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "product_catalog_heads_product_check" CHECK ("product_catalog_heads"."product_id" <> '')
);--> statement-breakpoint
CREATE TABLE "product_catalog_versions" (
	"product_id" text NOT NULL,
	"generation" bigint NOT NULL,
	"schema_version" bigint NOT NULL,
	"capability_generation" bigint NOT NULL,
	"capability_digest" text NOT NULL,
	"content_digest" text NOT NULL,
	"actor_type" text NOT NULL,
	"actor_id" text,
	"reason" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "product_catalog_versions_product_id_generation_pk" PRIMARY KEY("product_id","generation"),
	CONSTRAINT "product_catalog_versions_capability_unique" UNIQUE("product_id","generation","capability_generation"),
	CONSTRAINT "product_catalog_versions_digest_unique" UNIQUE("product_id","generation","content_digest"),
	CONSTRAINT "product_catalog_versions_job_target_unique" UNIQUE("product_id","generation","schema_version","content_digest"),
	CONSTRAINT "product_catalog_versions_identity_check" CHECK (
    "product_catalog_versions"."product_id" <> ''
    AND "product_catalog_versions"."generation" > 0
    AND "product_catalog_versions"."schema_version" > 0
    AND "product_catalog_versions"."capability_generation" > 0
    AND "product_catalog_versions"."capability_digest" <> ''
    AND "product_catalog_versions"."content_digest" <> ''
    AND "product_catalog_versions"."actor_type" <> ''
    AND "product_catalog_versions"."reason" <> ''
  )
);--> statement-breakpoint
CREATE TABLE "provider_capability_aliases" (
	"generation" bigint NOT NULL,
	"provider_id" text NOT NULL,
	"alias_model_id" text NOT NULL,
	"canonical_model_id" text NOT NULL,
	CONSTRAINT "provider_capability_aliases_pk" PRIMARY KEY("generation","provider_id","alias_model_id"),
	CONSTRAINT "provider_capability_aliases_identity_check" CHECK (
    "provider_capability_aliases"."provider_id" <> ''
    AND "provider_capability_aliases"."alias_model_id" <> ''
    AND "provider_capability_aliases"."canonical_model_id" <> ''
    AND "provider_capability_aliases"."alias_model_id" <> "provider_capability_aliases"."canonical_model_id"
  )
);--> statement-breakpoint
CREATE TABLE "provider_capability_entries" (
	"generation" bigint NOT NULL,
	"provider_id" text NOT NULL,
	"canonical_model_id" text NOT NULL,
	"entry_digest" text NOT NULL,
	"capability_data" jsonb NOT NULL,
	CONSTRAINT "provider_capability_entries_pk" PRIMARY KEY("generation","provider_id","canonical_model_id"),
	CONSTRAINT "provider_capability_entries_identity_check" CHECK (
    "provider_capability_entries"."provider_id" <> ''
    AND "provider_capability_entries"."canonical_model_id" <> ''
    AND "provider_capability_entries"."entry_digest" <> ''
  ),
	CONSTRAINT "provider_capability_entries_data_check" CHECK (
    jsonb_typeof("provider_capability_entries"."capability_data") = 'object'
  )
);--> statement-breakpoint
CREATE TABLE "provider_capability_head" (
	"singleton" integer PRIMARY KEY NOT NULL,
	"active_generation" bigint NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "provider_capability_head_singleton_check" CHECK ("provider_capability_head"."singleton" = 1)
);--> statement-breakpoint
CREATE TABLE "provider_capability_versions" (
	"generation" bigint PRIMARY KEY NOT NULL,
	"schema_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"source_runtime" text,
	"source_revision" text,
	"observed_at" timestamp with time zone NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "provider_capability_versions_digest_unique" UNIQUE("generation","content_digest"),
	CONSTRAINT "provider_capability_versions_generation_check" CHECK ("provider_capability_versions"."generation" > 0),
	CONSTRAINT "provider_capability_versions_schema_check" CHECK ("provider_capability_versions"."schema_version" > 0),
	CONSTRAINT "provider_capability_versions_digest_check" CHECK ("provider_capability_versions"."content_digest" <> '')
);--> statement-breakpoint
CREATE TABLE "provider_switch_entries" (
	"generation" bigint NOT NULL,
	"provider_id" text NOT NULL,
	"scope_type" text NOT NULL,
	"product_id" text DEFAULT '' NOT NULL,
	"segment" text DEFAULT '' NOT NULL,
	"enabled" boolean NOT NULL,
	CONSTRAINT "provider_switch_entries_pk" PRIMARY KEY("generation","provider_id","scope_type","product_id","segment"),
	CONSTRAINT "provider_switch_entries_identity_check" CHECK ("provider_switch_entries"."provider_id" <> ''),
	CONSTRAINT "provider_switch_entries_scope_check" CHECK (
    ("provider_switch_entries"."scope_type" = 'master' AND "provider_switch_entries"."product_id" = '' AND "provider_switch_entries"."segment" = '')
    OR ("provider_switch_entries"."scope_type" = 'product' AND "provider_switch_entries"."product_id" <> '' AND "provider_switch_entries"."segment" = '')
    OR (
      "provider_switch_entries"."scope_type" = 'segment'
      AND "provider_switch_entries"."product_id" <> ''
      AND "provider_switch_entries"."segment" IN ('b2c', 'b2b')
    )
  )
);--> statement-breakpoint
CREATE TABLE "provider_switch_head" (
	"singleton" integer PRIMARY KEY NOT NULL,
	"active_generation" bigint NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "provider_switch_head_singleton_check" CHECK ("provider_switch_head"."singleton" = 1)
);--> statement-breakpoint
CREATE TABLE "provider_switch_versions" (
	"generation" bigint PRIMARY KEY NOT NULL,
	"schema_version" bigint NOT NULL,
	"content_digest" text NOT NULL,
	"actor_type" text NOT NULL,
	"actor_id" text,
	"reason" text NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "provider_switch_versions_job_target_unique" UNIQUE("generation","schema_version","content_digest"),
	CONSTRAINT "provider_switch_versions_identity_check" CHECK (
    "provider_switch_versions"."generation" > 0
    AND "provider_switch_versions"."schema_version" > 0
    AND "provider_switch_versions"."content_digest" <> ''
    AND "provider_switch_versions"."actor_type" <> ''
    AND "provider_switch_versions"."reason" <> ''
  )
);--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_user_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_engine_record_fk" FOREIGN KEY ("engine_account_record_id") REFERENCES "public"."engine_accounts"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_policy_fk" FOREIGN KEY ("policy_id","product_id") REFERENCES "public"."pricing_policies"("id","product_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_desired_fk" FOREIGN KEY ("id","desired_effective_version","desired_digest") REFERENCES "public"."account_policy_versions"("binding_id","effective_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_bindings" ADD CONSTRAINT "account_policy_bindings_applied_fk" FOREIGN KEY ("id","applied_effective_version","applied_digest") REFERENCES "public"."account_policy_versions"("binding_id","effective_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_reconciliations" ADD CONSTRAINT "account_policy_reconciliations_binding_fk" FOREIGN KEY ("binding_id") REFERENCES "public"."account_policy_bindings"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_reconciliations" ADD CONSTRAINT "account_policy_reconciliations_version_fk" FOREIGN KEY ("binding_id","effective_version") REFERENCES "public"."account_policy_versions"("binding_id","effective_version") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_rules" ADD CONSTRAINT "account_policy_rules_version_fk" FOREIGN KEY ("binding_id","effective_version","product_id","catalog_generation") REFERENCES "public"."account_policy_versions"("binding_id","effective_version","product_id","catalog_generation") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_rules" ADD CONSTRAINT "account_policy_rules_model_fk" FOREIGN KEY ("product_id","catalog_generation","provider_id","canonical_model_id") REFERENCES "public"."product_catalog_entries"("product_id","generation","provider_id","canonical_model_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_versions" ADD CONSTRAINT "account_policy_versions_source_policy_fk" FOREIGN KEY ("policy_id","policy_version","product_id","catalog_generation","policy_digest") REFERENCES "public"."pricing_policy_versions"("policy_id","version","product_id","catalog_generation","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_versions" ADD CONSTRAINT "account_policy_versions_catalog_fk" FOREIGN KEY ("product_id","catalog_generation") REFERENCES "public"."product_catalog_versions"("product_id","generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "account_policy_versions" ADD CONSTRAINT "account_policy_versions_switch_fk" FOREIGN KEY ("switch_generation") REFERENCES "public"."provider_switch_versions"("generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_invite_fk" FOREIGN KEY ("invite_id") REFERENCES "public"."business_invites"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_user_fk" FOREIGN KEY ("copied_to_user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_binding_fk" FOREIGN KEY ("copied_to_binding_id") REFERENCES "public"."account_policy_bindings"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_copy_target_fk" FOREIGN KEY ("copied_to_binding_id","copied_to_user_id","copied_client_policy_id") REFERENCES "public"."account_policy_bindings"("id","user_id","policy_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_current_fk" FOREIGN KEY ("invitation_policy_id","current_policy_version","current_policy_digest") REFERENCES "public"."pricing_policy_versions"("policy_id","version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_redeemed_fk" FOREIGN KEY ("invitation_policy_id","redeemed_source_policy_version","redeemed_source_policy_digest") REFERENCES "public"."pricing_policy_versions"("policy_id","version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "business_invite_policy_bindings" ADD CONSTRAINT "business_invite_policy_bindings_copied_fk" FOREIGN KEY ("copied_client_policy_id","copied_client_policy_version","copied_client_policy_digest") REFERENCES "public"."pricing_policy_versions"("policy_id","version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_catalog_jobs" ADD CONSTRAINT "engine_catalog_jobs_target_fk" FOREIGN KEY ("product_id","generation","schema_version","content_digest") REFERENCES "public"."product_catalog_versions"("product_id","generation","schema_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_policy_jobs" ADD CONSTRAINT "engine_policy_jobs_binding_target_fk" FOREIGN KEY ("binding_id","engine_account_id") REFERENCES "public"."account_policy_bindings"("id","engine_account_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_policy_jobs" ADD CONSTRAINT "engine_policy_jobs_target_fk" FOREIGN KEY ("binding_id","effective_version","policy_id","policy_version","catalog_generation","switch_generation","schema_version","content_digest") REFERENCES "public"."account_policy_versions"("binding_id","effective_version","policy_id","policy_version","catalog_generation","switch_generation","schema_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_switch_jobs" ADD CONSTRAINT "engine_switch_jobs_target_fk" FOREIGN KEY ("generation","schema_version","content_digest") REFERENCES "public"."provider_switch_versions"("generation","schema_version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_policy_heads" ADD CONSTRAINT "pricing_policy_heads_version_fk" FOREIGN KEY ("policy_id","current_version","current_digest") REFERENCES "public"."pricing_policy_versions"("policy_id","version","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_policy_rules" ADD CONSTRAINT "pricing_policy_rules_version_fk" FOREIGN KEY ("policy_id","policy_version","product_id","catalog_generation") REFERENCES "public"."pricing_policy_versions"("policy_id","version","product_id","catalog_generation") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_policy_rules" ADD CONSTRAINT "pricing_policy_rules_model_fk" FOREIGN KEY ("product_id","catalog_generation","provider_id","canonical_model_id") REFERENCES "public"."product_catalog_entries"("product_id","generation","provider_id","canonical_model_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_policy_versions" ADD CONSTRAINT "pricing_policy_versions_policy_fk" FOREIGN KEY ("policy_id","product_id") REFERENCES "public"."pricing_policies"("id","product_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_policy_versions" ADD CONSTRAINT "pricing_policy_versions_catalog_fk" FOREIGN KEY ("product_id","catalog_generation") REFERENCES "public"."product_catalog_versions"("product_id","generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_usage_attributions" ADD CONSTRAINT "pricing_usage_attributions_event_fk" FOREIGN KEY ("pricing_usage_event_id") REFERENCES "public"."pricing_usage_events"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_usage_funding_allocations" ADD CONSTRAINT "pricing_usage_funding_allocations_attribution_fk" FOREIGN KEY ("pricing_usage_event_id") REFERENCES "public"."pricing_usage_attributions"("pricing_usage_event_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "product_catalog_entries" ADD CONSTRAINT "product_catalog_entries_version_fk" FOREIGN KEY ("product_id","generation","capability_generation") REFERENCES "public"."product_catalog_versions"("product_id","generation","capability_generation") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "product_catalog_entries" ADD CONSTRAINT "product_catalog_entries_capability_fk" FOREIGN KEY ("capability_generation","provider_id","canonical_model_id") REFERENCES "public"."provider_capability_entries"("generation","provider_id","canonical_model_id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "product_catalog_heads" ADD CONSTRAINT "product_catalog_heads_version_fk" FOREIGN KEY ("product_id","active_generation") REFERENCES "public"."product_catalog_versions"("product_id","generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "product_catalog_versions" ADD CONSTRAINT "product_catalog_versions_capability_fk" FOREIGN KEY ("capability_generation","capability_digest") REFERENCES "public"."provider_capability_versions"("generation","content_digest") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_capability_aliases" ADD CONSTRAINT "provider_capability_aliases_entry_fk" FOREIGN KEY ("generation","provider_id","canonical_model_id") REFERENCES "public"."provider_capability_entries"("generation","provider_id","canonical_model_id") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_capability_entries" ADD CONSTRAINT "provider_capability_entries_version_fk" FOREIGN KEY ("generation") REFERENCES "public"."provider_capability_versions"("generation") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_capability_head" ADD CONSTRAINT "provider_capability_head_version_fk" FOREIGN KEY ("active_generation") REFERENCES "public"."provider_capability_versions"("generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_switch_entries" ADD CONSTRAINT "provider_switch_entries_version_fk" FOREIGN KEY ("generation") REFERENCES "public"."provider_switch_versions"("generation") ON DELETE cascade ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "provider_switch_head" ADD CONSTRAINT "provider_switch_head_version_fk" FOREIGN KEY ("active_generation") REFERENCES "public"."provider_switch_versions"("generation") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_bindings_user_uidx" ON "account_policy_bindings" USING btree ("user_id") WHERE "account_policy_bindings"."user_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_bindings_engine_record_uidx" ON "account_policy_bindings" USING btree ("engine_account_record_id") WHERE "account_policy_bindings"."engine_account_record_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_bindings_engine_account_uidx" ON "account_policy_bindings" USING btree ("engine_account_id") WHERE "account_policy_bindings"."engine_account_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "account_policy_bindings_sync_idx" ON "account_policy_bindings" USING btree ("sync_state","reconciliation_state","updated_at");--> statement-breakpoint
CREATE INDEX "account_policy_reconciliations_binding_idx" ON "account_policy_reconciliations" USING btree ("binding_id","created_at");--> statement-breakpoint
CREATE INDEX "account_policy_reconciliations_status_idx" ON "account_policy_reconciliations" USING btree ("status","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_rules_digest_uidx" ON "account_policy_rules" USING btree ("binding_id","effective_version","rule_id","rule_digest");--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_rules_provider_scope_uidx" ON "account_policy_rules" USING btree ("binding_id","effective_version","provider_id") WHERE "account_policy_rules"."scope_type" = 'provider';--> statement-breakpoint
CREATE UNIQUE INDEX "account_policy_rules_model_scope_uidx" ON "account_policy_rules" USING btree ("binding_id","effective_version","provider_id","canonical_model_id") WHERE "account_policy_rules"."scope_type" = 'model';--> statement-breakpoint
CREATE UNIQUE INDEX "engine_catalog_jobs_target_uidx" ON "engine_catalog_jobs" USING btree ("product_id","generation");--> statement-breakpoint
CREATE INDEX "engine_catalog_jobs_claim_idx" ON "engine_catalog_jobs" USING btree ("next_attempt_at","created_at") WHERE "engine_catalog_jobs"."status" IN ('pending', 'retry');--> statement-breakpoint
CREATE UNIQUE INDEX "engine_policy_jobs_target_uidx" ON "engine_policy_jobs" USING btree ("binding_id","effective_version");--> statement-breakpoint
CREATE INDEX "engine_policy_jobs_claim_idx" ON "engine_policy_jobs" USING btree ("next_attempt_at","created_at") WHERE "engine_policy_jobs"."status" IN ('pending', 'retry');--> statement-breakpoint
CREATE UNIQUE INDEX "engine_switch_jobs_target_uidx" ON "engine_switch_jobs" USING btree ("generation");--> statement-breakpoint
CREATE INDEX "engine_switch_jobs_claim_idx" ON "engine_switch_jobs" USING btree ("next_attempt_at","created_at") WHERE "engine_switch_jobs"."status" IN ('pending', 'retry');--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policies_owner_uidx" ON "pricing_policies" USING btree ("owner_type","owner_id","product_id");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policies_global_b2c_uidx" ON "pricing_policies" USING btree ("owner_type","product_id") WHERE "pricing_policies"."owner_type" = 'global_b2c';--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policy_rules_digest_uidx" ON "pricing_policy_rules" USING btree ("policy_id","policy_version","rule_id","rule_digest");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policy_rules_provider_scope_uidx" ON "pricing_policy_rules" USING btree ("policy_id","policy_version","provider_id") WHERE "pricing_policy_rules"."scope_type" = 'provider';--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_policy_rules_model_scope_uidx" ON "pricing_policy_rules" USING btree ("policy_id","policy_version","provider_id","canonical_model_id") WHERE "pricing_policy_rules"."scope_type" = 'model';--> statement-breakpoint
CREATE INDEX "pricing_usage_attributions_policy_idx" ON "pricing_usage_attributions" USING btree ("policy_id","policy_version") WHERE "pricing_usage_attributions"."policy_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "pricing_usage_attributions_provider_model_idx" ON "pricing_usage_attributions" USING btree ("provider_id","canonical_model_id") WHERE "pricing_usage_attributions"."provider_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "pricing_usage_funding_allocations_source_idx" ON "pricing_usage_funding_allocations" USING btree ("source_type","source_ref");--> statement-breakpoint
CREATE INDEX "product_catalog_entries_enabled_idx" ON "product_catalog_entries" USING btree ("product_id","generation","provider_id") WHERE "product_catalog_entries"."enabled";--> statement-breakpoint
CREATE UNIQUE INDEX "provider_switch_versions_digest_uidx" ON "provider_switch_versions" USING btree ("generation","content_digest");
