-- Expand-only persistence foundation for versioned multi-provider pricing.
--
-- This migration intentionally installs no catalog, switch, policy, or funding data. Existing
-- accounts continue to use the scalar multiplier path until a later policy-aware runtime has
-- durably materialized, reconciled, and acknowledged their new state.
--
-- Existing money and lifecycle tables only receive nullable attribution columns. An old engine
-- release can therefore keep inserting and settling requests without inventing historical facts.

CREATE TABLE IF NOT EXISTS pricing_catalog_versions (
    product_id text NOT NULL CHECK (product_id <> ''),
    generation bigint NOT NULL CHECK (generation > 0),
    schema_version bigint NOT NULL CHECK (schema_version > 0),
    capability_digest text NOT NULL CHECK (capability_digest <> ''),
    content_digest text NOT NULL CHECK (content_digest <> ''),
    created_ts bigint NOT NULL,
    PRIMARY KEY (product_id, generation)
);

CREATE TABLE IF NOT EXISTS pricing_catalog_entries (
    product_id text NOT NULL,
    generation bigint NOT NULL,
    provider_id text NOT NULL CHECK (provider_id <> ''),
    canonical_model_id text NOT NULL CHECK (canonical_model_id <> ''),
    enabled boolean NOT NULL,
    PRIMARY KEY (product_id, generation, provider_id, canonical_model_id),
    FOREIGN KEY (product_id, generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS pricing_catalog_entries_enabled
    ON pricing_catalog_entries(product_id, generation, provider_id)
    WHERE enabled;

CREATE TABLE IF NOT EXISTS pricing_catalog_heads (
    product_id text PRIMARY KEY CHECK (product_id <> ''),
    active_generation bigint NOT NULL CHECK (active_generation > 0),
    updated_ts bigint NOT NULL,
    FOREIGN KEY (product_id, active_generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS provider_switch_versions (
    generation bigint PRIMARY KEY CHECK (generation > 0),
    schema_version bigint NOT NULL CHECK (schema_version > 0),
    content_digest text NOT NULL CHECK (content_digest <> ''),
    created_ts bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS provider_switch_entries (
    generation bigint NOT NULL REFERENCES provider_switch_versions(generation) ON DELETE CASCADE,
    provider_id text NOT NULL CHECK (provider_id <> ''),
    scope_type text NOT NULL CHECK (scope_type IN ('master', 'product', 'segment')),
    product_id text NOT NULL DEFAULT '',
    segment text NOT NULL DEFAULT '',
    enabled boolean NOT NULL,
    PRIMARY KEY (generation, provider_id, scope_type, product_id, segment),
    CHECK (
        (scope_type = 'master' AND product_id = '' AND segment = '')
        OR (scope_type = 'product' AND product_id <> '' AND segment = '')
        OR (
            scope_type = 'segment'
            AND product_id <> ''
            AND segment IN ('b2c', 'b2b')
        )
    )
);

CREATE TABLE IF NOT EXISTS provider_switch_head (
    singleton smallint PRIMARY KEY CHECK (singleton = 1),
    active_generation bigint NOT NULL REFERENCES provider_switch_versions(generation)
        ON DELETE RESTRICT,
    updated_ts bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS account_policy_versions (
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    effective_version bigint NOT NULL CHECK (effective_version > 0),
    policy_id text NOT NULL CHECK (policy_id <> ''),
    policy_version bigint NOT NULL CHECK (policy_version > 0),
    owner_type text NOT NULL
        CHECK (owner_type IN ('global_b2c', 'b2b_client', 'openkeys', 'service')),
    owner_id text NOT NULL CHECK (owner_id <> ''),
    product_id text NOT NULL CHECK (product_id <> ''),
    schema_version bigint NOT NULL CHECK (schema_version > 0),
    catalog_generation bigint NOT NULL CHECK (catalog_generation > 0),
    content_digest text NOT NULL CHECK (content_digest <> ''),
    replacement_locked boolean NOT NULL,
    created_ts bigint NOT NULL,
    PRIMARY KEY (account_id, effective_version),
    UNIQUE (account_id, effective_version, product_id),
    UNIQUE (
        account_id,
        effective_version,
        policy_id,
        policy_version,
        product_id,
        catalog_generation,
        content_digest
    ),
    FOREIGN KEY (product_id, catalog_generation)
        REFERENCES pricing_catalog_versions(product_id, generation) ON DELETE RESTRICT
);
COMMENT ON COLUMN account_policy_versions.replacement_locked IS
    'True when this policy owner may never be superseded (for example, legacy OpenKeys). '
    'Every version row is append-only regardless of this flag.';
CREATE INDEX IF NOT EXISTS account_policy_versions_policy
    ON account_policy_versions(policy_id, policy_version);

CREATE TABLE IF NOT EXISTS account_policy_rules (
    account_id text NOT NULL,
    effective_version bigint NOT NULL,
    rule_id text NOT NULL CHECK (rule_id <> ''),
    rule_digest text NOT NULL CHECK (rule_digest <> ''),
    scope_type text NOT NULL CHECK (scope_type IN ('provider', 'model')),
    provider_id text NOT NULL CHECK (provider_id <> ''),
    canonical_model_id text,
    pricing_mode text NOT NULL CHECK (pricing_mode IN ('track', 'discount')),
    rule_origin text NOT NULL CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps bigint,
    payable_multiplier_bp bigint NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    track_eligible boolean NOT NULL,
    retention_eligible boolean NOT NULL,
    commission_eligible boolean NOT NULL,
    PRIMARY KEY (account_id, effective_version, rule_id),
    UNIQUE (account_id, effective_version, rule_id, rule_digest),
    FOREIGN KEY (account_id, effective_version)
        REFERENCES account_policy_versions(account_id, effective_version) ON DELETE CASCADE,
    CHECK (
        (scope_type = 'provider' AND canonical_model_id IS NULL)
        OR (
            scope_type = 'model'
            AND canonical_model_id IS NOT NULL
            AND canonical_model_id <> ''
        )
    ),
    CHECK (
        (
            pricing_mode = 'track'
            AND rule_origin = 'managed'
            AND discount_bps IS NULL
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'managed'
            AND discount_bps IS NOT NULL
            AND discount_bps BETWEEN 0 AND 9500
            AND discount_bps % 100 = 0
            AND payable_multiplier_bp = 10000 - discount_bps
        )
        OR (
            pricing_mode = 'discount'
            AND rule_origin = 'legacy'
            AND discount_bps IS NULL
            AND payable_multiplier_bp BETWEEN 1 AND 10000
        )
    ),
    CHECK (
        (
            pricing_mode = 'track'
            AND track_eligible
            AND retention_eligible
        )
        OR (
            pricing_mode = 'discount'
            AND NOT track_eligible
            AND NOT retention_eligible
            AND NOT commission_eligible
        )
    ),
    CHECK (NOT commission_eligible OR pricing_mode = 'track')
);
CREATE UNIQUE INDEX IF NOT EXISTS account_policy_rules_provider_scope
    ON account_policy_rules(account_id, effective_version, provider_id)
    WHERE scope_type = 'provider';
CREATE UNIQUE INDEX IF NOT EXISTS account_policy_rules_model_scope
    ON account_policy_rules(account_id, effective_version, provider_id, canonical_model_id)
    WHERE scope_type = 'model';

CREATE TABLE IF NOT EXISTS account_policy_bindings (
    account_id text PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    product_id text NOT NULL CHECK (product_id <> ''),
    account_class text NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    active_effective_version bigint,
    policy_enforcement text NOT NULL
        CHECK (policy_enforcement IN ('legacy_scalar', 'shadow', 'strict')),
    funding_enforcement text NOT NULL
        CHECK (funding_enforcement IN ('legacy_single', 'shadow', 'strict')),
    reconciliation_state text NOT NULL
        CHECK (reconciliation_state IN ('pending', 'verified', 'exception')),
    updated_ts bigint NOT NULL,
    FOREIGN KEY (account_id, active_effective_version, product_id)
        REFERENCES account_policy_versions(account_id, effective_version, product_id)
        ON DELETE RESTRICT,
    CHECK (policy_enforcement <> 'strict' OR active_effective_version IS NOT NULL),
    CHECK (funding_enforcement <> 'strict' OR reconciliation_state = 'verified')
);
CREATE INDEX IF NOT EXISTS account_policy_bindings_enforcement
    ON account_policy_bindings(policy_enforcement, funding_enforcement, reconciliation_state);

CREATE TABLE IF NOT EXISTS funding_buckets (
    bucket_id text PRIMARY KEY CHECK (bucket_id <> ''),
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    source_type text NOT NULL CHECK (source_type <> ''),
    source_ref text NOT NULL DEFAULT '',
    eligibility text NOT NULL CHECK (eligibility IN ('any', 'track', 'none')),
    balance_nano bigint NOT NULL,
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    spent_nano bigint NOT NULL CHECK (spent_nano >= 0),
    version bigint NOT NULL CHECK (version > 0),
    status text NOT NULL CHECK (status IN ('active', 'exhausted', 'retired')),
    created_ts bigint NOT NULL,
    updated_ts bigint NOT NULL,
    UNIQUE (account_id, source_type, source_ref),
    UNIQUE (bucket_id, account_id),
    UNIQUE (bucket_id, account_id, source_type),
    CHECK (source_type = 'paid' OR balance_nano >= 0),
    CHECK (source_type <> 'paid' OR eligibility = 'any'),
    CHECK (source_type <> 'welcome_track_bonus' OR eligibility = 'track'),
    CHECK (source_type <> 'legacy_restricted' OR eligibility = 'none')
);
CREATE INDEX IF NOT EXISTS funding_buckets_account_status
    ON funding_buckets(account_id, status);
CREATE UNIQUE INDEX IF NOT EXISTS funding_buckets_one_welcome
    ON funding_buckets(account_id)
    WHERE source_type = 'welcome_track_bonus';

CREATE TABLE IF NOT EXISTS pricing_admission_snapshots (
    request_id text PRIMARY KEY REFERENCES reservations(request_id) ON DELETE CASCADE,
    account_id text NOT NULL,
    snapshot_kind text NOT NULL CHECK (snapshot_kind IN ('policy_v1', 'legacy_scalar')),
    schema_version bigint NOT NULL CHECK (schema_version > 0),
    provider_id text NOT NULL CHECK (provider_id <> ''),
    product_id text,
    account_class text CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    requested_model_id text NOT NULL CHECK (requested_model_id <> ''),
    canonical_model_id text NOT NULL CHECK (canonical_model_id <> ''),
    alias_generation bigint NOT NULL CHECK (alias_generation > 0),
    rule_id text,
    rule_digest text,
    rule_scope text CHECK (rule_scope IN ('provider', 'model')),
    pricing_mode text NOT NULL CHECK (pricing_mode IN ('track', 'discount', 'legacy_scalar')),
    rule_origin text NOT NULL CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps bigint,
    payable_multiplier_bp bigint NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    policy_id text,
    policy_version bigint CHECK (policy_version > 0),
    effective_policy_version bigint CHECK (effective_policy_version > 0),
    policy_digest text,
    catalog_generation bigint CHECK (catalog_generation > 0),
    switch_generation bigint CHECK (switch_generation > 0),
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    tariff_priced_ts bigint NOT NULL CHECK (tariff_priced_ts > 0),
    admission_ts bigint NOT NULL CHECK (admission_ts > 0),
    official_hold_nano bigint NOT NULL CHECK (official_hold_nano >= 0),
    charged_hold_nano bigint NOT NULL CHECK (charged_hold_nano >= 0),
    track_eligible boolean,
    retention_eligible boolean,
    commission_eligible boolean,
    premium_modifiers jsonb NOT NULL CHECK (jsonb_typeof(premium_modifiers) = 'object'),
    snapshot_digest text NOT NULL CHECK (snapshot_digest <> ''),
    UNIQUE (request_id, account_id),
    FOREIGN KEY (
        account_id,
        effective_policy_version,
        policy_id,
        policy_version,
        product_id,
        catalog_generation,
        policy_digest
    )
        REFERENCES account_policy_versions(
            account_id,
            effective_version,
            policy_id,
            policy_version,
            product_id,
            catalog_generation,
            content_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, effective_policy_version, rule_id, rule_digest)
        REFERENCES account_policy_rules(
            account_id,
            effective_version,
            rule_id,
            rule_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (product_id, catalog_generation, provider_id, canonical_model_id)
        REFERENCES pricing_catalog_entries(
            product_id,
            generation,
            provider_id,
            canonical_model_id
        ) ON DELETE RESTRICT,
    FOREIGN KEY (switch_generation)
        REFERENCES provider_switch_versions(generation) ON DELETE RESTRICT,
    CHECK (
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
                    AND discount_bps BETWEEN 0 AND 9500
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
    ),
    CHECK (commission_eligible IS NOT TRUE OR pricing_mode = 'track')
);
CREATE INDEX IF NOT EXISTS pricing_admission_snapshots_account
    ON pricing_admission_snapshots(account_id, admission_ts);

CREATE OR REPLACE FUNCTION enforce_pricing_snapshot_reservation_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM reservations
        WHERE request_id = NEW.request_id
          AND account_id = NEW.account_id
    ) THEN
        RAISE EXCEPTION 'pricing snapshot account does not match reservation'
            USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgname = 'pricing_snapshot_reservation_account'
          AND tgrelid = 'pricing_admission_snapshots'::regclass
          AND NOT tgisinternal
    ) THEN
        EXECUTE
            'CREATE TRIGGER pricing_snapshot_reservation_account
             BEFORE INSERT ON pricing_admission_snapshots
             FOR EACH ROW EXECUTE FUNCTION enforce_pricing_snapshot_reservation_account()';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION reject_pricing_snapshot_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pricing admission snapshots are immutable'
        USING ERRCODE = '55000';
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgname = 'pricing_snapshot_immutable_update'
          AND tgrelid = 'pricing_admission_snapshots'::regclass
          AND NOT tgisinternal
    ) THEN
        EXECUTE
            'CREATE TRIGGER pricing_snapshot_immutable_update
             BEFORE UPDATE ON pricing_admission_snapshots
             FOR EACH ROW EXECUTE FUNCTION reject_pricing_snapshot_update()';
    END IF;
END $$;

CREATE TABLE IF NOT EXISTS reservation_funding_allocations (
    request_id text NOT NULL,
    account_id text NOT NULL,
    bucket_id text NOT NULL,
    bucket_version bigint NOT NULL CHECK (bucket_version > 0),
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    charged_nano bigint CHECK (charged_nano IS NULL OR charged_nano >= 0),
    released_nano bigint CHECK (released_nano IS NULL OR released_nano >= 0),
    PRIMARY KEY (request_id, bucket_id),
    FOREIGN KEY (request_id, account_id)
        REFERENCES pricing_admission_snapshots(request_id, account_id) ON DELETE CASCADE,
    FOREIGN KEY (bucket_id, account_id)
        REFERENCES funding_buckets(bucket_id, account_id) ON DELETE RESTRICT,
    CHECK (released_nano IS NULL OR released_nano <= reserved_nano)
);
CREATE INDEX IF NOT EXISTS reservation_funding_allocations_bucket
    ON reservation_funding_allocations(bucket_id, request_id);

CREATE TABLE IF NOT EXISTS ledger_funding_allocations (
    ledger_id bigint NOT NULL REFERENCES ledger(id) ON DELETE CASCADE,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    bucket_id text NOT NULL,
    bucket_source_type text NOT NULL CHECK (bucket_source_type <> ''),
    bucket_version bigint NOT NULL CHECK (bucket_version > 0),
    direction text NOT NULL CHECK (direction IN ('debit', 'credit')),
    amount_nano bigint NOT NULL CHECK (amount_nano >= 0),
    PRIMARY KEY (ledger_id, bucket_id),
    FOREIGN KEY (bucket_id, account_id, bucket_source_type)
        REFERENCES funding_buckets(bucket_id, account_id, source_type) ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS ledger_funding_allocations_bucket
    ON ledger_funding_allocations(bucket_id, ledger_id);

CREATE OR REPLACE FUNCTION enforce_ledger_funding_allocation_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM ledger
        WHERE id = NEW.ledger_id
          AND account_id = NEW.account_id
    ) THEN
        RAISE EXCEPTION 'funding allocation account does not match ledger'
            USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgname = 'ledger_funding_allocation_account'
          AND tgrelid = 'ledger_funding_allocations'::regclass
          AND NOT tgisinternal
    ) THEN
        EXECUTE
            'CREATE TRIGGER ledger_funding_allocation_account
             BEFORE INSERT OR UPDATE ON ledger_funding_allocations
             FOR EACH ROW EXECUTE FUNCTION enforce_ledger_funding_allocation_account()';
    END IF;
END $$;

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS attribution_schema_version bigint,
    ADD COLUMN IF NOT EXISTS snapshot_kind text,
    ADD COLUMN IF NOT EXISTS product_id text,
    ADD COLUMN IF NOT EXISTS account_class text,
    ADD COLUMN IF NOT EXISTS requested_model_id text,
    ADD COLUMN IF NOT EXISTS canonical_model_id text,
    ADD COLUMN IF NOT EXISTS served_model_id text,
    ADD COLUMN IF NOT EXISTS served_canonical_model_id text,
    ADD COLUMN IF NOT EXISTS billing_invariant_code text,
    ADD COLUMN IF NOT EXISTS alias_generation bigint,
    ADD COLUMN IF NOT EXISTS rule_id text,
    ADD COLUMN IF NOT EXISTS rule_digest text,
    ADD COLUMN IF NOT EXISTS rule_scope text,
    ADD COLUMN IF NOT EXISTS pricing_mode text,
    ADD COLUMN IF NOT EXISTS rule_origin text,
    ADD COLUMN IF NOT EXISTS discount_bps bigint,
    ADD COLUMN IF NOT EXISTS payable_multiplier_bp bigint,
    ADD COLUMN IF NOT EXISTS policy_id text,
    ADD COLUMN IF NOT EXISTS policy_version bigint,
    ADD COLUMN IF NOT EXISTS effective_policy_version bigint,
    ADD COLUMN IF NOT EXISTS policy_digest text,
    ADD COLUMN IF NOT EXISTS catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS switch_generation bigint,
    ADD COLUMN IF NOT EXISTS tariff_schedule_id text,
    ADD COLUMN IF NOT EXISTS tariff_priced_ts bigint,
    ADD COLUMN IF NOT EXISTS official_cost_json jsonb,
    ADD COLUMN IF NOT EXISTS paid_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS bonus_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS other_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS funding_allocation_json jsonb,
    ADD COLUMN IF NOT EXISTS track_eligible boolean,
    ADD COLUMN IF NOT EXISTS retention_eligible boolean,
    ADD COLUMN IF NOT EXISTS commission_eligible boolean,
    ADD COLUMN IF NOT EXISTS snapshot_digest text;

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS attribution_schema_version bigint,
    ADD COLUMN IF NOT EXISTS snapshot_kind text,
    ADD COLUMN IF NOT EXISTS product_id text,
    ADD COLUMN IF NOT EXISTS account_class text,
    ADD COLUMN IF NOT EXISTS requested_model_id text,
    ADD COLUMN IF NOT EXISTS canonical_model_id text,
    ADD COLUMN IF NOT EXISTS served_model_id text,
    ADD COLUMN IF NOT EXISTS served_canonical_model_id text,
    ADD COLUMN IF NOT EXISTS billing_invariant_code text,
    ADD COLUMN IF NOT EXISTS alias_generation bigint,
    ADD COLUMN IF NOT EXISTS rule_id text,
    ADD COLUMN IF NOT EXISTS rule_digest text,
    ADD COLUMN IF NOT EXISTS rule_scope text,
    ADD COLUMN IF NOT EXISTS pricing_mode text,
    ADD COLUMN IF NOT EXISTS rule_origin text,
    ADD COLUMN IF NOT EXISTS discount_bps bigint,
    ADD COLUMN IF NOT EXISTS payable_multiplier_bp bigint,
    ADD COLUMN IF NOT EXISTS policy_id text,
    ADD COLUMN IF NOT EXISTS policy_version bigint,
    ADD COLUMN IF NOT EXISTS effective_policy_version bigint,
    ADD COLUMN IF NOT EXISTS policy_digest text,
    ADD COLUMN IF NOT EXISTS catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS switch_generation bigint,
    ADD COLUMN IF NOT EXISTS tariff_schedule_id text,
    ADD COLUMN IF NOT EXISTS tariff_priced_ts bigint,
    ADD COLUMN IF NOT EXISTS official_cost_json jsonb,
    ADD COLUMN IF NOT EXISTS paid_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS bonus_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS other_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS funding_allocation_json jsonb,
    ADD COLUMN IF NOT EXISTS track_eligible boolean,
    ADD COLUMN IF NOT EXISTS retention_eligible boolean,
    ADD COLUMN IF NOT EXISTS commission_eligible boolean,
    ADD COLUMN IF NOT EXISTS snapshot_digest text;

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS provider text,
    ADD COLUMN IF NOT EXISTS attribution_schema_version bigint,
    ADD COLUMN IF NOT EXISTS snapshot_kind text,
    ADD COLUMN IF NOT EXISTS product_id text,
    ADD COLUMN IF NOT EXISTS account_class text,
    ADD COLUMN IF NOT EXISTS requested_model_id text,
    ADD COLUMN IF NOT EXISTS canonical_model_id text,
    ADD COLUMN IF NOT EXISTS served_model_id text,
    ADD COLUMN IF NOT EXISTS served_canonical_model_id text,
    ADD COLUMN IF NOT EXISTS billing_invariant_code text,
    ADD COLUMN IF NOT EXISTS alias_generation bigint,
    ADD COLUMN IF NOT EXISTS rule_id text,
    ADD COLUMN IF NOT EXISTS rule_digest text,
    ADD COLUMN IF NOT EXISTS rule_scope text,
    ADD COLUMN IF NOT EXISTS pricing_mode text,
    ADD COLUMN IF NOT EXISTS rule_origin text,
    ADD COLUMN IF NOT EXISTS discount_bps bigint,
    ADD COLUMN IF NOT EXISTS payable_multiplier_bp bigint,
    ADD COLUMN IF NOT EXISTS policy_id text,
    ADD COLUMN IF NOT EXISTS policy_version bigint,
    ADD COLUMN IF NOT EXISTS effective_policy_version bigint,
    ADD COLUMN IF NOT EXISTS policy_digest text,
    ADD COLUMN IF NOT EXISTS catalog_generation bigint,
    ADD COLUMN IF NOT EXISTS switch_generation bigint,
    ADD COLUMN IF NOT EXISTS tariff_schedule_id text,
    ADD COLUMN IF NOT EXISTS tariff_priced_ts bigint,
    ADD COLUMN IF NOT EXISTS official_nano bigint,
    ADD COLUMN IF NOT EXISTS official_cost_json jsonb,
    ADD COLUMN IF NOT EXISTS paid_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS bonus_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS other_funded_nano bigint,
    ADD COLUMN IF NOT EXISTS funding_allocation_json jsonb,
    ADD COLUMN IF NOT EXISTS track_eligible boolean,
    ADD COLUMN IF NOT EXISTS retention_eligible boolean,
    ADD COLUMN IF NOT EXISTS commission_eligible boolean,
    ADD COLUMN IF NOT EXISTS snapshot_digest text;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'settlement_outbox_multi_discount_ranges'
          AND conrelid = 'settlement_outbox'::regclass
    ) THEN
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
                    discount_bps BETWEEN 0 AND 9500 AND discount_bps % 100 = 0
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
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'usage_events_multi_discount_ranges'
          AND conrelid = 'usage_events'::regclass
    ) THEN
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
                    discount_bps BETWEEN 0 AND 9500 AND discount_bps % 100 = 0
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
    END IF;

    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'ledger_multi_discount_ranges'
          AND conrelid = 'ledger'::regclass
    ) THEN
        ALTER TABLE ledger
            ADD CONSTRAINT ledger_multi_discount_ranges CHECK (
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
                AND (official_nano IS NULL OR official_nano >= 0)
                AND (discount_bps IS NULL OR (
                    discount_bps BETWEEN 0 AND 9500 AND discount_bps % 100 = 0
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
                        kind = 'charge'
                        AND paid_funded_nano IS NOT NULL
                        AND bonus_funded_nano IS NOT NULL
                        AND other_funded_nano IS NOT NULL
                        AND paid_funded_nano >= 0
                        AND bonus_funded_nano >= 0
                        AND other_funded_nano >= 0
                        AND paid_funded_nano + bonus_funded_nano + other_funded_nano
                            = amount_nano
                    )
                )
            ) NOT VALID;
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (6)
ON CONFLICT (version) DO NOTHING;
