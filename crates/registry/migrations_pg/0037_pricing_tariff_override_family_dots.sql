-- Hot tariff override family format correction (expand-only).
--
-- Migration 0036 constrained `tariff_family` to `^[a-z0-9][a-z0-9/_-]{0,127}$`, which excludes
-- the dot. The compiled `metering` family keys are built from canonical model ids, and those ids
-- contain dots (`gemini-2.5-pro`, `gpt-5.6-sol`, `glm-5.2`, `kimi-k2.7-code`), so no per-model
-- override family could ever be inserted. This migration widens the CHECK to admit the dot; every
-- row valid under the old rule stays valid, and the append-only/sequence triggers are untouched.

ALTER TABLE pricing_tariff_overrides
    DROP CONSTRAINT pricing_tariff_overrides_tariff_family_check;

ALTER TABLE pricing_tariff_overrides
    ADD CONSTRAINT pricing_tariff_overrides_family_format
    CHECK (tariff_family ~ '^[a-z0-9][a-z0-9/._-]{0,127}$');

INSERT INTO engine_schema_migrations(version) VALUES (37)
ON CONFLICT (version) DO NOTHING;
