-- Service meter-only strict lane.
--
-- Release-v2 retirement migrates service accounts (internal metered-but-never-charged) to the
-- direct strict-policy path. The strict path could not express meter-only: managed discount rules
-- were capped at 9500 bps, strict snapshots required positive holds, and the deferred
-- strict-reservation guard required at least one positive funding allocation. This migration is
-- the schema half of the lane; the dependent runtime ships in a separate SHA after a green
-- migration/watchdog of this checkpoint.
--
-- The payable-zero allowance is bound to the service class at every durable surface:
--   * account_policy_rules keeps a row-level 0..10000 range (the row has no class column) and a
--     BEFORE trigger binds any discount above 9500 to a service parent policy;
--   * pricing_admission_snapshots and the settlement_outbox/usage_events attribution ranges carry
--     the account_class column, so their CHECKs admit >9500 only for 'service' rows;
--   * assert_strict_reservation permits zero funding allocations only for a zero-hold reservation
--     whose pinned snapshot is service/payable-0.
-- No existing row, arm or semantic changes; every new predicate is a superset of the old one.

DO $$
DECLARE
    constraint_name text;
BEGIN
    SELECT conname INTO constraint_name
    FROM pg_constraint
    WHERE conrelid = 'account_policy_rules'::regclass
      AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%discount_bps%9500%';
    IF constraint_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE account_policy_rules DROP CONSTRAINT %I', constraint_name);
    END IF;
END $$;

ALTER TABLE account_policy_rules
    ADD CONSTRAINT account_policy_rules_managed_discount_ranges CHECK (
        (
            pricing_mode = 'track'
            AND rule_origin = 'managed'
            AND discount_bps IS NULL
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'managed'
            AND discount_bps IS NOT NULL
            AND discount_bps BETWEEN 0 AND 10000
            AND discount_bps % 100 = 0
            AND payable_multiplier_bp = 10000 - discount_bps
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'legacy'
            AND discount_bps IS NULL
            AND payable_multiplier_bp BETWEEN 1 AND 10000
        )
    );

CREATE OR REPLACE FUNCTION enforce_service_meter_only_policy_rule()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.pricing_mode = 'discount'
       AND NEW.rule_origin = 'managed'
       AND NEW.discount_bps IS NOT NULL
       AND NEW.discount_bps > 9500
       AND NOT EXISTS (
           SELECT 1
           FROM account_policy_versions
           WHERE account_id = NEW.account_id
             AND effective_version = NEW.effective_version
             AND account_class = 'service'
       )
    THEN
        RAISE EXCEPTION 'payable-zero managed discount rules are reserved for service policies'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'account_policy_rules_service_meter_only'
          AND tgrelid = 'account_policy_rules'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER account_policy_rules_service_meter_only
        BEFORE INSERT OR UPDATE ON account_policy_rules
        FOR EACH ROW EXECUTE FUNCTION enforce_service_meter_only_policy_rule();
    END IF;
END $$;

DO $$
DECLARE
    constraint_name text;
BEGIN
    SELECT conname INTO constraint_name
    FROM pg_constraint
    WHERE conrelid = 'pricing_admission_snapshots'::regclass
      AND contype = 'c'
      AND pg_get_constraintdef(oid) LIKE '%discount_bps%9500%';
    IF constraint_name IS NOT NULL THEN
        EXECUTE format('ALTER TABLE pricing_admission_snapshots DROP CONSTRAINT %I', constraint_name);
    END IF;
END $$;

ALTER TABLE pricing_admission_snapshots
    ADD CONSTRAINT pricing_admission_snapshots_kind_shape CHECK (
        (
            snapshot_kind = 'policy_v1'
            AND product_id IS NOT NULL
            AND product_id <> ''
            AND account_class IS NOT NULL
            AND rule_id IS NOT NULL
            AND rule_id <> ''
            AND rule_digest IS NOT NULL
            AND rule_digest <> ''
            AND rule_scope IS NOT NULL
            AND policy_id IS NOT NULL
            AND policy_id <> ''
            AND policy_version IS NOT NULL
            AND effective_policy_version IS NOT NULL
            AND policy_digest IS NOT NULL
            AND policy_digest <> ''
            AND catalog_generation IS NOT NULL
            AND switch_generation IS NOT NULL
            AND track_eligible IS NOT NULL
            AND retention_eligible IS NOT NULL
            AND commission_eligible IS NOT NULL
            AND (
                (
                    pricing_mode = 'track'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NULL
                    AND track_eligible
                    AND retention_eligible
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'managed'
                    AND discount_bps IS NOT NULL
                    AND (
                        discount_bps BETWEEN 0 AND 9500
                        OR (account_class = 'service' AND discount_bps BETWEEN 0 AND 10000)
                    )
                    AND discount_bps % 100 = 0
                    AND payable_multiplier_bp = 10000 - discount_bps
                    AND NOT track_eligible
                    AND NOT retention_eligible
                    AND NOT commission_eligible
                )
                OR (
                    pricing_mode = 'discount'
                    AND rule_origin = 'legacy'
                    AND discount_bps IS NULL
                    AND payable_multiplier_bp BETWEEN 1 AND 10000
                    AND NOT track_eligible
                    AND NOT retention_eligible
                    AND NOT commission_eligible
                )
            )
        )
        OR (
            snapshot_kind = 'legacy_scalar'
            AND product_id IS NULL
            AND account_class IS NULL
            AND rule_id IS NULL
            AND rule_digest IS NULL
            AND rule_scope IS NULL
            AND pricing_mode = 'legacy_scalar'
            AND rule_origin = 'legacy'
            AND discount_bps IS NULL
            AND policy_id IS NULL
            AND policy_version IS NULL
            AND effective_policy_version IS NULL
            AND policy_digest IS NULL
            AND catalog_generation IS NULL
            AND switch_generation IS NULL
            AND track_eligible IS NULL
            AND retention_eligible IS NULL
            AND commission_eligible IS NULL
        )
    );

ALTER TABLE settlement_outbox DROP CONSTRAINT IF EXISTS settlement_outbox_multi_discount_ranges;
ALTER TABLE settlement_outbox
    ADD CONSTRAINT settlement_outbox_multi_discount_ranges CHECK (
        (attribution_schema_version IS NULL OR attribution_schema_version > 0)
        AND (
            snapshot_kind IS NULL
            OR snapshot_kind IN ('policy_v1', 'legacy_scalar')
        )
        AND (alias_generation IS NULL OR alias_generation > 0)
        AND (
            served_canonical_model_id IS NULL
            OR served_canonical_model_id <> ''
        )
        AND (
            billing_invariant_code IS NULL
            OR billing_invariant_code <> ''
        )
        AND (
            pricing_mode IS NULL
            OR pricing_mode IN ('track', 'discount', 'legacy_scalar')
        )
        AND (rule_origin IS NULL OR rule_origin IN ('managed', 'legacy'))
        AND (
            tariff_priced_ts IS NULL
            OR tariff_priced_ts = priced_ts
        )
        AND (discount_bps IS NULL OR (
            discount_bps % 100 = 0
            AND (
                discount_bps BETWEEN 0 AND 9500
                OR (account_class = 'service' AND discount_bps BETWEEN 0 AND 10000)
            )
        ))
        AND (
            payable_multiplier_bp IS NULL
            OR payable_multiplier_bp BETWEEN 0 AND 10000
        )
        AND (
            (paid_funded_nano IS NULL
                AND bonus_funded_nano IS NULL
                AND other_funded_nano IS NULL)
            OR (
                paid_funded_nano IS NOT NULL
                AND bonus_funded_nano IS NOT NULL
                AND other_funded_nano IS NOT NULL
                AND paid_funded_nano >= 0
                AND bonus_funded_nano >= 0
                AND other_funded_nano >= 0
                AND paid_funded_nano + bonus_funded_nano + other_funded_nano
                    = actual_nano
            )
        )
    ) NOT VALID;
ALTER TABLE settlement_outbox VALIDATE CONSTRAINT settlement_outbox_multi_discount_ranges;

ALTER TABLE usage_events DROP CONSTRAINT IF EXISTS usage_events_multi_discount_ranges;
ALTER TABLE usage_events
    ADD CONSTRAINT usage_events_multi_discount_ranges CHECK (
        (attribution_schema_version IS NULL OR attribution_schema_version > 0)
        AND (
            snapshot_kind IS NULL
            OR snapshot_kind IN ('policy_v1', 'legacy_scalar')
        )
        AND (alias_generation IS NULL OR alias_generation > 0)
        AND (
            served_canonical_model_id IS NULL
            OR served_canonical_model_id <> ''
        )
        AND (
            billing_invariant_code IS NULL
            OR billing_invariant_code <> ''
        )
        AND (
            pricing_mode IS NULL
            OR pricing_mode IN ('track', 'discount', 'legacy_scalar')
        )
        AND (rule_origin IS NULL OR rule_origin IN ('managed', 'legacy'))
        AND (
            tariff_priced_ts IS NULL
            OR tariff_priced_ts = priced_ts
        )
        AND (discount_bps IS NULL OR (
            discount_bps % 100 = 0
            AND (
                discount_bps BETWEEN 0 AND 9500
                OR (account_class = 'service' AND discount_bps BETWEEN 0 AND 10000)
            )
        ))
        AND (
            payable_multiplier_bp IS NULL
            OR payable_multiplier_bp BETWEEN 0 AND 10000
        )
        AND (
            (paid_funded_nano IS NULL
                AND bonus_funded_nano IS NULL
                AND other_funded_nano IS NULL)
            OR (
                paid_funded_nano IS NOT NULL
                AND bonus_funded_nano IS NOT NULL
                AND other_funded_nano IS NOT NULL
                AND paid_funded_nano >= 0
                AND bonus_funded_nano >= 0
                AND other_funded_nano >= 0
                AND paid_funded_nano + bonus_funded_nano + other_funded_nano
                    = charge_nano
            )
        )
    ) NOT VALID;
ALTER TABLE usage_events VALIDATE CONSTRAINT usage_events_multi_discount_ranges;

-- The deferred strict-reservation guard required at least one positive funding allocation. A
-- service meter-only reservation holds exactly zero customer money, so it legitimately has none;
-- every other reservation keeps the original complete/allocation-bound verdict.
CREATE OR REPLACE FUNCTION assert_strict_reservation(p_request_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    reservation_row reservations%ROWTYPE;
    policy_mode text;
    funding_mode text;
    reconciliation text;
    snapshot_mode text;
    snapshot_track boolean;
    snapshot_class text;
    snapshot_payable bigint;
    allocation_count bigint;
    reserved_total numeric;
    charged_total numeric;
    released_total numeric;
    terminalized_total bigint;
    invalid_terminal_total bigint;
    ineligible_total bigint;
BEGIN
    SELECT * INTO reservation_row
    FROM reservations
    WHERE request_id = p_request_id;
    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT policy_enforcement, funding_enforcement, reconciliation_state
    INTO policy_mode, funding_mode, reconciliation
    FROM account_policy_bindings
    WHERE account_id = reservation_row.account_id;
    IF NOT FOUND OR (policy_mode <> 'strict' AND funding_mode <> 'strict') THEN
        RETURN;
    END IF;
    IF policy_mode <> 'strict' OR funding_mode <> 'strict' OR reconciliation <> 'verified' THEN
        RAISE EXCEPTION 'strict reservation has a partial or unreconciled binding'
            USING ERRCODE = '23514';
    END IF;

    SELECT snapshot_kind, track_eligible, account_class, payable_multiplier_bp
    INTO snapshot_mode, snapshot_track, snapshot_class, snapshot_payable
    FROM pricing_admission_snapshots
    WHERE request_id = p_request_id
      AND account_id = reservation_row.account_id;
    IF NOT FOUND OR snapshot_mode <> 'policy_v1' THEN
        RAISE EXCEPTION 'strict reservation lacks a policy_v1 admission snapshot'
            USING ERRCODE = '23514';
    END IF;

    SELECT
        count(*),
        COALESCE(sum(a.reserved_nano), 0),
        COALESCE(sum(a.charged_nano), 0),
        COALESCE(sum(a.released_nano), 0),
        count(*) FILTER (
            WHERE a.allocation_order IS NULL
               OR a.reserved_nano <= 0
               OR (snapshot_track AND b.eligibility NOT IN ('track', 'any'))
               OR (NOT snapshot_track AND b.eligibility <> 'any')
        ),
        count(*) FILTER (
            WHERE a.charged_nano IS NOT NULL OR a.released_nano IS NOT NULL
        ),
        count(*) FILTER (
            WHERE a.charged_nano IS NULL
               OR a.released_nano IS NULL
               OR a.charged_nano + a.released_nano <> a.reserved_nano
        )
    INTO
        allocation_count,
        reserved_total,
        charged_total,
        released_total,
        ineligible_total,
        terminalized_total,
        invalid_terminal_total
    FROM reservation_funding_allocations a
    JOIN funding_buckets b
      ON b.bucket_id = a.bucket_id
     AND b.account_id = a.account_id
    WHERE a.request_id = p_request_id
      AND a.account_id = reservation_row.account_id;

    IF allocation_count = 0
       OR reserved_total <> reservation_row.hold_nano
       OR ineligible_total <> 0 THEN
        IF NOT (
            allocation_count = 0
            AND reservation_row.hold_nano = 0
            AND snapshot_class = 'service'
            AND snapshot_payable = 0
        ) THEN
            RAISE EXCEPTION 'strict reservation funding allocation is incomplete or ineligible'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF reservation_row.state IN ('settled', 'canceled') THEN
        IF reservation_row.actual_nano IS NULL
           OR invalid_terminal_total <> 0
           OR charged_total <> reservation_row.actual_nano
           OR released_total <> reservation_row.hold_nano - reservation_row.actual_nano THEN
            RAISE EXCEPTION 'strict terminal reservation funding settlement is inconsistent'
                USING ERRCODE = '23514';
        END IF;
    ELSE
        IF terminalized_total <> 0 THEN
            RAISE EXCEPTION 'active strict reservation already has terminal funding allocation'
                USING ERRCODE = '23514';
        END IF;
    END IF;
END;
$$;

INSERT INTO engine_schema_migrations(version) VALUES (40)
ON CONFLICT (version) DO NOTHING;
