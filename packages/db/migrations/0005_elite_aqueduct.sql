CREATE TYPE "public"."customer_type" AS ENUM('b2c', 'b2b');--> statement-breakpoint
CREATE TYPE "public"."pricing_job_status" AS ENUM('pending', 'processing', 'retry', 'confirmed');--> statement-breakpoint
CREATE TABLE "business_invites" (
	"id" uuid PRIMARY KEY NOT NULL,
	"email" text NOT NULL,
	"token_hash" text NOT NULL,
	"multiplier_bp" integer NOT NULL,
	"expires_at" timestamp with time zone NOT NULL,
	"consumed_at" timestamp with time zone,
	"consumed_by_user_id" uuid,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "business_invites_multiplier_check" CHECK ("business_invites"."multiplier_bp" BETWEEN 0 AND 10000)
);
--> statement-breakpoint
CREATE TABLE "customer_profiles" (
	"user_id" uuid PRIMARY KEY NOT NULL,
	"customer_type" "customer_type" DEFAULT 'b2c' NOT NULL,
	"current_tier" integer,
	"multiplier_bp" integer DEFAULT 4000 NOT NULL,
	"pricing_month_start" timestamp with time zone NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "customer_profiles_multiplier_check" CHECK ("customer_profiles"."multiplier_bp" BETWEEN 0 AND 10000),
	CONSTRAINT "customer_profiles_tier_check" CHECK ("customer_profiles"."current_tier" IS NULL OR "customer_profiles"."current_tier" BETWEEN 0 AND 4),
	CONSTRAINT "customer_profiles_type_tier_check" CHECK (
    ("customer_profiles"."customer_type" = 'b2c' AND "customer_profiles"."current_tier" IS NOT NULL)
    OR ("customer_profiles"."customer_type" = 'b2b' AND "customer_profiles"."current_tier" IS NULL)
  )
);
--> statement-breakpoint
CREATE TABLE "engine_pricing_jobs" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"multiplier_bp" integer NOT NULL,
	"reason" text NOT NULL,
	"status" "pricing_job_status" DEFAULT 'pending' NOT NULL,
	"attempts" integer DEFAULT 0 NOT NULL,
	"next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
	"locked_at" timestamp with time zone,
	"locked_by" text,
	"last_error" text,
	"confirmed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "engine_pricing_jobs_multiplier_check" CHECK ("engine_pricing_jobs"."multiplier_bp" BETWEEN 0 AND 10000)
);
--> statement-breakpoint
CREATE TABLE "pricing_months" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"month_start" timestamp with time zone NOT NULL,
	"opening_tier" integer NOT NULL,
	"highest_tier" integer NOT NULL,
	"spent_nano" bigint DEFAULT 0 NOT NULL,
	"closed_at" timestamp with time zone,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_months_opening_tier_check" CHECK ("pricing_months"."opening_tier" BETWEEN 0 AND 4),
	CONSTRAINT "pricing_months_highest_tier_check" CHECK ("pricing_months"."highest_tier" BETWEEN 0 AND 4),
	CONSTRAINT "pricing_months_spent_check" CHECK ("pricing_months"."spent_nano" >= 0)
);
--> statement-breakpoint
CREATE TABLE "pricing_usage_cursors" (
	"engine_account_id" text PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"last_ledger_id" bigint DEFAULT 0 NOT NULL,
	"updated_at" timestamp with time zone DEFAULT now() NOT NULL
);
--> statement-breakpoint
CREATE TABLE "pricing_usage_events" (
	"id" uuid PRIMARY KEY NOT NULL,
	"user_id" uuid NOT NULL,
	"engine_account_id" text NOT NULL,
	"ledger_entry_id" bigint NOT NULL,
	"amount_nano" bigint NOT NULL,
	"occurred_at" timestamp with time zone NOT NULL,
	"created_at" timestamp with time zone DEFAULT now() NOT NULL,
	CONSTRAINT "pricing_usage_events_amount_check" CHECK ("pricing_usage_events"."amount_nano" > 0)
);
--> statement-breakpoint
ALTER TABLE "engine_accounts" ALTER COLUMN "mult_bp" SET DEFAULT 4000;--> statement-breakpoint
ALTER TABLE "business_invites" ADD CONSTRAINT "business_invites_consumed_by_user_id_users_id_fk" FOREIGN KEY ("consumed_by_user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD CONSTRAINT "customer_profiles_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "engine_pricing_jobs" ADD CONSTRAINT "engine_pricing_jobs_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_months" ADD CONSTRAINT "pricing_months_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_usage_cursors" ADD CONSTRAINT "pricing_usage_cursors_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
ALTER TABLE "pricing_usage_events" ADD CONSTRAINT "pricing_usage_events_user_id_users_id_fk" FOREIGN KEY ("user_id") REFERENCES "public"."users"("id") ON DELETE restrict ON UPDATE no action;--> statement-breakpoint
CREATE UNIQUE INDEX "business_invites_token_hash_uidx" ON "business_invites" USING btree ("token_hash");--> statement-breakpoint
CREATE INDEX "business_invites_email_idx" ON "business_invites" USING btree ("email","created_at");--> statement-breakpoint
CREATE UNIQUE INDEX "engine_pricing_jobs_user_uidx" ON "engine_pricing_jobs" USING btree ("user_id");--> statement-breakpoint
CREATE INDEX "engine_pricing_jobs_claim_idx" ON "engine_pricing_jobs" USING btree ("status","next_attempt_at");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_months_user_month_uidx" ON "pricing_months" USING btree ("user_id","month_start");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_usage_cursors_user_uidx" ON "pricing_usage_cursors" USING btree ("user_id");--> statement-breakpoint
CREATE UNIQUE INDEX "pricing_usage_events_engine_ledger_uidx" ON "pricing_usage_events" USING btree ("engine_account_id","ledger_entry_id");--> statement-breakpoint
CREATE INDEX "pricing_usage_events_user_time_idx" ON "pricing_usage_events" USING btree ("user_id","occurred_at");--> statement-breakpoint
INSERT INTO "customer_profiles" (
	"user_id", "customer_type", "current_tier", "multiplier_bp", "pricing_month_start"
)
SELECT "id", 'b2c', 0, 4000, date_trunc('month', now() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC'
FROM "users"
ON CONFLICT ("user_id") DO NOTHING;--> statement-breakpoint
INSERT INTO "pricing_months" (
	"id", "user_id", "month_start", "opening_tier", "highest_tier"
)
SELECT md5("user_id"::text || ':pricing-month')::uuid, "user_id", "pricing_month_start", 0, 0
FROM "customer_profiles"
WHERE "customer_type" = 'b2c'
ON CONFLICT ("user_id", "month_start") DO NOTHING;--> statement-breakpoint
UPDATE "engine_accounts" SET "mult_bp" = 4000, "updated_at" = now();--> statement-breakpoint
INSERT INTO "engine_pricing_jobs" (
	"id", "user_id", "engine_account_id", "multiplier_bp", "reason"
)
SELECT md5(ea."user_id"::text || ':pricing-backfill')::uuid,
	ea."user_id", ea."engine_account_id", 4000, 'b2c_migration'
FROM "engine_accounts" ea
JOIN "users" u ON u."id" = ea."user_id"
WHERE ea."status" = 'active' AND ea."engine_account_id" IS NOT NULL AND u."status" = 'active'
ON CONFLICT ("user_id") DO NOTHING;
