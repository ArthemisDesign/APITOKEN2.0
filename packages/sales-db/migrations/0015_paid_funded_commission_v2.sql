-- Expand-only referral attribution v2. The legacy tables retain their historical eligibility
-- checks; new consumers use these tables where commission eligibility depends only on B2C
-- attribution and the exact paid-funded settlement amount.

CREATE TABLE "partner_usage_events_v2" (
  "id" bigserial PRIMARY KEY,
  "commerce_event_id" bigint NOT NULL UNIQUE,
  "commerce_user_id" uuid NOT NULL,
  "partner_id" uuid NOT NULL REFERENCES "partners"("id") ON DELETE restrict,
  "provider_id" text NOT NULL,
  "account_class" text NOT NULL,
  "official_nano" bigint NOT NULL,
  "charged_nano" bigint NOT NULL,
  "paid_funded_nano" bigint NOT NULL,
  "bonus_funded_nano" bigint NOT NULL,
  "other_funded_nano" bigint NOT NULL,
  "commission_eligible" boolean NOT NULL,
  "release_generation" bigint NOT NULL,
  "release_digest" text NOT NULL,
  "snapshot_digest" text NOT NULL,
  "occurred_at" timestamp with time zone NOT NULL,
  "imported_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_usage_events_v2_shape_check" CHECK (
    "provider_id" <> ''
    AND "account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
    AND "official_nano" >= 0
    AND "charged_nano" >= 0
    AND "paid_funded_nano" >= 0
    AND "bonus_funded_nano" >= 0
    AND "other_funded_nano" >= 0
    AND "paid_funded_nano" + "bonus_funded_nano" + "other_funded_nano" = "charged_nano"
    AND "release_generation" > 0
    AND "release_digest" <> ''
    AND "snapshot_digest" <> ''
    AND (
      NOT "commission_eligible"
      OR ("account_class" = 'b2c' AND "paid_funded_nano" > 0)
    )
  )
);--> statement-breakpoint

CREATE INDEX "partner_usage_events_v2_partner_time_idx"
  ON "partner_usage_events_v2"("partner_id", "occurred_at");--> statement-breakpoint
CREATE INDEX "partner_usage_events_v2_user_idx"
  ON "partner_usage_events_v2"("commerce_user_id", "commerce_event_id");--> statement-breakpoint

CREATE TABLE "pending_referral_usage_events_v2" (
  "id" bigserial PRIMARY KEY,
  "commerce_ref" text NOT NULL UNIQUE,
  "commerce_event_id" bigint NOT NULL UNIQUE,
  "commerce_user_id" uuid NOT NULL,
  "provider_id" text NOT NULL,
  "account_class" text NOT NULL,
  "official_nano" bigint NOT NULL,
  "charged_nano" bigint NOT NULL,
  "paid_funded_nano" bigint NOT NULL,
  "bonus_funded_nano" bigint NOT NULL,
  "other_funded_nano" bigint NOT NULL,
  "commission_eligible" boolean NOT NULL,
  "release_generation" bigint NOT NULL,
  "release_digest" text NOT NULL,
  "snapshot_digest" text NOT NULL,
  "occurred_at" timestamp with time zone NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "pending_referral_usage_events_v2_shape_check" CHECK (
    "commerce_ref" <> ''
    AND "provider_id" <> ''
    AND "account_class" IN ('b2c', 'b2b', 'openkeys', 'service')
    AND "official_nano" >= 0
    AND "charged_nano" >= 0
    AND "paid_funded_nano" >= 0
    AND "bonus_funded_nano" >= 0
    AND "other_funded_nano" >= 0
    AND "paid_funded_nano" + "bonus_funded_nano" + "other_funded_nano" = "charged_nano"
    AND "release_generation" > 0
    AND "release_digest" <> ''
    AND "snapshot_digest" <> ''
    AND (
      NOT "commission_eligible"
      OR ("account_class" = 'b2c' AND "paid_funded_nano" > 0)
    )
  )
);--> statement-breakpoint

CREATE INDEX "pending_referral_usage_events_v2_user_idx"
  ON "pending_referral_usage_events_v2"("commerce_user_id", "commerce_event_id");--> statement-breakpoint

CREATE TABLE "commission_entries_v2" (
  "id" bigserial PRIMARY KEY,
  "usage_event_id" bigint NOT NULL
    REFERENCES "partner_usage_events_v2"("id") ON DELETE restrict,
  "partner_id" uuid NOT NULL REFERENCES "partners"("id") ON DELETE restrict,
  "level" integer NOT NULL,
  "applied_bps" integer NOT NULL,
  "base_paid_funded_nano" bigint NOT NULL,
  "amount_nano" bigint NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "commission_entries_v2_source_partner_unique"
    UNIQUE("usage_event_id", "partner_id"),
  CONSTRAINT "commission_entries_v2_source_level_unique"
    UNIQUE("usage_event_id", "level"),
  CONSTRAINT "commission_entries_v2_shape_check" CHECK (
    "level" BETWEEN 0 AND 10
    AND "applied_bps" BETWEEN 0 AND 10000
    AND "base_paid_funded_nano" > 0
    AND "amount_nano" > 0
    AND "amount_nano" <= "base_paid_funded_nano"
  )
);--> statement-breakpoint

CREATE INDEX "commission_entries_v2_partner_time_idx"
  ON "commission_entries_v2"("partner_id", "created_at");--> statement-breakpoint

CREATE FUNCTION "enforce_commission_entry_v2_source"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  event_partner_id uuid;
  event_paid_funded_nano bigint;
  expected_partner_id uuid;
  expected_input_nano bigint;
  expected_bps integer;
BEGIN
  SELECT event."partner_id", event."paid_funded_nano"
  INTO event_partner_id, event_paid_funded_nano
  FROM "partner_usage_events_v2" event
  WHERE event."id" = NEW."usage_event_id"
    AND event."commission_eligible" IS TRUE
    AND event."account_class" = 'b2c'
    AND event."paid_funded_nano" > 0;

  IF NOT FOUND OR event_paid_funded_nano <> NEW."base_paid_funded_nano" THEN
    RAISE EXCEPTION 'commission v2 requires exact eligible paid-funded usage'
      USING ERRCODE = '23514';
  END IF;

  IF NEW."level" = 0 THEN
    expected_partner_id := event_partner_id;
    expected_input_nano := event_paid_funded_nano;
    SELECT partner."commission_bps"
    INTO expected_bps
    FROM "partners" partner
    WHERE partner."id" = expected_partner_id
      AND partner."status" = 'active'
    FOR SHARE;
  ELSE
    SELECT partner."parent_partner_id", previous."amount_nano"
    INTO expected_partner_id, expected_input_nano
    FROM "commission_entries_v2" previous
    JOIN "partners" partner ON partner."id" = previous."partner_id"
    WHERE previous."usage_event_id" = NEW."usage_event_id"
      AND previous."level" = NEW."level" - 1
    FOR SHARE OF partner;

    IF FOUND AND expected_partner_id IS NOT NULL THEN
      SELECT partner."sub_commission_bps"
      INTO expected_bps
      FROM "partners" partner
      WHERE partner."id" = expected_partner_id
        AND partner."status" = 'active'
      FOR SHARE;
    END IF;
  END IF;

  IF expected_partner_id IS NULL
     OR expected_input_nano IS NULL
     OR expected_bps IS NULL
     OR NEW."partner_id" <> expected_partner_id
     OR NEW."applied_bps" <> expected_bps
     OR NEW."amount_nano" <> floor(
       expected_input_nano::numeric * expected_bps::numeric / 10000
     )::bigint THEN
    RAISE EXCEPTION 'commission v2 entry does not match the active referral chain'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "commission_entries_v2_source_guard"
BEFORE INSERT ON "commission_entries_v2"
FOR EACH ROW EXECUTE FUNCTION "enforce_commission_entry_v2_source"();

CREATE FUNCTION "reject_paid_funded_commission_v2_mutation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'paid-funded commission v2 authority is immutable'
    USING ERRCODE = '23514';
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_usage_events_v2_immutable"
BEFORE UPDATE OR DELETE ON "partner_usage_events_v2"
FOR EACH ROW EXECUTE FUNCTION "reject_paid_funded_commission_v2_mutation"();--> statement-breakpoint

CREATE TRIGGER "commission_entries_v2_immutable"
BEFORE UPDATE OR DELETE ON "commission_entries_v2"
FOR EACH ROW EXECUTE FUNCTION "reject_paid_funded_commission_v2_mutation"();
