-- Expand the cross-feed buffer before the attributed sales writer. Existing pending deposits and
-- legacy spends remain all-NULL; new attributed spends must preserve the complete immutable
-- commission identity while waiting for their referral attribution.
ALTER TABLE "pending_referral_events" ADD COLUMN "provider_id" text;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD COLUMN "account_class" text;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD COLUMN "pricing_mode" text;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD COLUMN "paid_funded_nano" bigint;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD COLUMN "commission_eligible" boolean;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD COLUMN "snapshot_digest" text;--> statement-breakpoint
ALTER TABLE "pending_referral_events" ADD CONSTRAINT "pending_referral_events_attribution_check" CHECK (
  (
    "provider_id" IS NULL
    AND "account_class" IS NULL
    AND "pricing_mode" IS NULL
    AND "paid_funded_nano" IS NULL
    AND "commission_eligible" IS NULL
    AND "snapshot_digest" IS NULL
  )
  OR (
    "kind" = 'spend'
    AND "provider_id" IS NOT NULL
    AND "provider_id" <> ''
    AND "account_class" = 'b2c'
    AND "pricing_mode" = 'track'
    AND "paid_funded_nano" IS NOT NULL
    AND "paid_funded_nano" > 0
    AND "amount_nano" = "paid_funded_nano"
    AND "commission_eligible" IS TRUE
    AND "snapshot_digest" IS NOT NULL
    AND "snapshot_digest" <> ''
  )
);--> statement-breakpoint
ALTER TABLE "partner_usage_events" ADD CONSTRAINT "partner_usage_events_commission_authority_check" CHECK (
  "provider_id" IS NULL
  OR (
    "account_class" = 'b2c'
    AND "pricing_mode" = 'track'
    AND "commission_eligible" IS TRUE
    AND "paid_funded_nano" = "amount_nano"
  )
);
