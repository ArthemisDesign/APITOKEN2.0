# Retired pricing schema closeout

This runbook is the contract for removing the pricing policy/catalog/release/funding schema that
was taken out of every live runtime path on 2026-08-09. It is deliberately fail-closed: the tables
remain immutable audit evidence until every gate below passes. The final migrations use explicit
object names and never `CASCADE` or `IF EXISTS`.

## Current status and retention boundary

The latest authoritative timestamp in the retired set is
`2026-08-10 09:26:32 UTC`, from engine funding generation/lot rows written during the last
pre-retirement reconciliation. The mandatory 30-day retention therefore ends at
`2026-09-09 09:26:32 UTC`. Final admission uses the conservative boundary
`2026-09-09 10:00:00 UTC`, not the exact second, and recomputes the maximum from production rather
than trusting this snapshot. A newer authoritative timestamp moves admission to 30 full days after
that timestamp even when it is later than the conservative boundary.

The read-only production inventory on 2026-08-11 contained:

- 31 engine tables, `154804224` bytes including indexes (about 148 MiB);
- 43 commerce tables, `104783872` bytes including indexes (about 100 MiB);
- `259588096` bytes total (about 247.6 MiB).

The machine-readable inventory is `deploy/pricing-retired-schema-baseline.tsv`: all 74 tables,
their exact row counts, observed physical sizes, and SHA-256 of their canonical row stream. It was
captured read-only on 2026-08-11 and contains no row data. The content digests make a same-count
`UPDATE` or `DELETE` detectable; physical size is reported but is not an immutability verdict,
because VACUUM and index maintenance can change it without changing a row.

A later consumer snapshot that day contained 174 mapped commerce cursors. All 174 had completed
their current polling cycle and had identical ledger/top-up watermarks; all 122 mapped accounts
with engine ledger rows were exactly at their per-account engine head, while the other 52 had no
ledger row to consume. All three sales cursors equalled their source watermarks, with no sync or
usage-parser error since the serving sales release was activated. These counts are observations,
not fixed admission criteria: future correctly provisioned accounts must add their own cursors.

The size is not a reason to shorten retention. The tables are not on a live request path.

## Objects that stay live

The following authorities form the minimal post-drop survival allowlist. It is intentionally not
the whole application schema: it pins the migration journals plus the identity, money, queue,
pricing and reconciliation tables whose accidental loss would invalidate a successful contraction.
The canonical arrays live in `deploy/pricing-retired-schema-manifest.sh`:

- engine: `engine_schema_migrations`, `accounts`, `account_provider_discounts`, `api_keys`, `reservations`,
  `settlement_outbox`, `ledger`, `ledger_consumer_checkpoints`, `usage_events`,
  `pricing_tariff_overrides`;
- commerce: `users`, `customer_profiles`, `customer_provider_discounts`, `engine_accounts`,
  `engine_pricing_jobs`, `engine_credits`, `engine_adjustments`, `pricing_credit_accruals`,
  `pricing_usage_events`, `pricing_usage_cursors`, `pricing_usage_topups`, `payments`,
  `referral_attributions`, `webhook_events`.

`pricing_months` is retired. Registration, pricing, funding, commission and worker code no longer
reads or writes it; keeping it because its name lacks a release suffix would preserve a dead
progressive-pricing authority by accident.

## Engine drop set (`claude_engine` database)

The groups below are a valid foreign-key topological order for the current production schema.
Every group must be dropped before the next. Tables within one group may be listed in any order.

Before group 1, explicitly drop the only live-to-retired foreign key:
`api_keys_activation_policy_fk` (`api_keys` → `account_policy_versions`). It is `NOT VALID`, but it
still enforces new non-null values and therefore still blocks the parent drop.

### Engine group 1 — leaf evidence and allocations

- `public.account_funding_head_v2`
- `public.account_policy_bindings`
- `public.funding_ledger_allocations_v2`
- `public.funding_reservation_allocations_v2`
- `public.ledger_funding_allocations`
- `public.pricing_catalog_heads`
- `public.pricing_release_assignment_extensions_v2`
- `public.pricing_release_assignments`
- `public.pricing_release_head_v2`
- `public.pricing_release_recovery_links`
- `public.pricing_request_funding_allocations_v2`
- `public.pricing_shadow_admission_evaluations`
- `public.provider_switch_entries`
- `public.provider_switch_head`
- `public.reservation_funding_allocations`

### Engine group 2 — request/funding snapshots and activations

- `public.funding_buckets`
- `public.funding_lots_v2`
- `public.funding_reservation_snapshots_v2`
- `public.pricing_admission_snapshots`
- `public.pricing_release_activations_v2`
- `public.pricing_request_snapshots_v2`

### Engine group 3 — generation children

- `public.account_funding_generations_v2`
- `public.account_policy_rules`
- `public.pricing_catalog_entries`
- `public.pricing_release_policy_rules`
- `public.pricing_stage8_evidence_v2`

### Engine group 4 — versions

- `public.account_policy_versions`
- `public.pricing_release_policy_versions`
- `public.pricing_release_versions`

### Engine group 5 — roots

- `public.pricing_catalog_versions`
- `public.provider_switch_versions`

Dropping these tables removes their owned indexes, constraints and triggers. It does not remove the
trigger functions. The final migration must explicitly drop these 48 retired functions with their
exact identity arguments after the tables:

- `public.assert_active_funding_account_v2(text)`
- `public.assert_funding_generation_v2(text, bigint)`
- `public.assert_funding_reservation_snapshot_v2(text)`
- `public.assert_normalized_reservation_funding_v2(text)`
- `public.assert_pricing_release_assignment_extension_pair_v2()`
- `public.assert_pricing_request_funding_v2(text)`
- `public.assert_strict_funding_account(text)`
- `public.assert_strict_reservation(text)`
- `public.enforce_account_funding_head_step_v2()`
- `public.enforce_active_funding_v2_from_account()`
- `public.enforce_active_funding_v2_from_head()`
- `public.enforce_funding_generation_v2_from_generation()`
- `public.enforce_funding_generation_v2_from_lot()`
- `public.enforce_funding_ledger_v2_account()`
- `public.enforce_funding_reservation_allocation_v2_update()`
- `public.enforce_funding_reservation_snapshot_v2_account()`
- `public.enforce_funding_reservation_v2_from_allocation()`
- `public.enforce_funding_reservation_v2_from_head()`
- `public.enforce_funding_reservation_v2_from_reservation()`
- `public.enforce_funding_reservation_v2_from_snapshot()`
- `public.enforce_ledger_funding_allocation_account()`
- `public.enforce_pricing_release_activation_v2()`
- `public.enforce_pricing_release_assignment_extension_v2()`
- `public.enforce_pricing_release_head_audit_v2()`
- `public.enforce_pricing_release_head_step_v2()`
- `public.enforce_pricing_release_recovery_kinds_v2()`
- `public.enforce_pricing_release_rule_owner_v2()`
- `public.enforce_pricing_request_funding_v2_from_allocation()`
- `public.enforce_pricing_request_funding_v2_from_reservation()`
- `public.enforce_pricing_request_funding_v2_from_snapshot()`
- `public.enforce_pricing_request_v2_account()`
- `public.enforce_pricing_shadow_admission_rule_identity()`
- `public.enforce_pricing_snapshot_reservation_account()`
- `public.enforce_release_v2_lineage()`
- `public.enforce_service_meter_only_policy_rule()`
- `public.enforce_strict_binding_cutover()`
- `public.enforce_strict_binding_runtime_floor()`
- `public.enforce_strict_binding_state()`
- `public.enforce_strict_funding_account_from_account()`
- `public.enforce_strict_funding_account_from_bucket()`
- `public.enforce_strict_key_policy_ack()`
- `public.enforce_strict_reservation_from_allocation()`
- `public.enforce_strict_reservation_from_reservation()`
- `public.reject_funding_reservation_snapshot_v2_update()`
- `public.reject_immutable_pricing_release_v2_mutation()`
- `public.reject_pricing_request_v2_update()`
- `public.reject_pricing_shadow_admission_evaluation_update()`
- `public.reject_pricing_snapshot_update()`

The other four `public` functions are live and must remain:

- `public.enforce_account_settlement_floor_fence()`
- `public.enforce_priced_terminal_collection_fence()`
- `public.enforce_pricing_tariff_override_version()`
- `public.reject_pricing_tariff_override_mutation()`

## Commerce drop set (`commerce` database)

### Commerce group 1 — leaf jobs, receipts and allocations

- `public.account_policy_reconciliations`
- `public.account_policy_rules`
- `public.business_invite_policy_bindings`
- `public.business_invite_policy_snapshots_v2`
- `public.engine_catalog_jobs`
- `public.engine_policy_jobs`
- `public.engine_switch_jobs`
- `public.pricing_funding_normalizations_v2`
- `public.pricing_months`
- `public.pricing_policy_heads`
- `public.pricing_policy_rules`
- `public.pricing_policy_rules_v2`
- `public.pricing_release_activation_receipts_v2`
- `public.pricing_release_assignments_v2`
- `public.pricing_release_control_jobs_v2`
- `public.pricing_release_orchestrations_v2`
- `public.pricing_shadow_policy_jobs_v2`
- `public.pricing_stage5_blockers_v2`
- `public.pricing_stage5_prepare_acks_v2`
- `public.pricing_stage8_capture_artifacts_v2`
- `public.pricing_usage_funding_allocations`
- `public.product_catalog_heads`
- `public.provider_capability_aliases`
- `public.provider_capability_head`
- `public.provider_switch_entries`
- `public.provider_switch_head`
- `public.service_account_inventory_v2`

### Commerce group 2 — bindings, documents and captured evidence

- `public.account_policy_bindings`
- `public.pricing_policy_documents_v2`
- `public.pricing_shadow_rollouts_v2`
- `public.pricing_stage8_capture_jobs_v2`
- `public.pricing_stage8_evidence_v2`
- `public.pricing_usage_attributions`
- `public.product_catalog_entries`

### Commerce group 3 — versions and plans

- `public.account_policy_versions`
- `public.pricing_release_plans_v2`
- `public.pricing_stage5_runs_v2`
- `public.provider_capability_entries`

### Commerce group 4 — policy/switch versions

- `public.pricing_policy_versions`
- `public.provider_switch_versions`

### Commerce group 5 — policy and catalog roots

- `public.pricing_policies`
- `public.product_catalog_versions`

### Commerce group 6 — capability root

- `public.provider_capability_versions`

Commerce has no live-to-retired foreign key in the current production graph. Drop these five
retired functions explicitly after the tables:

- `public.enforce_pricing_policy_rule_v2_owner()`
- `public.guard_pricing_release_assignment_v2()`
- `public.guard_pricing_release_plan_v2()`
- `public.guard_pricing_stage5_run_v2()`
- `public.notify_pricing_control_job()`

Keep the live content functions `public.enforce_blog_draft_identity()` and
`public.enforce_blog_first_publication()`.

## Mandatory pre-drop gates

All gates are conjunctive. A failure postpones the drop; it is never waived because a backup exists.

1. **Time and immutable evidence.** Recompute the maximum authoritative created/updated/normalized
   time and every table's row count plus canonical content digest. The maximum must still be
   `2026-08-10 09:26:32 UTC` or earlier; counts and digests must equal
   `deploy/pricing-retired-schema-baseline.tsv`; and at least 30 complete days plus the conservative
   boundary above must have elapsed. Any content difference is an investigation, not a baseline
   update. A newer authoritative timestamp restarts retention from its own time.
2. **No source reader/writer.** `deploy/pricing-retired-schema.test.sh` must pass on the exact
   migration parent. It scans every engine and commerce runtime source path and pins the 31/43
   manifest, the 43 Drizzle symbol mappings and the removed route fragments. The migration commit
   must not remove or weaken this proof.
3. **No selectable rollback consumer.** Every supported engine rollback target must descend from
   `e8cf49ae121b581042c582ddb3621ee29fae8103`, the release that removed the final diagnostic CLI
   reader. Every supported commerce rollback target must descend from
   `0c236aa2334f539786f53429d815d6b7c791adbe`, the scalar-only commerce runtime. The canonical
   `rollback.sh` enforces those exact Git-ancestry floors before any `current`/`previous` link move,
   including the watchdog's automatic post-admission rollback path. `current`, `previous`, every
   active PID and every recorded deployment SHA must also pass the same boundary; checking only the
   serving symlink is insufficient. Older immutable directories may remain during normal ten-release
   retention, but they are not selectable and manual link movement is forbidden. The 2026-08-11
   inventory found twelve such old engine directories; their presence is no longer a contraction
   blocker only after the floor-bearing controller is GREEN in production and a real dry-run proves
   that one pre-floor SHA is rejected while `previous` is accepted.
4. **Consumer watermarks.** Every currently mapped commerce account must have a pricing cursor and
   a completed polling cycle no older than 180 seconds;
   `last_ledger_id` must equal `topups_scanned_through_ledger_id`, and it must cover the engine head
   at the 120-second stable source boundary. The gate retries three snapshots to avoid rejecting the
   instant in which a healthy worker invalidates its completion marker before network I/O. Sales
   `attributions`, `usage_events` and `topups` cursors must cover their own 120-second stable source
   boundary, two complete default sync intervals; sync parse errors must be zero. The lag is fixed,
   not widened to make a failing deployment pass, and no cursor is ever advanced by hand.
5. **Catalog dependency graph.** Re-run the foreign-key, view, materialized-view, trigger and
   `pg_proc` inventory. The only permitted external edge before the engine migration is
   `api_keys_activation_policy_fk`; commerce permits none. There must be no view/materialized view.
   Function names and counts must match the allowlists above. Any new dependency is investigated,
   not included via `CASCADE`.
6. **Recovery evidence.** The watchdog must create fresh custom-format dumps of both `commerce` and
   `claude_engine` immediately before each destructive migration. Validate each with
   `pg_restore --list`, record its path, size, SHA-256 and the exact migration SHA in the final
   closeout, and confirm the off-host Borg path will include it. Both dumps and their root-owned
   completion marker must be no more than 30 minutes old, cannot be future-dated, and the dumps must
   predate the marker. The pre-drop row-count/size/time/FK inventory is the audit manifest retained
   with that closeout.
7. **Business health.** Pricing default/provider/status drift, stale confirmed jobs, charge
   mismatch, balance divergence, sales cursor stall, settlement backlog and current unresolved
   provider growth must all be clear. Historical terminal provider gaps do not authorize inferred
   data or delay unrelated schema cleanup.

## Automated preflight

`deploy/pricing-retirement-preflight.sh` evaluates the gates above with read-only PostgreSQL
sessions. It verifies the canonical source manifest/reader guard, exact table rows and SHA-256
content, authoritative timestamps, current/previous/recorded/active rollback ancestry, pricing and
sales watermarks, public function/trigger/FK/view dependencies, live queue and money health, pricing
authority reconciliation, OpenKeys integrity, and Sales sync errors since the serving process was
activated.

Diagnostic mode is intentionally not an approval signal: it prints `NOT AUTHORIZED` and exits 3
even if retention has elapsed. Run it from the immutable candidate named by the watchdog's
`processed.sha`, never from the mutable `/opt/apitoken/repo` checkout: that fetch-only repository's
object database and `origin/master` are current, but its checked-out `HEAD` may deliberately lag.
The report neither writes the databases nor advances a cursor:

```bash
SHA=$(cat /var/lib/apitoken/watchdog/processed.sha)
CANDIDATE=/var/lib/apitoken/watchdog/candidates/$SHA
test "$(git -C "$CANDIDATE" rev-parse HEAD)" = "$SHA"
cd "$CANDIDATE"
deploy/pricing-retirement-preflight.sh --report
```

Validated candidates are retention-bounded. If that exact directory is no longer present, stop and
use the next watchdog-validated `processed.sha`; do not fall back to the stale checkout or assemble
an unvalidated source tree by hand. Final mode always runs inside the fresh exact migration
candidate before candidate retention can remove it.

Final mode is a watchdog migration-stage primitive, not an ad-hoc operator command:

```text
pricing-retirement-preflight.sh --final commerce <exact-migration-sha>
pricing-retirement-preflight.sh --final engine <exact-migration-sha>
```

It must run as root from that exact candidate after `watchdog-backup.sh` has preserved both
`commerce.pre-deploy-<sha>.dump` and `claude_engine.pre-deploy-<sha>.dump`. It validates both archives
with the production `pg_restore` toolchain, rejects evidence older than 30 minutes, emits
path/size/SHA-256/mtime/age/migration-SHA evidence, and requires the Borg source path to cover
`/var/lib/apitoken`. Only a successful `--final` line beginning with `AUTHORIZED:<plane>` may feed
the matching contraction. The migration itself retains its own transactional time, row-count and
dependency assertions; this script is not a substitute for them.

The fixed root-owned `deploy/pricing-retirement-admission.sh` is the executable bridge between this
evidence and the two migrators. Every ordinary migration passes through it, but it invokes final
preflight only for the exact immutable artifacts
`packages/db/migrations/0048_retire_pricing_schema.sql` and
`crates/registry/migrations_pg/0049_retire_pricing_schema.sql`. It binds the candidate SHA/tree,
test marker, migration manifest and (for engine) tested release-binary digest before inspecting the
applied commerce manifest or exact engine schema version. Absence of the named artifact and a fully
recorded contraction are explicit no-ops; partial manifest state, a schema version other than the
unique 48→49 boundary at the initial contraction, a failed preflight, or anything except one exact
`AUTHORIZED:<plane> migration_sha=<sha> retention_epoch=<integer> all_conjunctive_gates=passed`
line stops before the migrator. Once 49 is recorded, a later append-only engine version is an
admission no-op so a failed post-drop proof can be repaired forward; that later version is
forbidden in the initial 48→49 delivery.

Every newly promoted engine release also contains immutable
`.pricing-retirement-admission-v1` metadata with exactly `pre-contraction` or `contraction-0049`.
The engine migration runner invokes final admission only for the latter. Releases created before
this metadata remain valid pre-contraction recovery anchors, and a later pruned watchdog candidate
therefore cannot break an ordinary restart; conversely, a 0049 release cannot lose its gate when
its candidate ages out.

The commerce runner calls admission after the exact-SHA backup and while holding both deployment
and migration locks. The engine runner calls it after taking the migration lock; its exact-SHA
backup is created by the watchdog immediately before the engine rollout. This wiring is mandatory:
elapsed retention or a manually successful diagnostic report cannot authorize either drop.

## Delivery sequence after retention

Use three independently green changes; do not combine the two database contractions into one
failure domain.

1. **Commerce migration-only contraction.** Add immutable migration
   `0048_retire_pricing_schema.sql`. It asserts
   the retention time and exact dependency set, sets a short lock timeout, drops the 43 tables in
   the order above and then the five explicit functions. The watchdog runs exact-SHA
   `--final commerce` after its fresh backups and before the migrator. The migration uses neither
   `CASCADE`, `IF EXISTS` nor dynamic SQL. Merge with `deploy/agent-merge.sh` and wait for exact-SHA
   `deploy/migration` and `deploy/watchdog` GREEN.
2. **Engine migration-only contraction.** From fresh `origin/master`, add engine migration 0049,
   register it in the contiguous migrator and add a real-PostgreSQL contract test. The watchdog runs
   exact-SHA `--final engine` after its fresh backups. The migration first drops
   `api_keys_activation_policy_fk`, then the 31 tables in order and the 48 explicit functions. The
   inactive engine applies it transactionally before readiness, so failure leaves the serving slot
   and schema unchanged. Wait for exact-SHA `deploy/engine` and `deploy/watchdog` GREEN.
3. **Schema/code/document cleanup.** Remove the 43 retired Drizzle declarations from
   `packages/db/src/schema.ts`, update the retired sections of local instructions, remove the
   temporary source-reader regression and the one-time admission/preflight hooks only after both
   migration records and object absence are proven in production, and publish the final
   finding→SHA→tests→production-evidence matrix. No historical migration file is edited or deleted.

Destructive migrations are forward-only. Binary rollback never recreates these objects. If a drop
unexpectedly exposes a missed dependency, stop delivery and fix forward from the preserved dumps;
do not hand-create placeholder tables and do not restore one database over live post-drop writes.

## Post-drop verification

The watchdog derives a stage only from newly added paths between its durable `processed.sha` and
the exact candidate: commerce 0048 or engine 0049. Neither path means no-op; both paths in one
delivery fail closed. This range rule also makes a forward fix re-run the proof when a contraction
has committed but the candidate was not marked processed.

After normal component and final production verification, but before `processed.sha` and the
overall GREEN verdict, the watchdog invokes the fixed root-owned
`pricing-retirement-postdrop.sh --stage <commerce|engine> <exact-sha>`. The helper independently
binds the immutable candidate, test marker, migration manifest and stage, then uses PostgreSQL
sessions with `default_transaction_read_only=on` to prove:

- the stage's retired tables/functions are absent and every survival-allowlisted object remains;
- commerce 0048 has its unique exact hash and the database ends at the exact candidate's latest
  Drizzle entry (0048 in the initial contraction); engine migrations are the contiguous `1..48`
  pre-engine sequence or `1..N`, where `N >= 49` is the exact candidate version;
- the other plane is in the required pre/post state, so the contractions cannot be reordered;
- the exact-SHA commerce and engine dumps are still root-only, complete, hashable and fully
  readable by the production `pg_restore` toolchain, with Borg covering their path;
- pricing and Sales cursors cover stable source watermarks; live queues, pricing mirrors/provider
  overrides/status, settlement accounting, Sales reconciliation and OpenKeys contracts are clean;
- the sole permitted below-floor row, if it still exists, is exactly the documented bonus-revocation
  debt with its unique ledger adjustment—any additional or changed debt fails;
- no database-owning service has logged `42P01`, `undefined_table`, `relation ... does not exist`,
  or a Sales sync iteration failure since the exact candidate's test marker;
- a monitoring collector cycle completed after the contraction, business reconciliation and all
  backups are healthy, and no targeted money/pricing/settlement/Sales/OpenKeys/backup alert is
  pending or firing apart from the validated `EngineAccountsBelowFloor` exception.

The helper finally reports both current database sizes. A failed post-drop proof quarantines the
candidate without rolling back binaries or recreating objects: destructive migrations are
forward-only, so recovery is a new exact candidate containing a forward fix. The audit is closed
only after both database deliveries and the final schema/code/document cleanup are GREEN in
production.
