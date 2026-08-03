# Database migrations

`pnpm --filter @claude-api/db db:migrate` builds the package, opens a dedicated PostgreSQL connection, and holds a session-level advisory lock while Drizzle applies migrations. Concurrent deploys therefore serialize instead of racing. The lock wait defaults to 30 seconds and each SQL statement to 15 minutes; override them with `DB_MIGRATION_LOCK_TIMEOUT_MS` and `DB_MIGRATION_STATEMENT_TIMEOUT_MS`.

## Expand/contract policy

1. **Expand:** ship additive, backward-compatible schema changes first (new nullable columns, tables, indexes, or compatible defaults).
2. **Backfill:** populate and verify existing rows while both old and new application versions remain valid.
3. **Contract later:** only drop old columns/tables or tighten `NOT NULL` in a later release, after the old code is fully removed and no deployed process depends on the old shape.

Roll application code back by deploying the previous release; never down-migrate production schema during rollback.

## Automatic production gate

Do not run production migrations manually during normal delivery. A commit merged to `master` is
tested against disposable PostgreSQL first. Only after the complete TypeScript and Rust suite passes,
the production watchdog:

1. verifies that every already-applied migration still exists byte-for-byte;
2. creates and validates a fresh production PostgreSQL backup;
3. runs the exact tested `packages/db/dist/migrate.js` under the file and PostgreSQL advisory locks;
4. atomically commits the new migration manifest;
5. permits the backend blue-green deployment to start.

Any backup or migration failure quarantines the SHA and blocks application deployment. Production
does not run the package script because it rebuilds. The watchdog consumes the prebuilt immutable
candidate directly. The manual `deploy/deploy.sh --api-only <sha>` path remains a recovery tool and
uses the same locked prebuilt migrator.

For a schema-dependent change, merge a migration-only expand commit first and wait for its
`deploy/migration` and `deploy/watchdog` GitHub statuses to pass. Merge the dependent code only
after production has the expanded schema, and keep the old release compatible throughout. Never
edit, rename, reorder, or delete a committed migration. Destructive contract changes require a
later release after backfill and after all old processes no longer depend on the old shape. See the top-level [`CONTRIBUTING.md`](../../CONTRIBUTING.md) and
[`docs/ops/DEPLOYMENT.md`](../../docs/ops/DEPLOYMENT.md).

## Pricing release v2 checkpoint

Migration `0026_pricing_release_expand.sql` is the commerce half of the Stage 5/6/8/9 expansion.
It creates empty policy documents/rules, B2B invitation snapshots, service inventory,
target/recovery plans, full-inventory assignments, resumable funding-normalization/control jobs,
Stage 8 evidence and activation receipts. Service policies deliberately have no product catalog or
product switch pins: they are `meter_only` and capability-gated. The declarations in
`packages/db/src/schema.ts` exist to keep Drizzle metadata exact; no deployed consumer may read or
write these tables until this migration SHA has green `deploy/migration` and `deploy/watchdog`.

Migration `0027_funding_normalization_blockers.sql` expands the Stage 6 account queue so an exact
engine plan that is not ready can be persisted without inventing a target funding digest or
generation. Target identity is nullable only while the row is unfinished; a `ready` row still
requires an applied digest equal to its target. The additive `normalization_source` and `blockers`
fields preserve the producer's typed technical evidence for retry or fail-closed handling. The
orchestration consumer is delivered only after this migration SHA is green in production.

Migration `0028_pricing_stage5_evidence.sql` adds empty operational evidence tables for the new
Stage 5 consumer. One immutable run stores the exact validated inventory and plan artifacts plus
both engine/OpenKeys scan digests; child rows retain typed blockers and successful prepare/readback
ACKs. Database checks require the two scans of each external inventory to match and require every
durable ACK readback digest to equal its expected digest. The migration cannot create a pricing
release, control job or active head, and the Stage 5 consumer is delivered only after this schema
SHA has green `deploy/migration` and `deploy/watchdog`.

Migration `0029_pricing_release_two_phase_finalize.sql` removes an impossible ordering dependency
without weakening activation safety. Stage 5 may now reserve target/recovery generations and store
immutable ownership/policy assignments while balance `funding_generation`, the final funding
manifest and the engine release digest are still unknown. Stage 6 may only finalize an assignment
from `NULL` to one positive funding generation; replacement and identity mutation are rejected.
Before a plan can become `prepared`, database guards require nonempty assignments, exact one-to-one
coverage by ready funding-normalization rows for every balance assignment, no extra funding rows,
and both final funding/engine identities. Prepared assignments and finalized Stage 5 release
digests are frozen. The migration performs no backfill, creates no job or release head and does not
stop money writers; the two-phase consumer follows only after this schema SHA is green.

After the engine schema-v2 Stage 8 producer checkpoint is green, the deployed commerce consumer
uses the existing `pricing_stage8_evidence_v2` table from migration 0026 without another schema
change. It validates the canonical integer-preserving engine JSON, double-scans OpenKeys and binds
current commerce/service authority to exact prepared target/recovery releases in one
`SERIALIZABLE` transaction. Existing releases permit immutable `passed=true` or `passed=false`
rows; a missing release pair is reported as `not_persisted`. The row has a five-minute TTL and does
not mutate a release head, balance, policy or account. Its sales-contract digest is an identity,
not proof of a deployed sales consumer.

Migration `0030_pricing_stage8_zero_drain.sql` expands the combined-evidence contract for the
traffic-preserving Stage 9 cutover. `legacy_inflight_count` remains mandatory and non-negative audit
evidence, but a passed row now depends on zero blockers rather than an impossible moment with no
pre-head traffic. The deployed Stage 8 consumer preserves both format-specific counts in the
canonical engine and combined digests while allowing `passed=true` when they are nonzero; it never
waits for, drains or stops traffic. Older consumers writing the stricter subset remain valid.

Migration `0031_pricing_activation_evidence_capture.sql` adds four nullable, dormant capture fields
needed for safe activation recovery: the source engine evidence digest and exact capture timestamp,
the immutable activation request, and the complete validated engine receipt. It performs no
backfill, creates no control job and cannot move the release head. Existing API/worker versions
ignore the columns. The dependent durable activation consumer may be delivered only after this
migration SHA has green `deploy/migration` and `deploy/watchdog` in production. That consumer now
stores exact requests and complete receipts but remains dormant without an explicitly staged job;
it also rejects evidence whose nullable source fields have not yet been filled by the follow-up
Stage 8 collector checkpoint.

Migration `0032_pricing_activation_service_evidence.sql` adds one nullable, dormant service
inventory digest to Stage 8 evidence. Existing writers remain compatible and no row is backfilled,
job is created, or release head is moved. The dependent collector fills the field for every new
evidence row; activation staging rejects historical rows where it remains `NULL`, and first
delivery requires a fresh service-authority digest match. This proves that post-cutover
service-account authority did not change between fresh recovery evidence and the single global
CAS.

Migration `0033_pricing_stage8_managed_capture.sql` adds empty durable job and append-only artifact
tables for a protected managed Stage 8 capture lane. One job freezes exact target/recovery,
observation-window and sampling inputs; every attempt can retain the original integer-preserving
engine JSON before attaching the combined commerce result. The schema is dormant: it stages no
job, performs no engine request, changes no evidence/head/account/money row and does not replace the
manual collector until the protected engine producer and commerce worker consumer have each passed
their producer-first delivery checkpoints.

Migration `0034_pricing_welcome_bonus_amount.sql` adds nullable exact nanoUSD storage to the
existing signup anti-fraud profile and backfills every already-granted bonus to its historical
`$4.000000000` nominal. Nullable is intentional during expand: the previously deployed writer can
continue claiming the boolean without knowing the new column. The dependent consumer treats a
granted `NULL` row as the same historical `$4`, records `$5.000000000` atomically for every new
claim, and uses the persisted amount for recovery and administrative revocation. The migration
itself changes no grant, engine balance or eligibility decision.
