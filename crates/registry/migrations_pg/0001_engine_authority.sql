-- Engine-owned PostgreSQL authority. This migration is intentionally transactional and idempotent.
-- Money is integer nanodollars; epochs and wall-clock leases are signed 64-bit Unix seconds.

CREATE SEQUENCE IF NOT EXISTS engine_owner_epoch_seq AS bigint;

CREATE TABLE IF NOT EXISTS engine_schema_migrations (
    version bigint PRIMARY KEY,
    applied_at timestamptz NOT NULL DEFAULT clock_timestamp()
);

CREATE TABLE IF NOT EXISTS engine_instances (
    instance_id text PRIMARY KEY,
    owner_epoch bigint NOT NULL UNIQUE,
    lease_until bigint NOT NULL,
    started_ts bigint NOT NULL,
    updated_ts bigint NOT NULL
);

CREATE TABLE IF NOT EXISTS subs (
    email text PRIMARY KEY,
    token text,
    token_file text,
    proxy text NOT NULL DEFAULT '',
    plan text NOT NULL DEFAULT '',
    status text NOT NULL DEFAULT 'active',
    fleet text NOT NULL DEFAULT 'prod',
    added_ts bigint NOT NULL DEFAULT 0,
    added text NOT NULL DEFAULT '',
    proxy_expire text NOT NULL DEFAULT '',
    proxy_checked_ts bigint,
    proxy_ok boolean
);

CREATE TABLE IF NOT EXISTS accounts (
    id text PRIMARY KEY,
    handle text UNIQUE,
    balance_nano bigint NOT NULL DEFAULT 0,
    spent_nano bigint NOT NULL DEFAULT 0,
    reserved_nano bigint NOT NULL DEFAULT 0 CHECK (reserved_nano >= 0),
    mult_bp bigint NOT NULL DEFAULT 2000 CHECK (mult_bp BETWEEN 0 AND 10000),
    status text NOT NULL DEFAULT 'active',
    created_ts bigint NOT NULL DEFAULT 0,
    created text NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS api_keys (
    key text PRIMARY KEY,
    key_id text NOT NULL UNIQUE,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    label text,
    spent_nano bigint NOT NULL DEFAULT 0,
    status text NOT NULL DEFAULT 'active',
    created_ts bigint NOT NULL DEFAULT 0,
    created text NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS api_keys_account ON api_keys(account_id);

CREATE TABLE IF NOT EXISTS reservations (
    request_id text PRIMARY KEY,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key text NOT NULL,
    hold_nano bigint NOT NULL CHECK (hold_nano >= 0),
    balance_after_reserve_nano bigint NOT NULL,
    owner_instance text NOT NULL,
    owner_epoch bigint NOT NULL,
    lease_until bigint NOT NULL,
    state text NOT NULL CHECK (state IN ('reserved','delivering','settlement_pending','settled','canceled')),
    actual_nano bigint CHECK (actual_nano IS NULL OR actual_nano >= 0),
    created_ts bigint NOT NULL,
    updated_ts bigint NOT NULL,
    settled_ts bigint
);
CREATE INDEX IF NOT EXISTS reservations_owner_state_lease
    ON reservations(owner_instance, owner_epoch, state, lease_until);
CREATE INDEX IF NOT EXISTS reservations_account_state
    ON reservations(account_id, state);

CREATE TABLE IF NOT EXISTS settlement_outbox (
    request_id text PRIMARY KEY REFERENCES reservations(request_id) ON DELETE RESTRICT,
    actual_nano bigint NOT NULL CHECK (actual_nano >= 0),
    disposition text NOT NULL CHECK (disposition IN ('settle','cancel','reconcile_full_hold')),
    reference text,
    model text,
    input_tokens bigint NOT NULL DEFAULT 0,
    output_tokens bigint NOT NULL DEFAULT 0,
    cache_read_tokens bigint NOT NULL DEFAULT 0,
    cache_write_5m_tokens bigint NOT NULL DEFAULT 0,
    cache_write_1h_tokens bigint NOT NULL DEFAULT 0,
    web_search_requests bigint NOT NULL DEFAULT 0,
    real_nano bigint NOT NULL DEFAULT 0,
    state text NOT NULL DEFAULT 'pending' CHECK (state IN ('pending','processing','done')),
    attempts bigint NOT NULL DEFAULT 0,
    next_attempt_ts bigint NOT NULL DEFAULT 0,
    last_error text,
    created_ts bigint NOT NULL,
    updated_ts bigint NOT NULL,
    committed_ts bigint
);
CREATE INDEX IF NOT EXISTS settlement_outbox_pending
    ON settlement_outbox(state, next_attempt_ts, created_ts);

CREATE TABLE IF NOT EXISTS ledger (
    id bigserial PRIMARY KEY,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key text,
    kind text NOT NULL,
    request_id text,
    amount_nano bigint NOT NULL,
    ref text,
    balance_after_nano bigint,
    ts bigint NOT NULL,
    model text
);
CREATE INDEX IF NOT EXISTS ledger_acct ON ledger(account_id, id);
CREATE UNIQUE INDEX IF NOT EXISTS ledger_money_ref
    ON ledger(ref) WHERE kind IN ('topup','adjust') AND ref IS NOT NULL;
CREATE UNIQUE INDEX IF NOT EXISTS ledger_request_once
    ON ledger(kind, request_id) WHERE request_id IS NOT NULL;

CREATE TABLE IF NOT EXISTS usage_events (
    id bigserial PRIMARY KEY,
    request_id text UNIQUE,
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key text,
    model text,
    input_tokens bigint NOT NULL DEFAULT 0,
    output_tokens bigint NOT NULL DEFAULT 0,
    cache_read_tokens bigint NOT NULL DEFAULT 0,
    cache_write_5m_tokens bigint NOT NULL DEFAULT 0,
    cache_write_1h_tokens bigint NOT NULL DEFAULT 0,
    web_search_requests bigint NOT NULL DEFAULT 0,
    real_nano bigint NOT NULL DEFAULT 0,
    charge_nano bigint NOT NULL DEFAULT 0,
    ref text,
    ts bigint NOT NULL
);
CREATE INDEX IF NOT EXISTS usage_events_acct_ts ON usage_events(account_id, ts);

CREATE TABLE IF NOT EXISTS pool_state (
    email text PRIMARY KEY,
    cooling_until bigint NOT NULL DEFAULT 0,
    cap5h double precision NOT NULL DEFAULT 0,
    cap7d double precision NOT NULL DEFAULT 0,
    spent_total double precision NOT NULL DEFAULT 0,
    util5 double precision NOT NULL DEFAULT 0,
    util7 double precision NOT NULL DEFAULT 0,
    reset5 bigint NOT NULL DEFAULT 0,
    reset7 bigint NOT NULL DEFAULT 0,
    calib_n bigint NOT NULL DEFAULT 0,
    inflight bigint NOT NULL DEFAULT 0 CHECK (inflight >= 0),
    version bigint NOT NULL DEFAULT 0,
    writer_instance text,
    writer_epoch bigint,
    updated_ts bigint NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS capacity_leases (
    lease_id text PRIMARY KEY,
    request_id text NOT NULL,
    subscription_email text NOT NULL REFERENCES subs(email) ON DELETE CASCADE,
    owner_instance text NOT NULL,
    owner_epoch bigint NOT NULL,
    lease_until bigint NOT NULL,
    state text NOT NULL CHECK (state IN ('active','released','expired')),
    created_ts bigint NOT NULL,
    released_ts bigint
);
CREATE UNIQUE INDEX IF NOT EXISTS capacity_request_subscription_once
    ON capacity_leases(request_id, subscription_email);
CREATE INDEX IF NOT EXISTS capacity_active_expiry
    ON capacity_leases(subscription_email, state, lease_until);

CREATE TABLE IF NOT EXISTS leader_leases (
    name text PRIMARY KEY,
    owner_instance text NOT NULL,
    owner_epoch bigint NOT NULL,
    lease_until bigint NOT NULL,
    updated_ts bigint NOT NULL
);

INSERT INTO engine_schema_migrations(version) VALUES (1)
ON CONFLICT (version) DO NOTHING;
