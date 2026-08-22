-- Terminal Commerce invitations are historical authority evidence. Migration 0026 introduced
-- revoked_at after the Team/B2B narrowing guards had already been installed, so those older
-- guards still treated a revoked, unconsumed invitation as a live dependent grant. Narrowing then
-- either failed or required rewriting revoked evidence. Ignore only terminal revoked rows in the
-- live-grant scans and fail closed on later mutation/deletion of terminal Commerce invitations.

CREATE OR REPLACE FUNCTION "enforce_partner_team_override_ceiling_update"()
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
      AND invite."revoked_at" IS NULL
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

CREATE OR REPLACE FUNCTION "enforce_partner_b2b_authority_narrowing"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW."b2b_enabled"
     AND NEW."b2b_can_delegate"
     AND OLD."b2b_enabled"
     AND OLD."b2b_can_delegate"
     AND NEW."b2b_max_discount_bps" >= OLD."b2b_max_discount_bps" THEN
    RETURN NEW;
  END IF;

  IF EXISTS (
    SELECT 1
    FROM "partners" child
    WHERE child."b2b_grant_source_partner_id" = NEW."id"
      AND child."b2b_enabled"
      AND (
        NOT NEW."b2b_enabled"
        OR NOT NEW."b2b_can_delegate"
        OR child."b2b_max_discount_bps" > NEW."b2b_max_discount_bps"
      )
  ) OR EXISTS (
    SELECT 1
    FROM "partner_invites" invite
    WHERE invite."partner_id" = NEW."id"
      AND invite."consumed_at" IS NULL
      AND invite."revoked_at" IS NULL
      AND invite."b2b_enabled"
      AND (
        NOT NEW."b2b_enabled"
        OR NOT NEW."b2b_can_delegate"
        OR invite."b2b_max_discount_bps" > NEW."b2b_max_discount_bps"
      )
  ) THEN
    RAISE EXCEPTION 'B2B authority has inherited grants above the requested value'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE FUNCTION "enforce_terminal_commerce_invite_immutable"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD."commerce_user_id" IS NOT NULL
     AND (OLD."consumed_at" IS NOT NULL OR OLD."revoked_at" IS NOT NULL)
     AND NEW IS DISTINCT FROM OLD THEN
    RAISE EXCEPTION 'terminal Commerce invitation is immutable'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_invites_terminal_commerce_immutable_guard"
BEFORE UPDATE ON "partner_invites"
FOR EACH ROW EXECUTE FUNCTION "enforce_terminal_commerce_invite_immutable"();--> statement-breakpoint

CREATE FUNCTION "prevent_commerce_invite_delete"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF OLD."commerce_user_id" IS NOT NULL THEN
    RAISE EXCEPTION 'Commerce invitation evidence cannot be deleted'
      USING ERRCODE = '23514';
  END IF;
  RETURN OLD;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_invites_commerce_delete_guard"
BEFORE DELETE ON "partner_invites"
FOR EACH ROW EXECUTE FUNCTION "prevent_commerce_invite_delete"();
