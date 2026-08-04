# Redis in this project — current topology and where it pays off next

Redis is shared infrastructure owned by the ops lane, but every consumer today lives in the Rust
engine. This document records what the two instances actually hold, what the operational posture
of that data is, and — the reason this file exists — an evidence-based, ranked list of the places
where introducing Redis would materially reduce load or latency.

It is a decision map, not a mandate. Nothing here is scheduled; each item names its own risk and
the boundary it must respect. Items marked **do not do** are recorded so the same idea is not
re-proposed every quarter.

## Current topology

Two instances, loopback-only, defined in `deploy/affinity-redis.compose.yaml` and started by
`systemd/apitoken-affinity-redis.service`:

| Instance | Port | Owner | Contents | Budget | Loss impact |
| --- | --- | --- | --- | --- | --- |
| `affinity-redis` | `127.0.0.1:6379` | `crates/forward/src/codex/history.rs` | Codex response history, encrypted envelopes keyed by `response_id` | 512 MiB, `allkeys-lru`, AOF `everysec` | Customer-visible: a lost key answers `previous_response_id` with a 400 |
| `cache-affinity-redis` | `127.0.0.1:6380` | `crates/forward/src/affinity.rs` | Cache-lineage affinity aliases, session bindings, cache roots, cooling hints | 128 MiB, `allkeys-lru`, AOF `everysec` | Prompt-cache hit rate for one TTL |

The split into two instances rather than two logical databases is deliberate: `maxmemory` and
`maxmemory-policy` are per-instance, so a single instance would let large conversations evict
affinity digests and affinity churn evict paid conversations. The rationale is written into
`deploy/affinity-redis.compose.yaml:3-19`.

Both instances are scraped by dedicated `redis_exporter` containers distinguished by an
`instance_role` label (`observability/compose.yaml:152-195`,
`observability/prometheus/prometheus.yml:30-35`).

## Standing rules for any new Redis consumer

These are not style preferences. A proposal that violates one of them is rejected, not amended.

1. **Redis never authorizes money, quota, or subscription capacity.** PostgreSQL is the Stage 2
   authority (`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`). Balances, reservations, spend, and
   capacity leases are read from PostgreSQL on every decision that moves money. A cache may hold
   the *inputs* to a price calculation; it may never hold the calculated charge or the balance it
   was checked against.
2. **Layer boundaries hold.** `crates/pool` has no network and no HTTP; `crates/registry` has no
   HTTP and no external network. Engine-side Redis therefore lives in `crates/forward` or
   `crates/server` only — exactly where the two existing consumers live.
3. **Commerce reaches the engine only through the Control API.** A commerce-side cache may hold
   commerce's own data. It may not hold, mirror, or shortcut engine state.
4. **Prefer a self-invalidating key over a TTL.** If the cached value already carries a version or
   content digest, put it in the key. A stale read then becomes structurally impossible rather
   than merely unlikely.
5. **Declare fail-open or fail-closed explicitly, and bound every call with a timeout.** Affinity
   is fail-open because losing it costs only a cache hit; Codex history is fail-closed because a
   wrong answer costs the customer a conversation they paid to build.

## Ranked opportunities

Ordered by impact divided by effort. Every claim below is anchored to code.

### 1. Collapse the router's duplicated preflight — no Redis required

One customer request currently executes the `key_account` query **five times**. That query is a
five-table join over `api_keys → accounts → account_policy_bindings → account_policy_versions`
plus two correlated `SUM()` subqueries over `funding_buckets` under strict funding enforcement
(`crates/registry/src/pg.rs:4198-4229`).

The amplification comes from two places. `crates/router/src/auth.rs:59-62` races a preflight
against all three planes concurrently and returns on the first conclusive answer without
cancelling the losers, so three engine processes each run the join. Then
`crates/router/src/routing.rs:461` runs a second, separate policy preflight
(`crates/server/src/router_policy.rs:131-153`), and the proxied request runs the join a fifth time
(`crates/forward/src/proxy.rs:429-430`).

Making the auth preflight sequential with a short deadline, or having the policy preflight return
the auth verdict so the two collapse into one, removes roughly three of the five joins with no
caching and therefore no staleness risk at all.

**This is the first thing to do, and it needs no new infrastructure.**

### 2. Cache the pricing read bundle — self-invalidating, low risk

`crates/registry/src/pricing/postgres.rs:1210-1290` opens an explicit `RepeatableRead read_only`
transaction and runs a sequence of statements: account multiplier, policy binding, policy by
version, catalog by generation, plus the admission catalog and switches. It runs twice per
request — `crates/server/src/router_policy.rs:153` and `crates/forward/src/proxy.rs:1603`.

The bundle is versioned data, not time-varying data. `account_id`, the active effective version,
and the content digest are all already present in the `key_account` result set
(`crates/registry/src/pg.rs:4203-4207`).

- Key: `claude-api:pricebundle:v1:<account_id>:<effective_version>:<content_digest>`
- Value: the serialized `PricingReadBundle`
- TTL: 300s as a janitor only — a policy rebind produces a different key, so the cache is
  self-invalidating

This caches pricing *policy input*. Charge resolution stays in `crates/metering` on integers, and
the reservation path stays in PostgreSQL. Placement: `crates/server`, never `registry`.

### 3. Cache the active engine-account mapping in commerce

`apps/api/src/account.service.ts:43-51` opens a dedicated pool client, begins a transaction, takes
`pg_advisory_xact_lock(hashtextextended($1,0))`, runs a four-table join with `FOR UPDATE OF ea`,
and commits — on **every** read request. It is reached through `withEngineAccountId`
(`apps/api/src/account.service.ts:487`), which backs `GET /v1/account`, `/v1/account/ledger`,
`/v1/account/usage`, `/v1/api-keys`, and `POST /v1/checkouts`.

For an account that is already `active` — the overwhelmingly common case, early-returned at
`apps/api/src/account.service.ts:81-101` — this is a write-lock transaction serving a pure read.
`apps/api` runs blue-green (`systemd/apitoken-api@.service`), so both slots pay it.

- Key: `claude-api:commerce:engineacct:v1:<user_id>`
- Value: `{engineAccountId, status:"active"}` only
- TTL: 60s, plus explicit invalidation on any `engine_accounts` status write

Only the `active` status may be cached positively. Any other status must take the slow path, so a
disabled account can never be served from cache.

### 4. Fix the OpenKeys poll/cache mismatch, then consider caching the usage report

`crates/registry/src/pg.rs:4601-4790` runs five separate `GROUP BY` aggregations over
`usage_events` inside one read-only snapshot transaction, over a default 30-day window
(`apps/api/src/account.controller.ts:52-58`).

`apps/openkeys/src/components/key-profile.tsx:149` polls it every 6s against a dedup cache with a
5s TTL (`apps/openkeys/src/lib/keys.ts:312-324`), so the cache essentially never hits. **Fix the
interval mismatch first — it costs nothing and may remove the problem entirely.** Only if load
remains is a `claude-api:usage:v1:<account_id>:<window>` entry with a 10s TTL worth adding, in the
Control API handler in `crates/server`.

This is spend *reporting*, not billing authority. The same relaxation must never be extended to
the account balance.

### 5. Shared rate limiting — a correctness gap, not a speed win

`apps/openkeys/src/lib/request-guard.ts:6` keeps its buckets in a process-local `Map`. That is
correct today because OpenKeys runs as a single instance, and the file says so — but it becomes
silently wrong the moment the service is scaled or blue-greened.

`apps/api` has no general request rate limiter at all outside the auth-specific database limiter
in `packages/db/src/auth.ts:39-53`, and it *is* blue-green, so any in-memory limiter added there
would be wrong from the first day.

A shared token bucket at `claude-api:rl:v1:<scope>:<subject>` with `INCR`/`EXPIRE` fixes both.
Rank this on abuse control, not on latency.

## Considered and rejected

- **Shared `key_auth` identity cache.** Would remove the last per-request join, but
  `crates/forward/src/billing.rs:5145-5147` records a deliberate decision against caching
  authorization, because policies are mutable and a key can gain a limit or expiry on a different
  engine instance. A *shared, invalidatable* cache is materially different from the per-process
  one that was removed, so the idea is not dead — but it makes revocation latency bounded by the
  TTL instead of immediate. **Requires an explicit product decision, not a quiet patch.**
- **Sharing the router catalog cache across blue-green slots.** `crates/router/src/catalog.rs:206`
  already has a 30s TTL with per-plane single-flight and last-good degradation. Two slots × three
  planes is twelve loopback fetches per minute in total. **Do not do it** — it would add a failure
  mode to a path whose entire design point is graceful degradation.
- **Pool cooldown and rotation state in Redis.** Templated engine units already share rotation
  state through CAS-versioned `pool_state` and `capacity_leases` in PostgreSQL. `crates/pool`
  cannot reach the network by contract, and the shared-hint layer that *is* permissible already
  exists as the cooling hints in `crates/forward/src/affinity.rs:19`. **Nothing to do.**
- **A Redis job queue for `apps/worker`.** The polling loops in `apps/worker/src/config.ts:11-35`
  cost a few trivial indexed queries per second at idle. Moving job claiming out of PostgreSQL
  would split the transaction boundary on paid-credit writes. If event-driven delivery is ever
  wanted, PostgreSQL `LISTEN/NOTIFY` is the correct tool because it keeps the claim in the same
  transaction as the money write. **Do not do it.**
- **Redis idempotency or distributed locks for payment webhooks.** The webhook path is already
  transactional and money-critical: `packages/db/src/payments.ts:138`, the unique
  `idempotency_ref` insert at `:205-215`, and the credit lease at
  `apps/worker/src/credit-worker.service.ts:64-69`. A Redis key here would be either redundant
  with the database constraint or, worse, trusted in place of it. **Do not do it** — it violates
  rule 1 above.

## Related documents

- `docs/ops/MONITORING.md` — Redis alerts and their runbook anchors.
- `docs/ops/INFRASTRUCTURE.md` — hosts, ports, and secret locations.
- `docs/engine/STAGE2_POSTGRES_AUTHORITY.md` — why money is never served from a cache.
- `crates/forward/CLAUDE.md` — boundaries of the crate that owns both current consumers.
