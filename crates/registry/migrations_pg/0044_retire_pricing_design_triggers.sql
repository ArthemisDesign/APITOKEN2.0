-- Release the retired pricing design's hold on the live money tables.
--
-- The catalog/switch/policy/release authority and the funding buckets/lots ledger are gone from
-- the runtime: a request is priced by the account's discount and funded from the account balance.
-- What remains in the database are constraint triggers that police those retired structures on
-- every money mutation. They are not neutral: `accounts_active_funding_v2_parity` raises unless a
-- normalized account's lot aggregates equal its own, and `reservations_funding_snapshot_v2`
-- requires every new reservation of such an account to carry a funding snapshot. With the writers
-- that maintained them removed, they would refuse the one reserve path that is left.
--
-- Only the triggers go here. The tables keep their rows as history, per the expand-only rule; a
-- separate migration drops them once nothing has read them for a full retention window.
--
-- The blue-green peer still running the previous binary keeps writing those rows while it drains.
-- That stays valid — dropping a check never invalidates data that already satisfied it.

-- Money tables: the account row, its reservations, the ledger and the settlement/usage records.
DROP TRIGGER IF EXISTS accounts_active_funding_v2_parity ON accounts;
DROP TRIGGER IF EXISTS accounts_strict_funding_parity ON accounts;
DROP TRIGGER IF EXISTS reservations_funding_snapshot_v2 ON reservations;
DROP TRIGGER IF EXISTS reservations_pricing_funding_v2 ON reservations;
DROP TRIGGER IF EXISTS reservations_strict_policy_funding ON reservations;
DROP TRIGGER IF EXISTS ledger_release_v2_lineage ON ledger;
DROP TRIGGER IF EXISTS settlement_outbox_release_v2_lineage ON settlement_outbox;
DROP TRIGGER IF EXISTS usage_events_release_v2_lineage ON usage_events;

-- Key issuance no longer acknowledges a policy head.
DROP TRIGGER IF EXISTS api_keys_strict_policy_ack ON api_keys;

-- The retired funding ledgers police themselves; nothing writes them any more.
DROP TRIGGER IF EXISTS funding_buckets_strict_account_parity ON funding_buckets;
DROP TRIGGER IF EXISTS funding_lots_v2_generation_parity ON funding_lots_v2;
DROP TRIGGER IF EXISTS reservation_allocations_strict_policy_funding ON reservation_funding_allocations;
DROP TRIGGER IF EXISTS funding_reservation_snapshot_v2_account ON funding_reservation_snapshots_v2;
DROP TRIGGER IF EXISTS funding_reservation_snapshot_v2_immutable ON funding_reservation_snapshots_v2;
DROP TRIGGER IF EXISTS funding_reservation_snapshots_v2_parity ON funding_reservation_snapshots_v2;

INSERT INTO engine_schema_migrations(version) VALUES (44)
ON CONFLICT (version) DO NOTHING;
