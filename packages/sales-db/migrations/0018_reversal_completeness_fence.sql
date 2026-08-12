-- Close two dormant races before the payment-reversal consumer is allowed to ship:
--   * a reversal with zero inserted adjustments still needs a deferred completeness check;
--   * a late funding/commission allocation must not appear behind an already committed reversal.
-- All four writers serialize on the immutable funding-lot row. Reversal creation is always
-- SERIALIZABLE; a late allocation against a reversed lot can commit only in a SERIALIZABLE
-- transaction that also appends every exact negative adjustment.

CREATE FUNCTION "lock_partner_usage_funding_lot"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM 1
  FROM "partner_paid_funding_lots" lot
  WHERE lot."id" = NEW."funding_lot_id"
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'usage funding allocation requires a funding lot'
      USING ERRCODE = '23503';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_usage_funding_alloc_lot_lock"
BEFORE INSERT ON "partner_usage_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "lock_partner_usage_funding_lot"();--> statement-breakpoint

CREATE FUNCTION "lock_partner_commission_funding_lot"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  target_lot_id bigint;
BEGIN
  SELECT usage_allocation."funding_lot_id"
  INTO target_lot_id
  FROM "partner_usage_funding_allocations" usage_allocation
  WHERE usage_allocation."id" = NEW."usage_funding_allocation_id"
  FOR SHARE;

  IF target_lot_id IS NULL THEN
    RAISE EXCEPTION 'commission funding allocation requires a usage allocation'
      USING ERRCODE = '23503';
  END IF;

  PERFORM 1
  FROM "partner_paid_funding_lots" lot
  WHERE lot."id" = target_lot_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'commission funding allocation requires a funding lot'
      USING ERRCODE = '23503';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_commission_funding_lot_lock"
BEFORE INSERT ON "partner_commission_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "lock_partner_commission_funding_lot"();--> statement-breakpoint

CREATE FUNCTION "lock_partner_reversal_funding_lot"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  IF current_setting('transaction_isolation') <> 'serializable' THEN
    RAISE EXCEPTION 'payment reversal accounting requires SERIALIZABLE isolation'
      USING ERRCODE = '25001';
  END IF;

  PERFORM 1
  FROM "partner_paid_funding_lots" lot
  WHERE lot."id" = NEW."funding_lot_id"
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'payment reversal requires a funding lot'
      USING ERRCODE = '23503';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_payment_reversal_lot_lock"
BEFORE INSERT ON "partner_payment_reversals"
FOR EACH ROW EXECUTE FUNCTION "lock_partner_reversal_funding_lot"();--> statement-breakpoint

CREATE FUNCTION "lock_partner_adjustment_funding_lot"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  target_lot_id bigint;
BEGIN
  SELECT usage_allocation."funding_lot_id"
  INTO target_lot_id
  FROM "partner_commission_funding_allocations" commission_allocation
  JOIN "partner_usage_funding_allocations" usage_allocation
    ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
  WHERE commission_allocation."id" = NEW."commission_funding_allocation_id"
  FOR SHARE OF commission_allocation, usage_allocation;

  IF target_lot_id IS NULL THEN
    RAISE EXCEPTION 'commission adjustment requires a funding allocation'
      USING ERRCODE = '23503';
  END IF;

  PERFORM 1
  FROM "partner_paid_funding_lots" lot
  WHERE lot."id" = target_lot_id
  FOR UPDATE;
  IF NOT FOUND THEN
    RAISE EXCEPTION 'commission adjustment requires a funding lot'
      USING ERRCODE = '23503';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_commission_adjustment_lot_lock"
BEFORE INSERT ON "partner_commission_adjustments"
FOR EACH ROW EXECUTE FUNCTION "lock_partner_adjustment_funding_lot"();--> statement-breakpoint

CREATE FUNCTION "assert_partner_reversal_complete"(target_lot_id bigint)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
  target_reversal_id bigint;
  target_user_id uuid;
  target_reversed_at timestamp with time zone;
BEGIN
  SELECT reversal."id", reversal."commerce_user_id", reversal."reversed_at"
  INTO target_reversal_id, target_user_id, target_reversed_at
  FROM "partner_payment_reversals" reversal
  WHERE reversal."funding_lot_id" = target_lot_id
  FOR SHARE;

  IF NOT FOUND THEN
    RETURN;
  END IF;

  -- A reversal can be consumed only after every locally known earlier usage row for that user has
  -- a complete paid-funding allocation. The cross-database consumer additionally fences this with
  -- source cursor catch-up; this DB check prevents a partial local backfill from being accepted.
  IF EXISTS (
    SELECT 1
    FROM "partner_usage_events" usage
    WHERE usage."commerce_user_id" = target_user_id
      AND usage."occurred_at" <= target_reversed_at
      AND COALESCE((
        SELECT sum(allocation."allocated_paid_nano")
        FROM "partner_usage_funding_allocations" allocation
        WHERE allocation."usage_event_id" = usage."id"
      ), 0) <> usage."amount_nano"
    UNION ALL
    SELECT 1
    FROM "partner_usage_events_v2" usage
    WHERE usage."commerce_user_id" = target_user_id
      AND usage."occurred_at" <= target_reversed_at
      AND COALESCE((
        SELECT sum(allocation."allocated_paid_nano")
        FROM "partner_usage_funding_allocations" allocation
        WHERE allocation."usage_event_v2_id" = usage."id"
      ), 0) <> usage."paid_funded_nano"
  ) THEN
    RAISE EXCEPTION 'payment reversal requires complete prior usage funding allocation'
      USING ERRCODE = '23514';
  END IF;

  -- Every commission row attached to a slice of this payment must have its deterministic slice.
  IF EXISTS (
    SELECT 1
    FROM "partner_usage_funding_allocations" usage_allocation
    JOIN "commission_entries" entry
      ON entry."usage_event_id" = usage_allocation."usage_event_id"
    WHERE usage_allocation."funding_lot_id" = target_lot_id
      AND NOT EXISTS (
        SELECT 1
        FROM "partner_commission_funding_allocations" commission_allocation
        WHERE commission_allocation."usage_funding_allocation_id" = usage_allocation."id"
          AND commission_allocation."commission_entry_id" = entry."id"
      )
    UNION ALL
    SELECT 1
    FROM "partner_usage_funding_allocations" usage_allocation
    JOIN "commission_entries_v2" entry
      ON entry."usage_event_id" = usage_allocation."usage_event_v2_id"
    WHERE usage_allocation."funding_lot_id" = target_lot_id
      AND NOT EXISTS (
        SELECT 1
        FROM "partner_commission_funding_allocations" commission_allocation
        WHERE commission_allocation."usage_funding_allocation_id" = usage_allocation."id"
          AND commission_allocation."commission_entry_v2_id" = entry."id"
      )
  ) THEN
    RAISE EXCEPTION 'payment reversal requires complete commission funding allocation'
      USING ERRCODE = '23514';
  END IF;

  -- Zero positive slices is a valid complete set. This predicate still runs from the reversal row
  -- itself, so a transaction cannot bypass validation merely by inserting zero adjustments.
  IF EXISTS (
    SELECT 1
    FROM "partner_commission_funding_allocations" commission_allocation
    JOIN "partner_usage_funding_allocations" usage_allocation
      ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
    WHERE usage_allocation."funding_lot_id" = target_lot_id
      AND commission_allocation."allocated_commission_nano" > 0
      AND NOT EXISTS (
        SELECT 1
        FROM "partner_commission_adjustments" adjustment
        WHERE adjustment."reversal_id" = target_reversal_id
          AND adjustment."commission_funding_allocation_id" = commission_allocation."id"
      )
  ) OR (
    SELECT count(*)
    FROM "partner_commission_adjustments" adjustment
    WHERE adjustment."reversal_id" = target_reversal_id
  ) <> (
    SELECT count(*)
    FROM "partner_commission_funding_allocations" commission_allocation
    JOIN "partner_usage_funding_allocations" usage_allocation
      ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
    WHERE usage_allocation."funding_lot_id" = target_lot_id
      AND commission_allocation."allocated_commission_nano" > 0
  ) THEN
    RAISE EXCEPTION 'payment reversal requires every exact commission adjustment'
      USING ERRCODE = '23514';
  END IF;
END;
$$;--> statement-breakpoint

CREATE FUNCTION "check_reversal_from_payment"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM "assert_partner_reversal_complete"(NEW."funding_lot_id");
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE CONSTRAINT TRIGGER "partner_reversal_insert_complete_guard"
AFTER INSERT ON "partner_payment_reversals"
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION "check_reversal_from_payment"();--> statement-breakpoint

CREATE FUNCTION "check_reversal_from_usage_allocation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  PERFORM "assert_partner_reversal_complete"(NEW."funding_lot_id");
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE CONSTRAINT TRIGGER "partner_reversed_usage_complete_guard"
AFTER INSERT ON "partner_usage_funding_allocations"
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION "check_reversal_from_usage_allocation"();--> statement-breakpoint

CREATE FUNCTION "check_reversal_from_commission_allocation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  target_lot_id bigint;
BEGIN
  SELECT usage_allocation."funding_lot_id"
  INTO target_lot_id
  FROM "partner_usage_funding_allocations" usage_allocation
  WHERE usage_allocation."id" = NEW."usage_funding_allocation_id";
  PERFORM "assert_partner_reversal_complete"(target_lot_id);
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE CONSTRAINT TRIGGER "partner_reversed_commission_complete_guard"
AFTER INSERT ON "partner_commission_funding_allocations"
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION "check_reversal_from_commission_allocation"();
