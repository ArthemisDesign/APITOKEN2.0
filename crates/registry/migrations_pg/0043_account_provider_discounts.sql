-- Per-account, per-provider discount. One row overrides `accounts.mult_bp` for exactly one
-- provider; no row means the account default applies. This is the entire pricing policy surface:
-- a B2C account keeps its single default (50% today), a B2B account gets one row per provider it
-- negotiated separately, and OpenKeys stay at 10000 bp.
--
-- It deliberately replaces the account policy/binding/catalog/switch/release machinery. That
-- design put a second representation of both price and money in front of admission, and on
-- 2026-08-09 the two disagreed: 166 of 168 accounts moved to strict enforcement had no legacy
-- funding rows, so every funded request was refused with "insufficient balance". A discount is a
-- number attached to an account, and this table keeps it that way — a write takes effect on the
-- next request, with no generation, no cutover and no snapshot to drift.
CREATE TABLE IF NOT EXISTS account_provider_discounts (
    account_id text NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    -- Engine provider id: `anthropic`, `openai`, `google`, `kimi`, `zhipu`.
    provider_id text NOT NULL,
    -- Payable multiplier in basis points: 10000 = list price, 5000 = 50% off, 0 = free.
    mult_bp bigint NOT NULL CHECK (mult_bp >= 0 AND mult_bp <= 10000),
    updated_ts bigint NOT NULL,
    PRIMARY KEY (account_id, provider_id)
);

INSERT INTO engine_schema_migrations(version) VALUES (43)
ON CONFLICT (version) DO NOTHING;
