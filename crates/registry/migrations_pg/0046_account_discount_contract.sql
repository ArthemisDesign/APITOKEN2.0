-- The runtime has always accepted only these five canonical pricing provider ids. Migration 0043
-- accidentally named the GLM provider `zhipu` in a comment while every writer and request path uses
-- `glm`; the free-form PostgreSQL column therefore failed to enforce the actual closed contract.
-- Production was verified to contain only google/openai rows before this checkpoint.
ALTER TABLE account_provider_discounts
    ADD CONSTRAINT account_provider_discounts_provider_id_check
    CHECK (provider_id IN ('anthropic', 'openai', 'google', 'kimi', 'glm'));

INSERT INTO engine_schema_migrations(version) VALUES (46)
ON CONFLICT (version) DO NOTHING;
