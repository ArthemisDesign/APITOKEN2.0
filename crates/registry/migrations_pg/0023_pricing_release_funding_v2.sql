-- Expand-only authority for the zero-downtime pricing release and funding-v2 cutover.
--
-- Every table starts empty and every column added to an existing writer surface is nullable.
-- The currently serving scalar/Stage-3 runtime therefore remains valid after this migration. The
-- dependent runtime is delivered separately and must dual-write before any release head exists.

CREATE TABLE IF NOT EXISTS pricing_release_policy_versions (
    policy_id text NOT NULL CHECK (policy_id <> ''),
    policy_version bigint NOT NULL CHECK (policy_version > 0),
    owner_type text NOT NULL
        CHECK (owner_type IN ('global_b2c', 'b2b_client', 'openkeys', 'service')),
    owner_id text NOT NULL CHECK (owner_id <> ''),
    account_class text NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    product_id text,
    billing_mode text NOT NULL CHECK (billing_mode IN ('balance', 'meter_only')),
    schema_version bigint NOT NULL CHECK (schema_version >= 2),
    capability_generation bigint NOT NULL CHECK (capability_generation > 0),
    capability_digest text NOT NULL CHECK (capability_digest <> ''),
    catalog_generation bigint CHECK (catalog_generation IS NULL OR catalog_generation > 0),
    catalog_digest text,
    switch_generation bigint CHECK (switch_generation IS NULL OR switch_generation > 0),
    switch_digest text,
    content_digest text NOT NULL CHECK (content_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (policy_id, policy_version),
    UNIQUE (policy_id, policy_version, content_digest),
    CHECK (
        (owner_type = 'global_b2c' AND account_class = 'b2c')
        OR (owner_type = 'b2b_client' AND account_class = 'b2b')
        OR (owner_type = 'openkeys' AND account_class = 'openkeys')
        OR (owner_type = 'service' AND account_class = 'service')
    ),
    CHECK (
        (
            account_class = 'service'
            AND billing_mode = 'meter_only'
            AND product_id IS NULL
            AND catalog_generation IS NULL
            AND catalog_digest IS NULL
            AND switch_generation IS NULL
            AND switch_digest IS NULL
        )
        OR (
            account_class <> 'service'
            AND billing_mode = 'balance'
            AND product_id IS NOT NULL AND product_id <> ''
            AND catalog_generation IS NOT NULL
            AND catalog_digest IS NOT NULL AND catalog_digest <> ''
            AND switch_generation IS NOT NULL
            AND switch_digest IS NOT NULL AND switch_digest <> ''
        )
    )
);

CREATE TABLE IF NOT EXISTS pricing_release_policy_rules (
    policy_id text NOT NULL,
    policy_version bigint NOT NULL,
    rule_id text NOT NULL CHECK (rule_id <> ''),
    rule_digest text NOT NULL CHECK (rule_digest <> ''),
    scope_type text NOT NULL CHECK (scope_type IN ('global', 'provider', 'model')),
    provider_id text,
    canonical_model_id text,
    discount_bps bigint NOT NULL CHECK (discount_bps BETWEEN 0 AND 10000),
    payable_multiplier_bp bigint NOT NULL CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    PRIMARY KEY (policy_id, policy_version, rule_id),
    UNIQUE (policy_id, policy_version, rule_id, rule_digest),
    FOREIGN KEY (policy_id, policy_version)
        REFERENCES pricing_release_policy_versions(policy_id, policy_version) ON DELETE RESTRICT,
    CHECK (payable_multiplier_bp = 10000 - discount_bps),
    CHECK (
        (scope_type = 'global' AND provider_id IS NULL AND canonical_model_id IS NULL)
        OR (
            scope_type = 'provider'
            AND provider_id IS NOT NULL
            AND provider_id <> ''
            AND canonical_model_id IS NULL
        )
        OR (
            scope_type = 'model'
            AND provider_id IS NOT NULL
            AND provider_id <> ''
            AND canonical_model_id IS NOT NULL
            AND canonical_model_id <> ''
        )
    )
);
CREATE UNIQUE INDEX IF NOT EXISTS pricing_release_rules_global
    ON pricing_release_policy_rules(policy_id, policy_version)
    WHERE scope_type = 'global';
CREATE UNIQUE INDEX IF NOT EXISTS pricing_release_rules_provider
    ON pricing_release_policy_rules(policy_id, policy_version, provider_id)
    WHERE scope_type = 'provider';
CREATE UNIQUE INDEX IF NOT EXISTS pricing_release_rules_model
    ON pricing_release_policy_rules(
        policy_id,
        policy_version,
        provider_id,
        canonical_model_id
    ) WHERE scope_type = 'model';

CREATE OR REPLACE FUNCTION enforce_pricing_release_rule_owner_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_release_policy_versions policy
        WHERE policy.policy_id = NEW.policy_id
          AND policy.policy_version = NEW.policy_version
          AND policy.account_class <> 'service'
          AND policy.billing_mode = 'balance'
    ) THEN
        RAISE EXCEPTION 'pricing v2 rules are forbidden for meter-only service policies'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER pricing_release_rule_owner_v2
BEFORE INSERT ON pricing_release_policy_rules
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_rule_owner_v2();

CREATE TABLE IF NOT EXISTS pricing_release_versions (
    generation bigint PRIMARY KEY CHECK (generation > 0),
    release_kind text NOT NULL CHECK (release_kind IN ('target', 'recovery')),
    schema_version bigint NOT NULL CHECK (schema_version >= 2),
    capability_generation bigint NOT NULL CHECK (capability_generation > 0),
    capability_digest text NOT NULL CHECK (capability_digest <> ''),
    main_catalog_generation bigint NOT NULL CHECK (main_catalog_generation > 0),
    main_catalog_digest text NOT NULL CHECK (main_catalog_digest <> ''),
    openkeys_catalog_generation bigint NOT NULL CHECK (openkeys_catalog_generation > 0),
    openkeys_catalog_digest text NOT NULL CHECK (openkeys_catalog_digest <> ''),
    switch_generation bigint NOT NULL CHECK (switch_generation > 0),
    switch_digest text NOT NULL CHECK (switch_digest <> ''),
    inventory_digest text NOT NULL CHECK (inventory_digest <> ''),
    policy_manifest_digest text NOT NULL CHECK (policy_manifest_digest <> ''),
    assignment_manifest_digest text NOT NULL CHECK (assignment_manifest_digest <> ''),
    funding_manifest_digest text NOT NULL CHECK (funding_manifest_digest <> ''),
    minimum_runtime_schema_version bigint NOT NULL
        CHECK (minimum_runtime_schema_version >= 2),
    content_digest text NOT NULL CHECK (content_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    UNIQUE (generation, content_digest),
    UNIQUE (generation, content_digest, schema_version)
);

CREATE TABLE IF NOT EXISTS pricing_release_recovery_links (
    target_generation bigint NOT NULL,
    target_digest text NOT NULL CHECK (target_digest <> ''),
    recovery_generation bigint NOT NULL,
    recovery_digest text NOT NULL CHECK (recovery_digest <> ''),
    link_digest text NOT NULL UNIQUE CHECK (link_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (target_generation, recovery_generation),
    FOREIGN KEY (target_generation, target_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    FOREIGN KEY (recovery_generation, recovery_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    CHECK (recovery_generation > target_generation)
);

CREATE OR REPLACE FUNCTION enforce_pricing_release_recovery_kinds_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_release_versions target
        JOIN pricing_release_versions recovery
          ON recovery.generation = NEW.recovery_generation
         AND recovery.content_digest = NEW.recovery_digest
        WHERE target.generation = NEW.target_generation
          AND target.content_digest = NEW.target_digest
          AND target.release_kind = 'target'
          AND recovery.release_kind = 'recovery'
    ) THEN
        RAISE EXCEPTION 'pricing v2 recovery link must connect target to recovery'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER pricing_release_recovery_kinds_v2
BEFORE INSERT ON pricing_release_recovery_links
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_recovery_kinds_v2();

CREATE TABLE IF NOT EXISTS pricing_release_assignments (
    release_generation bigint NOT NULL,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    account_class text NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    policy_id text NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL CHECK (policy_digest <> ''),
    billing_mode text NOT NULL CHECK (billing_mode IN ('balance', 'meter_only')),
    funding_generation bigint CHECK (funding_generation IS NULL OR funding_generation > 0),
    purpose text,
    responsible text,
    assignment_digest text NOT NULL CHECK (assignment_digest <> ''),
    PRIMARY KEY (release_generation, account_id),
    UNIQUE (release_generation, account_id, assignment_digest),
    FOREIGN KEY (release_generation)
        REFERENCES pricing_release_versions(generation) ON DELETE RESTRICT,
    FOREIGN KEY (policy_id, policy_version, policy_digest)
        REFERENCES pricing_release_policy_versions(policy_id, policy_version, content_digest)
        ON DELETE RESTRICT,
    CHECK (
        (
            account_class = 'service'
            AND billing_mode = 'meter_only'
            AND funding_generation IS NULL
            AND purpose IS NOT NULL
            AND purpose <> ''
            AND responsible IS NOT NULL
            AND responsible <> ''
        )
        OR (
            account_class = 'openkeys'
            AND billing_mode = 'balance'
            AND funding_generation IS NOT NULL
            AND purpose IS NULL
            AND responsible IS NULL
        )
        OR (
            account_class IN ('b2c', 'b2b')
            AND billing_mode = 'balance'
            AND funding_generation IS NOT NULL
            AND purpose IS NULL
            AND responsible IS NULL
        )
    )
);
CREATE INDEX IF NOT EXISTS pricing_release_assignments_class
    ON pricing_release_assignments(release_generation, account_class, account_id);

CREATE TABLE IF NOT EXISTS account_funding_generations_v2 (
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    generation bigint NOT NULL CHECK (generation > 0),
    schema_version bigint NOT NULL CHECK (schema_version >= 2),
    source_state_digest text NOT NULL CHECK (source_state_digest <> ''),
    normalization_digest text NOT NULL CHECK (normalization_digest <> ''),
    balance_nano bigint NOT NULL,
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    spent_nano bigint NOT NULL CHECK (spent_nano >= 0),
    version bigint NOT NULL CHECK (version >= 0),
    normalized_ts bigint NOT NULL CHECK (normalized_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= normalized_ts),
    PRIMARY KEY (account_id, generation),
    UNIQUE (account_id, generation, normalization_digest)
);

CREATE TABLE IF NOT EXISTS account_funding_head_v2 (
    account_id text PRIMARY KEY REFERENCES accounts(id) ON DELETE RESTRICT,
    active_generation bigint NOT NULL CHECK (active_generation > 0),
    head_version bigint NOT NULL CHECK (head_version > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    FOREIGN KEY (account_id, active_generation)
        REFERENCES account_funding_generations_v2(account_id, generation) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS funding_lots_v2 (
    lot_id text PRIMARY KEY CHECK (lot_id <> ''),
    account_id text NOT NULL,
    funding_generation bigint NOT NULL CHECK (funding_generation > 0),
    source_type text NOT NULL CHECK (source_type IN ('paid', 'welcome_bonus')),
    source_ref text NOT NULL CHECK (source_ref <> ''),
    balance_nano bigint NOT NULL,
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    spent_nano bigint NOT NULL CHECK (spent_nano >= 0),
    version bigint NOT NULL CHECK (version >= 0),
    status text NOT NULL CHECK (status IN ('active', 'exhausted', 'retired')),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts >= created_ts),
    UNIQUE (account_id, funding_generation, source_type, source_ref),
    UNIQUE (lot_id, account_id, funding_generation, source_type),
    FOREIGN KEY (account_id, funding_generation)
        REFERENCES account_funding_generations_v2(account_id, generation) ON DELETE RESTRICT,
    CHECK (source_type = 'paid' OR balance_nano >= 0)
);
CREATE INDEX IF NOT EXISTS funding_lots_v2_account_status
    ON funding_lots_v2(account_id, funding_generation, status, source_type);

ALTER TABLE pricing_release_assignments
    ADD CONSTRAINT pricing_release_assignment_funding_v2_fk
    FOREIGN KEY (account_id, funding_generation)
    REFERENCES account_funding_generations_v2(account_id, generation)
    ON DELETE RESTRICT;

CREATE OR REPLACE FUNCTION assert_funding_generation_v2(
    p_account_id text,
    p_generation bigint
)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    generation_balance bigint;
    generation_reserved bigint;
    generation_spent bigint;
    lot_balance numeric;
    lot_reserved numeric;
    lot_spent numeric;
BEGIN
    SELECT balance_nano, reserved_nano, spent_nano
    INTO generation_balance, generation_reserved, generation_spent
    FROM account_funding_generations_v2
    WHERE account_id = p_account_id AND generation = p_generation;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT
        COALESCE(sum(balance_nano), 0),
        COALESCE(sum(reserved_nano), 0),
        COALESCE(sum(spent_nano), 0)
    INTO lot_balance, lot_reserved, lot_spent
    FROM funding_lots_v2
    WHERE account_id = p_account_id AND funding_generation = p_generation;

    IF lot_balance <> generation_balance
       OR lot_reserved <> generation_reserved
       OR lot_spent <> generation_spent THEN
        RAISE EXCEPTION 'funding v2 lots do not match generation aggregates'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION assert_active_funding_account_v2(p_account_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    account_balance bigint;
    account_reserved bigint;
    account_spent bigint;
    generation_balance bigint;
    generation_reserved bigint;
    generation_spent bigint;
BEGIN
    SELECT
        account.balance_nano,
        account.reserved_nano,
        account.spent_nano,
        generation.balance_nano,
        generation.reserved_nano,
        generation.spent_nano
    INTO
        account_balance,
        account_reserved,
        account_spent,
        generation_balance,
        generation_reserved,
        generation_spent
    FROM account_funding_head_v2 head
    JOIN accounts account ON account.id = head.account_id
    JOIN account_funding_generations_v2 generation
      ON generation.account_id = head.account_id
     AND generation.generation = head.active_generation
    WHERE head.account_id = p_account_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    IF account_balance <> generation_balance
       OR account_reserved <> generation_reserved
       OR account_spent <> generation_spent THEN
        RAISE EXCEPTION 'active funding v2 generation does not match account aggregates'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_generation_v2_from_generation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_funding_generation_v2(OLD.account_id, OLD.generation);
        PERFORM assert_active_funding_account_v2(OLD.account_id);
    ELSE
        PERFORM assert_funding_generation_v2(NEW.account_id, NEW.generation);
        PERFORM assert_active_funding_account_v2(NEW.account_id);
        IF TG_OP = 'UPDATE'
           AND (NEW.account_id, NEW.generation)
               IS DISTINCT FROM (OLD.account_id, OLD.generation) THEN
            PERFORM assert_funding_generation_v2(OLD.account_id, OLD.generation);
            PERFORM assert_active_funding_account_v2(OLD.account_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_funding_generation_v2_from_lot()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_funding_generation_v2(OLD.account_id, OLD.funding_generation);
        PERFORM assert_active_funding_account_v2(OLD.account_id);
    ELSE
        PERFORM assert_funding_generation_v2(NEW.account_id, NEW.funding_generation);
        PERFORM assert_active_funding_account_v2(NEW.account_id);
        IF TG_OP = 'UPDATE'
           AND (NEW.account_id, NEW.funding_generation)
               IS DISTINCT FROM (OLD.account_id, OLD.funding_generation) THEN
            PERFORM assert_funding_generation_v2(OLD.account_id, OLD.funding_generation);
            PERFORM assert_active_funding_account_v2(OLD.account_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_active_funding_v2_from_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM assert_active_funding_account_v2(NEW.id);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_active_funding_v2_from_head()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_active_funding_account_v2(OLD.account_id);
    ELSE
        PERFORM assert_active_funding_account_v2(NEW.account_id);
        IF TG_OP = 'UPDATE' AND NEW.account_id IS DISTINCT FROM OLD.account_id THEN
            PERFORM assert_active_funding_account_v2(OLD.account_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER funding_generation_v2_lot_parity
AFTER INSERT OR UPDATE OR DELETE ON account_funding_generations_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_generation_v2_from_generation();

CREATE CONSTRAINT TRIGGER funding_lots_v2_generation_parity
AFTER INSERT OR UPDATE OR DELETE ON funding_lots_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_funding_generation_v2_from_lot();

CREATE CONSTRAINT TRIGGER accounts_active_funding_v2_parity
AFTER INSERT OR UPDATE OF balance_nano, reserved_nano, spent_nano ON accounts
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_active_funding_v2_from_account();

CREATE CONSTRAINT TRIGGER funding_head_v2_account_parity
AFTER INSERT OR UPDATE OR DELETE ON account_funding_head_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_active_funding_v2_from_head();

CREATE TABLE IF NOT EXISTS pricing_stage8_evidence_v2 (
    evidence_digest text PRIMARY KEY CHECK (evidence_digest <> ''),
    target_generation bigint NOT NULL,
    target_digest text NOT NULL CHECK (target_digest <> ''),
    recovery_generation bigint NOT NULL,
    recovery_digest text NOT NULL CHECK (recovery_digest <> ''),
    inventory_digest text NOT NULL CHECK (inventory_digest <> ''),
    funding_digest text NOT NULL CHECK (funding_digest <> ''),
    shadow_digest text NOT NULL CHECK (shadow_digest <> ''),
    runtime_floor_digest text NOT NULL CHECK (runtime_floor_digest <> ''),
    legacy_inflight_count bigint NOT NULL CHECK (legacy_inflight_count >= 0),
    blocker_count bigint NOT NULL CHECK (blocker_count >= 0),
    passed boolean NOT NULL,
    observed_ts bigint NOT NULL CHECK (observed_ts > 0),
    valid_until_ts bigint NOT NULL CHECK (valid_until_ts > observed_ts),
    FOREIGN KEY (target_generation, target_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    FOREIGN KEY (recovery_generation, recovery_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    CHECK (
        (passed AND blocker_count = 0 AND legacy_inflight_count = 0)
        OR NOT passed
    )
);

CREATE TABLE IF NOT EXISTS pricing_release_head_v2 (
    singleton smallint PRIMARY KEY CHECK (singleton = 1),
    active_generation bigint NOT NULL,
    active_digest text NOT NULL CHECK (active_digest <> ''),
    head_version bigint NOT NULL CHECK (head_version > 0),
    updated_ts bigint NOT NULL CHECK (updated_ts > 0),
    FOREIGN KEY (active_generation, active_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT
);

CREATE TABLE IF NOT EXISTS pricing_release_activations_v2 (
    id bigserial PRIMARY KEY,
    activation_kind text NOT NULL CHECK (activation_kind IN ('cutover', 'recovery')),
    from_generation bigint,
    from_digest text,
    to_generation bigint NOT NULL,
    to_digest text NOT NULL CHECK (to_digest <> ''),
    expected_head_version bigint NOT NULL CHECK (expected_head_version >= 0),
    resulting_head_version bigint NOT NULL CHECK (resulting_head_version > 0),
    evidence_digest text NOT NULL,
    operator_id text NOT NULL CHECK (operator_id <> ''),
    reason text NOT NULL CHECK (reason <> ''),
    activated_ts bigint NOT NULL CHECK (activated_ts > 0),
    UNIQUE (to_generation, to_digest, evidence_digest),
    UNIQUE (resulting_head_version),
    FOREIGN KEY (to_generation, to_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    FOREIGN KEY (from_generation, from_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    FOREIGN KEY (evidence_digest)
        REFERENCES pricing_stage8_evidence_v2(evidence_digest) ON DELETE RESTRICT,
    CHECK (resulting_head_version = expected_head_version + 1),
    CHECK (
        (from_generation IS NULL AND from_digest IS NULL AND expected_head_version = 0)
        OR (from_generation IS NOT NULL AND from_generation > 0
            AND from_digest IS NOT NULL AND from_digest <> '')
    )
);

CREATE OR REPLACE FUNCTION enforce_pricing_release_activation_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_stage8_evidence_v2 evidence
        WHERE evidence.evidence_digest = NEW.evidence_digest
          AND evidence.passed
          AND NEW.activated_ts >= evidence.observed_ts
          AND NEW.activated_ts <= evidence.valid_until_ts
          AND (
              (
                  NEW.activation_kind = 'cutover'
                  AND NEW.to_generation = evidence.target_generation
                  AND NEW.to_digest = evidence.target_digest
              )
              OR (
                  NEW.activation_kind = 'recovery'
                  AND NEW.to_generation = evidence.recovery_generation
                  AND NEW.to_digest = evidence.recovery_digest
              )
          )
    ) THEN
        RAISE EXCEPTION 'pricing v2 activation requires fresh passed evidence for its release'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER pricing_release_activation_evidence_v2
BEFORE INSERT ON pricing_release_activations_v2
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_activation_v2();

CREATE OR REPLACE FUNCTION enforce_pricing_release_head_step_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        RAISE EXCEPTION 'pricing v2 release head cannot be deleted'
            USING ERRCODE = '23514';
    END IF;
    IF TG_OP = 'INSERT' THEN
        IF NEW.head_version <> 1 THEN
            RAISE EXCEPTION 'initial pricing v2 release head version must be 1'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.head_version <> OLD.head_version + 1
       OR NEW.active_generation <= OLD.active_generation THEN
        RAISE EXCEPTION 'pricing v2 release head must advance one version and generation'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER pricing_release_head_step_v2
BEFORE INSERT OR UPDATE OR DELETE ON pricing_release_head_v2
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_head_step_v2();

CREATE OR REPLACE FUNCTION enforce_pricing_release_head_audit_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'INSERT' THEN
        IF NOT EXISTS (
            SELECT 1
            FROM pricing_release_activations_v2 activation
            WHERE activation.from_generation IS NULL
              AND activation.from_digest IS NULL
              AND activation.to_generation = NEW.active_generation
              AND activation.to_digest = NEW.active_digest
              AND activation.expected_head_version = 0
              AND activation.resulting_head_version = NEW.head_version
        ) THEN
            RAISE EXCEPTION 'initial pricing v2 release head lacks matching activation audit'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NOT EXISTS (
        SELECT 1
        FROM pricing_release_activations_v2 activation
        WHERE activation.from_generation = OLD.active_generation
          AND activation.from_digest = OLD.active_digest
          AND activation.to_generation = NEW.active_generation
          AND activation.to_digest = NEW.active_digest
          AND activation.expected_head_version = OLD.head_version
          AND activation.resulting_head_version = NEW.head_version
    ) THEN
        RAISE EXCEPTION 'pricing v2 release head transition lacks matching activation audit'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER pricing_release_head_audit_v2
AFTER INSERT OR UPDATE ON pricing_release_head_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_head_audit_v2();

CREATE TABLE IF NOT EXISTS pricing_request_snapshots_v2 (
    request_id text PRIMARY KEY REFERENCES reservations(request_id) ON DELETE RESTRICT,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    release_schema_version bigint NOT NULL CHECK (release_schema_version >= 2),
    release_generation bigint NOT NULL,
    release_digest text NOT NULL CHECK (release_digest <> ''),
    assignment_digest text NOT NULL CHECK (assignment_digest <> ''),
    account_class text NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    policy_id text NOT NULL,
    policy_version bigint NOT NULL,
    policy_digest text NOT NULL CHECK (policy_digest <> ''),
    billing_mode text NOT NULL CHECK (billing_mode IN ('balance', 'meter_only')),
    funding_generation bigint CHECK (funding_generation IS NULL OR funding_generation > 0),
    provider_id text NOT NULL CHECK (provider_id <> ''),
    canonical_model_id text NOT NULL CHECK (canonical_model_id <> ''),
    rule_id text,
    rule_digest text,
    rule_scope text CHECK (rule_scope IN ('global', 'provider', 'model')),
    discount_bps bigint CHECK (discount_bps IS NULL OR discount_bps BETWEEN 0 AND 10000),
    payable_multiplier_bp bigint
        CHECK (payable_multiplier_bp IS NULL OR payable_multiplier_bp BETWEEN 0 AND 10000),
    tariff_schedule_id text NOT NULL CHECK (tariff_schedule_id <> ''),
    tariff_priced_ts bigint NOT NULL CHECK (tariff_priced_ts > 0),
    official_hold_nano bigint NOT NULL CHECK (official_hold_nano >= 0),
    charged_hold_nano bigint NOT NULL CHECK (charged_hold_nano >= 0),
    official_cost_json jsonb NOT NULL CHECK (jsonb_typeof(official_cost_json) = 'object'),
    snapshot_digest text NOT NULL UNIQUE CHECK (snapshot_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    UNIQUE (request_id, account_id, funding_generation),
    FOREIGN KEY (release_generation, release_digest, release_schema_version)
        REFERENCES pricing_release_versions(generation, content_digest, schema_version)
        ON DELETE RESTRICT,
    FOREIGN KEY (release_generation, account_id, assignment_digest)
        REFERENCES pricing_release_assignments(
            release_generation,
            account_id,
            assignment_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (policy_id, policy_version, policy_digest)
        REFERENCES pricing_release_policy_versions(policy_id, policy_version, content_digest)
        ON DELETE RESTRICT,
    FOREIGN KEY (policy_id, policy_version, rule_id, rule_digest)
        REFERENCES pricing_release_policy_rules(
            policy_id,
            policy_version,
            rule_id,
            rule_digest
        ) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, funding_generation)
        REFERENCES account_funding_generations_v2(account_id, generation)
        ON DELETE RESTRICT,
    CHECK (
        (
            billing_mode = 'balance'
            AND account_class <> 'service'
            AND funding_generation IS NOT NULL
            AND rule_id IS NOT NULL AND rule_id <> ''
            AND rule_digest IS NOT NULL AND rule_digest <> ''
            AND rule_scope IS NOT NULL
            AND discount_bps IS NOT NULL
            AND payable_multiplier_bp = 10000 - discount_bps
            AND charged_hold_nano = floor(
                official_hold_nano::numeric * payable_multiplier_bp::numeric / 10000
            )::bigint
        )
        OR (
            billing_mode = 'meter_only'
            AND account_class = 'service'
            AND funding_generation IS NULL
            AND rule_id IS NULL
            AND rule_digest IS NULL
            AND rule_scope IS NULL
            AND discount_bps IS NULL
            AND payable_multiplier_bp IS NULL
            AND charged_hold_nano = 0
        )
    )
);
CREATE INDEX IF NOT EXISTS pricing_request_snapshots_v2_account
    ON pricing_request_snapshots_v2(account_id, created_ts);

CREATE OR REPLACE FUNCTION enforce_pricing_request_v2_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM reservations
        WHERE request_id = NEW.request_id AND account_id = NEW.account_id
    ) THEN
        RAISE EXCEPTION 'pricing v2 snapshot account does not match reservation'
            USING ERRCODE = '23503';
    END IF;
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_release_assignments assignment
        WHERE assignment.release_generation = NEW.release_generation
          AND assignment.account_id = NEW.account_id
          AND assignment.assignment_digest = NEW.assignment_digest
          AND assignment.account_class = NEW.account_class
          AND assignment.policy_id = NEW.policy_id
          AND assignment.policy_version = NEW.policy_version
          AND assignment.policy_digest = NEW.policy_digest
          AND assignment.billing_mode = NEW.billing_mode
          AND assignment.funding_generation IS NOT DISTINCT FROM NEW.funding_generation
    ) THEN
        RAISE EXCEPTION 'pricing v2 snapshot does not match release assignment'
            USING ERRCODE = '23503';
    END IF;
    IF NEW.billing_mode = 'balance' AND NOT EXISTS (
        SELECT 1
        FROM pricing_release_policy_rules rule
        WHERE rule.policy_id = NEW.policy_id
          AND rule.policy_version = NEW.policy_version
          AND rule.rule_id = NEW.rule_id
          AND rule.rule_digest = NEW.rule_digest
          AND rule.scope_type = NEW.rule_scope
          AND rule.discount_bps = NEW.discount_bps
          AND rule.payable_multiplier_bp = NEW.payable_multiplier_bp
          AND (
              rule.scope_type = 'global'
              OR (
                  rule.scope_type = 'provider'
                  AND rule.provider_id = NEW.provider_id
              )
              OR (
                  rule.scope_type = 'model'
                  AND rule.provider_id = NEW.provider_id
                  AND rule.canonical_model_id = NEW.canonical_model_id
              )
          )
    ) THEN
        RAISE EXCEPTION 'pricing v2 snapshot rule is not applicable to provider/model'
            USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'pricing_request_v2_account'
          AND tgrelid = 'pricing_request_snapshots_v2'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER pricing_request_v2_account
        BEFORE INSERT ON pricing_request_snapshots_v2
        FOR EACH ROW EXECUTE FUNCTION enforce_pricing_request_v2_account();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION reject_pricing_request_v2_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pricing v2 request snapshot is immutable'
        USING ERRCODE = '23514';
END;
$$;

CREATE TRIGGER pricing_request_v2_immutable
BEFORE UPDATE ON pricing_request_snapshots_v2
FOR EACH ROW EXECUTE FUNCTION reject_pricing_request_v2_update();

CREATE TABLE IF NOT EXISTS pricing_request_funding_allocations_v2 (
    request_id text NOT NULL,
    account_id text NOT NULL,
    funding_generation bigint NOT NULL CHECK (funding_generation > 0),
    allocation_order bigint NOT NULL CHECK (allocation_order > 0),
    lot_id text NOT NULL,
    lot_source_type text NOT NULL CHECK (lot_source_type IN ('paid', 'welcome_bonus')),
    lot_version bigint NOT NULL CHECK (lot_version >= 0),
    reserved_nano bigint NOT NULL CHECK (reserved_nano >= 0),
    charged_nano bigint CHECK (charged_nano IS NULL OR charged_nano >= 0),
    released_nano bigint CHECK (released_nano IS NULL OR released_nano >= 0),
    PRIMARY KEY (request_id, allocation_order),
    UNIQUE (request_id, lot_id),
    FOREIGN KEY (request_id, account_id, funding_generation)
        REFERENCES pricing_request_snapshots_v2(request_id, account_id, funding_generation)
        ON DELETE RESTRICT,
    FOREIGN KEY (lot_id, account_id, funding_generation, lot_source_type)
        REFERENCES funding_lots_v2(lot_id, account_id, funding_generation, source_type)
        ON DELETE RESTRICT,
    CHECK (released_nano IS NULL OR released_nano <= reserved_nano),
    CHECK (
        charged_nano IS NULL
        OR released_nano IS NULL
        OR (
            (charged_nano <= reserved_nano
                AND charged_nano + released_nano = reserved_nano)
            OR (charged_nano > reserved_nano AND released_nano = 0)
        )
    )
);

CREATE OR REPLACE FUNCTION assert_pricing_request_funding_v2(p_request_id text)
RETURNS void
LANGUAGE plpgsql
AS $$
DECLARE
    reservation_state text;
    reservation_hold bigint;
    reservation_actual bigint;
    snapshot_mode text;
    snapshot_hold bigint;
    allocation_count bigint;
    min_order bigint;
    max_order bigint;
    reserved_total numeric;
    charged_total numeric;
    released_total numeric;
    terminalized_count bigint;
    incomplete_terminal_count bigint;
BEGIN
    SELECT reservation.state, reservation.hold_nano, reservation.actual_nano,
           snapshot.billing_mode, snapshot.charged_hold_nano
    INTO reservation_state, reservation_hold, reservation_actual, snapshot_mode, snapshot_hold
    FROM reservations reservation
    JOIN pricing_request_snapshots_v2 snapshot
      ON snapshot.request_id = reservation.request_id
     AND snapshot.account_id = reservation.account_id
    WHERE reservation.request_id = p_request_id;

    IF NOT FOUND THEN
        RETURN;
    END IF;

    SELECT
        count(*),
        min(allocation_order),
        max(allocation_order),
        COALESCE(sum(reserved_nano), 0),
        COALESCE(sum(charged_nano), 0),
        COALESCE(sum(released_nano), 0),
        count(*) FILTER (WHERE charged_nano IS NOT NULL OR released_nano IS NOT NULL),
        count(*) FILTER (WHERE charged_nano IS NULL OR released_nano IS NULL)
    INTO
        allocation_count,
        min_order,
        max_order,
        reserved_total,
        charged_total,
        released_total,
        terminalized_count,
        incomplete_terminal_count
    FROM pricing_request_funding_allocations_v2
    WHERE request_id = p_request_id;

    IF snapshot_mode = 'meter_only' THEN
        IF reservation_hold <> 0
           OR snapshot_hold <> 0
           OR allocation_count <> 0
           OR (
               reservation_state IN ('settled', 'canceled')
               AND reservation_actual IS DISTINCT FROM 0
           ) THEN
            RAISE EXCEPTION 'meter-only pricing v2 request mutated balance funding'
                USING ERRCODE = '23514';
        END IF;
        RETURN;
    END IF;

    IF snapshot_hold <> reservation_hold
       OR reserved_total <> reservation_hold
       OR (allocation_count = 0 AND reservation_hold <> 0)
       OR (
           allocation_count > 0
           AND (min_order <> 1 OR max_order <> allocation_count)
       ) THEN
        RAISE EXCEPTION 'pricing v2 request funding does not cover reserved hold exactly'
            USING ERRCODE = '23514';
    END IF;

    IF reservation_state IN ('settled', 'canceled') THEN
        IF reservation_actual IS NULL
           OR incomplete_terminal_count <> 0
           OR charged_total <> reservation_actual
           OR released_total <> GREATEST(reservation_hold - reservation_actual, 0) THEN
            RAISE EXCEPTION 'terminal pricing v2 request funding is inconsistent'
                USING ERRCODE = '23514';
        END IF;
    ELSIF terminalized_count <> 0 THEN
        RAISE EXCEPTION 'active pricing v2 request has terminal funding allocations'
            USING ERRCODE = '23514';
    END IF;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_pricing_request_funding_v2_from_reservation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    PERFORM assert_pricing_request_funding_v2(NEW.request_id);
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_pricing_request_funding_v2_from_snapshot()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_pricing_request_funding_v2(OLD.request_id);
    ELSE
        PERFORM assert_pricing_request_funding_v2(NEW.request_id);
    END IF;
    RETURN NULL;
END;
$$;

CREATE OR REPLACE FUNCTION enforce_pricing_request_funding_v2_from_allocation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF TG_OP = 'DELETE' THEN
        PERFORM assert_pricing_request_funding_v2(OLD.request_id);
    ELSE
        PERFORM assert_pricing_request_funding_v2(NEW.request_id);
        IF TG_OP = 'UPDATE' AND NEW.request_id IS DISTINCT FROM OLD.request_id THEN
            PERFORM assert_pricing_request_funding_v2(OLD.request_id);
        END IF;
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER reservations_pricing_funding_v2
AFTER INSERT OR UPDATE ON reservations
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_request_funding_v2_from_reservation();

CREATE CONSTRAINT TRIGGER snapshots_pricing_funding_v2
AFTER INSERT OR DELETE ON pricing_request_snapshots_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_request_funding_v2_from_snapshot();

CREATE CONSTRAINT TRIGGER allocations_pricing_funding_v2
AFTER INSERT OR UPDATE OR DELETE ON pricing_request_funding_allocations_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_request_funding_v2_from_allocation();

CREATE TABLE IF NOT EXISTS funding_ledger_allocations_v2 (
    ledger_id bigint NOT NULL REFERENCES ledger(id) ON DELETE RESTRICT,
    account_id text NOT NULL,
    funding_generation bigint NOT NULL CHECK (funding_generation > 0),
    allocation_order bigint NOT NULL CHECK (allocation_order > 0),
    lot_id text NOT NULL,
    lot_source_type text NOT NULL CHECK (lot_source_type IN ('paid', 'welcome_bonus')),
    lot_version bigint NOT NULL CHECK (lot_version >= 0),
    direction text NOT NULL CHECK (direction IN ('debit', 'credit')),
    amount_nano bigint NOT NULL CHECK (amount_nano >= 0),
    PRIMARY KEY (ledger_id, allocation_order),
    FOREIGN KEY (lot_id, account_id, funding_generation, lot_source_type)
        REFERENCES funding_lots_v2(lot_id, account_id, funding_generation, source_type)
        ON DELETE RESTRICT
);
CREATE INDEX IF NOT EXISTS funding_ledger_allocations_v2_lot
    ON funding_ledger_allocations_v2(lot_id, ledger_id);

CREATE OR REPLACE FUNCTION enforce_funding_ledger_v2_account()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM ledger
        WHERE id = NEW.ledger_id AND account_id = NEW.account_id
    ) THEN
        RAISE EXCEPTION 'funding v2 allocation account does not match ledger'
            USING ERRCODE = '23503';
    END IF;
    RETURN NEW;
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_trigger
        WHERE tgname = 'funding_ledger_v2_account'
          AND tgrelid = 'funding_ledger_allocations_v2'::regclass
          AND NOT tgisinternal
    ) THEN
        CREATE TRIGGER funding_ledger_v2_account
        BEFORE INSERT ON funding_ledger_allocations_v2
        FOR EACH ROW EXECUTE FUNCTION enforce_funding_ledger_v2_account();
    END IF;
END $$;

CREATE OR REPLACE FUNCTION reject_immutable_pricing_release_v2_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'immutable pricing v2 authority cannot be updated or deleted'
        USING ERRCODE = '23514';
END;
$$;

DO $$
DECLARE
    table_name text;
    trigger_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY[
        'pricing_release_policy_versions',
        'pricing_release_policy_rules',
        'pricing_release_versions',
        'pricing_release_recovery_links',
        'pricing_release_assignments',
        'pricing_stage8_evidence_v2',
        'pricing_release_activations_v2'
    ]
    LOOP
        trigger_name := table_name || '_immutable';
        EXECUTE format(
            'CREATE TRIGGER %I BEFORE UPDATE OR DELETE ON %I
             FOR EACH ROW EXECUTE FUNCTION reject_immutable_pricing_release_v2_mutation()',
            trigger_name,
            table_name
        );
    END LOOP;
END $$;

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS release_schema_version bigint,
    ADD COLUMN IF NOT EXISTS release_generation bigint,
    ADD COLUMN IF NOT EXISTS release_digest text,
    ADD COLUMN IF NOT EXISTS release_billing_mode text,
    ADD COLUMN IF NOT EXISTS release_funding_generation bigint,
    ADD COLUMN IF NOT EXISTS release_snapshot_digest text;

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS release_schema_version bigint,
    ADD COLUMN IF NOT EXISTS release_generation bigint,
    ADD COLUMN IF NOT EXISTS release_digest text,
    ADD COLUMN IF NOT EXISTS release_billing_mode text,
    ADD COLUMN IF NOT EXISTS release_funding_generation bigint,
    ADD COLUMN IF NOT EXISTS release_snapshot_digest text;

ALTER TABLE ledger
    ADD COLUMN IF NOT EXISTS release_schema_version bigint,
    ADD COLUMN IF NOT EXISTS release_generation bigint,
    ADD COLUMN IF NOT EXISTS release_digest text,
    ADD COLUMN IF NOT EXISTS release_billing_mode text,
    ADD COLUMN IF NOT EXISTS release_funding_generation bigint,
    ADD COLUMN IF NOT EXISTS release_snapshot_digest text;

DO $$
DECLARE
    table_name text;
    constraint_name text;
BEGIN
    FOREACH table_name IN ARRAY ARRAY['settlement_outbox', 'usage_events', 'ledger']
    LOOP
        constraint_name := table_name || '_release_v2_shape';
        IF NOT EXISTS (
            SELECT 1 FROM pg_constraint
            WHERE conname = constraint_name
              AND conrelid = format('%I', table_name)::regclass
        ) THEN
            EXECUTE format(
                'ALTER TABLE %I ADD CONSTRAINT %I CHECK (
                    (
                        release_schema_version IS NULL
                        AND release_generation IS NULL
                        AND release_digest IS NULL
                        AND release_billing_mode IS NULL
                        AND release_funding_generation IS NULL
                        AND release_snapshot_digest IS NULL
                    )
                    OR (
                        release_schema_version >= 2
                        AND release_generation > 0
                        AND release_digest IS NOT NULL AND release_digest <> ''''
                        AND release_billing_mode IN (''balance'', ''meter_only'')
                        AND (
                            (release_billing_mode = ''balance''
                                AND release_funding_generation > 0)
                            OR (release_billing_mode = ''meter_only''
                                AND release_funding_generation IS NULL)
                        )
                        AND release_snapshot_digest IS NOT NULL
                        AND release_snapshot_digest <> ''''
                    )
                ) NOT VALID',
                table_name,
                constraint_name
            );
        END IF;
    END LOOP;
END $$;

CREATE OR REPLACE FUNCTION enforce_release_v2_lineage()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    charge_nano bigint;
BEGIN
    IF NEW.release_schema_version IS NULL THEN
        RETURN NEW;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pricing_request_snapshots_v2 snapshot
        WHERE snapshot.request_id = NEW.request_id
          AND snapshot.release_schema_version = NEW.release_schema_version
          AND snapshot.release_generation = NEW.release_generation
          AND snapshot.release_digest = NEW.release_digest
          AND snapshot.billing_mode = NEW.release_billing_mode
          AND snapshot.funding_generation IS NOT DISTINCT FROM NEW.release_funding_generation
          AND snapshot.snapshot_digest = NEW.release_snapshot_digest
    ) THEN
        RAISE EXCEPTION 'release v2 lineage does not match immutable request snapshot'
            USING ERRCODE = '23503';
    END IF;

    IF TG_TABLE_NAME = 'settlement_outbox' THEN
        charge_nano := NEW.actual_nano;
    ELSIF TG_TABLE_NAME = 'usage_events' THEN
        charge_nano := NEW.charge_nano;
    ELSE
        charge_nano := NEW.amount_nano;
    END IF;
    IF NEW.release_billing_mode = 'meter_only' AND charge_nano <> 0 THEN
        RAISE EXCEPTION 'meter-only release v2 lineage cannot carry customer charge'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

CREATE TRIGGER settlement_outbox_release_v2_lineage
BEFORE INSERT OR UPDATE OF
    request_id,
    actual_nano,
    release_schema_version,
    release_generation,
    release_digest,
    release_billing_mode,
    release_funding_generation,
    release_snapshot_digest
ON settlement_outbox
FOR EACH ROW EXECUTE FUNCTION enforce_release_v2_lineage();

CREATE TRIGGER usage_events_release_v2_lineage
BEFORE INSERT OR UPDATE OF
    request_id,
    charge_nano,
    release_schema_version,
    release_generation,
    release_digest,
    release_billing_mode,
    release_funding_generation,
    release_snapshot_digest
ON usage_events
FOR EACH ROW EXECUTE FUNCTION enforce_release_v2_lineage();

CREATE TRIGGER ledger_release_v2_lineage
BEFORE INSERT OR UPDATE OF
    request_id,
    amount_nano,
    release_schema_version,
    release_generation,
    release_digest,
    release_billing_mode,
    release_funding_generation,
    release_snapshot_digest
ON ledger
FOR EACH ROW EXECUTE FUNCTION enforce_release_v2_lineage();

ALTER TABLE engine_instances
    ADD COLUMN IF NOT EXISTS pricing_release_schema_version bigint,
    ADD COLUMN IF NOT EXISTS funding_schema_version bigint,
    ADD COLUMN IF NOT EXISTS pricing_release_runtime_digest text;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint
        WHERE conname = 'engine_instances_release_v2_shape'
          AND conrelid = 'engine_instances'::regclass
    ) THEN
        ALTER TABLE engine_instances
            ADD CONSTRAINT engine_instances_release_v2_shape CHECK (
                (
                    pricing_release_schema_version IS NULL
                    AND funding_schema_version IS NULL
                    AND pricing_release_runtime_digest IS NULL
                )
                OR (
                    pricing_release_schema_version >= 2
                    AND funding_schema_version >= 2
                    AND pricing_release_runtime_digest IS NOT NULL
                    AND pricing_release_runtime_digest <> ''
                )
            ) NOT VALID;
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (23)
ON CONFLICT (version) DO NOTHING;
