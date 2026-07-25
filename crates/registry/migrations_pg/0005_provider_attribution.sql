-- Provider attribution for settled traffic.
--
-- Two independent upstreams now settle into the same money tables: the Claude subscription fleet and
-- the OpenAI-compatible Codex home pool. Without an explicit column, "which provider earned this"
-- can only be guessed from the model string, which would silently misattribute every future alias.
--
-- Expand-only and backward compatible: an engine slot still running the previous release keeps
-- inserting rows without this column and they default to the Claude path it serves.

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS provider text NOT NULL DEFAULT 'anthropic';

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS provider text NOT NULL DEFAULT 'anthropic';

-- The spend breakdown groups by provider over a time window; without this the panel query would
-- sequentially scan the whole retention window on every refresh.
CREATE INDEX IF NOT EXISTS usage_events_provider_ts ON usage_events(provider, ts);

INSERT INTO engine_schema_migrations(version) VALUES (5)
ON CONFLICT (version) DO NOTHING;
