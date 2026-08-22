-- Dormant authority and request-workflow checkpoint for the unified partner administration.
--
-- This migration is deliberately application-compatible: current binaries keep their existing
-- Team/B2B behavior, while the dependent release gains durable places to record delegated
-- authority, review requests and Commerce effects. No request, grant or outbox item is created by
-- the migration itself.

CREATE TYPE "partner_request_type" AS ENUM (
  'b2b_conversion',
  'b2b_pricing',
  'commission_change'
);--> statement-breakpoint

CREATE TYPE "partner_request_status" AS ENUM (
  'pending',
  'approved',
  'rejected',
  'applied',
  'apply_failed'
);--> statement-breakpoint

CREATE TYPE "partner_request_effect_status" AS ENUM (
  'pending',
  'processing',
  'applied',
  'failed'
);--> statement-breakpoint

-- Team invitations can be disabled independently from suspending a partner. B2B delegation is a
-- separate authority from B2B self-service: a partner may price their own referrals without being
-- allowed to pass that margin authority further down the team.
ALTER TABLE "partners" ADD COLUMN "team_invites_enabled" boolean DEFAULT true NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "b2b_can_delegate" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partners" ADD COLUMN "b2b_grant_source_partner_id" uuid;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_b2b_grant_source_fk"
  FOREIGN KEY ("b2b_grant_source_partner_id") REFERENCES "partners"("id")
  ON DELETE RESTRICT ON UPDATE NO ACTION;--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_b2b_authority_shape_check" CHECK (
  (
    "b2b_enabled"
    AND "b2b_max_discount_bps" BETWEEN 0 AND 9500
  ) OR (
    NOT "b2b_enabled"
    AND "b2b_max_discount_bps" = 0
    AND NOT "b2b_can_delegate"
    AND "b2b_grant_source_partner_id" IS NULL
  )
);--> statement-breakpoint
ALTER TABLE "partners" ADD CONSTRAINT "partners_b2b_delegate_requires_grant_check" CHECK (
  NOT "b2b_can_delegate" OR "b2b_enabled"
);--> statement-breakpoint

ALTER TABLE "partner_invites" ADD COLUMN "team_invites_enabled" boolean DEFAULT true NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD COLUMN "b2b_can_delegate" boolean DEFAULT false NOT NULL;--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_b2b_authority_shape_check" CHECK (
  (
    "b2b_enabled"
    AND "b2b_max_discount_bps" BETWEEN 0 AND 9500
  ) OR (
    NOT "b2b_enabled"
    AND "b2b_max_discount_bps" = 0
    AND NOT "b2b_can_delegate"
  )
);--> statement-breakpoint
ALTER TABLE "partner_invites" ADD CONSTRAINT "partner_invites_b2b_delegate_requires_grant_check" CHECK (
  NOT "b2b_can_delegate" OR "b2b_enabled"
);--> statement-breakpoint

-- A non-NULL source means the grant was delegated by the direct parent. NULL means the platform
-- granted it directly, including an admin override on a partner who happens to have a parent.
CREATE FUNCTION "enforce_partner_b2b_authority_bounds"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  source_enabled boolean;
  source_can_delegate boolean;
  source_max_bps integer;
BEGIN
  IF NOT NEW."b2b_enabled" THEN
    RETURN NEW;
  END IF;

  IF NEW."b2b_grant_source_partner_id" IS NULL THEN
    RETURN NEW;
  END IF;

  IF NEW."parent_partner_id" IS DISTINCT FROM NEW."b2b_grant_source_partner_id" THEN
    RAISE EXCEPTION 'delegated B2B grant source must be the direct parent'
      USING ERRCODE = '23514';
  END IF;

  SELECT source."b2b_enabled", source."b2b_can_delegate", source."b2b_max_discount_bps"
  INTO source_enabled, source_can_delegate, source_max_bps
  FROM "partners" source
  WHERE source."id" = NEW."b2b_grant_source_partner_id"
  FOR KEY SHARE;

  IF source_enabled IS DISTINCT FROM true
     OR source_can_delegate IS DISTINCT FROM true
     OR NEW."b2b_max_discount_bps" > source_max_bps THEN
    RAISE EXCEPTION 'delegated B2B authority exceeds the direct parent grant'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partners_b2b_authority_bounds_guard"
BEFORE INSERT OR UPDATE OF
  "parent_partner_id", "b2b_enabled", "b2b_max_discount_bps", "b2b_can_delegate",
  "b2b_grant_source_partner_id"
ON "partners"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_b2b_authority_bounds"();--> statement-breakpoint

-- A partner-authored invite may carry B2B rights only when the inviter owns a delegable grant.
-- Root/admin invites have partner_id=NULL and therefore snapshot a direct platform grant.
CREATE FUNCTION "enforce_partner_invite_b2b_authority_bounds"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  inviter_enabled boolean;
  inviter_can_delegate boolean;
  inviter_max_bps integer;
BEGIN
  IF NOT NEW."b2b_enabled" OR NEW."partner_id" IS NULL THEN
    RETURN NEW;
  END IF;

  SELECT inviter."b2b_enabled", inviter."b2b_can_delegate", inviter."b2b_max_discount_bps"
  INTO inviter_enabled, inviter_can_delegate, inviter_max_bps
  FROM "partners" inviter
  WHERE inviter."id" = NEW."partner_id"
  FOR KEY SHARE;

  IF inviter_enabled IS DISTINCT FROM true
     OR inviter_can_delegate IS DISTINCT FROM true
     OR NEW."b2b_max_discount_bps" > inviter_max_bps THEN
    RAISE EXCEPTION 'partner invite B2B authority exceeds the inviter grant'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_invites_b2b_authority_bounds_guard"
BEFORE INSERT OR UPDATE OF
  "partner_id", "b2b_enabled", "b2b_max_discount_bps", "b2b_can_delegate"
ON "partner_invites"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_invite_b2b_authority_bounds"();--> statement-breakpoint

-- The application must clamp/revoke inherited descendants and pending invites in the same
-- transaction before narrowing a source grant. This guard makes a partial cascade impossible.
CREATE FUNCTION "enforce_partner_b2b_authority_narrowing"()
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

CREATE TRIGGER "partners_b2b_authority_narrowing_guard"
BEFORE UPDATE OF "b2b_enabled", "b2b_max_discount_bps", "b2b_can_delegate"
ON "partners"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_b2b_authority_narrowing"();--> statement-breakpoint

-- One immutable request records what the partner asked for. Approval fields record the exact
-- platform decision; a B2B request becomes `applied` only after Commerce acknowledges the durable
-- effect. Commission changes can move pending→applied in the same Sales transaction.
CREATE TABLE "partner_requests" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
  "request_type" "partner_request_type" NOT NULL,
  "status" "partner_request_status" DEFAULT 'pending' NOT NULL,
  "requester_partner_id" uuid NOT NULL,
  "subject_partner_id" uuid,
  "commerce_user_id" uuid,
  "reason" text NOT NULL,
  "state_snapshot" jsonb DEFAULT '{}'::jsonb NOT NULL,
  "requested_commission_bps" integer,
  "requested_discount_bps" integer,
  "approved_commission_bps" integer,
  "approved_discount_bps" integer,
  "reviewer_actor" text,
  "reviewer_note" text,
  "reviewed_at" timestamp with time zone,
  "applied_at" timestamp with time zone,
  "apply_attempts" integer DEFAULT 0 NOT NULL,
  "last_apply_error" text,
  "idempotency_key" text NOT NULL,
  "version" integer DEFAULT 1 NOT NULL,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  "updated_at" timestamp with time zone DEFAULT now() NOT NULL,
  CONSTRAINT "partner_requests_requester_fk" FOREIGN KEY ("requester_partner_id")
    REFERENCES "partners"("id") ON DELETE RESTRICT ON UPDATE NO ACTION,
  CONSTRAINT "partner_requests_subject_fk" FOREIGN KEY ("subject_partner_id")
    REFERENCES "partners"("id") ON DELETE RESTRICT ON UPDATE NO ACTION,
  CONSTRAINT "partner_requests_reason_check" CHECK (length(btrim("reason")) BETWEEN 1 AND 4000),
  CONSTRAINT "partner_requests_snapshot_check" CHECK (jsonb_typeof("state_snapshot") = 'object'),
  CONSTRAINT "partner_requests_idempotency_check" CHECK (length("idempotency_key") BETWEEN 8 AND 200),
  CONSTRAINT "partner_requests_attempts_check" CHECK ("apply_attempts" >= 0),
  CONSTRAINT "partner_requests_version_check" CHECK ("version" > 0),
  CONSTRAINT "partner_requests_requested_commission_check" CHECK (
    "requested_commission_bps" IS NULL OR "requested_commission_bps" BETWEEN 0 AND 10000
  ),
  CONSTRAINT "partner_requests_requested_discount_check" CHECK (
    "requested_discount_bps" IS NULL OR "requested_discount_bps" BETWEEN 0 AND 9500
  ),
  CONSTRAINT "partner_requests_approved_commission_check" CHECK (
    "approved_commission_bps" IS NULL OR "approved_commission_bps" BETWEEN 0 AND 10000
  ),
  CONSTRAINT "partner_requests_approved_discount_check" CHECK (
    "approved_discount_bps" IS NULL OR "approved_discount_bps" BETWEEN 0 AND 9500
  ),
  CONSTRAINT "partner_requests_subject_shape_check" CHECK (
    (
      "request_type" = 'commission_change'
      AND "subject_partner_id" IS NOT NULL
      AND "subject_partner_id" = "requester_partner_id"
      AND "commerce_user_id" IS NULL
      AND "requested_commission_bps" IS NOT NULL
      AND "requested_discount_bps" IS NULL
    ) OR (
      "request_type" IN ('b2b_conversion', 'b2b_pricing')
      AND "subject_partner_id" IS NULL
      AND "commerce_user_id" IS NOT NULL
      AND "requested_commission_bps" IS NULL
      AND "requested_discount_bps" IS NOT NULL
    )
  ),
  CONSTRAINT "partner_requests_type_status_check" CHECK (
    "request_type" <> 'commission_change'
    OR "status" IN ('pending', 'rejected', 'applied')
  ),
  CONSTRAINT "partner_requests_decision_shape_check" CHECK (
    (
      "status" = 'pending'
      AND "approved_commission_bps" IS NULL
      AND "approved_discount_bps" IS NULL
      AND "reviewer_actor" IS NULL
      AND "reviewer_note" IS NULL
      AND "reviewed_at" IS NULL
      AND "applied_at" IS NULL
      AND "last_apply_error" IS NULL
    ) OR (
      "status" = 'rejected'
      AND "approved_commission_bps" IS NULL
      AND "approved_discount_bps" IS NULL
      AND COALESCE(length(btrim("reviewer_actor")), 0) > 0
      AND COALESCE(length(btrim("reviewer_note")), 0) > 0
      AND "reviewed_at" IS NOT NULL
      AND "applied_at" IS NULL
      AND "last_apply_error" IS NULL
    ) OR (
      "status" IN ('approved', 'applied', 'apply_failed')
      AND COALESCE(length(btrim("reviewer_actor")), 0) > 0
      AND COALESCE(length(btrim("reviewer_note")), 0) > 0
      AND "reviewed_at" IS NOT NULL
      AND (
        ("request_type" = 'commission_change'
          AND "approved_commission_bps" IS NOT NULL
          AND "approved_discount_bps" IS NULL)
        OR
        ("request_type" IN ('b2b_conversion', 'b2b_pricing')
          AND "approved_commission_bps" IS NULL
          AND "approved_discount_bps" IS NOT NULL)
      )
      AND (("status" = 'applied') = ("applied_at" IS NOT NULL))
      AND (("status" = 'apply_failed') = ("last_apply_error" IS NOT NULL))
    )
  )
);--> statement-breakpoint

CREATE UNIQUE INDEX "partner_requests_idempotency_uidx"
  ON "partner_requests" ("idempotency_key");--> statement-breakpoint
CREATE UNIQUE INDEX "partner_requests_pending_commission_uidx"
  ON "partner_requests" ("subject_partner_id")
  WHERE "status" = 'pending' AND "request_type" = 'commission_change';--> statement-breakpoint
CREATE UNIQUE INDEX "partner_requests_pending_b2b_uidx"
  ON "partner_requests" ("requester_partner_id", "commerce_user_id")
  WHERE "status" = 'pending' AND "request_type" IN ('b2b_conversion', 'b2b_pricing');--> statement-breakpoint
CREATE INDEX "partner_requests_admin_queue_idx"
  ON "partner_requests" ("status", "created_at", "id");--> statement-breakpoint
CREATE INDEX "partner_requests_partner_time_idx"
  ON "partner_requests" ("requester_partner_id", "created_at", "id");--> statement-breakpoint
CREATE INDEX "partner_requests_commerce_user_idx"
  ON "partner_requests" ("commerce_user_id", "created_at")
  WHERE "commerce_user_id" IS NOT NULL;--> statement-breakpoint

-- Requested provider terms are write-once. NULL means “remove this provider override and use the
-- approved base discount”. The closed provider set matches Commerce's current pricing contract.
CREATE TABLE "partner_request_provider_terms" (
  "request_id" uuid NOT NULL,
  "provider_id" text NOT NULL,
  "requested_discount_bps" integer,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  CONSTRAINT "partner_request_provider_terms_pk" PRIMARY KEY ("request_id", "provider_id"),
  CONSTRAINT "partner_request_provider_terms_request_fk" FOREIGN KEY ("request_id")
    REFERENCES "partner_requests"("id") ON DELETE RESTRICT ON UPDATE NO ACTION,
  CONSTRAINT "partner_request_provider_terms_provider_check" CHECK (
    "provider_id" IN ('anthropic', 'openai', 'google', 'kimi', 'glm')
  ),
  CONSTRAINT "partner_request_provider_terms_discount_check" CHECK (
    "requested_discount_bps" IS NULL OR "requested_discount_bps" BETWEEN 0 AND 9500
  )
);--> statement-breakpoint

-- The administrator's provider-level decision is a separate immutable fact, so an approved NULL
-- (remove override) is distinguishable from an undecided request.
CREATE TABLE "partner_request_provider_decisions" (
  "request_id" uuid NOT NULL,
  "provider_id" text NOT NULL,
  "approved_discount_bps" integer,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  CONSTRAINT "partner_request_provider_decisions_pk" PRIMARY KEY ("request_id", "provider_id"),
  CONSTRAINT "partner_request_provider_decisions_term_fk"
    FOREIGN KEY ("request_id", "provider_id")
    REFERENCES "partner_request_provider_terms"("request_id", "provider_id")
    ON DELETE RESTRICT ON UPDATE NO ACTION,
  CONSTRAINT "partner_request_provider_decisions_discount_check" CHECK (
    "approved_discount_bps" IS NULL OR "approved_discount_bps" BETWEEN 0 AND 9500
  )
);--> statement-breakpoint

-- One durable Commerce effect per approved B2B request. The JSON payload is the exact approved
-- bundle and is immutable; retries reuse the stable idempotency key.
CREATE TABLE "partner_request_effects" (
  "id" uuid PRIMARY KEY DEFAULT gen_random_uuid() NOT NULL,
  "request_id" uuid NOT NULL,
  "status" "partner_request_effect_status" DEFAULT 'pending' NOT NULL,
  "payload" jsonb NOT NULL,
  "idempotency_key" text NOT NULL,
  "attempts" integer DEFAULT 0 NOT NULL,
  "next_attempt_at" timestamp with time zone DEFAULT now() NOT NULL,
  "locked_at" timestamp with time zone,
  "locked_by" text,
  "applied_at" timestamp with time zone,
  "last_error" text,
  "created_at" timestamp with time zone DEFAULT now() NOT NULL,
  "updated_at" timestamp with time zone DEFAULT now() NOT NULL,
  CONSTRAINT "partner_request_effects_request_fk" FOREIGN KEY ("request_id")
    REFERENCES "partner_requests"("id") ON DELETE RESTRICT ON UPDATE NO ACTION,
  CONSTRAINT "partner_request_effects_request_uidx" UNIQUE ("request_id"),
  CONSTRAINT "partner_request_effects_idempotency_uidx" UNIQUE ("idempotency_key"),
  CONSTRAINT "partner_request_effects_payload_check" CHECK (jsonb_typeof("payload") = 'object'),
  CONSTRAINT "partner_request_effects_idempotency_check" CHECK (length("idempotency_key") BETWEEN 8 AND 200),
  CONSTRAINT "partner_request_effects_attempts_check" CHECK ("attempts" >= 0),
  CONSTRAINT "partner_request_effects_status_shape_check" CHECK (
    ("status" = 'pending' AND "locked_at" IS NULL AND "locked_by" IS NULL
      AND "applied_at" IS NULL AND "last_error" IS NULL)
    OR
    ("status" = 'processing' AND "locked_at" IS NOT NULL
      AND COALESCE(length(btrim("locked_by")), 0) > 0
      AND "applied_at" IS NULL)
    OR
    ("status" = 'applied' AND "locked_at" IS NULL AND "locked_by" IS NULL
      AND "applied_at" IS NOT NULL AND "last_error" IS NULL)
    OR
    ("status" = 'failed' AND "locked_at" IS NULL AND "locked_by" IS NULL
      AND "applied_at" IS NULL AND COALESCE(length(btrim("last_error")), 0) > 0)
  )
);--> statement-breakpoint

CREATE INDEX "partner_request_effects_claim_idx"
  ON "partner_request_effects" ("status", "next_attempt_at", "created_at")
  WHERE "status" IN ('pending', 'failed');--> statement-breakpoint

CREATE FUNCTION "enforce_partner_request_transition"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW."request_type" IS DISTINCT FROM OLD."request_type"
     OR NEW."requester_partner_id" IS DISTINCT FROM OLD."requester_partner_id"
     OR NEW."subject_partner_id" IS DISTINCT FROM OLD."subject_partner_id"
     OR NEW."commerce_user_id" IS DISTINCT FROM OLD."commerce_user_id"
     OR NEW."reason" IS DISTINCT FROM OLD."reason"
     OR NEW."state_snapshot" IS DISTINCT FROM OLD."state_snapshot"
     OR NEW."requested_commission_bps" IS DISTINCT FROM OLD."requested_commission_bps"
     OR NEW."requested_discount_bps" IS DISTINCT FROM OLD."requested_discount_bps"
     OR NEW."idempotency_key" IS DISTINCT FROM OLD."idempotency_key"
     OR NEW."created_at" IS DISTINCT FROM OLD."created_at" THEN
    RAISE EXCEPTION 'partner request payload is immutable' USING ERRCODE = '23514';
  END IF;

  IF OLD."status" IN ('rejected', 'applied') THEN
    RAISE EXCEPTION 'terminal partner request is immutable' USING ERRCODE = '23514';
  END IF;

  IF NOT (
    NEW."status" = OLD."status"
    OR (OLD."status" = 'pending' AND NEW."status" IN ('approved', 'rejected', 'applied'))
    OR (OLD."status" = 'approved' AND NEW."status" IN ('applied', 'apply_failed'))
    OR (OLD."status" = 'apply_failed' AND NEW."status" IN ('applied', 'apply_failed'))
  ) THEN
    RAISE EXCEPTION 'invalid partner request status transition' USING ERRCODE = '23514';
  END IF;

  IF OLD."status" <> 'pending' AND (
    NEW."approved_commission_bps" IS DISTINCT FROM OLD."approved_commission_bps"
    OR NEW."approved_discount_bps" IS DISTINCT FROM OLD."approved_discount_bps"
    OR NEW."reviewer_actor" IS DISTINCT FROM OLD."reviewer_actor"
    OR NEW."reviewer_note" IS DISTINCT FROM OLD."reviewer_note"
    OR NEW."reviewed_at" IS DISTINCT FROM OLD."reviewed_at"
  ) THEN
    RAISE EXCEPTION 'partner request decision is immutable' USING ERRCODE = '23514';
  END IF;

  NEW."version" := OLD."version" + 1;
  NEW."updated_at" := now();
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_requests_transition_guard"
BEFORE UPDATE ON "partner_requests"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_request_transition"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_request_provider_term_insert"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  request_kind partner_request_type;
  request_state partner_request_status;
BEGIN
  SELECT request."request_type", request."status"
  INTO request_kind, request_state
  FROM "partner_requests" request
  WHERE request."id" = NEW."request_id"
  FOR KEY SHARE;

  IF request_kind NOT IN ('b2b_conversion', 'b2b_pricing') OR request_state <> 'pending' THEN
    RAISE EXCEPTION 'provider term requires a pending B2B request' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_request_provider_terms_insert_guard"
BEFORE INSERT ON "partner_request_provider_terms"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_request_provider_term_insert"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_request_provider_decision_insert"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  request_state partner_request_status;
BEGIN
  SELECT request."status"
  INTO request_state
  FROM "partner_requests" request
  WHERE request."id" = NEW."request_id"
  FOR KEY SHARE;

  IF request_state NOT IN ('approved', 'applied', 'apply_failed') THEN
    RAISE EXCEPTION 'provider decision requires an approved B2B request' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_request_provider_decisions_insert_guard"
BEFORE INSERT ON "partner_request_provider_decisions"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_request_provider_decision_insert"();--> statement-breakpoint

CREATE FUNCTION "reject_partner_request_evidence_mutation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'partner request evidence is immutable' USING ERRCODE = '23514';
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_request_provider_terms_immutable"
BEFORE UPDATE OR DELETE ON "partner_request_provider_terms"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_request_evidence_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_request_provider_decisions_immutable"
BEFORE UPDATE OR DELETE ON "partner_request_provider_decisions"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_request_evidence_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_requests_immutable_delete"
BEFORE DELETE ON "partner_requests"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_request_evidence_mutation"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_request_effect_insert"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  request_kind partner_request_type;
  request_state partner_request_status;
BEGIN
  SELECT request."request_type", request."status"
  INTO request_kind, request_state
  FROM "partner_requests" request
  WHERE request."id" = NEW."request_id"
  FOR KEY SHARE;

  IF request_kind NOT IN ('b2b_conversion', 'b2b_pricing') OR request_state <> 'approved' THEN
    RAISE EXCEPTION 'Commerce effect requires an approved B2B request' USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_request_effects_insert_guard"
BEFORE INSERT ON "partner_request_effects"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_request_effect_insert"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_request_effect_transition"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF NEW."request_id" IS DISTINCT FROM OLD."request_id"
     OR NEW."payload" IS DISTINCT FROM OLD."payload"
     OR NEW."idempotency_key" IS DISTINCT FROM OLD."idempotency_key"
     OR NEW."created_at" IS DISTINCT FROM OLD."created_at" THEN
    RAISE EXCEPTION 'partner request effect identity is immutable' USING ERRCODE = '23514';
  END IF;

  IF OLD."status" = 'applied' THEN
    RAISE EXCEPTION 'applied partner request effect is immutable' USING ERRCODE = '23514';
  END IF;

  IF NOT (
    NEW."status" = OLD."status"
    OR (OLD."status" IN ('pending', 'failed') AND NEW."status" = 'processing')
    OR (OLD."status" = 'processing' AND NEW."status" IN ('pending', 'applied', 'failed'))
  ) THEN
    RAISE EXCEPTION 'invalid partner request effect transition' USING ERRCODE = '23514';
  END IF;

  NEW."updated_at" := now();
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_request_effects_transition_guard"
BEFORE UPDATE ON "partner_request_effects"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_request_effect_transition"();--> statement-breakpoint
CREATE TRIGGER "partner_request_effects_immutable_delete"
BEFORE DELETE ON "partner_request_effects"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_request_evidence_mutation"();--> statement-breakpoint

-- Audit evidence is append-only. Existing application code only inserts, so the guard is safe for
-- the currently deployed binary and makes every future admin/request decision reconstructable.
CREATE FUNCTION "reject_sales_audit_log_mutation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'sales audit log is immutable' USING ERRCODE = '23514';
END;
$$;--> statement-breakpoint

CREATE TRIGGER "sales_audit_log_immutable"
BEFORE UPDATE OR DELETE ON "sales_audit_log"
FOR EACH ROW EXECUTE FUNCTION "reject_sales_audit_log_mutation"();--> statement-breakpoint

-- The central Admin's existing LISTEN/SSE invalidation channel gains the two new live resources.
CREATE TRIGGER "partner_requests_admin_change_notify"
AFTER INSERT OR UPDATE OR DELETE ON "partner_requests"
FOR EACH STATEMENT EXECUTE FUNCTION "notify_sales_admin_change"();--> statement-breakpoint
CREATE TRIGGER "partner_request_effects_admin_change_notify"
AFTER INSERT OR UPDATE OR DELETE ON "partner_request_effects"
FOR EACH STATEMENT EXECUTE FUNCTION "notify_sales_admin_change"();
