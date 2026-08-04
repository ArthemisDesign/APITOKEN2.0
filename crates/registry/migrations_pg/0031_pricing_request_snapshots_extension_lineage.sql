-- Release-v2 assignment extensions may supersede a base assignment with a strictly newer policy
-- version. Reserves written through such an override pin the extension's assignment identity,
-- which the base-only foreign key and the base-only snapshot trigger both rejected, so every
-- reserve of an override account failed with "pricing v2 snapshot does not match release
-- assignment". The composite key to the base manifest cannot express base-or-extension, so the
-- foreign key is dropped in favor of the trigger, which now accepts either exact lineage. All
-- other snapshot invariants are unchanged, and existing rows are untouched.

ALTER TABLE pricing_request_snapshots_v2
    DROP CONSTRAINT pricing_request_snapshots_v2_release_generation_account_id_fkey;

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
        FROM (
            SELECT account_id, release_generation, assignment_digest, account_class,
                   policy_id, policy_version, policy_digest, billing_mode, funding_generation
              FROM pricing_release_assignments
            UNION ALL
            SELECT account_id, release_generation, assignment_digest, account_class,
                   policy_id, policy_version, policy_digest, billing_mode, funding_generation
              FROM pricing_release_assignment_extensions_v2
        ) assignment
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

INSERT INTO engine_schema_migrations(version) VALUES (31)
ON CONFLICT (version) DO NOTHING;
