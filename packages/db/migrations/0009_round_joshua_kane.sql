CREATE TYPE "public"."engine_adjustment_kind" AS ENUM('refund', 'dispute');--> statement-breakpoint
CREATE TYPE "public"."engine_adjustment_status" AS ENUM('pending', 'processing', 'retry', 'confirmed', 'dead');--> statement-breakpoint
CREATE TABLE "engine_adjustments" (
	"id" uuid PRIMARY KEY NOT NULL,
	"payment_id" uuid NOT NULL,
	"webhook_event_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"kind" "engine_adjustment_kind" NOT NULL,
	"amount_nano" bigint NOT NULL,
	"idempotency_ref" text NOT NULL,
	"status" "engine_adjustment_status" DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"engine_balance_after_nano" bigint,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_adjustments_amount_check" CHECK ("engine_adjustments"."amount_nano" < 0)
);
--> statement-breakpoint
CREATE TABLE "pricing_credit_accruals" (
	"credit_id" uuid PRIMARY KEY NOT NULL,
	"applied_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
ALTER TABLE "customer_profiles" DROP CONSTRAINT "customer_profiles_tier_check";--> statement-breakpoint
ALTER TABLE "engine_adjustments" ADD CONSTRAINT "engine_adjustments_payment_id_payments_id_fk" FOREIGN KEY ("payment_id") REFERENCES "public"."payments"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_adjustments" ADD CONSTRAINT "engine_adjustments_webhook_event_id_webhook_events_id_fk" FOREIGN KEY ("webhook_event_id") REFERENCES "public"."webhook_events"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_credit_accruals" ADD CONSTRAINT "pricing_credit_accruals_credit_id_engine_credits_id_fk" FOREIGN KEY ("credit_id") REFERENCES "public"."engine_credits"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "engine_adjustments_payment_event_uidx" ON "engine_adjustments" USING btree ("payment_id","webhook_event_id");--> statement-breakpoint
CREATE UNIQUE INDEX "engine_adjustments_ref_uidx" ON "engine_adjustments" USING btree ("idempotency_ref");--> statement-breakpoint
CREATE INDEX "engine_adjustments_payment_idx" ON "engine_adjustments" USING btree ("payment_id","created_at");--> statement-breakpoint
CREATE INDEX "engine_adjustments_claim_idx" ON "engine_adjustments" USING btree ("status","next_attempt_at");--> statement-breakpoint
-- Keep the 0008 expansion rollout-compatible. Contracting this constraint while an older
-- application can still write tier 5 would make the migration unsafe for rolling deploys.
ALTER TABLE "customer_profiles" ADD CONSTRAINT "customer_profiles_tier_check" CHECK ("customer_profiles"."current_tier" IS NULL OR "customer_profiles"."current_tier" BETWEEN 0 AND 5);
