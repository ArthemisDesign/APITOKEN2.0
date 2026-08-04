# Backend PostgreSQL communications and performance audit

> **Provenance.** Audited 2026-08-04 against `origin/master` at
> `9ba644b87120e4338a52c48ee03fd3c091af2191`. Line numbers are accurate for that tree and will
> drift; every finding also names its function or query. This is a static source, schema, deployment,
> and observability audit. It did not access production, inspect secrets, run mutating SQL, or measure
> production cardinality or latency.
>
> **Method.** Four parallel domain reviews covered the Rust engine, commerce API/worker, Sales and
> OpenKeys, and cross-cutting database operations. Four independent follow-up reviews challenged the
> findings, checked migration/index coverage, inventoried every PostgreSQL owner, and designed a safe
> measurement runbook. Claims below distinguish source-confirmed query amplification from hypotheses
> that require `pg_stat_statements`, table cardinalities, or `EXPLAIN` on a representative database.
>
> **Scope.** Direct PostgreSQL clients, SQL repositories, worker loops, readiness/monitoring queries,
> connection budgets, transactions, locks, indexes, retention, and HTTP paths that indirectly cause
> PostgreSQL work. SQLite import/audit paths and the external CRM repository are outside scope.

## 1. Executive conclusion

The backend's main PostgreSQL risk is not one globally slow query. It is repeated client/server
round trips inside logically single operations:

1. Funding-v2 reserve and settlement scale with the number of historical funding lots and combine
   row-by-row writes with deferred aggregate triggers.
2. Commerce can turn one 1,000-entry engine ledger page into thousands of sequential SQL statements
   while holding a customer row lock.
3. Sales can turn one 1,000-event feed page into roughly 24,000 serial database commands when a full
   ten-level partner chain is present.
4. Pricing-release admission resolves the same immutable graph before reserve and again under the
   authoritative reserve transaction.
5. Node pools have a fixed size but no explicit acquisition/query/idle-transaction timeout policy,
   no pool-error handler in the shared constructors, and no pool-wait metrics.

The money, idempotency, fencing, and crash-recovery design is generally strong. Optimizations must
preserve transaction boundaries, immutable evidence, bonus-first funding, exact replay, owner epochs,
and outbox durability. The correct approach is set-based SQL and measured caching of immutable data,
not removing safety checks or making PostgreSQL eventually consistent.

No production outage or latency number is proven by this audit. The first operational task is to
measure query fingerprints, pool waits, lock waits, lot-count distribution, and table size. The first
code tasks can proceed in parallel where amplification is source-confirmed and query-count tests can
prove the reduction.

## 2. PostgreSQL ownership map

| Context | Direct client | Database | Runtime shape | Important traffic |
|---|---|---|---|---|
| Rust engine | synchronous `postgres::Client` in `crates/registry` | `claude_engine` | one writer plus 1-64 readers per provider process; blue-green overlap | auth, release resolution, reserve, capacity, settlement, ledger, usage, calibration |
| Auth Bot | `registry::PgStore` | `claude_engine` | singleton | Claude subscription publication and registry reads |
| Commerce API | `pg.Pool`, max 10 | `commerce` | blue-green API slots | sessions, account mapping, payments, pricing, admin reports, Content Studio |
| Commerce worker | `pg.Pool`, max 10 | `commerce` | singleton with several polling loops | credits, ledger ingest, pricing jobs, email, reconciliation |
| Sales API | `pg.Pool`, max 10 | `sales` | singleton API plus sync/email/payout loops | referrals, commissions, analytics, payouts, sessions |
| OpenKeys | `pg.Pool`, max 10 | `openkeys` | singleton Next.js process | issuance journal, key warehouse, profile/admin reads, pricing inventory |
| Monitoring | `psql`, postgres exporter | all applicable databases | timer plus exporter | queue and money aggregates, server statistics |

`apps/admin`, `apps/content-studio`, `apps/sales-web`, `apps/web`, and `crates/router` do not open
PostgreSQL directly. They cause database work through HTTP. `apps/devbot` has no PostgreSQL path.
Commerce and OpenKeys access engine authority only through the Control API; Sales accesses commerce
only through the internal HTTP feed.

Redis does not remove engine auth, reserve, capacity, or settlement SQL. It stores rebuildable
affinity/history only. Sales' 30-second session cache is the clearest existing database-request
reduction. OpenKeys coalesces identical profile loads for five seconds.

## 3. Ranked confirmed findings

Severity here represents potential production impact plus certainty of the amplification mechanism.
Actual resource share must be measured before choosing between findings with the same severity.

### P0 candidate: funding-v2 work and deferred checks grow with funding-lot count

**Evidence**

- `crates/registry/src/funding_v2.rs:493-688`, `reserve_funding_v2`
- `crates/registry/src/funding_v2.rs:695-858`, `reserve_pricing_release_funding_v2`
- `crates/registry/src/funding_v2.rs:861-1219`, settlement paths
- `crates/registry/migrations_pg/0023_pricing_release_funding_v2.sql:292-468,836-983`

Reserve fetches and locks every lot whose status is not `retired`, including exhausted historical
lots, in one query. Let `N` be all fetched lots and `L` the lots selected for this reservation. The
query transfers and locks `O(N)` rows; updates and allocation inserts then add `O(L)` sequential
protocol round trips. Settlement repeats per-allocation lot and evidence writes. Deferred row-level
triggers re-aggregate all generation lots or all request allocations for each affected row, making
commit-time work approximately `O(N * L + L^2)`. Many exhausted lots with one selected anchor remain
`O(N)`, not quadratic; high selected-lot counts create the stronger amplification.

No runtime compaction or retirement path was found that bounds paid-lot growth. The issue becomes P0
only if production accounts have accumulated many total and/or simultaneously selected lots; those
cardinalities are not known.

**Remediation**

- Select positive-balance active lots plus the one deterministic paid overdraft anchor, rather than
  every non-retired lot.
- Define and test a safe exhausted-lot retirement/compaction policy.
- Replace per-lot updates and allocation inserts with set-based `UPDATE ... FROM` and bulk insert.
- Replace repeated row-trigger aggregation with one authoritative deferred validation per affected
  account/generation/request while preserving database-enforced parity.

**Acceptance proof:** reserve/settlement results and exact allocation evidence remain identical;
query count is bounded or materially reduced as lot count rises; real-PostgreSQL money/race matrices
remain green; no lock-order change or new deadlock appears.

### High: commerce ledger ingestion is row-by-row under one customer lock

**Evidence**

- `apps/worker/src/pricing-worker.service.ts:161-179`, pages of up to 1,000
- `packages/db/src/pricing.ts:2280-2461`, `applyPricingLedgerPage`
- `packages/db/src/pricing.ts:1881-2001`, attribution and allocation insertion
- `packages/db/src/pricing.ts:2243-2269`, top-up backfill

One page locks `customer_profiles`, then loops over entries. Each charge can insert the usage event,
validate an attribution binding, insert attribution, insert each funding allocation separately, and
upsert a pricing month. Top-ups are inserted separately. Backfill adds a redundant existence query
before an idempotent `ON CONFLICT DO NOTHING` insert. The transaction and customer lock last through
all those round trips.

**Remediation:** calculate ordered free-balance effects in memory, prevalidate immutable attribution
in batches, use arrays/recordsets or staging CTEs for event, attribution, allocation and top-up
inserts, aggregate month increments by month, and use `RETURNING` instead of a pre-read. Preserve
ledger-ID ordering, all-or-nothing cursor advancement, and replay conflict detection.

### High: Sales commission ingestion multiplies every feed event

**Evidence**

- `apps/sales-api/src/sync.service.ts:274-292`, usage pages
- `packages/sales-db/src/commissions.ts:215-238`, one query per parent
- `packages/sales-db/src/commissions.ts:303-410`, v1 writer
- `packages/sales-db/src/commissions-v2.ts:86-160`, v2 writer
- `packages/sales-db/src/commissions.ts:426-491` and
  `commissions-v2.ts:176-240`, pending replay

A ten-level event can execute `BEGIN`, referral lookup, event insert, ten partner reads, ten
commission inserts, and `COMMIT`: about 24 serial commands. A full 1,000-event feed page can
therefore approach 24,000 commands. Each pending queue replays up to 200 events through the same
writer and deletes them individually, adding thousands more commands per sync tick.

**Remediation:** fetch the bounded parent chain with one recursive CTE, compute exact integer amounts,
bulk-insert all commission rows, and claim/process/delete pending pages in bounded transactions with
`FOR UPDATE SKIP LOCKED`. Preserve chain stop conditions, v1/v2 separation, source-validation
triggers, and event idempotency.

### High: pricing-release admission resolves the same graph twice

**Evidence**

- `crates/forward/src/proxy.rs:745-823` and provider billing equivalents
- `crates/registry/src/pricing/postgres.rs:4324-4581`, resolver
- `crates/registry/src/pg.rs:2551-2565`, reserve-time re-resolution

The resolver performs several sequential reads for head/assignment, policy/rules, provider master
switch, catalog model, and scoped switch. The request needs a resolution to build its quote, then the
reserve transaction intentionally resolves again to close the time-of-check/time-of-use race. A
normal request therefore reads the same mostly immutable graph twice; retry and alias paths can add
more resolutions.

The reserve-time authoritative check must remain. Consolidate the immutable graph into fewer SQL
statements or cache immutable release/policy/catalog objects by generation/digest. Under reserve,
revalidate the current head, assignment identity, and funding authority and compare the immutable
identity. Do not replace this with a TTL-only cache.

### High availability: Node pools can wait without an application bound

**Evidence**

- `packages/db/src/client.ts:10-12`
- `packages/sales-db/src/client.ts:10-12`
- `packages/openkeys-db/src/client.ts:10-12`

All shared constructors set only `connectionString`, `max: 10`, and `application_name`. They do not
set an acquisition timeout, query timeout, statement timeout, lock timeout, idle-transaction timeout,
or connection lifetime. No shared pool-error listener was found. A saturated pool or lock convoy can
therefore make a request wait without an application-level bound; an idle-client `error` event also
has no common handler.

Add role-specific, tested pool policy rather than one arbitrary global timeout. Include bounded
connection acquisition, server-side session timeouts, a non-secret error handler, and metrics for
total/idle/used/waiting clients and acquisition duration. Commerce API, worker, and Sales already
close their pools through shutdown hooks; add or verify equivalent graceful draining for OpenKeys.
Long migrations and explicitly reviewed administrative operations need separate policies.

### High operational: the minute collector re-aggregates lifetime engine funding

**Evidence**

- `deploy/collect-monitoring-metrics.sh:88-114`
- `systemd/apitoken-monitoring-collector.service:6-25`
- `systemd/apitoken-monitoring-collector.timer:5-7`

Every collector run groups all `topup` and `adjust` ledger rows by account to recompute balance
divergence. The cost grows monotonically with retained ledger history. The script does not set a
statement/lock timeout and the oneshot unit has no `TimeoutStartSec`.

Immediately bound collector SQL and unit runtime. Then source this invariant from an incrementally
maintained aggregate or an engine-owned bounded metric. The invariant is valuable and must not be
silently removed to improve speed.

### Medium-high: Sales partner analytics repeats correlated aggregates

**Evidence:** `packages/sales-db/src/analytics.ts:104-203`.

The admin query computes approximately 18 correlated aggregates per partner across deposits,
referrals, v1/v2 usage, v1/v2 commissions, payouts, team, links, promos, and sessions. Sorting occurs
before pagination can fully limit aggregate work. The route permits up to 500 rows.

Pre-aggregate each fact table once in CTEs and join one row per partner, or maintain reviewed summary
tables if measured volume justifies them. Benchmark sort modes such as unpaid and total earned.

### Medium-high: paying-user administration repeats large aggregates three times

**Evidence:** `packages/db/src/admin-finance.ts:382-548`.

One request runs page, count, and summary queries concurrently. Page and count repeat paid, usage,
session, and key CTEs; summary independently repeats payment and usage aggregation. It can occupy
three of ten pool connections and scan the selected usage window several times.

Derive the filtered cohort, page, count, and summary in one statement/snapshot, for example as one
JSON result. Summary must continue to describe the full filtered cohort rather than the page.

### Medium: payout confirmation re-finalizes the same batch per row

**Evidence**

- `apps/sales-api/src/payout/payout.service.ts:395-440`
- `packages/sales-db/src/payout-batch.ts:278-292`

The poller reads up to 100 broadcast rows. After every row it reloads the batch and all batch payouts.
One 100-row batch can therefore be reread 100 times. The poller also holds a dedicated advisory-lock
pool connection while waiting on network receipt calls.

Process receipts first, collect distinct batch IDs, and finalize each batch once. Consider narrowing
the lock scope only after proving nonce/send correctness. A partial broadcast polling index is a
candidate after `EXPLAIN`.

### Medium: API-key listing writes once per key

**Evidence**

- `apps/api/src/account.service.ts:276-295`, `listApiKeys`
- `packages/db/src/engine.ts:96-123`, `syncEngineApiKey`

The read endpoint gets live engine keys and then performs one sequential commerce upsert per key.
Unchanged keys still create update/WAL churn.

Bulk synchronize with `unnest` or a recordset and update only changed label/mask/status fields. Raw
keys must never enter commerce PostgreSQL.

### Medium: active account reads take provisioning locks

**Evidence:** `apps/api/src/account.service.ts:44-103`, `ensureEngineAccount`.

Account, ledger, usage, and key paths call `ensureEngineAccount`. Even an already active mapping does
`BEGIN`, per-user advisory lock, joined `FOR UPDATE`, and `COMMIT`. This does not serialize different
users and commits before live engine reads, so it is not a global lock. It does add round trips and
serializes concurrent requests for the same user. Pending/error provisioning correctly holds the
lock across remote work and is a separate, correctness-sensitive path.

Add an unlocked active-mapping fast path. Re-enter the advisory-lock transaction and re-read when the
mapping is missing, pending, error, or changed. Preserve the single-winner and administrative-disable
fences.

### Medium: OpenKeys issuance serializes a large durable saga in HTTP

**Evidence:** `apps/openkeys/src/lib/keys.ts:137-276`, `issueBatch`.

A request may issue 100 keys. Each item serially creates a journal row, performs several engine calls,
writes multiple status transitions, inserts the key, and marks completion. A normal maximum batch is
roughly 601 local writes plus serialized engine work. The journal and compensation are valuable, but
the request stays open for the complete saga.

Create the batch and jobs quickly, return a job ID, and process a small bounded number concurrently
in a background worker. Preserve per-item durable transitions, secret-last issuance, and account
disable compensation.

### Medium: OpenKeys admin pagination occurs after bounded full materialization

**Evidence:** `apps/openkeys/src/lib/keys.ts:902-1033`, `loadAdminKeyDirectory`.

The endpoint caps its scan at 10,000 keys, so it is bounded rather than unbounded. It still reads and
materializes up to 10,000 rows, makes up to 20 engine batch calls, calculates usage summaries, then
filters and paginates in memory even when the caller asks for 50 rows.

Push database-local filters and pagination into SQL. Live usage filters need either a bounded cached
account snapshot or an engine-side filtered/pageable contract; do not cache live money as authority.

### Medium: job recovery work repeats during queue drains

**Evidence**

- `apps/worker/src/pricing-worker.service.ts:100-157`
- claim/recovery implementations in `packages/db/src/pricing-control-jobs.ts`, `pricing.ts`,
  `funding-normalization-jobs.ts`, and Stage 8/rollout job repositories

Several polling lanes run global stale-lease recovery before looking for work, and some claim
functions repeat recovery. During a backlog, maintenance updates can be repeated once per claimed
job; dormant lanes still create idle query traffic every five seconds.

Use a timed recovery cadence, as other workers already do, while retaining ordinary claims between
recovery passes. A failed retry-state write must still become recoverable without process restart.

### Medium-low: service-policy listing is an administrative N+1

**Evidence:** `packages/db/src/pricing-policy-write.ts:1008-1135`.

The list first reads policy IDs, then calls `managedPolicyView` sequentially for each policy. Each
view performs policy/rule reads, metadata, service audit, and binding/job reads. The behavior is a
real `O(N)` query count but the expected service-policy inventory is small, so it ranks below hot
customer and worker paths.

Batch each relation with `ANY($1)` while preserving the single repeatable-read snapshot.

### Low: Content Studio profile GET performs writes

**Evidence:** `packages/db/src/content-studio.ts:132-156`.

Every profile listing performs a transaction with one upsert per built-in profile, rewriting rows
through `DO UPDATE`, then reads the list. Seed built-ins through an additive migration or a bounded
startup reconciliation. If runtime repair is required, use one set-based statement and update only
changed rows.

### Low: OAuth start performs global cleanup

**Evidence:** `packages/db/src/oauth.ts:27-45`.

Every OAuth start deletes all expired/old-consumed transactions before inserting one row. Move
cleanup to bounded periodic maintenance or run it probabilistically. Validate the `consumed_at`
branch and table size before adding an index.

### Low: OpenKeys readiness uses two database connections

**Evidence:** `apps/openkeys/src/app/api/ready/route.ts:11-27`.

Every probe concurrently runs `SELECT 1` and a schema-shape query, consuming two of ten possible
connections. Combine them or cache successful schema validation after startup. Retain the engine
readiness dependency and fail-closed result.

## 4. Index and maintenance candidates requiring plans

These are source-confirmed predicate/index mismatches, not authorization to add every index. Test
each on representative cardinality and account for write amplification. Add accepted indexes only in
new expand-only migrations.

| Query | Existing mismatch | Candidate to evaluate |
|---|---|---|
| engine `reconcile_expired` (`pg.rs:3399-3406`) | indexes start with owner/account, not active state plus lease | partial active-state index led by `lease_until` or `created_ts` |
| engine `usage_prune` (`pg.rs:4893-4899`) | indexes start with account/provider | `(ts, id)` |
| execution-winner orphan cleanup (`pg.rs:4943-4949`) | no reservation index on effective group | expression index on `COALESCE(group_id, request_id)` and bounded cleanup |
| outbox drain (`pg.rs:3365-3370`) | current due-time index does not fully match `ORDER BY created_ts` | partial pending `(created_ts, request_id)` including due time, only if plan wins |
| commerce global recent usage aggregates | index starts with user | `(occurred_at, user_id)` with carefully selected includes |
| commerce provider recovery | identity/time/recovery predicates split across indexes | partial sentinel/recovery index after cardinality check |
| Sales broadcast payout poll | status/partner/nonce indexes do not provide requested-time order | partial `(requested_at, id)` for broadcast rows with hash |
| OpenKeys admin directory | identity and batch indexes do not support active created-time scan | partial non-removed created-time index; optional status/batch variants based on workload |

The following are already materially covered and should not receive duplicate indexes without a
plan: engine terminal reservation/outbox/capacity pruning, commerce ledger cursor identity and event
deduplication, Sales pending-event joins and commission partner/time aggregates, and OpenKeys stale
issuance reconciliation.

Engine retention has two additional operational issues:

- `ledger_prune_loop` in `crates/server/src/poller.rs:91-147` has no leader lease, unlike billing
  recovery. Blue-green overlap can duplicate scans/deletes. Give retention its own lease.
- `execution_group_winner` cleanup is unbounded inside otherwise bounded maintenance. Bound it to the
  same batch model.

## 5. Connection budget

The host configures PostgreSQL `max_connections=200` in
`deploy/commerce-postgres.compose.yaml:6-22`. Engine readers default to host parallelism clamped to
4-16 and may be configured to 64 (`crates/server/src/config.rs:1174-1185`). With the documented
16-reader host default:

- four active provider processes use about `4 * (1 writer + 16 readers) = 68` sessions;
- three pricing-shadow reader pairs add about 6, for roughly 74 engine sessions steady-state;
- one provider blue-green cutover raises the engine envelope to roughly 91-93;
- commerce API, worker, Sales, and OpenKeys pools add up to 40 configured sessions;
- combined cutover demand is therefore roughly 131-133 before Auth Bot, monitoring, backups,
  migrations, operators, CRM, or transient maintenance connections.

This fits the configured 200 today, but the budget is not enforced by database role limits or engine
startup validation. An environment override toward 64 readers can exhaust the host.

Define per-role/session budgets, expose actual versus waiting pool state, distinguish engine
writer/reader/shadow/provider/slot in `application_name`, and fail startup when configured aggregate
demand cannot fit a reserved operational margin. Do not add PgBouncer until telemetry shows that
connection churn or session count, rather than query work, is the bottleneck.

## 6. Observability gaps

Repository configuration does not establish the following:

- `pg_stat_statements` and query-fingerprint dashboards;
- query time/calls/I/O/temp/WAL ranking;
- lock waiter/blocker and long/idle transaction alerts;
- table dead-tuple, autovacuum freshness, freeze-age, and sequential-scan views;
- TypeScript pool used/idle/max/waiting and acquisition latency;
- per-route or per-job query counts;
- engine reader operation latency and queue depth;
- application attribution within engine sessions, which all use `claude-api-engine`;
- OpenKeys backup age metrics in `collect-monitoring-metrics.sh`.

Current PostgreSQL alerts cover exporter availability, aggregate connection utilization, and
deadlocks (`observability/prometheus/rules/operations.yml:194-226`). That is not enough to rank the
query changes above safely.

## 7. Safe measurement runbook

Run this first on a sanitized, production-shaped PostgreSQL 18 restore. Do not put DSNs, passwords,
raw key values, or raw query text into audit artifacts. Use a protected service definition or
`.pgpass`. Start read sessions with bounded waits:

```sql
BEGIN READ ONLY;
SET LOCAL statement_timeout = '10s';
SET LOCAL lock_timeout = '250ms';
SET LOCAL idle_in_transaction_session_timeout = '30s';
```

On the disposable clone, preserve existing preload libraries and enable `pg_stat_statements` with
`track = all`; initially leave planning tracking and broad `auto_explain` off. Enable I/O timing only
after measuring its host overhead. Create the extension separately in `claude_engine`, `commerce`,
`sales`, and `openkeys`.

Capture these groups for a fixed representative workload interval:

```sql
SELECT queryid, calls,
       round(total_exec_time::numeric, 1) AS total_exec_ms,
       round(mean_exec_time::numeric, 3) AS mean_exec_ms,
       rows, shared_blks_hit, shared_blks_read,
       temp_blks_read, temp_blks_written, wal_bytes
FROM pg_stat_statements
WHERE dbid = (SELECT oid FROM pg_database WHERE datname = current_database())
ORDER BY total_exec_time DESC
LIMIT 25;
```

Repeat ranking by `calls`, `mean_exec_time` with a minimum-call threshold, physical reads, temp
writes, and WAL. Do not reset shared production statistics; use before/after deltas by query ID.

Capture connection and wait attribution without raw query text:

```sql
SELECT datname, usename, COALESCE(NULLIF(application_name, ''), '<unset>') AS application,
       state, wait_event_type, wait_event, count(*) AS sessions,
       max(clock_timestamp() - COALESCE(xact_start, query_start, backend_start)) AS oldest_age
FROM pg_stat_activity
WHERE backend_type = 'client backend'
  AND datname IN ('claude_engine', 'commerce', 'sales', 'openkeys')
GROUP BY datname, usename, application_name, state, wait_event_type, wait_event
ORDER BY datname, sessions DESC;
```

Also collect `pg_blocking_pids`, long transactions, table live/dead tuples, autovacuum/analyze times,
relation sizes, sequential rows read, and index usage. A sequential scan on a small table is not a
problem by itself; an unused index may enforce uniqueness and must not be removed from a short
observation.

Use this plan ladder:

1. `EXPLAIN (COSTS, SETTINGS, FORMAT JSON)` on the restored clone.
2. Shortlist queries based on frequency/resource share.
3. `EXPLAIN (ANALYZE, BUFFERS, WAL, TIMING FALSE, FORMAT JSON)` only for bounded `SELECT` statements
   on the clone, with fixed parameters and a timeout.
4. Do not `EXPLAIN ANALYZE` mutating, DDL, maintenance, or externally side-effecting statements.
5. Compare selective and non-selective values and cold/warm runs.

For query amplification, run exactly `N` requests/jobs in a quiet clone and calculate each query
fingerprint's call delta divided by `N`. Add permanent low-cardinality metrics at the database
wrapper for query count/duration, transaction duration, pool acquisition, pool waiting, and
queries-per-route/job. Never label metrics by SQL, query ID, user, account, key, request, or error
text.

## 8. Remediation sequence

### Phase 0: measurement and fail-fast behavior

1. Add/verify `pg_stat_statements`, connection/wait/vacuum dashboards, and protected query-ID
   capture.
2. Add TypeScript pool error handling, bounded acquisition/session policy, and pool metrics.
3. Split engine `application_name` by provider, slot, and writer/reader/shadow role.
4. Put explicit timeouts around the monitoring collector and replace its lifetime scan.

### Phase 1: highest query-count reductions

1. Redesign funding-v2 lot selection/writes/triggers, gated by observed lot distribution.
2. Bulk commerce ledger-page ingestion and remove top-up pre-reads.
3. Convert Sales chain traversal, commission inserts, and pending replay to set-based pages.
4. Consolidate/cached immutable release resolution while keeping reserve-time authority checks.

### Phase 2: endpoint and worker fan-out

1. Rewrite Sales analytics and commerce paying-user reports around one aggregated cohort.
2. Bulk API-key synchronization and add the active-mapping fast path.
3. Finalize each payout batch once per poll.
4. Move OpenKeys issuance to a durable bounded worker and reduce admin materialization.
5. Batch service-policy views and reduce idle/repeated job recovery.

### Phase 3: measured indexes and maintenance

1. Add only plan-proven indexes in new migration-only commits.
2. Leader-elect engine retention and bound execution-winner cleanup.
3. Add production-shaped plan/query-count regression tests for recovery, pruning, ledger ingest,
   commissions, analytics, and queue claims.

## 9. Acceptance criteria

A performance change is accepted only when the same representative data, parameters, and
concurrency show:

- identical business results and all owning-context tests pass;
- at least 20% lower target app p95, or at least 30% lower target query calls, total execution time,
  physical reads, temp writes, or WAL;
- no more than 10% regression in unrelated throughput, p95, database CPU/I/O, or write volume;
- no new deadlocks, lock convoy, idle transaction, pool waiter, or connection-budget regression;
- preserved money, replay, fencing, and outbox invariants;
- acceptable plans for both selective and non-selective inputs;
- index size and write cost included in the decision;
- migration-first, expand-only delivery for every schema/index change.

## 10. Existing strengths

- Engine request and funding-account lock order is explicit and owner epochs are rechecked.
- Request IDs, unique constraints, and durable outboxes provide strong exactly-once behavior.
- Queue claims generally use `FOR UPDATE SKIP LOCKED` and bounded limits.
- Most retention deletes are bounded and their terminal predicates have supporting indexes.
- Commerce provider recovery already demonstrates set-based `unnest` updates that other paths can
  copy.
- Sales sessions cache successful authentication and throttle last-seen writes.
- OpenKeys issuance has durable per-item states and compensation rather than hiding partial failure.
- Money remains integer nanoUSD across the audited PostgreSQL paths.
- Migrations are advisory-locked and follow an expand-first delivery contract.

The project should optimize around these safeguards, not trade them for fewer database calls.
