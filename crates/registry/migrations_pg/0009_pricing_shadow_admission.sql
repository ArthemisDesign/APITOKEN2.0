-- Durable, non-authoritative attribution for a future pricing-policy shadow evaluator.
--
-- The actual price of a request continues to belong to pricing_admission_snapshots. A shadow
-- evaluation is deliberately separate: during the shadow rollout the actual request is charged by
-- legacy_scalar while this row records what the immutable policy resolver would have selected.
-- This migration installs no writer, no catalog/policy data and no active head. Old binaries keep
-- using reservations exactly as before.

CREATE UNIQUE INDEX IF NOT EXISTS account_policy_versions_shadow_identity
    ON account_policy_versions(
        account_id,
        effective_version,
        policy_id,
        policy_version,
        source_policy_digest,
        product_id,
        account_class,
        schema_version,
        catalog_generation,
        switch_generation,
        content_digest
    );
CREATE UNIQUE INDEX IF NOT EXISTS pricing_admission_snapshots_shadow_actual_identity
    ON pricing_admission_snapshots(
        request_id,
        account_id,
        snapshot_kind,
        provider_id,
        requested_model_id,
        canonical_model_id,
        alias_generation,
        payable_multiplier_bp,
        official_hold_nano,
        charged_hold_nano,
        snapshot_digest
    );

CREATE TABLE IF NOT EXISTS pricing_shadow_admission_evaluations (
    request_id text PRIMARY KEY,
    account_id text NOT NULL,
    actual_snapshot_kind text NOT NULL CHECK (actual_snapshot_kind = 'legacy_scalar'),
    actual_snapshot_digest text NOT NULL CHECK (actual_snapshot_digest <> ''),
    provider_id text NOT NULL CHECK (provider_id <> ''),
    requested_model_id text NOT NULL CHECK (requested_model_id <> ''),
    canonical_model_id text NOT NULL CHECK (canonical_model_id <> ''),
    alias_generation bigint NOT NULL CHECK (alias_generation > 0),
    evaluator_schema_version bigint NOT NULL CHECK (evaluator_schema_version > 0),
    runtime_manifest_generation bigint NOT NULL CHECK (runtime_manifest_generation > 0),
    runtime_manifest_digest text NOT NULL CHECK (runtime_manifest_digest <> ''),
    enqueued_ts bigint NOT NULL CHECK (enqueued_ts > 0),
    evaluated_ts bigint NOT NULL CHECK (evaluated_ts > 0),
    outcome text NOT NULL CHECK (outcome IN ('resolved', 'rejected', 'read_error')),
    reason_code text,
    authorized_multiplier_bp bigint NOT NULL CHECK (authorized_multiplier_bp BETWEEN 0 AND 10000),
    observed_multiplier_bp bigint CHECK (observed_multiplier_bp BETWEEN 0 AND 10000),
    official_hold_nano bigint NOT NULL CHECK (official_hold_nano >= 0),
    legacy_hold_nano bigint NOT NULL CHECK (legacy_hold_nano >= 0),
    product_id text,
    account_class text CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    effective_policy_version bigint CHECK (effective_policy_version > 0),
    policy_id text,
    policy_version bigint CHECK (policy_version > 0),
    source_policy_digest text,
    policy_digest text,
    policy_schema_version bigint CHECK (policy_schema_version > 0),
    policy_catalog_generation bigint CHECK (policy_catalog_generation > 0),
    policy_catalog_schema_version bigint CHECK (policy_catalog_schema_version > 0),
    policy_catalog_capability_generation bigint
        CHECK (policy_catalog_capability_generation > 0),
    policy_catalog_capability_digest text,
    policy_catalog_digest text,
    policy_switch_generation bigint CHECK (policy_switch_generation > 0),
    policy_switch_schema_version bigint CHECK (policy_switch_schema_version > 0),
    policy_switch_capability_generation bigint
        CHECK (policy_switch_capability_generation > 0),
    policy_switch_capability_digest text,
    policy_switch_digest text,
    admission_catalog_generation bigint CHECK (admission_catalog_generation > 0),
    admission_catalog_schema_version bigint CHECK (admission_catalog_schema_version > 0),
    admission_catalog_capability_generation bigint
        CHECK (admission_catalog_capability_generation > 0),
    admission_catalog_capability_digest text,
    admission_catalog_digest text,
    admission_switch_generation bigint CHECK (admission_switch_generation > 0),
    admission_switch_schema_version bigint CHECK (admission_switch_schema_version > 0),
    admission_switch_capability_generation bigint
        CHECK (admission_switch_capability_generation > 0),
    admission_switch_capability_digest text,
    admission_switch_digest text,
    rule_id text,
    rule_digest text,
    rule_scope text CHECK (rule_scope IN ('provider', 'model')),
    pricing_mode text CHECK (pricing_mode IN ('track', 'discount')),
    rule_origin text CHECK (rule_origin IN ('managed', 'legacy')),
    discount_bps bigint,
    payable_multiplier_bp bigint CHECK (payable_multiplier_bp BETWEEN 0 AND 10000),
    track_eligible boolean,
    retention_eligible boolean,
    commission_eligible boolean,
    policy_hold_nano bigint CHECK (policy_hold_nano >= 0),
    comparison_result text NOT NULL
        CHECK (comparison_result IN ('equal', 'different', 'not_comparable')),
    -- Best-effort, non-authoritative diagnostics. Immutable identity belongs in typed columns.
    diagnostic_context jsonb NOT NULL CHECK (jsonb_typeof(diagnostic_context) = 'object'),
    evaluation_digest text NOT NULL CHECK (evaluation_digest <> ''),
    UNIQUE (request_id, account_id),
    FOREIGN KEY (
        request_id,
        account_id,
        actual_snapshot_kind,
        provider_id,
        requested_model_id,
        canonical_model_id,
        alias_generation,
        authorized_multiplier_bp,
        official_hold_nano,
        legacy_hold_nano,
        actual_snapshot_digest
    ) REFERENCES pricing_admission_snapshots(
        request_id,
        account_id,
        snapshot_kind,
        provider_id,
        requested_model_id,
        canonical_model_id,
        alias_generation,
        payable_multiplier_bp,
        official_hold_nano,
        charged_hold_nano,
        snapshot_digest
    ) ON DELETE CASCADE,
    FOREIGN KEY (
        account_id,
        effective_policy_version,
        policy_id,
        policy_version,
        source_policy_digest,
        product_id,
        account_class,
        policy_schema_version,
        policy_catalog_generation,
        policy_switch_generation,
        policy_digest
    ) REFERENCES account_policy_versions(
        account_id,
        effective_version,
        policy_id,
        policy_version,
        source_policy_digest,
        product_id,
        account_class,
        schema_version,
        catalog_generation,
        switch_generation,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (account_id, effective_policy_version, rule_id, rule_digest)
        REFERENCES account_policy_rules(account_id, effective_version, rule_id, rule_digest)
        ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        policy_catalog_generation,
        provider_id,
        canonical_model_id
    ) REFERENCES pricing_catalog_entries(
        product_id,
        generation,
        provider_id,
        canonical_model_id
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        policy_catalog_generation,
        policy_catalog_schema_version,
        policy_catalog_capability_generation,
        policy_catalog_capability_digest,
        policy_catalog_digest
    ) REFERENCES pricing_catalog_versions(
        product_id,
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        policy_switch_generation,
        policy_switch_schema_version,
        policy_switch_capability_generation,
        policy_switch_capability_digest,
        policy_switch_digest
    ) REFERENCES provider_switch_versions(
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        admission_catalog_generation,
        provider_id,
        canonical_model_id
    ) REFERENCES pricing_catalog_entries(
        product_id,
        generation,
        provider_id,
        canonical_model_id
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        product_id,
        admission_catalog_generation,
        admission_catalog_schema_version,
        admission_catalog_capability_generation,
        admission_catalog_capability_digest,
        admission_catalog_digest
    ) REFERENCES pricing_catalog_versions(
        product_id,
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    FOREIGN KEY (
        admission_switch_generation,
        admission_switch_schema_version,
        admission_switch_capability_generation,
        admission_switch_capability_digest,
        admission_switch_digest
    ) REFERENCES provider_switch_versions(
        generation,
        schema_version,
        capability_generation,
        capability_digest,
        content_digest
    ) ON DELETE RESTRICT,
    CHECK (evaluated_ts >= enqueued_ts),
    CHECK (
        (
            outcome = 'resolved'
            AND reason_code IS NULL
            AND observed_multiplier_bp IS NOT NULL
            AND product_id IS NOT NULL AND product_id <> ''
            AND account_class IS NOT NULL
            AND effective_policy_version IS NOT NULL
            AND policy_id IS NOT NULL AND policy_id <> ''
            AND policy_version IS NOT NULL
            AND source_policy_digest IS NOT NULL AND source_policy_digest <> ''
            AND policy_digest IS NOT NULL AND policy_digest <> ''
            AND policy_schema_version IS NOT NULL
            AND policy_schema_version = evaluator_schema_version
            AND policy_catalog_generation IS NOT NULL
            AND policy_catalog_schema_version IS NOT NULL
            AND policy_catalog_schema_version = policy_schema_version
            AND policy_catalog_capability_generation IS NOT NULL
            AND policy_catalog_capability_digest IS NOT NULL
            AND policy_catalog_capability_digest <> ''
            AND policy_catalog_digest IS NOT NULL AND policy_catalog_digest <> ''
            AND policy_switch_generation IS NOT NULL
            AND policy_switch_schema_version IS NOT NULL
            AND policy_switch_schema_version = policy_schema_version
            AND policy_switch_capability_generation IS NOT NULL
            AND policy_switch_capability_generation = policy_catalog_capability_generation
            AND policy_switch_capability_digest IS NOT NULL
            AND policy_switch_capability_digest <> ''
            AND policy_switch_capability_digest = policy_catalog_capability_digest
            AND policy_switch_digest IS NOT NULL AND policy_switch_digest <> ''
            AND admission_catalog_generation IS NOT NULL
            AND admission_catalog_schema_version IS NOT NULL
            AND admission_catalog_schema_version = evaluator_schema_version
            AND admission_catalog_capability_generation IS NOT NULL
            AND admission_catalog_capability_digest IS NOT NULL
            AND admission_catalog_capability_digest <> ''
            AND admission_catalog_digest IS NOT NULL AND admission_catalog_digest <> ''
            AND admission_switch_generation IS NOT NULL
            AND admission_switch_schema_version IS NOT NULL
            AND admission_switch_schema_version = evaluator_schema_version
            AND admission_switch_capability_generation IS NOT NULL
            AND admission_switch_capability_digest IS NOT NULL
            AND admission_switch_capability_digest <> ''
            AND admission_switch_digest IS NOT NULL AND admission_switch_digest <> ''
            AND rule_id IS NOT NULL AND rule_id <> ''
            AND rule_digest IS NOT NULL AND rule_digest <> ''
            AND rule_scope IS NOT NULL
            AND pricing_mode IS NOT NULL
            AND rule_origin IS NOT NULL
            AND payable_multiplier_bp IS NOT NULL
            AND track_eligible IS NOT NULL
            AND retention_eligible IS NOT NULL
            AND commission_eligible IS NOT NULL
            AND policy_hold_nano IS NOT NULL
            AND (
                (comparison_result = 'equal' AND policy_hold_nano = legacy_hold_nano)
                OR (comparison_result = 'different' AND policy_hold_nano <> legacy_hold_nano)
            )
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
            outcome IN ('rejected', 'read_error')
            AND reason_code IS NOT NULL AND reason_code <> ''
            AND (
                (outcome = 'rejected' AND observed_multiplier_bp IS NOT NULL)
                OR (outcome = 'read_error' AND observed_multiplier_bp IS NULL)
            )
            AND product_id IS NULL
            AND account_class IS NULL
            AND effective_policy_version IS NULL
            AND policy_id IS NULL
            AND policy_version IS NULL
            AND source_policy_digest IS NULL
            AND policy_digest IS NULL
            AND policy_schema_version IS NULL
            AND policy_catalog_generation IS NULL
            AND policy_catalog_schema_version IS NULL
            AND policy_catalog_capability_generation IS NULL
            AND policy_catalog_capability_digest IS NULL
            AND policy_catalog_digest IS NULL
            AND policy_switch_generation IS NULL
            AND policy_switch_schema_version IS NULL
            AND policy_switch_capability_generation IS NULL
            AND policy_switch_capability_digest IS NULL
            AND policy_switch_digest IS NULL
            AND admission_catalog_generation IS NULL
            AND admission_catalog_schema_version IS NULL
            AND admission_catalog_capability_generation IS NULL
            AND admission_catalog_capability_digest IS NULL
            AND admission_catalog_digest IS NULL
            AND admission_switch_generation IS NULL
            AND admission_switch_schema_version IS NULL
            AND admission_switch_capability_generation IS NULL
            AND admission_switch_capability_digest IS NULL
            AND admission_switch_digest IS NULL
            AND rule_id IS NULL
            AND rule_digest IS NULL
            AND rule_scope IS NULL
            AND pricing_mode IS NULL
            AND rule_origin IS NULL
            AND discount_bps IS NULL
            AND payable_multiplier_bp IS NULL
            AND track_eligible IS NULL
            AND retention_eligible IS NULL
            AND commission_eligible IS NULL
            AND policy_hold_nano IS NULL
            AND comparison_result = 'not_comparable'
        )
    ),
    CHECK (commission_eligible IS NOT TRUE OR pricing_mode = 'track')
);

CREATE INDEX IF NOT EXISTS pricing_shadow_admission_evaluations_time
    ON pricing_shadow_admission_evaluations(evaluated_ts, outcome);
CREATE INDEX IF NOT EXISTS pricing_shadow_admission_evaluations_account
    ON pricing_shadow_admission_evaluations(account_id, evaluated_ts);

CREATE OR REPLACE FUNCTION enforce_pricing_shadow_admission_rule_identity()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.outcome = 'resolved' AND NOT EXISTS (
        SELECT 1
        FROM account_policy_rules AS rule
        WHERE rule.account_id = NEW.account_id
          AND rule.effective_version = NEW.effective_policy_version
          AND rule.rule_id = NEW.rule_id
          AND rule.rule_digest = NEW.rule_digest
          AND rule.scope_type = NEW.rule_scope
          AND rule.provider_id = NEW.provider_id
          AND rule.canonical_model_id IS NOT DISTINCT FROM
              CASE
                  WHEN NEW.rule_scope = 'model' THEN NEW.canonical_model_id
                  ELSE NULL
              END
          AND rule.pricing_mode = NEW.pricing_mode
          AND rule.rule_origin = NEW.rule_origin
          AND rule.discount_bps IS NOT DISTINCT FROM NEW.discount_bps
          AND rule.payable_multiplier_bp = NEW.payable_multiplier_bp
          AND rule.track_eligible = NEW.track_eligible
          AND rule.retention_eligible = NEW.retention_eligible
          AND rule.commission_eligible = NEW.commission_eligible
    ) THEN
        RAISE EXCEPTION 'pricing shadow admission rule identity does not match immutable policy rule'
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
        WHERE tgname = 'pricing_shadow_admission_evaluation_rule_identity'
          AND tgrelid = 'pricing_shadow_admission_evaluations'::regclass
          AND NOT tgisinternal
    ) THEN
        EXECUTE
            'CREATE TRIGGER pricing_shadow_admission_evaluation_rule_identity
             BEFORE INSERT ON pricing_shadow_admission_evaluations
             FOR EACH ROW
             EXECUTE FUNCTION enforce_pricing_shadow_admission_rule_identity()';
    END IF;
END $$;

CREATE OR REPLACE FUNCTION reject_pricing_shadow_admission_evaluation_update()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pricing shadow admission evaluations are immutable'
        USING ERRCODE = '55000';
END;
$$;

DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pg_trigger
        WHERE tgname = 'pricing_shadow_admission_evaluation_immutable_update'
          AND tgrelid = 'pricing_shadow_admission_evaluations'::regclass
          AND NOT tgisinternal
    ) THEN
        EXECUTE
            'CREATE TRIGGER pricing_shadow_admission_evaluation_immutable_update
             BEFORE UPDATE ON pricing_shadow_admission_evaluations
             FOR EACH ROW
             EXECUTE FUNCTION reject_pricing_shadow_admission_evaluation_update()';
    END IF;
END $$;

INSERT INTO engine_schema_migrations(version) VALUES (9)
ON CONFLICT (version) DO NOTHING;
