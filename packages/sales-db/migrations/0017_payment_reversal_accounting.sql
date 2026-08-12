-- Expand-only partner reversal accounting. This checkpoint creates dormant immutable evidence
-- and reserves the independent feed cursor; the consumer and payout readers ship only after this
-- migration is green in production. Existing commission/topup rows are neither rewritten nor
-- reinterpreted here.

ALTER TABLE "sync_cursors" ADD CONSTRAINT "sync_cursors_feed_v3_check"
  CHECK (
    "feed" IN (
      'attributions', 'usage_events', 'topups', 'topups_v2',
      'topup_funding_lots', 'payment_reversals'
    )
  ) NOT VALID;--> statement-breakpoint
ALTER TABLE "sync_cursors" VALIDATE CONSTRAINT "sync_cursors_feed_v3_check";--> statement-breakpoint
ALTER TABLE "sync_cursors" DROP CONSTRAINT "sync_cursors_feed_v2_check";--> statement-breakpoint
INSERT INTO "sync_cursors"("feed", "last_id") VALUES
  ('topup_funding_lots', 0),
  ('payment_reversals', 0)
ON CONFLICT ("feed") DO NOTHING;--> statement-breakpoint

-- Immutable snapshots of referred paid topups. A dedicated topups-v2 replay starts from sequence
-- zero under topup_funding_lots, joins each page to the existing idempotent referred_topups row and
-- then keeps following the source head. It does not reset the live topups_v2 analytics cursor.
CREATE TABLE "partner_paid_funding_lots" (
  "id" bigserial PRIMARY KEY,
  "referred_topup_id" bigint NOT NULL UNIQUE
    REFERENCES "referred_topups"("id") ON DELETE restrict,
  "commerce_topup_id" bigint NOT NULL UNIQUE,
  "commerce_payment_id" text NOT NULL UNIQUE,
  "commerce_user_id" uuid NOT NULL,
  "partner_id" uuid NOT NULL REFERENCES "partners"("id") ON DELETE restrict,
  "original_amount_nano" bigint NOT NULL,
  "paid_at" timestamp with time zone NOT NULL,
  "imported_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_paid_funding_lots_shape_check" CHECK (
    "commerce_topup_id" > 0
    AND "commerce_payment_id" <> ''
    AND "original_amount_nano" > 0
  )
);--> statement-breakpoint

CREATE INDEX "partner_paid_funding_lots_user_fifo_idx"
  ON "partner_paid_funding_lots"(
    "commerce_user_id", "commerce_topup_id"
  );--> statement-breakpoint

-- Exact FIFO consumption of a payment lot by one of the two append-only usage stores. Exactly one
-- usage FK is populated. A usage can span multiple lots and a lot can fund multiple usage rows.
CREATE TABLE "partner_usage_funding_allocations" (
  "id" bigserial PRIMARY KEY,
  "funding_lot_id" bigint NOT NULL
    REFERENCES "partner_paid_funding_lots"("id") ON DELETE restrict,
  "usage_event_id" bigint
    REFERENCES "partner_usage_events"("id") ON DELETE restrict,
  "usage_event_v2_id" bigint
    REFERENCES "partner_usage_events_v2"("id") ON DELETE restrict,
  "allocated_paid_nano" bigint NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_usage_funding_alloc_one_source_check" CHECK (
    (("usage_event_id" IS NOT NULL)::int + ("usage_event_v2_id" IS NOT NULL)::int) = 1
  ),
  CONSTRAINT "partner_usage_funding_alloc_amount_check" CHECK (
    "allocated_paid_nano" > 0
  )
);--> statement-breakpoint

CREATE UNIQUE INDEX "partner_usage_funding_alloc_v1_uidx"
  ON "partner_usage_funding_allocations"("funding_lot_id", "usage_event_id")
  WHERE "usage_event_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "partner_usage_funding_alloc_v2_uidx"
  ON "partner_usage_funding_allocations"("funding_lot_id", "usage_event_v2_id")
  WHERE "usage_event_v2_id" IS NOT NULL;--> statement-breakpoint
CREATE INDEX "partner_usage_funding_alloc_lot_idx"
  ON "partner_usage_funding_allocations"("funding_lot_id");--> statement-breakpoint

-- Deterministic attribution of a commission row to every paid-funding slice of its usage. Zero is
-- intentionally retained: very small upstream commissions can round to zero for an individual
-- lot, and preserving that row proves that the allocation is complete rather than missing.
CREATE TABLE "partner_commission_funding_allocations" (
  "id" bigserial PRIMARY KEY,
  "usage_funding_allocation_id" bigint NOT NULL
    REFERENCES "partner_usage_funding_allocations"("id") ON DELETE restrict,
  "commission_entry_id" bigint
    REFERENCES "commission_entries"("id") ON DELETE restrict,
  "commission_entry_v2_id" bigint
    REFERENCES "commission_entries_v2"("id") ON DELETE restrict,
  "allocated_commission_nano" bigint NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_commission_funding_one_source_check" CHECK (
    (("commission_entry_id" IS NOT NULL)::int
      + ("commission_entry_v2_id" IS NOT NULL)::int) = 1
  ),
  CONSTRAINT "partner_commission_funding_amount_check" CHECK (
    "allocated_commission_nano" >= 0
  )
);--> statement-breakpoint

CREATE UNIQUE INDEX "partner_commission_funding_v1_uidx"
  ON "partner_commission_funding_allocations"(
    "usage_funding_allocation_id", "commission_entry_id"
  ) WHERE "commission_entry_id" IS NOT NULL;--> statement-breakpoint
CREATE UNIQUE INDEX "partner_commission_funding_v2_uidx"
  ON "partner_commission_funding_allocations"(
    "usage_funding_allocation_id", "commission_entry_v2_id"
  ) WHERE "commission_entry_v2_id" IS NOT NULL;--> statement-breakpoint

-- One terminal commerce reversal maps to exactly one original payment lot. The original amount is
-- repeated deliberately and guarded against the immutable lot snapshot, making the evidence
-- directly auditable without trusting a later join or current payment state.
CREATE TABLE "partner_payment_reversals" (
  "id" bigserial PRIMARY KEY,
  "commerce_reversal_id" bigint NOT NULL UNIQUE,
  "funding_lot_id" bigint NOT NULL UNIQUE
    REFERENCES "partner_paid_funding_lots"("id") ON DELETE restrict,
  "commerce_payment_id" text NOT NULL UNIQUE,
  "commerce_user_id" uuid NOT NULL,
  "kind" text NOT NULL,
  "original_amount_nano" bigint NOT NULL,
  "reversed_at" timestamp with time zone NOT NULL,
  "imported_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_payment_reversals_shape_check" CHECK (
    "commerce_reversal_id" > 0
    AND "commerce_payment_id" <> ''
    AND "kind" IN ('refund', 'dispute')
    AND "original_amount_nano" > 0
  )
);--> statement-breakpoint

CREATE INDEX "partner_payment_reversals_time_idx"
  ON "partner_payment_reversals"("reversed_at", "commerce_reversal_id");--> statement-breakpoint

-- Signed ledger entries are strictly negative and point to the exact commission slice funded by
-- the reversed payment. A paid/committed payout is never mutated; readers can therefore expose a
-- negative net balance as explicit partner debt and retain future earnings against it.
CREATE TABLE "partner_commission_adjustments" (
  "id" bigserial PRIMARY KEY,
  "reversal_id" bigint NOT NULL
    REFERENCES "partner_payment_reversals"("id") ON DELETE restrict,
  "commission_funding_allocation_id" bigint NOT NULL
    REFERENCES "partner_commission_funding_allocations"("id") ON DELETE restrict,
  "partner_id" uuid NOT NULL REFERENCES "partners"("id") ON DELETE restrict,
  "amount_nano" bigint NOT NULL,
  "effective_at" timestamp with time zone NOT NULL,
  "created_at" timestamp with time zone NOT NULL DEFAULT now(),
  CONSTRAINT "partner_commission_adjustments_amount_check" CHECK (
    "amount_nano" < 0
  ),
  CONSTRAINT "partner_commission_adjustments_funding_allocation_key" UNIQUE (
    "commission_funding_allocation_id"
  ),
  CONSTRAINT "partner_commission_adjustments_source_unique" UNIQUE (
    "reversal_id", "commission_funding_allocation_id"
  )
);--> statement-breakpoint

CREATE INDEX "partner_commission_adjustments_partner_time_idx"
  ON "partner_commission_adjustments"("partner_id", "effective_at");--> statement-breakpoint

CREATE FUNCTION "enforce_partner_paid_funding_lot"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  source_payment_id text;
  source_user_id uuid;
  source_partner_id uuid;
  source_amount_nano bigint;
  source_paid_at timestamp with time zone;
BEGIN
  SELECT topup."commerce_payment_id", topup."commerce_user_id", topup."partner_id",
         topup."amount_nano", topup."paid_at"
  INTO source_payment_id, source_user_id, source_partner_id,
       source_amount_nano, source_paid_at
  FROM "referred_topups" topup
  WHERE topup."id" = NEW."referred_topup_id"
  FOR SHARE;

  IF NOT FOUND
     OR NEW."commerce_payment_id" <> source_payment_id
     OR NEW."commerce_user_id" <> source_user_id
     OR NEW."partner_id" <> source_partner_id
     OR NEW."original_amount_nano" <> source_amount_nano
     OR NEW."paid_at" <> source_paid_at THEN
    RAISE EXCEPTION 'paid funding lot must exactly snapshot its referred topup'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_paid_funding_lots_source_guard"
BEFORE INSERT ON "partner_paid_funding_lots"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_paid_funding_lot"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_usage_funding_allocation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  lot_user_id uuid;
  lot_partner_id uuid;
  lot_amount_nano bigint;
  lot_paid_at timestamp with time zone;
  lot_topup_id bigint;
  usage_user_id uuid;
  usage_partner_id uuid;
  usage_basis_nano bigint;
  usage_occurred_at timestamp with time zone;
  usage_commerce_event_id bigint;
  usage_schema integer;
  allocated_to_lot bigint;
  allocated_to_usage bigint;
BEGIN
  -- The referred-user row is the per-user allocation mutex. It also proves that the immutable
  -- lot still belongs to the same referral owner as the usage evidence.
  SELECT lot."commerce_user_id", lot."partner_id", lot."original_amount_nano",
         lot."paid_at", lot."commerce_topup_id"
  INTO lot_user_id, lot_partner_id, lot_amount_nano, lot_paid_at, lot_topup_id
  FROM "partner_paid_funding_lots" lot
  JOIN "referred_users" referred
    ON referred."commerce_user_id" = lot."commerce_user_id"
   AND referred."partner_id" = lot."partner_id"
  WHERE lot."id" = NEW."funding_lot_id"
  FOR UPDATE OF referred;

  IF NOT FOUND THEN
    RAISE EXCEPTION 'usage funding allocation requires the lot referral owner'
      USING ERRCODE = '23514';
  END IF;

  IF NEW."usage_event_id" IS NOT NULL THEN
    usage_schema := 1;
    SELECT usage."commerce_user_id", usage."partner_id", usage."amount_nano",
           usage."occurred_at", usage."commerce_event_id"
    INTO usage_user_id, usage_partner_id, usage_basis_nano,
         usage_occurred_at, usage_commerce_event_id
    FROM "partner_usage_events" usage
    WHERE usage."id" = NEW."usage_event_id"
    FOR SHARE;
  ELSE
    usage_schema := 2;
    SELECT usage."commerce_user_id", usage."partner_id", usage."paid_funded_nano",
           usage."occurred_at", usage."commerce_event_id"
    INTO usage_user_id, usage_partner_id, usage_basis_nano,
         usage_occurred_at, usage_commerce_event_id
    FROM "partner_usage_events_v2" usage
    WHERE usage."id" = NEW."usage_event_v2_id"
    FOR SHARE;
  END IF;

  IF NOT FOUND
     OR usage_user_id <> lot_user_id
     OR usage_partner_id <> lot_partner_id
     OR lot_paid_at > usage_occurred_at THEN
    RAISE EXCEPTION 'usage funding allocation does not match a causally available user lot'
      USING ERRCODE = '23514';
  END IF;

  -- A lot reversed before this usage was no longer spendable. A later reversal remains eligible
  -- for historical allocation so its exact commission share can be clawed back.
  IF EXISTS (
    SELECT 1
    FROM "partner_payment_reversals" reversal
    WHERE reversal."funding_lot_id" = NEW."funding_lot_id"
      AND reversal."reversed_at" <= usage_occurred_at
  ) THEN
    RAISE EXCEPTION 'usage cannot consume a payment lot already reversed at that time'
      USING ERRCODE = '23514';
  END IF;

  SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
  INTO allocated_to_lot
  FROM "partner_usage_funding_allocations" allocation
  WHERE allocation."funding_lot_id" = NEW."funding_lot_id";

  SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
  INTO allocated_to_usage
  FROM "partner_usage_funding_allocations" allocation
  WHERE (usage_schema = 1 AND allocation."usage_event_id" = NEW."usage_event_id")
     OR (usage_schema = 2 AND allocation."usage_event_v2_id" = NEW."usage_event_v2_id");

  IF allocated_to_lot + NEW."allocated_paid_nano" > lot_amount_nano
     OR allocated_to_usage + NEW."allocated_paid_nano" > usage_basis_nano THEN
    RAISE EXCEPTION 'usage funding allocation exceeds its lot or usage basis'
      USING ERRCODE = '23514';
  END IF;

  -- Usage is allocated strictly by (occurred_at, commerce_event_id, schema), so a restart cannot
  -- let a later event steal funding from an earlier incomplete event.
  IF EXISTS (
    WITH prior_usage AS (
      SELECT 1 AS source_schema, usage."id", usage."commerce_event_id",
             usage."amount_nano" AS basis_nano, usage."occurred_at"
      FROM "partner_usage_events" usage
      WHERE usage."commerce_user_id" = usage_user_id
      UNION ALL
      SELECT 2 AS source_schema, usage."id", usage."commerce_event_id",
             usage."paid_funded_nano" AS basis_nano, usage."occurred_at"
      FROM "partner_usage_events_v2" usage
      WHERE usage."commerce_user_id" = usage_user_id
    )
    SELECT 1
    FROM prior_usage prior
    WHERE (prior."occurred_at", prior."commerce_event_id", prior.source_schema)
          < (usage_occurred_at, usage_commerce_event_id, usage_schema)
      AND COALESCE((
        SELECT sum(allocation."allocated_paid_nano")
        FROM "partner_usage_funding_allocations" allocation
        WHERE (prior.source_schema = 1 AND allocation."usage_event_id" = prior."id")
           OR (prior.source_schema = 2 AND allocation."usage_event_v2_id" = prior."id")
      ), 0) < prior.basis_nano
  ) THEN
    RAISE EXCEPTION 'an earlier usage event still lacks complete paid funding allocation'
      USING ERRCODE = '23514';
  END IF;

  -- At the usage timestamp every earlier, not-yet-reversed FIFO lot must be exhausted before the
  -- current lot can be consumed.
  IF EXISTS (
    SELECT 1
    FROM "partner_paid_funding_lots" earlier
    WHERE earlier."commerce_user_id" = lot_user_id
      AND earlier."partner_id" = lot_partner_id
      AND earlier."paid_at" <= usage_occurred_at
      AND earlier."commerce_topup_id" < lot_topup_id
      AND NOT EXISTS (
        SELECT 1
        FROM "partner_payment_reversals" reversal
        WHERE reversal."funding_lot_id" = earlier."id"
          AND reversal."reversed_at" <= usage_occurred_at
      )
      AND COALESCE((
        SELECT sum(allocation."allocated_paid_nano")
        FROM "partner_usage_funding_allocations" allocation
        WHERE allocation."funding_lot_id" = earlier."id"
      ), 0) < earlier."original_amount_nano"
  ) THEN
    RAISE EXCEPTION 'usage funding allocation violates paid-lot FIFO order'
      USING ERRCODE = '23514';
  END IF;

  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_usage_funding_alloc_source_guard"
BEFORE INSERT ON "partner_usage_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_usage_funding_allocation"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_commission_funding_allocation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  allocation_usage_id bigint;
  allocation_usage_v2_id bigint;
  allocation_paid_nano bigint;
  lot_topup_id bigint;
  commission_usage_id bigint;
  commission_usage_v2_id bigint;
  commission_amount_nano bigint;
  usage_basis_nano bigint;
  total_usage_allocated bigint;
  cumulative_paid_nano bigint;
  expected_commission_nano bigint;
BEGIN
  SELECT allocation."usage_event_id", allocation."usage_event_v2_id",
         allocation."allocated_paid_nano", lot."commerce_topup_id"
  INTO allocation_usage_id, allocation_usage_v2_id, allocation_paid_nano,
       lot_topup_id
  FROM "partner_usage_funding_allocations" allocation
  JOIN "partner_paid_funding_lots" lot ON lot."id" = allocation."funding_lot_id"
  WHERE allocation."id" = NEW."usage_funding_allocation_id"
  FOR SHARE OF allocation, lot;

  IF NOT FOUND THEN
    RAISE EXCEPTION 'commission funding allocation requires a usage allocation'
      USING ERRCODE = '23514';
  END IF;

  IF NEW."commission_entry_id" IS NOT NULL THEN
    SELECT entry."usage_event_id", entry."amount_nano", usage."amount_nano"
    INTO commission_usage_id, commission_amount_nano, usage_basis_nano
    FROM "commission_entries" entry
    JOIN "partner_usage_events" usage ON usage."id" = entry."usage_event_id"
    WHERE entry."id" = NEW."commission_entry_id"
    FOR SHARE OF entry, usage;

    IF NOT FOUND
       OR allocation_usage_id IS NULL
       OR commission_usage_id <> allocation_usage_id THEN
      RAISE EXCEPTION 'commission funding allocation does not match its v1 usage'
        USING ERRCODE = '23514';
    END IF;

    SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
    INTO total_usage_allocated
    FROM "partner_usage_funding_allocations" allocation
    WHERE allocation."usage_event_id" = allocation_usage_id;

    SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
    INTO cumulative_paid_nano
    FROM "partner_usage_funding_allocations" allocation
    JOIN "partner_paid_funding_lots" lot ON lot."id" = allocation."funding_lot_id"
    WHERE allocation."usage_event_id" = allocation_usage_id
      AND lot."commerce_topup_id" <= lot_topup_id;
  ELSE
    SELECT entry."usage_event_id", entry."amount_nano", entry."base_paid_funded_nano"
    INTO commission_usage_v2_id, commission_amount_nano, usage_basis_nano
    FROM "commission_entries_v2" entry
    WHERE entry."id" = NEW."commission_entry_v2_id"
    FOR SHARE;

    IF NOT FOUND
       OR allocation_usage_v2_id IS NULL
       OR commission_usage_v2_id <> allocation_usage_v2_id THEN
      RAISE EXCEPTION 'commission funding allocation does not match its v2 usage'
        USING ERRCODE = '23514';
    END IF;

    SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
    INTO total_usage_allocated
    FROM "partner_usage_funding_allocations" allocation
    WHERE allocation."usage_event_v2_id" = allocation_usage_v2_id;

    SELECT COALESCE(sum(allocation."allocated_paid_nano"), 0)::bigint
    INTO cumulative_paid_nano
    FROM "partner_usage_funding_allocations" allocation
    JOIN "partner_paid_funding_lots" lot ON lot."id" = allocation."funding_lot_id"
    WHERE allocation."usage_event_v2_id" = allocation_usage_v2_id
      AND lot."commerce_topup_id" <= lot_topup_id;
  END IF;

  IF total_usage_allocated <> usage_basis_nano THEN
    RAISE EXCEPTION 'commission allocation requires a complete usage funding allocation'
      USING ERRCODE = '23514';
  END IF;

  expected_commission_nano :=
      floor(cumulative_paid_nano::numeric * commission_amount_nano::numeric
            / usage_basis_nano::numeric)::bigint
    - floor((cumulative_paid_nano - allocation_paid_nano)::numeric
            * commission_amount_nano::numeric / usage_basis_nano::numeric)::bigint;

  IF NEW."allocated_commission_nano" <> expected_commission_nano THEN
    RAISE EXCEPTION 'commission funding allocation does not match deterministic rounding'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_commission_funding_source_guard"
BEFORE INSERT ON "partner_commission_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_commission_funding_allocation"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_payment_reversal"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  lot_payment_id text;
  lot_user_id uuid;
  lot_amount_nano bigint;
  lot_paid_at timestamp with time zone;
BEGIN
  SELECT lot."commerce_payment_id", lot."commerce_user_id",
         lot."original_amount_nano", lot."paid_at"
  INTO lot_payment_id, lot_user_id, lot_amount_nano, lot_paid_at
  FROM "partner_paid_funding_lots" lot
  WHERE lot."id" = NEW."funding_lot_id"
  FOR SHARE;

  IF NOT FOUND
     OR NEW."commerce_payment_id" <> lot_payment_id
     OR NEW."commerce_user_id" <> lot_user_id
     OR NEW."original_amount_nano" <> lot_amount_nano
     OR NEW."reversed_at" < lot_paid_at THEN
    RAISE EXCEPTION 'payment reversal must exactly match its original paid funding lot'
      USING ERRCODE = '23514';
  END IF;

  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_payment_reversals_source_guard"
BEFORE INSERT ON "partner_payment_reversals"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_payment_reversal"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_commission_adjustment"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  reversal_lot_id bigint;
  reversal_at timestamp with time zone;
  allocation_lot_id bigint;
  allocation_commission_nano bigint;
  source_partner_id uuid;
BEGIN
  SELECT reversal."funding_lot_id", reversal."reversed_at"
  INTO reversal_lot_id, reversal_at
  FROM "partner_payment_reversals" reversal
  WHERE reversal."id" = NEW."reversal_id"
  FOR SHARE;

  SELECT usage_allocation."funding_lot_id",
         commission_allocation."allocated_commission_nano",
         COALESCE(entry."partner_id", entry_v2."partner_id")
  INTO allocation_lot_id, allocation_commission_nano, source_partner_id
  FROM "partner_commission_funding_allocations" commission_allocation
  JOIN "partner_usage_funding_allocations" usage_allocation
    ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
  LEFT JOIN "commission_entries" entry
    ON entry."id" = commission_allocation."commission_entry_id"
  LEFT JOIN "commission_entries_v2" entry_v2
    ON entry_v2."id" = commission_allocation."commission_entry_v2_id"
  WHERE commission_allocation."id" = NEW."commission_funding_allocation_id"
  FOR SHARE OF commission_allocation, usage_allocation;

  IF reversal_lot_id IS NULL
     OR allocation_lot_id IS NULL
     OR allocation_commission_nano IS NULL
     OR allocation_commission_nano <= 0
     OR source_partner_id IS NULL
     OR reversal_lot_id <> allocation_lot_id
     OR NEW."partner_id" <> source_partner_id
     OR NEW."amount_nano" <> -allocation_commission_nano
     OR NEW."effective_at" <> reversal_at THEN
    RAISE EXCEPTION 'commission adjustment must negate the exact reversed payment share'
      USING ERRCODE = '23514';
  END IF;
  IF current_setting('transaction_isolation') <> 'serializable' THEN
    RAISE EXCEPTION 'payment reversal accounting requires SERIALIZABLE isolation'
      USING ERRCODE = '25001';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_commission_adjustments_source_guard"
BEFORE INSERT ON "partner_commission_adjustments"
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_commission_adjustment"();--> statement-breakpoint

CREATE FUNCTION "enforce_partner_reversal_adjustment_set"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
  reversal_lot_id bigint;
BEGIN
  SELECT reversal."funding_lot_id"
  INTO reversal_lot_id
  FROM "partner_payment_reversals" reversal
  WHERE reversal."id" = NEW."reversal_id"
  FOR SHARE;

  -- This deferred constraint runs after a consumer transaction has inserted both the reversal and
  -- every non-zero negative slice. It rejects both incomplete allocation evidence and a reversal
  -- that omitted one of the exact slices funded by its payment lot.
  IF EXISTS (
    SELECT 1
    FROM "partner_usage_funding_allocations" usage_allocation
    WHERE usage_allocation."funding_lot_id" = reversal_lot_id
      AND (
        (
          usage_allocation."usage_event_id" IS NOT NULL
          AND EXISTS (
            SELECT 1
            FROM "commission_entries" entry
            WHERE entry."usage_event_id" = usage_allocation."usage_event_id"
              AND NOT EXISTS (
                SELECT 1
                FROM "partner_commission_funding_allocations" commission_allocation
                WHERE commission_allocation."usage_funding_allocation_id"
                      = usage_allocation."id"
                  AND commission_allocation."commission_entry_id" = entry."id"
              )
          )
        )
        OR (
          usage_allocation."usage_event_v2_id" IS NOT NULL
          AND EXISTS (
            SELECT 1
            FROM "commission_entries_v2" entry
            WHERE entry."usage_event_id" = usage_allocation."usage_event_v2_id"
              AND NOT EXISTS (
                SELECT 1
                FROM "partner_commission_funding_allocations" commission_allocation
                WHERE commission_allocation."usage_funding_allocation_id"
                      = usage_allocation."id"
                  AND commission_allocation."commission_entry_v2_id" = entry."id"
              )
          )
        )
      )
  ) OR (
    SELECT count(*)
    FROM "partner_commission_adjustments" adjustment
    WHERE adjustment."reversal_id" = NEW."reversal_id"
  ) <> (
    SELECT count(*)
    FROM "partner_commission_funding_allocations" commission_allocation
    JOIN "partner_usage_funding_allocations" usage_allocation
      ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
    WHERE usage_allocation."funding_lot_id" = reversal_lot_id
      AND commission_allocation."allocated_commission_nano" > 0
  ) OR EXISTS (
    SELECT 1
    FROM "partner_commission_funding_allocations" commission_allocation
    JOIN "partner_usage_funding_allocations" usage_allocation
      ON usage_allocation."id" = commission_allocation."usage_funding_allocation_id"
    WHERE usage_allocation."funding_lot_id" = reversal_lot_id
      AND commission_allocation."allocated_commission_nano" > 0
      AND NOT EXISTS (
        SELECT 1
        FROM "partner_commission_adjustments" adjustment
        WHERE adjustment."reversal_id" = NEW."reversal_id"
          AND adjustment."commission_funding_allocation_id" = commission_allocation."id"
      )
  ) THEN
    RAISE EXCEPTION 'payment reversal requires every exact commission adjustment'
      USING ERRCODE = '23514';
  END IF;
  RETURN NEW;
END;
$$;--> statement-breakpoint

CREATE CONSTRAINT TRIGGER "partner_reversal_adjustment_set_guard"
AFTER INSERT ON "partner_commission_adjustments"
DEFERRABLE INITIALLY IMMEDIATE
FOR EACH ROW EXECUTE FUNCTION "enforce_partner_reversal_adjustment_set"();--> statement-breakpoint

CREATE FUNCTION "reject_partner_reversal_accounting_mutation"()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
  RAISE EXCEPTION 'partner reversal accounting evidence is immutable'
    USING ERRCODE = '23514';
END;
$$;--> statement-breakpoint

CREATE TRIGGER "partner_paid_funding_lots_immutable"
BEFORE UPDATE OR DELETE ON "partner_paid_funding_lots"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_reversal_accounting_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_usage_funding_alloc_immutable"
BEFORE UPDATE OR DELETE ON "partner_usage_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_reversal_accounting_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_commission_funding_immutable"
BEFORE UPDATE OR DELETE ON "partner_commission_funding_allocations"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_reversal_accounting_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_payment_reversals_immutable"
BEFORE UPDATE OR DELETE ON "partner_payment_reversals"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_reversal_accounting_mutation"();--> statement-breakpoint
CREATE TRIGGER "partner_commission_adjustments_immutable"
BEFORE UPDATE OR DELETE ON "partner_commission_adjustments"
FOR EACH ROW EXECUTE FUNCTION "reject_partner_reversal_accounting_mutation"();
