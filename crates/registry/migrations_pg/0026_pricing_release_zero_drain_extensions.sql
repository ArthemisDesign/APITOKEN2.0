-- Expand the pricing-release authority for a traffic-preserving Stage 9 activation.
--
-- Legacy-format requests admitted before the global head commit remain independently settleable
-- from their immutable legacy snapshots. They stay visible in Stage 8 evidence, but their count is
-- no longer a database-level condition for passed evidence. The second half of this migration is
-- a dormant append-only authority for accounts provisioned after cutover. No existing writer reads
-- or writes it until a later producer release is deployed.

ALTER TABLE pricing_stage8_evidence_v2
    DROP CONSTRAINT pricing_stage8_evidence_v2_check1;
ALTER TABLE pricing_stage8_evidence_v2
    ADD CONSTRAINT pricing_stage8_evidence_v2_passed_check CHECK (
        (passed AND blocker_count = 0)
        OR NOT passed
    );

CREATE TABLE pricing_release_assignment_extensions_v2 (
    release_generation bigint NOT NULL,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE RESTRICT,
    account_class text NOT NULL CHECK (account_class IN ('b2c', 'b2b', 'openkeys', 'service')),
    policy_id text NOT NULL,
    policy_version bigint NOT NULL CHECK (policy_version > 0),
    policy_digest text NOT NULL CHECK (policy_digest <> ''),
    billing_mode text NOT NULL CHECK (billing_mode IN ('balance', 'meter_only')),
    funding_generation bigint CHECK (funding_generation IS NULL OR funding_generation > 0),
    purpose text,
    responsible text,
    assignment_digest text NOT NULL CHECK (assignment_digest <> ''),
    provisioning_head_generation bigint NOT NULL,
    provisioning_head_digest text NOT NULL CHECK (provisioning_head_digest <> ''),
    provisioning_head_version bigint NOT NULL CHECK (provisioning_head_version > 0),
    paired_recovery_generation bigint,
    paired_recovery_digest text,
    extension_group_digest text NOT NULL CHECK (extension_group_digest <> ''),
    extension_digest text NOT NULL UNIQUE CHECK (extension_digest <> ''),
    created_ts bigint NOT NULL CHECK (created_ts > 0),
    PRIMARY KEY (release_generation, account_id),
    UNIQUE (release_generation, account_id, assignment_digest),
    FOREIGN KEY (release_generation)
        REFERENCES pricing_release_versions(generation) ON DELETE RESTRICT,
    FOREIGN KEY (policy_id, policy_version, policy_digest)
        REFERENCES pricing_release_policy_versions(policy_id, policy_version, content_digest)
        ON DELETE RESTRICT,
    FOREIGN KEY (account_id, funding_generation)
        REFERENCES account_funding_generations_v2(account_id, generation) ON DELETE RESTRICT,
    FOREIGN KEY (provisioning_head_generation, provisioning_head_digest)
        REFERENCES pricing_release_versions(generation, content_digest) ON DELETE RESTRICT,
    FOREIGN KEY (provisioning_head_version)
        REFERENCES pricing_release_activations_v2(resulting_head_version) ON DELETE RESTRICT,
    CHECK (
        (paired_recovery_generation IS NULL AND paired_recovery_digest IS NULL)
        OR (
            paired_recovery_generation IS NOT NULL
            AND paired_recovery_generation > provisioning_head_generation
            AND paired_recovery_digest IS NOT NULL
            AND paired_recovery_digest <> ''
        )
    ),
    CHECK (
        (
            account_class = 'service'
            AND billing_mode = 'meter_only'
            AND funding_generation IS NULL
            AND purpose IS NOT NULL AND purpose <> ''
            AND responsible IS NOT NULL AND responsible <> ''
        )
        OR (
            account_class IN ('b2c', 'b2b', 'openkeys')
            AND billing_mode = 'balance'
            AND funding_generation IS NOT NULL
            AND purpose IS NULL
            AND responsible IS NULL
        )
    )
);

CREATE INDEX pricing_release_assignment_extensions_v2_account
    ON pricing_release_assignment_extensions_v2(account_id, provisioning_head_version);

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
        RAISE EXCEPTION 'pricing assignment extension duplicates the immutable release manifest'
            USING ERRCODE = '23514';
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

CREATE TRIGGER pricing_release_assignment_extension_v2_shape
BEFORE INSERT ON pricing_release_assignment_extensions_v2
FOR EACH ROW EXECUTE FUNCTION enforce_pricing_release_assignment_extension_v2();

CREATE OR REPLACE FUNCTION assert_pricing_release_assignment_extension_pair_v2()
RETURNS trigger
LANGUAGE plpgsql
AS $$
DECLARE
    expected_rows bigint;
    actual_rows bigint;
BEGIN
    expected_rows := CASE WHEN NEW.paired_recovery_generation IS NULL THEN 1 ELSE 2 END;
    SELECT count(*)::bigint INTO actual_rows
    FROM pricing_release_assignment_extensions_v2 extension
    WHERE extension.account_id = NEW.account_id
      AND extension.provisioning_head_generation = NEW.provisioning_head_generation
      AND extension.provisioning_head_digest = NEW.provisioning_head_digest
      AND extension.provisioning_head_version = NEW.provisioning_head_version
      AND extension.paired_recovery_generation IS NOT DISTINCT FROM NEW.paired_recovery_generation
      AND extension.paired_recovery_digest IS NOT DISTINCT FROM NEW.paired_recovery_digest
      AND extension.extension_group_digest = NEW.extension_group_digest
      AND extension.release_generation IN (
          NEW.provisioning_head_generation,
          COALESCE(NEW.paired_recovery_generation, NEW.provisioning_head_generation)
      );
    IF actual_rows <> expected_rows THEN
        RAISE EXCEPTION 'pricing assignment extension pair is incomplete'
            USING ERRCODE = '23514';
    END IF;
    RETURN NULL;
END;
$$;

CREATE CONSTRAINT TRIGGER pricing_release_assignment_extension_v2_pair
AFTER INSERT ON pricing_release_assignment_extensions_v2
DEFERRABLE INITIALLY DEFERRED
FOR EACH ROW EXECUTE FUNCTION assert_pricing_release_assignment_extension_pair_v2();

CREATE TRIGGER pricing_release_assignment_extensions_v2_immutable
BEFORE UPDATE OR DELETE ON pricing_release_assignment_extensions_v2
FOR EACH ROW EXECUTE FUNCTION reject_immutable_pricing_release_v2_mutation();

INSERT INTO engine_schema_migrations(version) VALUES (26)
ON CONFLICT (version) DO NOTHING;
