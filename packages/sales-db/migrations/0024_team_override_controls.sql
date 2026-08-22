-- Per-edge team commission control, delivered before any application starts writing it.
--
-- `team_override_max_bps` is the maximum team override this partner may select. The platform
-- hard ceiling is 20% (2000 bps). NULL means the rolling-deploy/default ceiling of 20%; the
-- dependent writer stores an explicit value whenever an admin or inviter narrows it.
-- `parent_override_bps` belongs to the child row and is therefore
-- the exact rate the direct parent receives from this child's commission. Existing relationships
-- stay NULL during expand and keep the deployed `parent.sub_commission_bps` behavior until the
-- consumer explicitly migrates them through ordinary writes.
ALTER TABLE "partners" ADD COLUMN "team_override_max_bps" integer;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "parent_override_bps" integer;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_team_override_max_check"
	CHECK ("team_override_max_bps" IS NULL OR "team_override_max_bps" BETWEEN 0 AND 2000);--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_parent_override_check"
	CHECK ("parent_override_bps" IS NULL OR "parent_override_bps" BETWEEN 0 AND 2000);--> statement-breakpoint

-- Invites snapshot both the exact parent edge and the ceiling delegated to the invited partner.
-- NULL `parent_override_bps` remains accepted for the currently deployed writer during rollout.
ALTER TABLE "partner_invites" ADD COLUMN "team_override_max_bps" integer;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "parent_override_bps" integer;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_team_override_max_check"
	CHECK ("team_override_max_bps" IS NULL OR "team_override_max_bps" BETWEEN 0 AND 2000);--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_parent_override_check"
	CHECK ("parent_override_bps" IS NULL OR "parent_override_bps" BETWEEN 0 AND 2000);--> statement-breakpoint

-- A child cannot receive more delegation than its parent owns, and an explicit edge rate cannot
-- exceed the parent's ceiling. Root partners have no edge rate. NULL remains the rolling-deploy
-- representation for relationships created by the previous binary.
CREATE FUNCTION "enforce_partner_team_override_bounds"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  parent_max_bps integer;
BEGIN
  IF NEW."parent_partner_id" IS NULL THEN
    IF NEW."parent_override_bps" IS NOT NULL THEN
      RAISE EXCEPTION 'root partner cannot have a parent override'
        USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
  END IF;

  SELECT COALESCE(parent."team_override_max_bps", 2000)
  INTO parent_max_bps
  FROM "partners" parent
  WHERE parent."id" = NEW."parent_partner_id"
  FOR KEY SHARE;

  IF parent_max_bps IS NULL
     OR (NEW."team_override_max_bps" IS NOT NULL AND NEW."team_override_max_bps" > parent_max_bps)
     OR (NEW."parent_override_bps" IS NOT NULL AND NEW."parent_override_bps" > parent_max_bps) THEN
    RAISE EXCEPTION 'partner team override exceeds the parent ceiling'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partners_team_override_bounds_guard"
BEFORE INSERT OR UPDATE OF "parent_partner_id", "parent_override_bps", "team_override_max_bps"
ON "partners"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_team_override_bounds"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_invite_team_override_bounds"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  parent_max_bps integer;
BEGIN
  IF NEW."partner_id" IS NULL THEN
    IF NEW."parent_override_bps" IS NOT NULL THEN
      RAISE EXCEPTION 'root invite cannot have a parent override'
        USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
  END IF;

  SELECT COALESCE(parent."team_override_max_bps", 2000)
  INTO parent_max_bps
  FROM "partners" parent
  WHERE parent."id" = NEW."partner_id"
  FOR KEY SHARE;

  IF parent_max_bps IS NULL
     OR (NEW."team_override_max_bps" IS NOT NULL AND NEW."team_override_max_bps" > parent_max_bps)
     OR (NEW."parent_override_bps" IS NOT NULL AND NEW."parent_override_bps" > parent_max_bps) THEN
    RAISE EXCEPTION 'partner invite override exceeds the inviter ceiling'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_invites_team_override_bounds_guard"
BEFORE INSERT OR UPDATE OF "partner_id", "parent_override_bps", "team_override_max_bps"
ON "partner_invites"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_invite_team_override_bounds"();--> statement-breakpoint

-- A ceiling cannot be lowered underneath an already explicit edge or delegated child ceiling.
-- The dependent admin writer will lower affected children and pending invites first in the same
-- transaction, then lower the parent. Existing NULL edges do not block the expand release.
CREATE FUNCTION "enforce_partner_team_override_ceiling_update"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF COALESCE(NEW."team_override_max_bps", 2000) >= COALESCE(OLD."team_override_max_bps", 2000) THEN
    RETURN NEW;
  END IF;

  IF EXISTS (
    SELECT 1
    FROM "partners" child
    WHERE child."parent_partner_id" = NEW."id"
      AND (
        child."team_override_max_bps" > COALESCE(NEW."team_override_max_bps", 2000)
        OR child."parent_override_bps" > COALESCE(NEW."team_override_max_bps", 2000)
      )
  ) OR EXISTS (
    SELECT 1
    FROM "partner_invites" invite
    WHERE invite."partner_id" = NEW."id"
      AND invite."consumed_at" IS NULL
      AND (
        invite."team_override_max_bps" > COALESCE(NEW."team_override_max_bps", 2000)
        OR invite."parent_override_bps" > COALESCE(NEW."team_override_max_bps", 2000)
      )
  ) THEN
    RAISE EXCEPTION 'team override ceiling has dependent grants above the requested value'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partners_team_override_ceiling_update_guard"
BEFORE UPDATE OF "team_override_max_bps" ON "partners"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_team_override_ceiling_update"();--> statement-breakpoint

-- Extend the immutable v2 authority before the application uses explicit edges. When the edge is
-- NULL, the old parent's `sub_commission_bps` is selected exactly, so the currently deployed
-- commission writer remains byte-for-byte valid during the migration-first interval.
CREATE OR REPLACE FUNCTION "enforce_commission_entry_v2_source"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  event_partner_id uuid;
  event_paid_funded_nano bigint;
  expected_partner_id uuid;
  expected_input_nano bigint;
  expected_bps integer;
  edge_override_bps integer;
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
