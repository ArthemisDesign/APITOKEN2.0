-- Allow an assignment extension to convert a base-covered B2C account to B2B post-cutover.
-- Migration 0030 admitted a base-covered account only as an exact policy-version override of the
-- same policy identity, so a B2C -> B2B conversion could never reach the release authority even
-- though the runtime resolver already prefers the extension row regardless of class. The relaxed
-- branch is deliberately narrow: the transition must be exactly base 'b2c' -> extension 'b2b',
-- billing stays 'balance' on both sides, the funding generation is non-null and identical to the
-- base's, and purpose/responsible metadata match the base (null for balance classes). The
-- strictly-newer-version requirement stays exclusive to same-policy-id overrides; a class change
-- starts a new policy lineage, so no version comparison applies to it. Every other class
-- transition (to/from openkeys, to/from service, b2b -> b2c) and any billing/funding/metadata
-- mismatch keep raising. All other shape guarantees from migrations 0026/0030 are unchanged;
-- existing rows are untouched.

CREATE OR REPLACE FUNCTION enforce_pricing_release_assignment_extension_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1
        FROM pricing_release_head_v2 head
        JOIN pricing_release_activations_v2 activation
          ON activation.resulting_head_version = head.head_version
         AND activation.to_generation = head.active_generation
         AND activation.to_digest = head.active_digest
        WHERE head.singleton = 1
          AND head.active_generation = NEW.provisioning_head_generation
          AND head.active_digest = NEW.provisioning_head_digest
          AND head.head_version = NEW.provisioning_head_version
    ) THEN
        RAISE EXCEPTION 'pricing assignment extension requires the exact current release head'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.release_generation <> NEW.provisioning_head_generation THEN
        IF NEW.paired_recovery_generation IS NULL
           OR NEW.release_generation <> NEW.paired_recovery_generation
           OR NOT EXISTS (
               SELECT 1
               FROM pricing_release_recovery_links link
               WHERE link.target_generation = NEW.provisioning_head_generation
                 AND link.target_digest = NEW.provisioning_head_digest
                 AND link.recovery_generation = NEW.paired_recovery_generation
                 AND link.recovery_digest = NEW.paired_recovery_digest
           ) THEN
            RAISE EXCEPTION 'pricing assignment extension is outside its active/recovery pair'
                USING ERRCODE = '23514';
        END IF;
    ELSIF NEW.paired_recovery_generation IS NOT NULL AND NOT EXISTS (
        SELECT 1
        FROM pricing_release_recovery_links link
        WHERE link.target_generation = NEW.provisioning_head_generation
          AND link.target_digest = NEW.provisioning_head_digest
          AND link.recovery_generation = NEW.paired_recovery_generation
          AND link.recovery_digest = NEW.paired_recovery_digest
    ) THEN
        RAISE EXCEPTION 'pricing assignment extension names an invalid recovery release'
            USING ERRCODE = '23514';
    END IF;

    IF NEW.paired_recovery_generation IS NULL AND EXISTS (
        SELECT 1
        FROM pricing_release_recovery_links link
        WHERE link.target_generation = NEW.provisioning_head_generation
          AND link.target_digest = NEW.provisioning_head_digest
    ) THEN
        RAISE EXCEPTION 'pricing assignment extension must cover a prepared recovery release'
            USING ERRCODE = '23514';
    END IF;

    IF EXISTS (
        SELECT 1 FROM pricing_release_assignments assignment
        WHERE assignment.release_generation = NEW.release_generation
          AND assignment.account_id = NEW.account_id
    ) THEN
        IF NOT EXISTS (
            SELECT 1 FROM pricing_release_assignments assignment
            WHERE assignment.release_generation = NEW.release_generation
              AND assignment.account_id = NEW.account_id
              AND assignment.account_class = NEW.account_class
              AND assignment.billing_mode = NEW.billing_mode
              AND assignment.policy_id = NEW.policy_id
              AND assignment.policy_version < NEW.policy_version
              AND assignment.funding_generation IS NOT DISTINCT FROM NEW.funding_generation
              AND assignment.purpose IS NOT DISTINCT FROM NEW.purpose
              AND assignment.responsible IS NOT DISTINCT FROM NEW.responsible
        ) AND NOT EXISTS (
            SELECT 1 FROM pricing_release_assignments assignment
            WHERE assignment.release_generation = NEW.release_generation
              AND assignment.account_id = NEW.account_id
              AND assignment.account_class = 'b2c'
              AND NEW.account_class = 'b2b'
              AND assignment.billing_mode = 'balance'
              AND NEW.billing_mode = 'balance'
              AND assignment.funding_generation IS NOT NULL
              AND assignment.funding_generation IS NOT DISTINCT FROM NEW.funding_generation
              AND assignment.purpose IS NOT DISTINCT FROM NEW.purpose
              AND assignment.responsible IS NOT DISTINCT FROM NEW.responsible
        ) THEN
            RAISE EXCEPTION 'pricing assignment extension duplicates the immutable release manifest'
                USING ERRCODE = '23514';
        END IF;
    END IF;

    IF NOT EXISTS (
        SELECT 1
        FROM pricing_release_policy_versions policy
        WHERE policy.policy_id = NEW.policy_id
          AND policy.policy_version = NEW.policy_version
          AND policy.content_digest = NEW.policy_digest
          AND policy.account_class = NEW.account_class
          AND policy.billing_mode = NEW.billing_mode
    ) THEN
        RAISE EXCEPTION 'pricing assignment extension policy identity is inconsistent'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

INSERT INTO engine_schema_migrations(version) VALUES (34)
ON CONFLICT (version) DO NOTHING;
