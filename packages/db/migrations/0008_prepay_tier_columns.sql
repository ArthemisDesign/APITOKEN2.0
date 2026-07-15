ALTER TABLE "customer_profiles" DROP CONSTRAINT "customer_profiles_tier_check";--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD COLUMN "cumulative_topup_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD COLUMN "tier_window_start" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD COLUMN "tier_window_spent_nano" bigint DEFAULT 0 NOT NULL;--> statement-breakpoint
ALTER TABLE "customer_profiles" ADD CONSTRAINT "customer_profiles_tier_check" CHECK ("customer_profiles"."current_tier" IS NULL OR "customer_profiles"."current_tier" BETWEEN 0 AND 5);