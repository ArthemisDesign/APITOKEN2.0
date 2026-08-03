-- Expand-only ledger attribution shape for Stage 9 release-v2 charge rows. The v2 settlement
-- already stamps immutable release lineage (migration 0023); this checkpoint only widens the
-- legacy multi-discount ranges so a charge row may carry snapshot_kind='release_v2', and adds a
-- dedicated shape constraint for that kind. No existing row changes meaning: policy_v1 and
-- legacy_scalar rows satisfy the same expression as before, and the new shape constraint accepts
-- every non-release_v2 row unchanged. The constraint swap is metadata-only (NOT VALID + VALIDATE)
-- and rewrites no data. No writer emits release_v2 attribution yet; the dependent producer ships
-- separately after this schema is green in production.

ALTER TABLE ledger
    DROP CONSTRAINT ledger_multi_discount_ranges;

ALTER TABLE ledger
    ADD CONSTRAINT ledger_multi_discount_ranges CHECK (
        (attribution_schema_version IS NULL OR attribution_schema_version > 0)
        AND (
            snapshot_kind IS NULL
            OR snapshot_kind IN ('policy_v1', 'legacy_scalar', 'release_v2')
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

ALTER TABLE ledger
    VALIDATE CONSTRAINT ledger_multi_discount_ranges;

ALTER TABLE ledger
    ADD CONSTRAINT ledger_release_v2_attribution_shape CHECK (
        snapshot_kind IS DISTINCT FROM 'release_v2'
        OR (
            attribution_schema_version >= 2
            AND kind = 'charge'
            AND release_schema_version >= 2
            AND release_generation > 0
            AND release_digest IS NOT NULL AND release_digest <> ''
            AND account_class IN ('b2c', 'b2b', 'openkeys', 'service')
            AND snapshot_digest IS NOT NULL AND snapshot_digest <> ''
            AND pricing_mode IS NULL
            AND rule_origin IS NULL
            AND track_eligible IS NULL
            AND retention_eligible IS NULL
            AND commission_eligible IS NULL
            AND paid_funded_nano IS NOT NULL
            AND bonus_funded_nano IS NOT NULL
            AND other_funded_nano IS NOT NULL
        )
    ) NOT VALID;

ALTER TABLE ledger
    VALIDATE CONSTRAINT ledger_release_v2_attribution_shape;

INSERT INTO engine_schema_migrations(version) VALUES (28)
ON CONFLICT (version) DO NOTHING;
