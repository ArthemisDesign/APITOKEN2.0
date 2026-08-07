-- Hot tariff override authority (expand-only).
--
-- Until now every per-model price vector lived exclusively in compiled `metering` constants, so a
-- price change required a recompile, a fleet redeploy and a full pricing-release cycle. This
-- migration creates an empty append-only authority where an operator can publish a NEW VERSION of a
-- tariff family without touching the binary: the runtime resolver (delivered by a separate SHA
-- after a green migration/watchdog of this checkpoint) prefers the newest override effective at the
-- priced timestamp and falls back to the compiled values when no override exists.
--
-- Identity model: the compiled constants are the IMPLICIT version 1 of each family
-- (`anthropic/standard/opus-current` is compiled schedule id `anthropic/standard/opus-current/v1`).
-- The first override row therefore carries version 2 and is pinned in reserve snapshots and ledger
-- rows as `<tariff_family>/v<version>`. Overrides are never updated or deleted: a correction is a
-- newer version, so every historical ledger row stays explainable against an immutable payload.
--
-- The table is deliberately empty here; the migration changes no existing row, trigger or
-- constraint, and the old runtime neither reads nor writes it.

CREATE TABLE IF NOT EXISTS pricing_tariff_overrides (
    tariff_family   text        NOT NULL,
    version         integer     NOT NULL,
    effective_from  bigint      NOT NULL,
    payload         jsonb       NOT NULL,
    payload_digest  text        NOT NULL,
    created_ts      bigint      NOT NULL,
    created_by      text        NOT NULL,
    reason          text        NOT NULL,
    PRIMARY KEY (tariff_family, version),
    CHECK (version >= 2),
    CHECK (effective_from >= 0),
    CHECK (created_ts >= 0),
    CHECK (char_length(created_by) > 0),
    CHECK (char_length(reason) > 0),
    CHECK (payload_digest ~ '^sha256:v2:[0-9a-f]{64}$'),
    CHECK (tariff_family ~ '^[a-z0-9][a-z0-9/_-]{0,127}$')
);

-- Versions form a strict gapless-per-writer sequence per family: the next insert must extend the
-- family's own maximum by exactly one. Exact replay of the same (family, version) with the same
-- payload digest is a no-op handled by the writer; a conflicting payload under an existing key is
-- rejected by the primary key.
CREATE OR REPLACE FUNCTION enforce_pricing_tariff_override_version()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    IF NEW.version <> COALESCE((
        SELECT MAX(version) FROM pricing_tariff_overrides
        WHERE tariff_family = NEW.tariff_family
    ), 1) + 1 THEN
        RAISE EXCEPTION 'pricing tariff override version must extend the family sequence'
            USING ERRCODE = '23514';
    END IF;
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS pricing_tariff_overrides_version ON pricing_tariff_overrides;
CREATE TRIGGER pricing_tariff_overrides_version
    BEFORE INSERT ON pricing_tariff_overrides
    FOR EACH ROW EXECUTE FUNCTION enforce_pricing_tariff_override_version();

CREATE OR REPLACE FUNCTION reject_pricing_tariff_override_mutation()
RETURNS trigger
LANGUAGE plpgsql
AS $$
BEGIN
    RAISE EXCEPTION 'pricing tariff overrides are append-only'
        USING ERRCODE = '23514';
END;
$$;

DROP TRIGGER IF EXISTS pricing_tariff_overrides_append_only ON pricing_tariff_overrides;
CREATE TRIGGER pricing_tariff_overrides_append_only
    BEFORE UPDATE OR DELETE ON pricing_tariff_overrides
    FOR EACH ROW EXECUTE FUNCTION reject_pricing_tariff_override_mutation();

INSERT INTO engine_schema_migrations(version) VALUES (36)
ON CONFLICT (version) DO NOTHING;
