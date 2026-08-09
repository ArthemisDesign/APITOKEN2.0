-- Per-provider B2B discounts, delivered through the existing durable pricing-job lane.
--
-- The customer's discount is one number; where their terms differ per provider, one row per
-- provider. This replaces the versioned policy/catalog/switch documents: those carried a second
-- copy of both price and eligibility, and on 2026-08-09 that second copy is what stopped funded
-- accounts from spending. The engine is the authority — these rows are the commerce-side record of
-- what was asked for, and the job queue is what makes delivery durable.
CREATE TABLE IF NOT EXISTS "customer_provider_discounts" (
  "user_id" uuid NOT NULL REFERENCES "users"("id") ON DELETE CASCADE,
  "provider_id" text NOT NULL,
  "multiplier_bp" integer NOT NULL,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  "updated_at" timestamp with time zone DEFAULT now() NOT NULL,
  CONSTRAINT "customer_provider_discounts_pkey" PRIMARY KEY ("user_id", "provider_id"),
  CONSTRAINT "customer_provider_discounts_multiplier_check"
    CHECK ("multiplier_bp" BETWEEN 0 AND 10000),
  CONSTRAINT "customer_provider_discounts_provider_check"
    CHECK ("provider_id" IN ('anthropic', 'openai', 'google', 'kimi', 'glm'))
);

-- A pricing job now targets either the account default (provider_id NULL) or one provider
-- override. A NULL multiplier on a provider job removes the override, returning that provider to
-- the account default.
ALTER TABLE "engine_pricing_jobs" ADD COLUMN IF NOT EXISTS "provider_id" text;
ALTER TABLE "engine_pricing_jobs" ALTER COLUMN "multiplier_bp" DROP NOT NULL;
ALTER TABLE "engine_pricing_jobs" DROP CONSTRAINT IF EXISTS "engine_pricing_jobs_multiplier_check";
ALTER TABLE "engine_pricing_jobs" ADD CONSTRAINT "engine_pricing_jobs_multiplier_check"
  CHECK ("multiplier_bp" IS NULL OR "multiplier_bp" BETWEEN 0 AND 10000);
ALTER TABLE "engine_pricing_jobs" ADD CONSTRAINT "engine_pricing_jobs_target_check"
  CHECK ("provider_id" IS NOT NULL OR "multiplier_bp" IS NOT NULL);
ALTER TABLE "engine_pricing_jobs" ADD CONSTRAINT "engine_pricing_jobs_provider_check"
  CHECK ("provider_id" IS NULL
         OR "provider_id" IN ('anthropic', 'openai', 'google', 'kimi', 'glm'));

-- One pending job per (user, target) instead of per user: a default change and a provider change
-- are independent deliveries and must not evict one another.
DROP INDEX IF EXISTS "engine_pricing_jobs_user_uidx";
CREATE UNIQUE INDEX IF NOT EXISTS "engine_pricing_jobs_user_provider_uidx"
  ON "engine_pricing_jobs" ("user_id", "provider_id") NULLS NOT DISTINCT;
