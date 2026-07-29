# Stage 2: PostgreSQL engine authority

Stage 2 is deployed. It moved the Rust engine's authoritative subscriptions, customer money,
request reservations, settlement retry state, subscription capacity, pool state, and poller
leadership from the single-process SQLite model into the isolated PostgreSQL database
`claude_engine`. The retained SQLite file is an import-era audit snapshot, not a live fallback.

This enables two Anthropic processes to overlap during a blue-green reload while permanent OpenAI
and Gemini provider processes each hold an independent owner epoch. It does **not** make the single
production host highly available; host-loss tolerance remains Stage 3 work.

## Ownership boundary

- The engine role owns only `claude_engine` and is `NOSUPERUSER NOCREATEDB NOCREATEROLE
  NOREPLICATION`.
- Commerce owns `commerce`, receives no engine DSN, and talks to the engine only through the Control
  API at `http://127.0.0.1:8790`.
- The production DSN is root-owned mode 0600 at
  `/srv/claude-api/data/engine-postgres.env` and is loaded by engine systemd units.
- PostgreSQL is the recovery floor. Redis/Valkey may later be a measured, rebuildable hot index, but
  never the source of truth for money or fencing.

## Schema and invariants

The transactional, idempotent schema is
[`crates/registry/migrations_pg/0001_engine_authority.sql`](../crates/registry/migrations_pg/0001_engine_authority.sql).
Money is signed 64-bit integer nanodollars; leases use Unix seconds.

| Table | Purpose and correctness rule |
|---|---|
| `engine_instances` | Unique monotonic `owner_epoch` plus heartbeat lease for each process identity. |
| `subs` | Engine-owned subscription credentials, proxy, plan, fleet, and status. |
| `accounts`, `api_keys` | Customer balances and access keys; reserved money cannot be negative. |
| `reservations` | One durable hold per generated `request_id`, owned by instance+epoch and state. |
| `settlement_outbox` | Idempotent settlement/cancel intent retried until the money transaction commits. |
| `ledger` | Monetary journal; `(kind, request_id)` and payment/adjustment references are unique. |
| `usage_events` | At most one usage record per request ID. |
| `pool_state` | Cooling/utilization/calibration with CAS `version` and fenced writer identity. |
| `capacity_leases` | Atomic per-request subscription admission and matching inflight ownership. |
| `leader_leases` | One PostgreSQL lease-epoch leader for polling; there is no Redlock path. |

The money invariant is `actual charge <= request hold <= available balance at reserve`. Settlement
closes the exact reservation and, in one transaction, changes balances, inserts the unique charge,
writes usage, and marks the outbox done. Retrying the request identity cannot double-charge.

## Request lifecycle

1. The engine generates its own unique `request_id`; an upstream request ID is audit metadata only.
2. A transaction authenticates key/account, checks balance, creates the reservation, increments
   account reserved money, and records owner instance/epoch/lease.
3. Before response delivery, the reservation moves from `reserved` to `delivering`.
4. Settlement or cancellation is inserted idempotently into `settlement_outbox`.
5. The outbox actor retries database failures until one transaction commits the final state.
6. Reconciliation may cancel an expired pre-delivery hold. Once delivery began, it conservatively
   charges the full hold when a dead owner cannot provide authoritative final usage; it does not
   refund potentially delivered inference.

Reservation states are `reserved`, `delivering`, `settlement_pending`, `settled`, and `canceled`.
Outbox states are `pending`, `processing`, and `done`.

## Fencing, capacity, and leadership

Every engine start claims the next database owner epoch and heartbeats it. A stale process cannot use
an older epoch to reserve money, persist pool state, or acquire capacity. Loss of the heartbeat makes
readiness fail closed so Caddy removes the instance from new traffic.

Capacity admission is one transaction: lock pool state, expire stale leases, validate
cooldown/utilization/inflight, insert the request lease, increment inflight, and return. Release is
idempotent and decrements once. Pool-state writes use versioned compare-and-swap; on overlap the loser
rebases conservatively rather than overwriting newer state. Polling uses a named PostgreSQL leader
lease carrying the same fenced epoch.

SQLite mode retains the host-local `flock` singleton because aggregate reservations cannot support
safe ownership overlap. PostgreSQL mode relaxes it only after the real fault matrix proves overlap.

## Readiness and blue-green routing

Anthropic slots are `claude-api-anthropic@8787.service` and `claude-api-anthropic@8788.service`, each pinned to provider
mode `anthropic`. `SIGUSR1` changes `/ready` to 503 immediately while leaving `/health`, the listener,
and established SSE streams alive. After Caddy depools old, `SIGTERM` begins the bounded graceful
drain. The OpenAI-compatible provider is the singleton `claude-api-openai.service`, pinned to mode
`openai` on 8793; Caddy exposes its stable loopback origin on 8792. Native Gemini is the separate
singleton `claude-api-gemini.service`, pinned to mode `gemini` on 8795 with stable origin 8794.

Public Anthropic traffic and the operator panel use the health-gated slots. Commerce uses
`127.0.0.1:8790`, an explicitly loopback-bound Caddy listener, so slot alternation cannot break
API/worker Control calls. OpenAI and Gemini traffic never traverse that listener; they use 8792 and
8794 respectively. The provider controller requires 8790 before, during, and after Anthropic drain,
then exact-release gates both singletons and their stable origins before committing the cohort.

Every process uses a distinct `CLAUDE_API_INSTANCE_ID`. PostgreSQL remains authoritative for shared
customer balances, request reservations, settlement and fencing across all provider processes. Codex adds a
host-local invariant: the OpenAI process takes `/run/apitoken/codex-home.lock` before discovering any
home and holds that single process-wide fence across every child restart.

## One-time cutover and rollback boundary

The completed cutover used:

1. `engine-postgres-provision.sh` to create the isolated role/database and stage a root-only DSN;
2. a consistent SQLite snapshot and full singleton drain;
3. `engine-postgres-cutover.sh` to run `db migrate-postgres`, which refuses anonymous aggregate
   reservations and atomically imports and reconciles subscriptions, accounts, keys, ledger, usage,
   and pool state;
4. `db verify-postgres`, systemd slot installation, readiness, Caddy routing, and blue-green handoff.

Before activation, the cutover trap can restore the prior unit and SQLite environment. After
PostgreSQL serves production writes, do not point the engine back to SQLite: it is stale. Rollback
must select an older compatible PostgreSQL-aware binary through the normal blue-green controller.

The one-time scripts refuse an already-active environment. They remain for reconstruction/new
environments, not routine releases.

## Verification and tests

```bash
cargo test --workspace
pnpm build && pnpm typecheck && pnpm test
```

`pg::tests::stage2_fault_matrix` exercises owner fencing, idempotent settlement, outbox recovery,
capacity expiry/release, leader leases, and CAS pool writes against real PostgreSQL when
`CLAUDE_API_TEST_DATABASE_URL` is set. The host E2E additionally proves:

- two simultaneous PostgreSQL owners on the same legacy DB path;
- concurrent requests create exactly one charge and usage row each;
- outbox, capacity, inflight, and reserved money return to zero;
- the engine role cannot read commerce users;
- SQLite still rejects a second process on the same path;
- `SIGUSR1` flips readiness without killing health/listener;
- graceful shutdown completes.

Run the E2E only with its temporary database harness:

```bash
sudo deploy/test-stage2-e2e.sh /path/to/test/claude-api
```

Production schema/totals verification (the DSN is not printed):

```bash
sudo bash -c 'set -a; . /srv/claude-api/data/engine-postgres.env; set +a; \
  exec /srv/claude-api/releases/current/claude-api db verify-postgres'
```

At idle steady state expect:

- one `engine_instances` row whose lease is current;
- zero reservations in `reserved`, `delivering`, or `settlement_pending`;
- zero outbox rows not in `done`;
- zero active capacity leases and zero summed pool inflight;
- zero summed `accounts.reserved_nano`;
- no duplicate `ledger` charge request IDs.

Nonzero request/capacity counts can be valid during traffic; they must drain and must not remain owned
by an expired instance.

Run this read-only query as the engine database role during an idle audit. The expected row is
`0:0:0:0:0:0:1` in the column order shown:

```sql
SELECT
  (SELECT count(*) FROM reservations
     WHERE state IN ('reserved','delivering','settlement_pending')) AS active_reservations,
  (SELECT count(*) FROM settlement_outbox WHERE state <> 'done') AS pending_outbox,
  (SELECT count(*) FROM capacity_leases WHERE state = 'active') AS active_capacity,
  (SELECT coalesce(sum(inflight), 0) FROM pool_state) AS inflight,
  (SELECT coalesce(sum(reserved_nano), 0) FROM accounts) AS reserved_nano,
  (SELECT count(*) FROM (
     SELECT request_id FROM ledger
       WHERE kind = 'charge' AND request_id IS NOT NULL
       GROUP BY request_id HAVING count(*) > 1
   ) duplicates) AS duplicate_charges,
  (SELECT count(*) FROM engine_instances
     WHERE lease_until >= extract(epoch FROM now())::bigint) AS live_owners;
```

## Backups and remaining limits

`deploy/apitoken-db-dump` atomically writes independent custom-format `commerce.dump` and
`claude_engine.dump` files under `/var/lib/apitoken/backups`. The hourly
`claude-api-backup.timer` stages them for daily off-host Borg backup. Validate both with the matching
PostgreSQL `pg_restore --list`; raw copies of live PostgreSQL files are not backups.

Stage 2 makes planned engine reloads invisible on the current host. It does not survive loss of that
host or PostgreSQL instance. Stage 3 requires at least two failure domains, an external
health-checked load balancer, synchronous zone-redundant PostgreSQL (or correctly operated
Patroni/etcd), and independent PITR/WAL backups.
