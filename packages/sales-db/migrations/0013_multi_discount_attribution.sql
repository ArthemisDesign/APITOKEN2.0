-- Expand-only immutable pricing attribution for referral usage. Existing rows remain valid as
-- legacy records with all new fields NULL; the dependent writer will populate the complete set.
ALTER TABLE "partner_usage_events" ADD COLUMN "provider_id" text;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD COLUMN "account_class" text;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD COLUMN "pricing_mode" text;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD COLUMN "paid_funded_nano" bigint;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD COLUMN "commission_eligible" boolean;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD COLUMN "snapshot_digest" text;--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD CONSTRAINT "partner_usage_events_multi_discount_check" CHECK (
  (
    "provider_id" IS NULL
    AND "account_class" IS NULL
    AND "pricing_mode" IS NULL
    AND "paid_funded_nano" IS NULL
    AND "commission_eligible" IS NULL
    AND "snapshot_digest" IS NULL
  )
  OR (
    "provider_id" IS NOT NULL
    AND "provider_id" <> ''
    AND "account_class" IN ('b2c', 'b2b', 'open_keys', 'service')
    AND "pricing_mode" IN ('track', 'discount')
    AND "paid_funded_nano" IS NOT NULL
    AND "paid_funded_nano" > 0
    AND "amount_nano" = "paid_funded_nano"
    AND "commission_eligible" IS NOT NULL
    AND (NOT "commission_eligible" OR (
      "pricing_mode" = 'track'
      AND "account_class" = 'b2c'
    ))
    AND "snapshot_digest" IS NOT NULL
    AND "snapshot_digest" <> ''
  )
);
