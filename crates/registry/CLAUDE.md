# crates/registry — CLAUDE.md

**Role:** engine-owned PostgreSQL authority. SQLite is a one-time migration source and emergency audit snapshot.

PostgreSQL schema DDL is an explicit operation (`claude-api db migrate-engine`), not a service-start
side effect. `serve` may only perform the read-only schema verification before claiming an owner.

**Owner branch:** `comp/registry`.

**Boundaries (hard):**
- Depends only on sync PostgreSQL client, `rusqlite` (import/fallback), `anyhow` and pure
  `serde`/`serde_json`/`sha2` for typed persistence and canonical immutable identities.
- NO network, HTTP, env reads, or subscription-selection logic. Persistence + CRUD + `load_active` only.
- **Billing: client ACCOUNTS (`accounts`) + access keys (`api_keys`) + journal (`ledger`)** live here,
  but ONLY storage/atomic movements in whole nanoUSD. Model: **balance on the ACCOUNT** (user
  profile); keys (`api_keys.account_id`) are access handles to the shared balance (1:N, per
  projects/team); per-key `spent_nano` is spend attribution by key. Functions: `account_create/get/by_handle/list/set_status/rm`,
  `account_topup` (+ledger), request-keyed `reserve_request`/`settle_request` (atomic: account balance + per-key
  spent + ledger row), `key_issue(account_id,label)/get/list/set_status/set_status_by_id/remove/clear`;
  `api_keys.key_id` — a stable non-secret control-plane ID for revocation without storing the full key,
  `key_account` (JOIN key→account for authorization), wrapper `Billing` (Mutex<Conn>). Cost computation
  (tokens→nano) does NOT belong here — that is `metering`; registry accepts the ready amount. **Money
  invariant (with overdraft buffer):** reserve keeps the balance FLOOR at −$1 (`OVERDRAFT_NANO`, in sync with
  `metering::OVERDRAFT_NANO`) — a funded request is NOT dropped with 402 due to a race of concurrent reserves; below the floor
  any positive hold is rejected (max $1 in debt per account). Settle charges the REAL amount (`actual`
  may be > hold — taken from the remaining balance), the forward cap keeps the charge within hold+$1. That is,
  `hold ≤ balance+$1` and `charge ≤ hold+$1` at the ACCOUNT level. `reservations.request_id` is the ownership key;
  `settlement_outbox` retries until the same transaction updates balances and inserts the unique
  `(kind,request_id)` charge. Upstream request IDs are audit metadata, never the money identity.
  Cursor consumers use `ledger_after(account, after_id, limit)` (oldest-first); account pricing uses
  `account_set_mult_bp`. Control reads use `account_funding_snapshot` so scalar totals and grouped
  paid/welcome/other buckets are one snapshot; the residual remains explicit `unattributed`.
  `ledger_recent`/`ledger_after` read each immutable ledger page and its normalized funding
  allocations in one backend snapshot. A pre-column charge whose ledger provider is `NULL` first
  uses exact `account_id + request_id` evidence. Older rows with no request ID may use only a
  request-less usage row with the same account, null-safe key/ref/model, exact charge and at most
  one second of settlement timestamp drift. Every candidate must carry the same non-empty provider;
  ambiguity stays unknown, and a conflict with persisted ledger provider fails the read. The model
  is a fingerprint field only, never provider inference. SQLite and PostgreSQL semantics must stay
  identical.
  Optional per-key `spend_limit_nano` and `expires_ts` are engine-authoritative. Reservation updates
  key `reserved_nano` in the same transaction and enforces
  `spent_nano + reserved_nano + hold <= spend_limit_nano`; settlement atomically converts the hold
  into actual spend or releases it. Mutable policy replacement is account-scoped and atomic with
  reservations; a non-null new limit must cover both settled and reserved usage.
  Soft migration of the old "key=wallet" model → account per-key (`migrate_legacy_keys`).
- Public type [`Sub`] (email/token/proxy/fleet) — the contract for `pool`/`forward`. Change it —
  check both consumers.
- **Durable auth-health of subscriptions** (detection of a banned token): additive columns on `subs`
  (`auth_state`/`auth_fail_streak`/`dead_since_ts`/`dead_reason`/`auth_token_fp`, migration `0003`).
  `SubHealth` + `load_sub_health(fleet)` / `save_sub_health(owner,&h)` — the verdict is written ONLY by the poller
  (owner-fenced in PostgreSQL, like money/pool_state); the state machine lives in `pool`, registry merely
  persists the ready row. `subs_admin` serves the verdict to the panel. `add`/`add_file` reset health to
  healthy upon (re-)issue of a token (auto-revival). PostgreSQL is the authority; the SQLite mirror is for builds only.
- **Pool state persistence (table `pool_state`)** — CAS-versioned and owner-epoch fenced. Atomic
  `capacity_leases` validate cooldown/utilization and atomically track inflight without rejecting
  concurrent work; release/renew/reconciliation remain durable and owner-fenced.
  `leader_leases` elect exactly one poller. `PoolStateRow` (primitives, registry
  does not know `pool` types) + `save_pool_state`/`load_pool_state`. Stores durable state (cooling/
  calibration/spent/util/reset) to survive restarts. `pool` decides the logic (export/import), registry
  merely writes/reads ready rows. No Redis/Redlock participates in correctness.
- Provider attribution is a stable registry constant: Anthropic/Codex keep existing
  values, native Gemini usage writes `PROVIDER_GOOGLE = "google"`. Never substitute the provider with a domain,
  profile/project ID or a client-side value; Google project IDs are not stored in registry.
- Gemini quota calibration persistence stores immutable raw observations, exact cumulative profile
  spend and CAS-derived state for two fixed buckets. `observed_spend_nano` was added by the expand-only
  migration 0014 and stores the exact `ΣΔspend` of workload estimator v2; the meaning of blend/envelope/confidence
  stays in `forward`, registry only validates and atomically persists primitives.
- Codex calibration migrations 0015/0018 expand-only add fixed-point quota evidence and
  an independent native ChatGPT-credit ledger. Every successful turn is recorded as an immutable
  `CodexTurnCalibrationEvent`: opaque internal `request_id`, home/model, effective Standard/Fast,
  provider-reported tier, both schedule identities, disjoint token classes, four API nanoUSD legs and
  three ChatGPT nanocredit legs. Event insert and advancement of both cumulative ledgers are executed
  in one transaction. Exact replay returns the already stored totals without a second charge; a different
  semantic payload under the same request ID yields a typed permanent conflict and changes no ledger.
  The aggregate report groups only immutable rows by home/model/tier/schedules.
  The nullable credit columns are intentional: the migration-first rollout survives the old runtime, and
  estimator v9, upon its first credit-bearing observation, stores the legacy API estimate in `last_*`, resets both
  current estimates and starts a single new anchor. Historical API spend never becomes
  false zero-credit evidence. Raw observations remain the replay authority, including rolling reset;
  workload/envelope/confidence and possibly-unattributed semantics belong to `forward`, registry
  only validates and CAS-persists exact integer primitives.
- Provider calibration migration 0019 expand-only creates a shared immutable event ledger for
  Claude/Gemini and a separate exact cumulative nanoUSD ledger per provider subject. An event stores
  provider-specific token/tool/search classes, tariff schedule, speed/geography and disjoint API-cost
  legs; native quota snapshots remain in their existing Claude/Gemini authorities. The old
  runtime neither reads nor writes these tables, so the migration SHA is safe to roll out before the dependent
  application commit. Operator reads of individual events are always newest-first and bounded to a maximum of
  512 rows; an unbounded ledger scan in the control room is forbidden.
- Gemini exact-calibration migration 0022 expand-only creates a new plan-scoped authority for
  `gemini-5h`/`gemini-weekly` without changing the legacy tables the old runtime keeps writing to.
  A new raw observation stores the real decimal resolution, source/request attribution and exact
  cumulative spend from the shared migration-0019 ledger; CAS state adds unattributed movement and
  honest nullable bounds. The dependent runtime switches to these tables only after a separate
  migration SHA with a green deploy, and old derived evidence is not carried over without the missing
  plan/resolution/source facts.
- Claude calibration migration 0020 expand-only creates separate fixed-point authorities for the 5h/7d
  windows: the plan is part of the identity, a raw observation stores the real quota-fraction resolution, the source
  and an optional request ID, and CAS state stores exact observed spend/fraction, low/high/confidence and
  unattributed movement. There is no subscription face value, prior/EMA or float money in the tables. The old runtime
  neither reads nor writes them; the dependent release must first durably record the turn from migration 0019,
  then link the quota snapshot with the obtained cumulative subject spend. Event insert + subject spend
  advance are one transaction: exact request replay returns the existing total and does not charge
  again, a differing semantic payload yields a typed permanent conflict without changing any ledger.
  An observation is immutable-keyed by subject/plan/window/reset/time/source; 5h and 7d never share an
  anchor/history. Registry stores and validates only integer evidence/CAS primitives; estimator
  replay, reset jitter, one-snapshot lag, plan pooling and delivery retry belong to `forward`/`server`.
  The PostgreSQL initial CAS insert must explicitly type the version placeholder as `bigint`: an expression
  with an untyped integer literal otherwise infers the parameter as `int4` and blocks durable FIFO until recovery.
- KIMI calibration migration 0027 stores a separate plan+duration authority with requested/served model
  and raw `used/limit` quota evidence. `record_kimi_turn` performs the immutable insert and the cumulative
  subject spend in one transaction: concurrent exact replay is serialized via
  `ON CONFLICT(request_id)`, a different payload conflicts, and tracking/update timestamps keep the
  earliest/latest even with out-of-order finalizers. `save_kimi_calibration` inserts the observation and
  advances the derived state in one version CAS; the CAS loser rolls back its observation, history reads
  oldest-first. Real-PostgreSQL gate — `pg::tests::kimi_calibration_postgres_matrix`.
- **Execution-group fencing runtime (phase 6.3):** migration 0021 expand-only added to
  `reservations` a nullable `group_id`, a positive one-based `attempt` and an insert-first-wins
  `execution_group_winner`. `group_id IS NULL` means effective group `request_id`; all reserve
  APIs keep direct wrappers and group-aware variants, and exact replay compares both fields.
  Nonzero settlement captures the winner slot in the same transaction; the loser gets effective actual
  0/full refund, strict funding terminates as a cancel without usage/charge, and the original outbox
  payload is not rewritten. Zero/cancel does not assign a winner. Winner pruning is allowed only after
  the deletion of the last reservation with the same `COALESCE(group_id,request_id)`. SQLite and PostgreSQL
  must remain semantically identical; the per-process loser counter/log is only an incident tripwire —
  correctness belongs to the table PK.
- **Multi-provider pricing Stage 3A** (`pricing`) — dormant persistence contract: immutable
  catalog/switch/account-policy versions are first `prepare`d, then separate heads/bindings move
  only via explicit monotonic CAS `activate`. SQLite and PostgreSQL must return identical typed
  outcomes and atomically persist the parent with all children. Registry-owned timestamps are not part of the
  identity; a policy stores complete catalog/switch/capability/source pins. `Strict` here is fail-closed,
  legacy OpenKeys is replacement-locked and checked against live `accounts.mult_bp`, current OpenKeys is only
  1:1 without model rules. Until Stage 3B the API has no runtime/HTTP writer and no production activation. The future
  resolver must verify the exact immutable policy dependencies on every read and independently
  apply the current admission heads. Current heads do not have to equal the policy pins: separate
  catalog → switches → policy activations are not a shared transaction.
- **Stage 3C Control API persistence surface:** server may expose the existing typed
  prepare/read/activate contract through its authenticated `/admin/pricing/*` routes, but registry
  remains HTTP-free. Every write still runs on the billing single writer; reads use one backend
  transaction. JSON serde on pricing DTOs is strict about unknown struct fields and uses stable
  snake_case enums. Exact replay returns `Unchanged`; invalid, missing dependency, stale,
  version-conflict, catalog/switch CAS, policy-binding CAS and immutable-lock remain distinct typed
  outcomes. This surface does not seed/backfill data, issue keys or enable strict enforcement.
  The sole exception to the generic replacement lock is
  `locked_openkeys_policy_transition`: one transaction inserts the exact next provider-only
  managed 1:1 OpenKeys policy and CAS-moves the exact active replacement-locked legacy binding to
  `shadow + legacy_single + verified`, then atomically consumes the source replacement lock so
  later generations advance the engine-validated canonical managed 1:1 successor through the
  generic prepare/activate CAS lane. Account/policy/owner/product identity and both version
  counters must be preserved/advance once; the successor's exact catalog and switches must already
  be active. Generic prepare/activate remain locked until that one-time unlock, any model
  rule/discount/eligibility flag is invalid, a lost-ACK exact replay is `Unchanged`, a second
  genuine transition is rejected by the successor identity validation, and any failed
  insert/CAS rolls back all rows. A lock whose row is no longer the active target can only
  come from a transition applied before lock consumption shipped, so it is spent history: the
  next generic prepare consumes that exact stale lock (account, effective version, content
  digest, flag set) atomically with the prepare and proceeds; a lock on the active row — or on
  a lineage with no active row — still rejects generic prepare/activate with `locked`.
  Separately, a **shadow lineage rebind** is the one accepted identity change on the generic
  prepare/activate path: while the stored binding is `shadow` and already active, a spec with a
  different class/product identity (B2C→B2B conversion) prepares as a new lineage — own
  `policy_version` sequence, `effective_version` stays account-monotonic — and activation
  CAS-pins the exact old lineage target, requires a shadow target binding, and atomically moves
  the binding row's class/product. Strict bindings keep full identity immutability; a same-class
  policy_id/owner change is never a rebind.
- **Stage 3B0/3B1b snapshot read — dormant:** `pricing_read_bundle(account_id)` returns in one read-only
  transaction the live `accounts.mult_bp`, binding/active policy, exact immutable
  `policy_catalog/policy_switches` and the current `admission_catalog/admission_switches`: SQLite via a
  deferred snapshot, PostgreSQL via `REPEATABLE READ READ ONLY`. An active policy must find both
  pinned dependencies or the read fails as an integrity error; an inactive binding gets only the
  admission heads, an unbound account gets neither pair. The scalar is part of the same snapshot, otherwise legacy
  OpenKeys validation races with the multiplier writer. Registry only materializes the data;
  the independent policy/admission gates are executed by the pure resolver in `forward`. Never assemble the bundle
  as a sequence of separate `active_*`/`*_by_generation` reads — that mixes generations.
  The only runtime caller is the default-off bounded Stage 3B1c worker: it reads the bundle only
  through a separate PostgreSQL shadow-reader actor and does not participate in readiness/admission/money.
- **Stage 3B1a shadow schema:** PostgreSQL migration `0009` and SQLite parity create a
  separate immutable `pricing_shadow_admission_evaluations`. It does not replace the actual
  `pricing_admission_snapshots`: a shadow row references an already recorded actual snapshot and
  stores both lineage pairs (`policy_*` and `admission_*`), the runtime manifest, the scalar comparison and a typed
  outcome. Dependency capability pins are exact-linked to immutable catalog/switch versions. The dormant
  typed SQLite/PostgreSQL insert/read API already computes the canonical manifest digest from the full
  sorted member-set, verifies membership of all four pins before writing and re-computes
  `evaluation_digest` on read. Exact replay with different timestamps/diagnostics returns the first
  row; a differing semantic digest yields a typed conflict, not an update. Manifest members serve as
  insert-time evidence and are not duplicated in the row; a standalone read confirms the manifest identity
  but does not re-enumerate members without the original manifest. The default-off Stage 3B1c worker may now
  write these rows only after an atomic actual snapshot; the migration itself still does not
  create heads, policies or seed data.
- **Stage 3B1c.1/3B1c.2 actual legacy snapshot foundation:** typed
  `LegacyScalarAdmissionSnapshot` records the exact request/account, fixed-plane provider
  (`anthropic|openai|google`), requested/canonical model, alias/tariff identities, timestamps, scalar,
  official/charged hold and provider-typed premium modifiers. Registry itself builds and re-verifies on every read
  the `sha256:v1:<hex>` over a versioned binary TLV with a separate domain separator; JSON premium
  modifiers are only a strict storage projection, not the digest source. The new
  `sqlite_reserve_request_with_legacy_snapshot` and
  `PgStore::reserve_request_with_legacy_snapshot` use `snapshot.charged_hold_nano` as
  the sole hold source and atomically persist the money, reservation and snapshot. Exact retry of an active
  `reserved|delivering` reservation returns the stored typed snapshot without extending the lease and without
  a second money mutation; mismatch, terminal state, non-legacy snapshot or an old reservation without
  a snapshot yield a typed conflict. PostgreSQL keeps the owner fence and the request advisory lock.
  The guarded variants of both APIs invoke the caller-owned commit gate only for insert/exact replay after
  all fallible writes and the final owner fence, immediately before commit; a closed gate
  fully rolls back the attempt as `AbortedBeforeCommit`. `NotReserved`, conflict and an earlier
  error do not invoke the gate. The old reserve APIs are unchanged and create no snapshot. No migrations were
  added: the actual schema `0006` is used. The default-off live sampler and the atomic caller
  serve Anthropic/OpenAI/Google; Google stores typed `gemini_v1` reserve modifiers and the durable
  provider ID `google`, not the deprecated `gemini`. OpenAI image generation/edit uses the typed
  `openai_image_v1` modifier with the exact operation, `opaque/low/auto` controls and reference count;
  provider mismatch or any other control shape fails closed, and the canonical digest covers every
  field. Only snapshot-bearing success may hand the work to the bounded shadow producer. Production
  config remains off.
  The new PostgreSQL writer, after a potential request-lock wait, re-checks the owner via
  `FOR UPDATE`, holds the epoch row until commit and uses a fresh reservation timestamp; the real-PG
  race test proves rollback of a stale epoch without money/orphan writes. The snapshot constructor
  guarantees the storage shape but not the model/tariff provenance: the live caller must build the input only
  from the `metering` canonicalizer. For the live atomic API a bounded idempotency contract is adopted: immutable
  `admission_ts` allows replay only when its age is `<24h`; a future/expired timestamp returns a
  typed conflict before any money mutation, including a re-check after a potential DB lock wait.
  Terminal reservations and actual/shadow children are retained separately from ledger/usage for 30 days, and
  registry rejects any fresher prune cutoff before opening the transaction; maintenance reports
  exact cascade counts. This is not a permanent tombstone and not infinite dedupe:
  the bridge must use only an internal CSPRNG UUIDv4, preserve the first timestamp and have a
  queue `max_age <24h`. SQLite and PostgreSQL keep their inherited different balance gates
  (full-cover versus overdraft floor);
  the parity of this checkpoint concerns atomic snapshot/replay/conflict, not `NotReserved`.
- **Stage 3B1c shadow evaluation persistence:** `ShadowActualSnapshotRef` is built
  only from a validated actual snapshot; fixed-plane identity, scalar and holds cannot be independently
  substituted by the caller. Registry computes the policy hold as a checked integer half-up and itself derives
  `equal|different`. An actual below the checked scalar quote counts as the exact funding ceiling, which
  bounds the policy candidate identically; an actual above the scalar quote fails closed. The compatibility
  enum of the old balance-cap drop is no longer emitted. The resolved outcome stores the exact immutable
  policy/rule and both lineage pairs; a rejection requires the observed scalar, a read error does not allow it.
  The diagnostic JSON
  is non-authoritative, excluded from the digest and bounded by a contract identical for SQLite/JSONB on compact
  bytes, NUL, depth and items. PostgreSQL serializes the request through a separate advisory namespace and
  holds the parent actual `FOR KEY SHARE` until the immutable insert; SQLite uses `BEGIN IMMEDIATE`.
  The API does not read current heads and does not re-resolve historical evidence. The pure forward work-item/builder
  uses the registry-owned typed eligibility gate before enqueue, derives the resolver manifest
  only from canonical evidence and verifies the identity before forming the input. The read-only outcome getter
  performs no persistence. Timed PostgreSQL wrappers set transaction-local statement/lock timeout;
  live reads use a separate bounded actor budget, while inserts pass through the existing billing
  writer without transient retry. SQLite APIs remain for parity/tests and have no live producer.
- **Stage 8 engine evidence v2:** the PostgreSQL-only read report is materialized in one
  `REPEATABLE READ READ ONLY` transaction and accepts exact target/recovery generations. Besides the
  active main/openkeys graph, classifications and full actual→shadow coverage, it re-reads the
  prepared target/recovery releases and the recovery link, verifies both full-inventory assignment sets,
  their shared funding identity, live funding heads/lots with aggregates, the current canonical
  `engine_inventory_digest`/`funding_digest`, the target rule precedence and the observed audit count
  of unfinished legacy-format inflight without requiring zero. The compile-fixed pricing capability and
  the separate release schema version are not
  mixed: every live `engine_instances` must declare release/funding schema v2 and a
  non-empty runtime digest; the absence of at least one such claim is a blocker until the separate Stage 9
  runtime checkpoint. `shadow_digest`, `runtime_floor_digest` and the entire report get the canonical
  `sha256:v2` identity. The external Gemini admission aggregate and the durable provider=`google`
  usage/outbox remain bounded audit counts but do not replace the mandatory Google actual snapshots
  and shadow evaluations. Subject identities come out only as SHA-256 digests. The report does not
  activate or fix anything; any blocker must stop Stage 9.
  With an absent release head, inventory means the full base manifest for cutover. With an exact target
  head the same endpoint builds fresh recovery evidence from the immutable base inventory and accepts a
  post-cutover account only via an exact paired target/recovery extension with live funding parity.
  With the head behind the requested pair the same endpoint builds successor evidence: the frozen
  legacy shadow/binding gates no longer apply, the target must be strictly newer than the active
  head, and both generations must cover the exact full live inventory in their BASE manifests —
  extensions bind to the outgoing head and never transfer, so a later account fails the capture
  closed and the consumer stages a fresh pair.
- **Target Stage 5/6/9 contract:** authoritative inventories fully replace the manual assignment
  matrix. Funding normalizes online account-local transactions: exact historical welcome remains
  bonus, residual counts as paid; new `$5` grants, reviewer artifacts and a global money drain are not
  needed. A prepared pricing release binds the entire inventory, and Stage 9 moves one global active
  head. Registry must atomically persist the reserve-time release/funding snapshot, allow
  in-flight v2 settlement across the cutover and support service `meter_only` without a balance debit.
- **Pricing release/funding v2 schema checkpoint:** PostgreSQL migration `0023` creates empty
  immutable policy/release/assignment/evidence authorities, one global head that remains absent until activation,
  per-account funding generations/lots/heads and request/ledger allocations. Deferred
  constraints hold the account↔generation↔lot and reservation↔allocation sums, including the overrun
  `charged > reserved` only when release is zero; the reserve snapshot pins the exact
  release/assignment/policy/rule/tariff and is never updated. Nullable lineage in
  `settlement_outbox`/`usage_events`/`ledger` keeps old writers valid and must reference the snapshot
  exactly for v2 rows. The service policy has no product catalog/switch/rules,
  `meter_only` requires a zero customer charge. The new runtime does not use these structures until
  a separate producer SHA after a green migration/watchdog of this checkpoint.
- **Pre-cutover funding snapshot checkpoint:** PostgreSQL migration `0024` adds the independent
  `funding_reservation_snapshots_v2`/`funding_reservation_allocations_v2` for account-local Stage 6.
  They do not select a pricing release and do not create a head: until Stage 9 the existing immutable pricing
  snapshot remains the price authority, and the new snapshot pins only the active funding generation,
  bonus-first lot order and paid-only overdraft. A deferred coverage rule forbids a new unfinished
  reservation of a normalized account without exactly one compatible funding snapshot. Runtime
  writers are wired in only by a separate SHA after a green migration/watchdog; a funding head after
  the first normalization is never deleted and moves only by a monotonic generation/version step.
- **Release-v2 ledger attribution checkpoint:** PostgreSQL migration `0028` metadata-only replaces
  `ledger_multi_discount_ranges` with a superset (`snapshot_kind` now allows `release_v2`) and
  adds a separate `ledger_release_v2_attribution_shape`: a release-v2 charge must carry
  attribution schema >= 2, release lineage, account class, an exact paid/bonus/other split and a
  snapshot digest, while legacy pricing-mode/eligibility fields remain NULL. Existing
  policy_v1/legacy_scalar rows satisfy the previous expression; the migration rewrites no data
  (NOT VALID + VALIDATE). No writer creates release-v2 attribution yet: the dependent producer
  is delivered by a separate SHA after a green migration/watchdog of this checkpoint.
- **Release-v2 ledger attribution producer:** release-v2 settlement in
  `postgres_process_pricing_release_settlement_v2` writes the charge row atomically with the full immutable
  attribution (`attribution_schema_version=2`, `snapshot_kind='release_v2'`, account class,
  rule/policy/tariff identity, `official_cost_json`, `snapshot_digest`) and the exact paid/bonus/other
  split that `settle_pricing_release_funding_v2` returns in `SettlementFundingV2` together with
  lot-level evidence; `funding_allocation_json` mirrors the durable `funding_ledger_allocations_v2`
  (lot_id/source_type/version/direction/amount_nano/allocation_order). The ledger read serves release
  lineage via `LedgerAttribution.release_*` (legacy/SQLite rows — `None`). `meter_only` still
  creates no charge row, terminal replay does not duplicate it (unique `(kind,request_id)`),
  and the pricing-mode/eligibility fields remain NULL by CHECK. Real-PG gate:
  `pg::tests::pricing_release_ledger_attribution_v2_postgres_matrix`.
- **Pre-cutover funding dual writers:** all three PostgreSQL reserve paths (scalar,
  legacy-snapshot and strict-policy), the settlement outbox and `account_topup` are serialized in the order
  `request advisory → funding account advisory → reread head → row locks/money writes` (top-up has
  no request lock). While no funding head exists, the transaction keeps the previous legacy semantics;
  once a head appears, the same transaction must update the account aggregate, active generation,
  lots and immutable reservation allocations together. Reserve allocates `welcome_bonus` first,
  then `paid`; the only permitted overrun does not exceed `$1` and applies to the last paid
  allocation, including a zero paid anchor for bonus-only/zero holds. A normalized
  balance generation must therefore contain a paid lot even with zero residual; its absence
  fails closed. Settlement uses only the
  reserve-time allocation, writes `funding_ledger_allocations_v2`, and terminal replay does not repeat the
  money mutation and remains valid after a monotonic advance of the funding head. A `signup-bonus:*`
  top-up creates a welcome lot; all other credits and negative adjustments create a paid lot. Real
  PostgreSQL gate: `pg::tests::pre_cutover_funding_v2_writer_postgres_matrix`; it proves
  replay/cancel/settle/overrun/outbox recovery and both lock-order races.
- **Online account-local funding normalization:** `funding_normalization_v2` builds a read-only
  content-addressed `sha256:v2` plan and applies only an exact source/target identity in one
  `SERIALIZABLE` PostgreSQL transaction under the same funding-account advisory lock. A legacy active
  reservation with ambiguous bonus/paid attribution blocks only its own account. If exact
  buckets or the ledger prove that the entire active reserve belongs to `paid` (including a fully
  drained welcome), the apply in the same transaction creates the generation/lots/head and immutable
  paid-only funding snapshots/allocations for each such request; the pricing snapshot is not
  rewritten. Stale state is re-planned, exact replay returns `unchanged`. An exact old
  welcome bucket is carried into `welcome_bonus`, otherwise the balance is
  reconstructed from `signup-bonus:*` and immutable balance gaps; an exact same-subject/full-amount
  `bonus-revoke:*` removes the entitlement and makes the entire current aggregate `paid`, while partial/mismatched/
  duplicate/mixed evidence is blocked. All other residual is `paid`, including the mandatory zero
  paid anchor. Apply does not move the pricing release. Real-PG gate:
  `funding_normalization_v2::tests::postgres_online_funding_normalization_v2_matrix`.
- **Stage 9 runtime-claim fence:** migration `0025` expand-only adds the nullable
  `engine_instances.pricing_release_claim_epoch`. While the global release head is absent, an old
  runtime with a nullable v2 claim remains compatible. After the first head, any insert, heartbeat or
  owner takeover must carry release/funding schema v2, a non-empty runtime digest and a claim epoch
  equal to the current owner epoch. This prevents an old binary from inheriting the v2 identity of the previous
  process via `ON CONFLICT`. The dependent claim writer is delivered only after a GREEN migration
  SHA.
- **Stage 9 zero-drain/provisioning schema checkpoint:** migration `0026` relaxes only the
  DB constraint of Stage 8 evidence: `legacy_inflight_count` remains a mandatory audit count, and
  the engine report now returns `passed=true` with zero real blockers while old-format
  requests complete normally against their immutable snapshots. The same migration creates an empty
  append-only `pricing_release_assignment_extensions_v2` for accounts that appeared after the cutover.
  Each extension is bound to the exact current head activation and an atomic active/recovery pair;
  manifest assignments never mutate. The dependent PostgreSQL producer now, under the shared pricing
  control lock, validates the exact head/recovery link, the absence of a base assignment and the policy; a balance
  assignment additionally takes the account funding lock and requires the exact active funding head. The writer
  atomically writes the pair, returns `unchanged` on exact replay and a typed
  `stale|version_conflict` without a partial write. Exact readback is keyed by
  `(provisioning_head_version, account_id)`, and the runtime resolver reads either the base or the extension in one
  snapshot. SQLite remains unavailable; the route neither creates nor moves a head. Real-PG coverage —
  `pg::tests::pricing_release_runtime_v2_postgres_matrix`.
- **Pricing release v2 producer checkpoint:** `pricing::release_v2` and PostgreSQL persistence
  add append-only policy/release/recovery prepare and read-only inventory/head. Policy reads expose
  both exact `(policy_id, policy_version)` lookup and the newest complete immutable version for one
  exact policy ID, so a lagging consumer can reconcile a remote-only successful prepare without
  guessing or rewriting lineage. Release prepare verifies exact full-account coverage (`active` + `disabled`) and ready funding
  dependencies. A disabled account intentionally remains in the immutable release so that a later
  enablement does not create a hole in the policy/funding authority. SQLite returns unavailable instead of
  a local authority.
- **Stage 9 activation producer:** `postgres_activate_pricing_release_v2` is the only writer of the
  global head. In one `SERIALIZABLE` transaction under `PRICING_RELEASE_CONTROL_LOCK_V2` it requires
  fresh combined evidence, an exact absent/target CAS, the immutable target/recovery link, current
  catalog/switch lineage, the full base inventory (or exact paired extensions on recovery), funding
  parity and the compile-fixed runtime floor with exact owner-epoch claims. Evidence, the activation audit and one
  head row commit together; a rejection rolls back entirely. Exact audit replay returns
  `unchanged`; recovery moves only forward from the target. A successor activation (migration
  `0035`) advances any exact active head to a newer prepared target: the from identity records the
  outgoing head, inventory authority is the full live engine inventory (extensions never
  transfer), and the provisioning context afterwards exposes the new target as active with its
  paired recovery, exactly as after a cutover. Accounts, balances, lots, reservations,
  ledger/usage are not written. Real-PG coverage —
  `pg::tests::postgres_stage8_engine_evidence_contract`.
- **Stage 9 successor activation schema checkpoint:** migration `0035` expand-only admits the
  `successor` activation kind in `pricing_release_activations_v2` and adds its arm to the
  activation-evidence trigger: a successor audit names the exact previous head
  (`from_generation`/`from_digest`, distinct from the evidence target) and the newer prepared
  target it activates, backed by the same fresh passed Stage 8 evidence. The head-step and
  head-audit triggers are kind-agnostic and already cover the monotonic transition. No existing
  row, arm or semantic changes; the dependent capture/activation producer ships in a separate SHA
  after a green migration/watchdog of this checkpoint.
- **Hot tariff override schema checkpoint:** migration `0036` expand-only creates the empty
  append-only `pricing_tariff_overrides` authority so a tariff family price vector can be
  republished as data without a recompile/redeploy. Compiled `metering` constants are the implicit
  version 1 of each family; every override row carries version >= 2 in a strict per-family
  sequence (trigger-enforced), an `effective_from` priced-timestamp bound, a canonical
  `sha256:v2` payload digest and operator attribution, and is never updated or deleted — a
  correction is a newer version. The old runtime neither reads nor writes the table; the dependent
  resolver/writer ships in a separate SHA after a green migration/watchdog of this checkpoint.
- **Hot tariff override family format checkpoint:** migration `0037` expand-only widens the
  `tariff_family` CHECK of the 0036 table to admit the dot (`^[a-z0-9][a-z0-9/._-]{0,127}$`):
  canonical model ids carry version dots (`gemini-2.5-pro`, `gpt-5.6-sol`, `glm-5.2`), so no
  per-model override family could ever be inserted under the original rule. Every row valid
  under the old rule stays valid; the append-only and sequence triggers are untouched, and the
  old runtime still neither reads nor writes the table. The dependent resolver/writer ships in a
  separate SHA after a green migration/watchdog of this checkpoint.
- **Pricing release v2 runtime foundation:** the PostgreSQL resolver reads the head, assignment, policy,
  catalog/switch gates and rule precedence `model → provider → global` in one snapshot; service
  `meter_only` bypasses the product catalog but keeps the provider master-switch. Reserve re-resolves
  the exact head under `request → funding-account → owner` locks, and atomically writes the reservation,
  the immutable pricing snapshot and bonus-first pricing funding allocations. Once the head appears, new
  legacy-format reserves fail closed, but exact old request IDs replay through their previous writer.
  Outbox/settlement selects exactly one funding format by snapshot: release-v2 does not require a
  pre-cutover funding snapshot and writes exact paid/bonus ledger allocations. An unfinished release
  settlement requires the pinned generation to still be active; after a monotonic advance
  only exact terminal replay is allowed, without repeating the money mutation. The provider adapter passes the already
  computed customer debit; registry verifies the provider, non-negative usage and the
  `hold+$1` ceiling, but does not recompute the debit from full official usage (Codex may honestly limit the billed
  output). The runtime itself does not invoke the activation producer: until a separate protected commerce consumer,
  the head remains absent. The PostgreSQL-only public image smoke reader is deliberately narrower than
  normal key CRUD: it resolves the unique engine account handle `crm-parsing` without comparing its opaque
  account ID to an external service ID, and requires the active assignment plus its linked policy to remain
  canonical service/meter-only with the OpenAI master switch enabled. It selects exactly one active
  unexpired key. The release policy's service owner metadata authorizes the account class but is not reused
  as the engine account identity. The reader returns the raw key only in a non-`Debug`/non-serializable
  process-local type and exposes separately a secret-free exact request snapshot/reservation/outbox/usage
  settlement report. A second PostgreSQL-only diagnostic reads reservation, snapshot, outbox, usage and
  principal presence independently in one repeatable-read/read-only transaction, then returns only bounded
  states, numeric usage/cost and canonical-identity booleans; it never returns request/account/key identity
  or raw errors. Neither reader creates or mutates a key, account, release, reservation, usage row, or
  balance.

**Invariants:**
- The token is resolved from the `token` column (inline) OR the `token_file` file. `import_sqlite` refuses a
  cutover while anonymous aggregate reservations remain and reconciles account totals before commit.
- Tokens/proxies are secrets: never log them; `list()` returns only a token-presence flag.
- The tariff (`plan`: pro|max5|max20) is STORED here (`set_plan`, column `plan`) but NOT detected —
  detection is network-bound, lives in `forward::detect_plan`, called from `server`. `get_creds` returns
  (token, proxy) for this detection.

**Verification:** `cargo test -p registry`; real PostgreSQL matrices use
`CLAUDE_API_TEST_DATABASE_URL=... cargo test -p registry pg::tests::stage2_fault_matrix` and
`CLAUDE_API_TEST_DATABASE_URL=... cargo test -p registry pricing::postgres::tests::postgres_pricing_contract_matrix`.
