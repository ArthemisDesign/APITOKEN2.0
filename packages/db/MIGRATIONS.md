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

Migration `0035_pricing_shadow_rollout_jobs.sql` creates empty parent/child storage for one
full-inventory pre-cutover shadow-policy alignment. A parent pins one prepared Stage 5
target/recovery pair, exact generation-3 catalog/switch identities, inventory and policy manifests,
operator and reason. Child rows retain the immutable engine policy/binding/CAS request and exact
terminal ACK for each account across commerce, OpenKeys and service ownership. The tables have no
startup producer, trigger or engine transport: applying the migration cannot advance commerce or
engine heads, activate a policy, change traffic, touch money rows or stage Stage 8/9. The protected
producer and durable worker consumer may be delivered only after this migration SHA has green
`deploy/migration` and `deploy/watchdog` in production.

Migration `0036_pricing_usage_provider_attribution.sql` adds a nullable `provider_id` to immutable
commerce `pricing_usage_events`. It performs no backfill and keeps the deployed usage consumer
compatible. The separately delivered consumer copies authoritative top-level provider evidence for
new charge rows and performs bounded resumable recovery pages over the same retained 30-day ledger
horizon for old rows; exact pricing attribution remains a query fallback. `unattributed` is a
provisional state selected by that recovery, including rows written before the engine producer could
restore legacy provider evidence. After one completed attempt without exact evidence the consumer
stores terminal `unavailable`, so idle polls do not rescan it. Neither state is inferred from a
model name.

Migration `0037_pricing_attribution_release_v2.sql` prepares immutable `pricing_usage_attributions`
for Stage 9 release-v2 charge rows. It adds five nullable release lineage columns
(`release_schema_version`, `release_generation`, `release_digest`, `release_billing_mode`,
`release_funding_generation`) and swaps the existing shape CHECKs for strict supersets: the
snapshot-kind set additionally allows `release_v2`, rule scope allows `global`, commission
eligibility no longer requires track eligibility for release-v2 rows (it derives from
`account_class` + `paid_funded_nano` instead of a pricing mode), and a dedicated release-v2 branch
requires release lineage, account class, exact funding split and NULL legacy pricing-mode fields.
Existing `policy_v1`/`legacy_scalar`/`legacy_b2c_track` rows satisfy the unchanged remainder of each
expression, so the swap rewrites no data and the currently deployed writer stays valid. No
application writer stores release-v2 attributions yet; the dependent consumer ships only after this
migration SHA has green `deploy/migration` and `deploy/watchdog` in production.

Migration `0038_pricing_attribution_release_v2_nullable_mode.sql` fixes the unreachable
release-v2 branch left by `0037_pricing_attribution_release_v2.sql`: the `release_v2` snapshot
CHECK requires `pricing_mode IS NULL` and `rule_origin IS NULL`, but both columns were still
`NOT NULL`, so no release-v2 attribution could ever be stored. The migration drops the two
`NOT NULL` constraints and re-adds `pricing_usage_attributions_base_check` as a strict superset
that permits NULL values. Legacy `policy_v1`/`legacy_scalar`/`legacy_b2c_track` writers keep
their non-NULL semantics because the unchanged per-kind branches of
`pricing_usage_attributions_snapshot_check` still require concrete `pricing_mode`/`rule_origin`
values for every non-release-v2 kind. The swap rewrites no data, and the release-v2 ingest
consumer ships only after this migration SHA has green `deploy/migration` and
`deploy/watchdog` in production.

Migration `0039_pricing_provider_recovery_version.sql` adds a non-negative
`provider_recovery_version` to immutable commerce usage rows with backward-compatible default `0`.
The migration itself does not reclassify provider spend or start a backfill. Recovery consumer v2,
delivered after the migration and strict engine producer were both green, selects NULL,
`unattributed`, and `unavailable` only when their stored version is older than `2`. Exact evidence
and exhausted attempts both advance the version, while new rows with an exact provider start at the
current version. This retries terminal rows from the weaker request-ID-only algorithm exactly once
without turning idle polling into an unbounded rescan.

Migration `0041_pricing_control_notify.sql` adds `AFTER INSERT` triggers on the three
pricing-control job tables (`engine_catalog_jobs`, `engine_switch_jobs`, `engine_policy_jobs`) that
emit `pg_notify('pricing_control_jobs', TG_TABLE_NAME)` per new durable job row. `pg_notify` is
delivered on COMMIT of the inserting transaction, so a notification can never reference a job that
rolled back, and every existing and future enqueue path is covered without call-site changes. The
migration changes no job, binding, policy, catalog or switch row and does not wake any worker by
itself: delivery remains owned by the periodic worker sweep until the separately delivered LISTEN
consumer ships, and that consumer treats the sweep as the recovery path for notifications missed
while no listener was connected. The LISTEN consumer may be delivered only after this migration SHA
has green `deploy/migration` and `deploy/watchdog` in production.

Migration `0042_pricing_strict_chain_pending.sql` adds `strict_chain_pending boolean NOT NULL
DEFAULT false` to `account_policy_bindings`. The flag is the durable intent behind the agreed
B2B enforcement contract (`docs/commerce/MULTI-DISCOUNT.md` decisions 13–14): a B2C→B2B conversion
and every `b2b_client` policy save must chain the per-account strict cutover automatically instead
of waiting for the fleet Stage 9 CAS. The default `false` keeps the deployed writers compatible and
backfills nothing — accounts converted before the chain existed keep the manual
`POST /v1/admin/users/:id/policy-enforcement-cutover` repair lane. The migration stages no job,
touches no policy/binding state, and does not enforce anything by itself; the dependent writers
(conversion/save setting the flag) and the worker sweep consuming it ship only after this
migration SHA has green `deploy/migration` and `deploy/watchdog` in production.

Migration `0043_pricing_successor_activation.sql` expand-only widens two Stage 9 check
constraints: `pricing_release_control_jobs_v2.job_kind` admits `activate_successor` and
`pricing_release_activation_receipts_v2.activation_kind` admits `successor`. A successor
activation advances the live pricing release head to a NEWER prepared target/recovery pair (the
standard path for publishing a new pricing generation, e.g. an added model); the durable job and
receipt identities stay exact, and every existing job kind, receipt and constraint arm is
unchanged. The migration stages no job and changes no row; the dependent consumer (staging,
collector and worker execution of successor jobs) ships only after this migration SHA has green
`deploy/migration` and `deploy/watchdog` in production.

Migration `0044_pricing_release_orchestrations.sql` expand-only creates
`pricing_release_orchestrations_v2`: one durable intent row that drives a full successor release
cycle (catalog/switch delivery, Stage 5 materialization, Stage 6 funding normalization, Stage 7
shadow rollout, Stage 8 capture, Stage 9 activation, verification) through the existing durable
sub-jobs and their unchanged gates. A partial unique index admits at most one active
orchestration. The migration changes no existing row, job or constraint and stages nothing by
itself; the orchestrator consumer (worker state machine and the AdminGuard stage/status routes)
ships only after this migration SHA has green `deploy/migration` and `deploy/watchdog` in
production.

Migration `0046_scalar_pricing_bounds.sql` adds the missing upper half of the commerce engine
account mirror's payable-multiplier contract as a separately named CHECK (`mult_bp <= 10000`). The
existing non-negative CHECK remains untouched, all deployed writers already emit the strict subset,
and the migration changes no value or job. Production was preflighted with zero rows outside
`0..10000`. The dependent shared-contract narrowing ships only after this migration SHA has green
`deploy/migration` and `deploy/watchdog`.
