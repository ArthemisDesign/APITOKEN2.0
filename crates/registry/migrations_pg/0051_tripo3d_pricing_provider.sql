-- Admit `tripo3d` as a priced reservation/discount provider id.
--
-- The Tripo3D runtime plane (docs/engine/TRIPO3D_PROVIDER.md §4, §5) reserves customer money
-- under `provider = 'tripo3d'` so its prepaid-credit economics can never blend with any other
-- provider's settlement attribution. Two closed sets stand in the way and both widen here,
-- expand-only (the set only grows; no existing id changes meaning):
--
--   * `account_provider_discounts_provider_id_check` (migration 0046) — the per-provider
--     discount override surface. A Tripo3D-specific customer discount is writable only after
--     this row exists; without it the admin write would fail closed on a constraint the
--     operator would have to interpret.
--   * `reservations_scalar_pricing_shape` (migration 0047) — the reservation's pinned
--     provider/multiplier pair. Without `tripo3d` here every Tripo3D admission reserve would
--     die as a CHECK violation inside the billing writer instead of holding money.
--
-- Both constraints are dropped and re-added NOT VALID, then validated: the re-added predicate
-- is strictly wider, so every existing row still satisfies it. No table is created, dropped,
-- truncated or rewritten; the KIMI/GLM/Tripo3D calibration authorities are untouched.

ALTER TABLE account_provider_discounts
    DROP CONSTRAINT IF EXISTS account_provider_discounts_provider_id_check;
ALTER TABLE account_provider_discounts
    ADD CONSTRAINT account_provider_discounts_provider_id_check
    CHECK (provider_id IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d')) NOT VALID;
ALTER TABLE account_provider_discounts
    VALIDATE CONSTRAINT account_provider_discounts_provider_id_check;

ALTER TABLE reservations
    DROP CONSTRAINT IF EXISTS reservations_scalar_pricing_shape;
ALTER TABLE reservations
    ADD CONSTRAINT reservations_scalar_pricing_shape
    CHECK (
        (provider IS NULL OR provider IN ('anthropic', 'openai', 'google', 'kimi', 'glm', 'tripo3d'))
        AND (payable_multiplier_bp IS NULL OR payable_multiplier_bp BETWEEN 0 AND 10000)
    ) NOT VALID;
ALTER TABLE reservations
    VALIDATE CONSTRAINT reservations_scalar_pricing_shape;

INSERT INTO engine_schema_migrations(version) VALUES (51)
ON CONFLICT (version) DO NOTHING;
