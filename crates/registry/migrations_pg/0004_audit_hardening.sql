-- Persist the exact pricing facts used at settlement. Historical dashboards must sum immutable
-- monetary components instead of repricing old tokens with today's catalog.

ALTER TABLE settlement_outbox
    ADD COLUMN IF NOT EXISTS speed text NOT NULL DEFAULT 'standard',
    ADD COLUMN IF NOT EXISTS inference_geo text NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS input_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS output_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_read_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_write_5m_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_write_1h_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS web_search_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS priced_ts bigint NOT NULL DEFAULT 0;

-- A logically corrupt/invariant-breaking row must not be retried on every writer pass forever.
-- Keep it durably for operator repair without allowing it to stall unrelated settlements.
ALTER TABLE settlement_outbox DROP CONSTRAINT IF EXISTS settlement_outbox_state_check;
ALTER TABLE settlement_outbox ADD CONSTRAINT settlement_outbox_state_check
    CHECK (state IN ('pending', 'processing', 'done', 'failed'));

ALTER TABLE usage_events
    ADD COLUMN IF NOT EXISTS speed text NOT NULL DEFAULT 'standard',
    ADD COLUMN IF NOT EXISTS inference_geo text NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS input_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS output_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_read_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_write_5m_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS cache_write_1h_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS web_search_nano bigint NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS priced_ts bigint NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS reservations_terminal_retention
    ON reservations(settled_ts, request_id)
    WHERE state IN ('settled', 'canceled');
CREATE INDEX IF NOT EXISTS settlement_outbox_done_retention
    ON settlement_outbox(committed_ts, request_id)
    WHERE state = 'done';
CREATE INDEX IF NOT EXISTS capacity_leases_terminal_retention
    ON capacity_leases(released_ts, lease_id)
    WHERE state IN ('released', 'expired');
CREATE INDEX IF NOT EXISTS ledger_charge_retention
    ON ledger(ts, id) WHERE kind = 'charge';

-- Required durable consumers acknowledge ledger IDs before old charge rows may be pruned.
CREATE TABLE IF NOT EXISTS ledger_consumer_checkpoints (
    consumer text NOT NULL,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    last_ledger_id bigint NOT NULL CHECK (last_ledger_id >= 0),
    updated_ts bigint NOT NULL,
    PRIMARY KEY (consumer, account_id)
);

INSERT INTO engine_schema_migrations(version) VALUES (4)
ON CONFLICT (version) DO NOTHING;
