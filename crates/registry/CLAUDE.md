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
  `key_account` (JOIN key→account for authorization) returns that ID together with the account/key
  financial and policy state in its existing single SQLite/PostgreSQL statement and snapshot; callers
  must keep using the raw key for reserve/settle. Wrapper `Billing` (Mutex<Conn>). Cost computation
  (tokens→nano) does NOT belong here — that is `metering`; registry accepts the ready amount. **Money
  invariant (with overdraft buffer):** reserve keeps the balance FLOOR at −$1 (`OVERDRAFT_NANO`, in sync with
  `metering::OVERDRAFT_NANO`) — a funded request is NOT dropped with 402 due to a race of concurrent reserves; below the floor
  any positive hold is rejected (max $1 in balance debt per account). Settle preserves the REAL
  amount even when `actual > hold`, but the account-row lock collects only what leaves the shared
  balance at or above −$1. If an explicit adjustment already recorded deeper debt while a request
  was in flight, settlement uses that pre-settle balance as its floor: it cannot worsen the debt,
  but it still collects the existing hold instead of refunding it as false pool loss. The remainder
  is pool-funded evidence:
  `actual_nano = collected_nano + uncollected_nano`. Account/key `spent_nano` and usage carry full
  actual; ledger `amount_nano` is that same full billed actual and `uncollected_nano` is its
  pool-funded subset. Customer balance movement is therefore `amount_nano - uncollected_nano`.
  Conservation is `balance + spent + reserved - uncollected = topup/adjust funding`. A future top-up
  is not auto-debited for old shortfall because registry cannot reconstruct its funding class.
  `reservations.request_id` is the ownership key;
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
  is a fingerprint field only, never provider inference. SQLite and PostgreSQL reserve semantics use
  the same shared `ACCOUNT_OVERDRAFT_NANO` floor; the audit backend must not reject a request that
  PostgreSQL would admit.
  Optional per-key `spend_limit_nano` and `expires_ts` are engine-authoritative. Reservation updates
  key `reserved_nano` in the same transaction and enforces
  `spent_nano + reserved_nano + hold <= spend_limit_nano`; settlement atomically converts the hold
  into actual spend or releases it. Mutable policy replacement is account-scoped and atomic with
  reservations; a non-null new limit must cover both settled and reserved usage.
  Soft migration of the old "key=wallet" model → account per-key (`migrate_legacy_keys`).
- **Gemini Batch authority (migrations `0055`–`0058`) is PostgreSQL-only; SQLite returns typed unsupported.**
  It owns isolated job/item/blob/file/outbox/profile-lease authorities plus normalized item→file
  references. Request, metadata and result bytes have only opaque `bytea` ciphertext storage; files
  use bounded encrypted chunks so the later 2 GiB logical contract never requires one giant value.
  Mutable job counters are absent and future `batchStats` must be derived from item rows. Batch rows
  retain non-secret creator `key_id` attribution without a foreign key to deletable API-key rows;
  nullable ledger/usage `key_id` preserves attribution after deletion. Result expiry starts only at
  completion, and outbox rows can carry the complete immutable Gemini calibration event. Migration
  0057 gives file rows an explicit `inline_legacy|chunked` storage shape without fake blob bytes.
  Migration 0058 adds a second profile-lease table without changing schema-57 slot 1; its promotion
  trigger moves a surviving slot-2 claim into slot 1 after deletion so an old rollback runtime can
  still renew/reconcile/settle it. The dependent runtime may therefore admit exactly two Batch items
  per active profile while interactive Gemini traffic remains unchanged.
  Registry exposes atomic create/read/file/claim/cancel/settlement/prune primitives plus one
  identity-free operational report. That report is read in one PostgreSQL `REPEATABLE READ`
  transaction and owns the closed current state plus rolling `1h|24h|7d` aggregates; it never
  returns account/job/item/model/profile identity or customer content. Interactive
  and batch APPLY share one private transaction-local account collection primitive, and provider-turn
  recording likewise has one transaction helper used by its public wrapper and batch APPLY. Exact
  calibration replay never advances spend twice; conflicting replay rolls back; subject tracking uses
  `LEAST(tracking_started_ts)` and `GREATEST(updated_ts)` for out-of-order completion. Batch drains
  isolate every row: failures persist attempts/error plus bounded exponential backoff, permanent
  failures become `failed`, and later rows continue. A `done` replay validates terminal item,
  ledger/usage/calibration evidence before returning the balance. Stage 3
  claim reads return the encrypted execution payload under the same owner/generation fence, blob and
  chunk reads stay account-scoped, chunk pages are ordered and capped at 128, and secret-bearing types
  redact ciphertext/nonces/digests from `Debug`. Completed jobs remain explicit as `Expired` metadata
  after result expiry while result/error payload reads close. Output-file linkage accepts only active
  same-account `batch_output` files and extends file expiry through the job result lifetime. Dispatch
  ignores stored priority, requires the account and creator key to remain active/unexpired, and caps
  each account at 16 claimed/dispatching/settlement-pending items. SQLite returns typed unsupported.
  No public HTTP route, env switch, scheduler loop or execution transport is composed here.
- **Request-observability S2 (migrations `0053` + `0054`) is PostgreSQL-only and opt-in.**
  Fact-aware reservation and delivery methods insert/validate admission and first-delivery evidence in
  the owning money transaction. Terminal evidence is durably enqueued with settlement, then copied to
  the fact only during authoritative outbox APPLY; `billing_outcome` is derived there and is never a
  caller/outbox field. Reconciliation synthesizes only honest unknown/not-started/interrupted evidence.
  The bounded terminal batch is for a separate low-priority connection and always writes
  `not_applicable`; callers cannot supply a billing outcome and it never runs on the money FIFO.
  Legacy methods remain fact-free wrappers and SQLite money behavior is unchanged. Runtime terminal
  commits update only compile-bounded in-process lifecycle counters after the PostgreSQL transaction
  commits; a separate bounded read counts facts still nonterminal after one hour for operations.
  There is still no private analytics read API. Lifecycle maintenance prunes request facts first under
  the existing validated 30-day cutoff.
  Contract and staged rollout — `docs/engine/REQUEST_OBSERVABILITY.md`.
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
- **Settlement-floor accounting runtime and mixed-version fence (migrations `0047` + `0048`).**
  The old-writer-compatible schema is live: lifetime
  `accounts.uncollected_nano`; per-reservation `collected_nano + uncollected_nano = actual_nano`;
  immutable ledger/usage shortfall; and nullable reserve-time provider/multiplier plus settlement
  charge-basis pins. Migration `0048` blocks a draining pre-runtime binary from increasing spend
  while crossing the shared −$1 floor (or worsening deeper balance debt already recorded by an
  adjustment), and requires a provider/multiplier pair plus collected/uncollected evidence before
  any priced reservation becomes terminal. An old both-null reservation remains compatible; an old
  writer touching a newly priced reservation fails its entire transaction with a retryable fence and
  leaves the outbox pending for the current runtime. Every metered provider call pins its provider
  and payable multiplier in the reservation transaction; an admin edit while the request is in
  flight affects only the next admission. Terminal settlement caps only balance collection at the
  shared account floor, keeps full billed usage in `actual_nano`/key and account spend, and records
  every difference explicitly as uncollected. If an explicit adjustment already recorded deeper
  debt, that pre-settle balance is the floor: settlement cannot worsen it and still consumes the
  in-flight hold instead of forgiving it as false shortfall. Charge ledger `amount_nano` remains
  full billed actual and `uncollected_nano` is its pool-funded subset, so consumers derive customer
  balance movement by subtraction and never treat shortfall as funding or commission basis. A
  zero-multiplier request has `actual_nano=0`, writes no ledger charge row, and still writes its
  authoritative usage with the pinned provider/multiplier. Exact replay validates the stored
  collection equation and never increments shortfall twice. Legacy reservations written before the
  runtime may have both collection fields NULL, but a half-populated pair is invalid. Silently
  lowering actual usage, charging a later top-up, or reviving retired funding tables is forbidden.
- **Account discounts (`account_provider_discounts`, migrations `0043` + `0046`)** — the entire pricing
  policy. An account carries one default multiplier (`accounts.mult_bp`) and, for a provider whose
  terms differ, one override row; `key_account` returns both, and the caller resolves them with
  `KeyAuth::mult_for(provider_id)`. The hot authorization read joins key, account and every bounded
  override in one database statement/snapshot; no TTL cache may delay a pricing edit. Writes are
  `set_account_provider_discount` / `clear_account_provider_discount`, bounded to `0..=10000` bp
  and to the five engine provider ids;
  PostgreSQL enforces that same closed set (`anthropic|openai|google|kimi|glm`) in the table; the
  `zhipu` word in migration 0043's historical comment was never a runtime provider id. Fresh
  SQLite audit databases carry the same provider/range checks in their CREATE TABLE; old snapshots
  retain writer validation because `CREATE TABLE IF NOT EXISTS` cannot retrofit a CHECK.
  A write is live on the next authorization. Model — `docs/commerce/PRICING_MODEL.md`.
- **Retired pricing machinery.** The per-account policy/binding tables, the immutable
  catalog/switch/policy versions, release-v2, the funding buckets/lots and the shadow evaluation
  authority are no longer read by any runtime path. They were removed on 2026-08-09: strict
  admission funded from `funding_buckets` while normalization wrote `funding_lots_v2`, so 166 of
  168 strict accounts resolved zero available funds and every request was refused with 402 while
  the balance was intact. Their exact 31-table manifest remains immutable until the retention
  boundary after `2026-09-09 09:26:32 UTC` and every rollback/watermark/dependency/backup/health gate
  in `docs/ops/PRICING_RETIREMENT.md` passes; nothing may start reading it again. Money has one
  authority (the account balance) and price has one (the account discount). Migration `0044` drops
  the constraint triggers that policed those structures
  on every money mutation, and `0045` drops the two `engine_instances` triggers that gated the
  owner lease on them (`engine_instances_policy_runtime_floor` from 0017 while any binding was
  strict, `engine_instances_release_v2_epoch_fence` from 0025 while a release head existed). No
  binary publishes those runtime pins any more, so both had become a permanent refusal of every
  blue-green cutover; their columns stay nullable so a draining peer on the previous binary keeps
  claiming its lease.
- **Hot tariff override runtime authority:** `pricing::tariffs` is the PostgreSQL-only
  read/write side of the 0036 table (migration `0037` widens the family CHECK to admit the dots
  that canonical model ids carry; the old runtime still neither reads nor writes the table).
  `postgres_list_tariff_overrides` reads the whole tiny table ordered by (family, version) and
  recomputes every canonical `sha256:v2` payload digest, failing closed on mismatch.
  `postgres_insert_tariff_override` validates the family format and the strict per-family-prefix
  payload schema (mirror structs of the `metering` price types — registry does not depend on
  `metering`; `server`/`forward` convert), rejects floats and negative legs (i128 money legs are
  canonical decimal strings in the payload JSON), computes the digest itself, owns `created_ts`,
  enforces `effective_from >= created_ts - 60s` except for a family's seed row (version 2 may
  carry `effective_from = 0`), and relies on the 0036 triggers for sequence enforcement: exact
  replay returns `Unchanged`, the same key with different content is a typed `Conflict`, a
  sequence gap is a typed `SequenceViolation`. The pure `resolve_tariff_override` picks the
  greatest version with `effective_from <= priced_ts`. SQLite has no entry points, exactly like
  the release-v2 producer. Real-PG gate: `pg::tests::tariff_overrides_postgres_matrix`. The
  runtime consumption (process-wide tariff book, reserve pinning, settlement replay) lives in
  `crates/forward` — contract in `crates/forward/CLAUDE.md`.
- **Public image smoke reader (PostgreSQL-only).** Deliberately narrower than normal key CRUD: it
  resolves the unique engine account handle `crm-parsing` without comparing its opaque account ID
  to an external service ID, and selects exactly one active unexpired key. The reader returns the
  raw key only in a non-`Debug`/non-serializable process-local type and exposes separately a
  secret-free exact request snapshot/reservation/outbox/usage settlement report. The temporary
  PostgreSQL diagnostic for the two fenced 2026-08 GPT Image 2 withdrawals was removed after both
  immutable deployment statuses recorded terminal settlement evidence; no current registry path
  reads `pricing_request_snapshots_v2`. The smoke reader never creates or mutates a key, account,
  release, reservation, usage row, or balance.

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
