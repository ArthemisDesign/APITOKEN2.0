-- Dormant Commerce-account membership and conserved Team-commission checkpoint.
--
-- The migration deliberately changes no partner, invite, session or money row. Existing Telegram
-- identities and additive commission evidence remain version 1 and the currently deployed binary
-- keeps working unchanged. The dependent producer will create only Commerce-linked memberships,
-- accept email-targeted invitations, and write calculation_version=2 commission rows whose net
-- amounts conserve the direct partner's gross commission across the active Team chain.

ALTER TABLE "partners" ADD COLUMN "commerce_user_id" uuid;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "program_enabled" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "program_started_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_program_membership_check" CHECK (
  NOT "program_enabled"
  OR ("commerce_user_id" IS NOT NULL AND "program_started_at" IS NOT NULL)
);--> statement-breakpoint
CREATE UNIQUE INDEX "partners_commerce_user_uidx"
  ON "partners"("commerce_user_id")
  WHERE "commerce_user_id" IS NOT NULL;--> statement-breakpoint

-- Legacy Telegram invitations keep both new fields NULL. A Commerce invitation is bound to the
-- immutable account UUID resolved by Commerce from the submitted email; the email itself remains
-- authoritative in Commerce and is used only as the existing outbox recipient snapshot.
ALTER TABLE "partner_invites" ADD COLUMN "commerce_user_id" uuid;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "revoked_at" timestamp with time zone;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_commerce_shape_check" CHECK (
  "commerce_user_id" IS NOT NULL OR "revoked_at" IS NULL
);--> statement-breakpoint
CREATE UNIQUE INDEX "partner_invites_open_commerce_uidx"
  ON "partner_invites"("commerce_user_id")
  WHERE "commerce_user_id" IS NOT NULL
    AND "consumed_at" IS NULL
    AND "revoked_at" IS NULL;--> statement-breakpoint

-- Version 1 is immutable historical/additive evidence. Version 2 stores, for each beneficiary,
-- the gross pool arriving at that level, the amount withheld for the next active ancestor, and
-- the resulting net payout. Thus amount_nano = gross_amount_nano - withheld_amount_nano and the
-- sum of all net entries for an event is exactly the direct partner's gross commission.
ALTER TABLE "commission_entries" ADD COLUMN "calculation_version" integer DEFAULT 1 NOT NULL;--> statement-breakpoint
ALTER TABLE "commission_entries" ADD COLUMN "gross_amount_nano" bigint;--> statement-breakpoint
ALTER TABLE "commission_entries" ADD COLUMN "withheld_amount_nano" bigint;--> statement-breakpoint
ALTER TABLE "commission_entries" ADD CONSTRAINT "commission_entries_calculation_check" CHECK (
  (
    "calculation_version" = 1
    AND "gross_amount_nano" IS NULL
    AND "withheld_amount_nano" IS NULL
  ) OR (
    "calculation_version" = 2
    AND "usage_event_id" IS NOT NULL
    AND "topup_id" IS NULL
    AND "gross_amount_nano" > 0
    AND "withheld_amount_nano" >= 0
    AND "withheld_amount_nano" < "gross_amount_nano"
    AND "amount_nano" = "gross_amount_nano" - "withheld_amount_nano"
    AND ("level" = 0 OR "applied_bps" BETWEEN 0 AND 2000)
  )
);--> statement-breakpoint

ALTER TABLE "commission_entries_v2" ADD COLUMN "calculation_version" integer DEFAULT 1 NOT NULL;--> statement-breakpoint
ALTER TABLE "commission_entries_v2" ADD COLUMN "gross_amount_nano" bigint;--> statement-breakpoint
ALTER TABLE "commission_entries_v2" ADD COLUMN "withheld_amount_nano" bigint;--> statement-breakpoint
ALTER TABLE "commission_entries_v2" ADD CONSTRAINT "commission_entries_v2_calculation_check" CHECK (
  (
    "calculation_version" = 1
    AND "gross_amount_nano" IS NULL
    AND "withheld_amount_nano" IS NULL
  ) OR (
    "calculation_version" = 2
    AND "gross_amount_nano" > 0
    AND "withheld_amount_nano" >= 0
    AND "withheld_amount_nano" < "gross_amount_nano"
    AND "amount_nano" = "gross_amount_nano" - "withheld_amount_nano"
    AND ("level" = 0 OR "applied_bps" BETWEEN 0 AND 2000)
  )
);--> statement-breakpoint

-- Validate one conserved entry independently from insert order. The function walks at most ten
-- payable levels, requires an active Commerce membership at every participating level, detects a
-- cycle, and derives the next cut from the child edge. A disabled/non-Commerce ancestor receives
-- nothing and therefore causes no withholding from the current partner.
CREATE FUNCTION "assert_conserved_commission_entry"(
  source_partner_id uuid,
  source_basis_nano bigint,
  source_occurred_at timestamp with time zone,
  entry_partner_id uuid,
  entry_level integer,
  entry_applied_bps integer,
  entry_gross_nano bigint,
  entry_withheld_nano bigint,
  entry_amount_nano bigint
)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
  current_partner_id uuid := source_partner_id;
  current_parent_id uuid;
  current_status text;
  current_program_enabled boolean;
  current_program_started_at timestamp with time zone;
  current_commission_bps integer;
  current_sub_commission_bps integer;
  current_parent_override_bps integer;
  parent_status text;
  parent_program_enabled boolean;
  parent_program_started_at timestamp with time zone;
  parent_sub_commission_bps integer;
  current_level integer := 0;
  current_applied_bps integer;
  current_gross_nano bigint;
  current_withheld_nano bigint;
  next_edge_bps integer;
  visited uuid[] := ARRAY[]::uuid[];
BEGIN
  IF source_basis_nano <= 0 OR entry_level < 0 OR entry_level >= 10 THEN
    RAISE EXCEPTION 'conserved commission source or level is invalid'
      USING ERRCODE = '23514';
  END IF;

  LOOP
    IF current_partner_id IS NULL OR current_level >= 10 THEN
      EXIT;
    END IF;
    IF current_partner_id = ANY(visited) THEN
      RAISE EXCEPTION 'conserved commission partner chain contains a cycle'
        USING ERRCODE = '23514';
    END IF;
    visited := array_append(visited, current_partner_id);

    SELECT partner."status"::text, partner."program_enabled", partner."program_started_at",
           partner."commission_bps",
           partner."sub_commission_bps", partner."parent_partner_id",
           partner."parent_override_bps"
    INTO current_status, current_program_enabled, current_program_started_at,
         current_commission_bps,
         current_sub_commission_bps, current_parent_id, current_parent_override_bps
    FROM "partners" partner
    WHERE partner."id" = current_partner_id
    FOR SHARE;

    IF NOT FOUND
       OR current_status <> 'active'
       OR NOT current_program_enabled
       OR current_program_started_at > source_occurred_at THEN
      EXIT;
    END IF;

    IF current_level = 0 THEN
      current_applied_bps := current_commission_bps;
      current_gross_nano := floor(
        source_basis_nano::numeric * current_commission_bps::numeric / 10000
      )::bigint;
    END IF;
    IF current_gross_nano IS NULL OR current_gross_nano <= 0 THEN
      EXIT;
    END IF;

    current_withheld_nano := 0;
    next_edge_bps := NULL;
    IF current_parent_id IS NOT NULL AND current_level + 1 < 10 THEN
      SELECT parent."status"::text, parent."program_enabled", parent."program_started_at",
             parent."sub_commission_bps"
      INTO parent_status, parent_program_enabled, parent_program_started_at,
           parent_sub_commission_bps
      FROM "partners" parent
      WHERE parent."id" = current_parent_id
      FOR SHARE;

      IF FOUND
         AND parent_status = 'active'
         AND parent_program_enabled
         AND parent_program_started_at <= source_occurred_at THEN
        next_edge_bps := COALESCE(current_parent_override_bps, parent_sub_commission_bps);
        IF next_edge_bps NOT BETWEEN 0 AND 2000 THEN
          RAISE EXCEPTION 'conserved Team share must be between 0 and 2000 bps'
            USING ERRCODE = '23514';
        END IF;
        current_withheld_nano := floor(
          current_gross_nano::numeric * next_edge_bps::numeric / 10000
        )::bigint;
      END IF;
    END IF;

    IF current_level = entry_level THEN
      IF entry_partner_id <> current_partner_id
         OR entry_applied_bps <> current_applied_bps
         OR entry_gross_nano <> current_gross_nano
         OR entry_withheld_nano <> current_withheld_nano
         OR entry_amount_nano <> current_gross_nano - current_withheld_nano THEN
        RAISE EXCEPTION 'commission entry does not match the conserved Team chain'
          USING ERRCODE = '23514';
      END IF;
      RETURN;
    END IF;

    IF current_withheld_nano <= 0 OR next_edge_bps IS NULL THEN
      EXIT;
    END IF;
    current_partner_id := current_parent_id;
    current_applied_bps := next_edge_bps;
    current_gross_nano := current_withheld_nano;
    current_level := current_level + 1;
  END LOOP;

  RAISE EXCEPTION 'commission entry is outside the conserved Team chain'
    USING ERRCODE = '23514';
END;
$$;--> statement-breakpoint

CREATE FUNCTION "enforce_commission_entry_calculation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  event_partner_id uuid;
  event_basis_nano bigint;
  event_occurred_at timestamp with time zone;
BEGIN
  IF NEW."calculation_version" = 1 THEN
    RETURN NEW;
  END IF;
  IF NEW."calculation_version" <> 2 OR NEW."usage_event_id" IS NULL OR NEW."topup_id" IS NOT NULL THEN
    RAISE EXCEPTION 'unsupported commission calculation version or source'
      USING ERRCODE = '23514';
  END IF;

  SELECT event."partner_id", event."amount_nano", event."occurred_at"
  INTO event_partner_id, event_basis_nano, event_occurred_at
  FROM "partner_usage_events" event
  WHERE event."id" = NEW."usage_event_id"
  FOR SHARE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'conserved commission requires a usage event'
      USING ERRCODE = '23514';
  END IF;

  PERFORM "assert_conserved_commission_entry"(
    event_partner_id, event_basis_nano, event_occurred_at,
    NEW."partner_id", NEW."level", NEW."applied_bps",
    NEW."gross_amount_nano", NEW."withheld_amount_nano", NEW."amount_nano"
  );
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "commission_entries_calculation_guard"
BEFORE INSERT ON "commission_entries"
FOR EACH ROW EXECUTE FUNCTION "enforce_commission_entry_calculation"();--> statement-breakpoint

-- Keep the historical version-1 validation byte-for-byte equivalent to migration 0024, while
-- dispatching version 2 to the conserved-chain authority above.
CREATE OR REPLACE FUNCTION "enforce_commission_entry_v2_source"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  event_partner_id uuid;
  event_paid_funded_nano bigint;
  event_occurred_at timestamp with time zone;
  expected_partner_id uuid;
  expected_input_nano bigint;
  expected_bps integer;
  edge_override_bps integer;
BEGIN
  SELECT event."partner_id", event."paid_funded_nano", event."occurred_at"
  INTO event_partner_id, event_paid_funded_nano, event_occurred_at
  FROM "partner_usage_events_v2" event
  WHERE event."id" = NEW."usage_event_id"
    AND event."commission_eligible" IS TRUE
    AND event."account_class" = 'b2c'
    AND event."paid_funded_nano" > 0;

  IF NOT FOUND OR event_paid_funded_nano <> NEW."base_paid_funded_nano" THEN
    RAISE EXCEPTION 'commission v2 requires exact eligible paid-funded usage'
      USING ERRCODE = '23514';
  END IF;

  IF NEW."calculation_version" = 2 THEN
    PERFORM "assert_conserved_commission_entry"(
      event_partner_id, event_paid_funded_nano, event_occurred_at,
      NEW."partner_id", NEW."level", NEW."applied_bps",
      NEW."gross_amount_nano", NEW."withheld_amount_nano", NEW."amount_nano"
    );
    RETURN NEW;
  END IF;
  IF NEW."calculation_version" <> 1 THEN
    RAISE EXCEPTION 'unsupported commission v2 calculation version'
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
    SELECT child."parent_partner_id", child."parent_override_bps", previous."amount_nano"
    INTO expected_partner_id, edge_override_bps, expected_input_nano
    FROM "commission_entries_v2" previous
    JOIN "partners" child ON child."id" = previous."partner_id"
    WHERE previous."usage_event_id" = NEW."usage_event_id"
      AND previous."level" = NEW."level" - 1
    FOR SHARE OF child;

    IF FOUND AND expected_partner_id IS NOT NULL THEN
      SELECT COALESCE(edge_override_bps, parent."sub_commission_bps")
      INTO expected_bps
      FROM "partners" parent
      WHERE parent."id" = expected_partner_id
        AND parent."status" = 'active'
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
$$;
