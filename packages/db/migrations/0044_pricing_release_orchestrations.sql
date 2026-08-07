CREATE TABLE "pricing_release_orchestrations_v2" (
	"id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
	"idempotency_key" text NOT NULL,
	"capability_generation" bigint NOT NULL,
	"step" text NOT NULL,
	"status" text DEFAULT 'active' NOT NULL,
	"cycle" integer DEFAULT 1 NOT NULL,
	"stage5_run_id" uuid,
	"target_generation" bigint,
	"recovery_generation" bigint,
	"evidence_digest" text,
	"activation_kind" text,
	"operator_id" text NOT NULL,
	"reason" text NOT NULL,
	"last_error" text,
	"result_digest" text,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_release_orchestrations_v2_idempotency_key_unique" UNIQUE("idempotency_key"),
	CONSTRAINT "pricing_release_orchestrations_v2_shape_check" CHECK (
    "pricing_release_orchestrations_v2"."capability_generation" > 0
    AND "pricing_release_orchestrations_v2"."step" IN (
      'materialize_pair',
      'deliver_catalogs',
      'normalize_funding',
      'rollout',
      'capture',
      'activate',
      'verify'
    )
    AND "pricing_release_orchestrations_v2"."status" IN ('active', 'confirmed', 'dead')
    AND "pricing_release_orchestrations_v2"."cycle" BETWEEN 1 AND 3
    AND ("pricing_release_orchestrations_v2"."activation_kind" IS NULL OR "pricing_release_orchestrations_v2"."activation_kind" IN ('cutover', 'recovery', 'successor'))
    AND "pricing_release_orchestrations_v2"."operator_id" <> ''
    AND "pricing_release_orchestrations_v2"."reason" <> ''
    AND (
      ("pricing_release_orchestrations_v2"."status" = 'confirmed' AND "pricing_release_orchestrations_v2"."result_digest" IS NOT NULL
        AND "pricing_release_orchestrations_v2"."confirmed_at" IS NOT NULL AND "pricing_release_orchestrations_v2"."step" = 'verify'
        AND "pricing_release_orchestrations_v2"."target_generation" IS NOT NULL)
      OR ("pricing_release_orchestrations_v2"."status" <> 'confirmed' AND "pricing_release_orchestrations_v2"."confirmed_at" IS NULL)
    )
  )
);
--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_release_orchestrations_v2_active_uq" ON "pricing_release_orchestrations_v2" USING btree ((1)) WHERE "pricing_release_orchestrations_v2"."status" = 'active';