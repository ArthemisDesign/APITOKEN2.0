# DEPENDENCIES.md — map of project relationships

A single map of all relationships between bounded contexts and components: who produces,
under which contract, who consumes. When you change something at a boundary — first find
the relationship line here, then act per `docs/CHANGE_CHECKLISTS.md` and the contract
protocol from the root `AGENTS.md`.

**Maintenance rule (part of the "living contract"):** a new cross-context relationship, a
new consumer of an existing relationship, or a new domain/service = a new line in this file
IN THE SAME commit + a line in the `docs/README.md` index for new contract documents.
Changed a contract — update both the map line and the contract document. A line that does
not match the code is a bug-level defect; if a relationship disappears, the line is
deleted, not kept "for history".

## 1. Contracts between contexts

Format: producer → contract/channel → consumers. The contract document is where the
relationship is described in subject terms.

### Engine Control API (engine → commerce, OpenKeys)

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `crates/server` (`src/http.rs`, `src/admin.rs`) | HTTP `/admin/*` under `x-api-key: CLAUDE_API_CONTROL_KEY`; routes only in Combined/Anthropic modes | `packages/engine-client` — the only client; direct calls to `/admin/*` outside it are forbidden | `docs/engine/CONTROL_API.md` |
| `crates/server` + `crates/forward` + `crates/registry` locked-OpenKeys producer | `POST /admin/pricing/policy/{account_id}/locked-openkeys-transition`: strict exact request, atomic immutable successor insert + binding CAS, only managed provider-level 1:1 rules, fixed `shadow + legacy_single + verified` target; generic replacement lock remains intact | consumers wired after GREEN exact producer SHA: strict `packages/contracts` → typed `packages/engine-client` → durable `packages/db` shadow-rollout store → bounded `apps/worker` delivery; the only staging producer is the AdminGuard `POST /v1/admin/pricing-shadow-rollout-v2/stage` in `apps/api`; no direct callers outside the durable lane | `docs/engine/CONTROL_API.md`, `docs/commerce/MULTI-DISCOUNT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE7.md` |
| `packages/engine-client` | TS client `EngineClient`, strict zod validation from `@claude-api/contracts`, money amounts as `json-bigint` strings; pricing v2 provisioning-context/cursor/prepare/readback and the single canonical Stage 5 policy/assignment digest builder | `apps/api`, `apps/worker`, `apps/openkeys`; `packages/db` Stage 5 materializer, Stage 8 collector, and pre-delivery activation authority (env `ENGINE_BASE_URL` + `ENGINE_CONTROL_KEY` only on runtime consumers) | `docs/engine/CONTROL_API.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE7.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md`, `docs/product/OPENKEYS.md` |
| `claude-api db stage8-evidence` (`crates/registry`, `crates/server`) | protected schema-v2 JSON artifact with signed-i64 nanoUSD and canonical Rust `sha256:v2` evidence digest; exact target/recovery, full engine inventory/funding/shadow/runtime floor | parity/diagnostic non-production input for `packages/db`; production no longer uses an SSH/file handoff | `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI-DISCOUNT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md` |
| `crates/server` + `crates/forward` Stage 8 capture producer | protected `POST /admin/pricing/v2/stage8-evidence/capture`; strict explicit inputs, server-owned compile-fixed manifest, unwrapped schema-v2 report including `passed=false`; PostgreSQL bounded reader only | after GREEN exact producer SHA: strict `packages/contracts` → raw-text/`json-bigint` `packages/engine-client` → `apps/worker`; exact raw engine bytes are durable before `packages/db` combines commerce/service and two exhaustive OpenKeys scans | `docs/engine/CONTROL_API.md`, `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI-DISCOUNT_STAGE9.md` |
| `crates/server` operator routes + `crates/authbot` proxy lifecycle | read-only `/overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health /glm-subs` (→ 8790; GLM is a backend inside the Anthropic runtime), `/codex-subs` (→ 8792), `/gemini-subs` (→ 8794), `/kimi-subs` (→ 8803, a dedicated default-off KIMI plane; the Anthropic-embedded gateway is dev/test only), plus authbot `GET /proxy-admin/inventory` and explicit idempotent `POST /proxy-admin/renew` (→ loopback 8806) via Caddy `admin.apitoken.sale`; Caddy injects `ADMIN_CONTROL_KEY` and the verified actor, while authbot exposes only opaque proxy/subscription projections, IPRoyal nanoUSD balance and manual allocation-level renewal. All new IPRoyal orders have auto-extend disabled and the background guard disables it on every existing order without performing a paid renewal | `apps/admin` (no engine-client and no secrets of its own); `/metrics` is also scraped by Prometheus directly over loopback, bypassing Caddy (`observability/prometheus/prometheus.yml`), including the KIMI origin 8803 with the target label `provider: kimi` | `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md`, `crates/authbot/CLAUDE.md` |

Control API endpoint groups: accounts, credit/ledger (idempotent credit by
provider-qualified `ref`, cursor protocol `ledger` + `ledger/ack`), usage, keys, versioned
pricing (catalog/switches/policy, including the narrow atomic locked-OpenKeys transition),
and PostgreSQL-only release-v2 prepare/read/activation under `/admin/pricing/v2/*`.
Legacy ledger provider recovery stays producer-owned: first the exact immutable
`usage_events.provider` of the same `account_id + request_id` pair is used; for
request-less history only a strict account/key/amount/ref/model/time settlement
fingerprint with a single non-empty provider across all candidates is acceptable. Conflict
is fail-closed, ambiguity stays unknown, model-name inference is forbidden. After GREEN
producer SHA, commerce in a separate consumer commit re-selects old terminal sentinel rows
by recovery version `2`, raises the exact evidence, and does not endlessly rescan a
fruitless algorithm. Strict fingerprint producer SHA
`d5f3d6bccdaa5015a443500d2530f1430596362b` received GREEN `deploy/watchdog` before this
commerce consumer was wired.
The release-v2 producer publishes immutable policy/release/recovery prepare, the full
engine inventory, nullable head, account-local funding normalization plan/apply, and an
append-only assignment extension for the exact active/recovery pair of an account created
after cutover. The read-only Stage 8 capture returns the same blocker-preserving report
as the CLI through a bounded PostgreSQL reader and does not stage collection/activation
work. Read-only
`GET /admin/pricing/v2/provisioning-context` publishes in one snapshot the exact
head/audit/evidence, active release lineage, and only evidence-selected recovery; before
cutover it returns `null`, and on authority divergence it fails closed. It is a producer
for independent OpenKeys/service provisioning and does not require those contexts to have
access to commerce-local activation tables. The only activation producer accepts fresh
combined evidence, re-verifies engine inventory/funding/runtime owner epochs, and
atomically writes audit + singleton head; cutover/recovery do not update accounts or
money rows. Funding apply is serialized with money writers and does not require a global
drain. After the green exact producer SHA, `packages/contracts` validates the strict
release/funding wire shape, and `packages/engine-client` is the only typed transport
consumer. `apps/worker` via `packages/db/src/funding-normalization-jobs.ts` implements a
separate bounded/resumable Stage 6 application consumer: exhaustive cursor scans, exact
service exclusion, fresh GET before every account-local POST, exact full welcome
revocation → paid-only current aggregate, fail-closed partial/mismatched revocation,
exact paid-only adoption of active legacy reservations without changing their pricing
snapshot, fail-closed ambiguous welcome reserve, full-coverage parent confirmation,
identical target/recovery funding evidence, and prepare+readback of both
releases/recovery link. Job staging/status is bound to the exact Stage 5 plan digest. The
production producer is the AdminGuard-protected `apps/api` endpoints for Stage 5 dry-run /
materialize and Stage 6 status / stage: they require a verified `x-admin-actor`, exact
plan digest, meaningful mutation reason, strict `packages/contracts` response, and
attributed transactional audit. The DB package CLI remains a non-production diagnostic
and is not permission for manual SSH. The presence of transport methods or a runner
without an explicitly staged job does not start a backfill, does not create Stage 8
evidence, and does not activate a release.
After the green assignment-extension and provisioning-context producer SHA, consumers use
only the chain strict `packages/contracts` → typed/canonical `packages/engine-client`.
Commerce key issuance proceeds through `packages/db/src/pricing-provisioning-v2.ts`;
OpenKeys issuance and service-account admin CAS use the shared external-owner builder
directly. With a non-null context, balance writers complete funding/policy/active+recovery
extension, the service writer completes a rule-free `meter_only` policy/extension, and all
of them require exact readback plus a fresh context before a usable result. With a null
context the release-v2 path is dormant. After the green policy-override producer SHA
(engine migration 0030), an operator B2B policy CAS (`PATCH /v1/admin/business-users/:id/pricing`)
also calls `packages/db/src/pricing-provisioning-v2.ts` `syncPricingReleasePolicyOverrideV2`:
for a base-covered account it prepares a strictly newer release policy version and pins it
through the append-only assignment extension under the exact current head, returning the
outcome additively in the CAS response.
The managed Stage 8 capture is wired through the chain strict `packages/contracts` →
raw-text `packages/engine-client` → `packages/db/src/pricing-stage8-capture-jobs-v2.ts` →
`apps/worker`. The only job producer is the AdminGuard-protected
`POST /v1/admin/pricing-stage8-capture-v2/stage` in `apps/api` with a UUID idempotency
key, verified actor, reason, and exact capture bounds; the paired GET returns a bounded
local job/artifact snapshot. The worker persists exact engine bytes before the combined
collector and atomically completes the combined artifact/job; the GET exposes only
freshness and a sanitized blocker source/code/count with hashed subjects. Engine subjects
keep canonical `sha256:v1`, commerce authority subjects keep canonical `sha256:v2`; the
combined/status schema accepts both opaque versions without extending the evidence
identity versions. OpenKeys first-delivery authority comes from the prepared target 1:1
policy, not from the pre-cutover legacy source/engine scalar.
Startup, migration, polling, and an activation request do not create a capture job;
capture does not create an activation job and does not move the head. After GREEN commerce
producer SHA, `apps/admin` is wired in a separate consumer checkpoint: `/pricing` shows a
bounded queue/artifact snapshot and stages a new job only after an explicit exact-bounds
form, a confirmation phrase, and a fresh browser preflight.
Activation is wired only through the chain strict `packages/contracts` → the single
transport `packages/engine-client` → `packages/db/src/pricing-release-activation-jobs.ts`
→ `apps/worker`. The DB consumer builds the request from persisted passed evidence and
engine release digests, stores the body before the network, repeats the double-scan
engine/OpenKeys and commerce/service ownership authority before the first delivery, and
after a possible delivery restores a lost ACK only by exact replay and stores the full
validated receipt. The persisted service-inventory digest is mandatory and must match a
fresh capture; old evidence rows with `NULL` are not staged. Raw identities never leave in
a blocker/error artifact. The recovery expectation comes only from the durable cutover
receipt. Startup, migration, the Stage 8 collector, and worker polling do not stage an
activation job. The only producer is the protected
`POST /v1/admin/pricing-release-activation-v2/stage` in `apps/api`; the paired GET returns
a bounded local snapshot and separately a timestamped engine head. `apps/admin` is wired
to this expand-only contract in a separate consumer commit after GREEN producer SHA:
`/pricing` shows a bounded snapshot and fail-closed stages only explicit cutover/recovery
after a fresh browser preflight.
Stage 7 durable shadow rollout (migration 0035) is wired through the chain strict
`packages/contracts` → typed `packages/engine-client` (including
`lockedOpenkeysPolicyTransition`) → `packages/db/src/pricing-shadow-rollout-jobs-v2.ts` →
bounded `apps/worker` `PricingShadowRolloutWorkerService`. The only rollout producer is
the AdminGuard-protected `POST /v1/admin/pricing-shadow-rollout-v2/stage` in `apps/api`
with a UUID idempotency key, exact `stage5_run_id`, verified actor, and reason; the paired
`GET /v1/admin/pricing-shadow-rollout-v2` returns a bounded snapshot with subject digests
without raw account identity. Staging in a single `SERIALIZABLE` transaction pins the
exact prepared target/recovery pair, cross-checks the fresh engine inventory digest
against the Stage 5 run, and fails closed on drift/collision/missing owner before writing;
the worker claims per-account jobs with a lease, delivers generic `policy_shadow`
(prepare → exact readback → activate) or the replacement-locked
`locked_openkeys_transition`, stores the exact ACK digest/payload, and atomically closes
the rollout `confirmed|blocked|dead`. Startup, migration, polling, and the read endpoint
do not create a rollout/job; the lane does not move the release head, balances, or the
live price.

### Sales feed (commerce ↔ sales)

A bidirectional perimeter under one key `SALES_CONTROL_KEY` (header `x-api-key`).

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `apps/api` (`src/sales-feed.controller.ts`, `/v1/internal/sales/*`) | GET feeds `attributions` / `usage-events` / `topups` (cursors `after_id`); usage-events emits schema v1 (policy_v1 track) and schema v2 (release_v2: exact referred-B2C `paid_funded_nano` regardless of pricing mode + `officialNano`/`chargedNano`/`bonusFundedNano`/`otherFundedNano`/`releaseGeneration`/`releaseDigest`, `pricingMode=null`); `referral-discount` lives only on the producer-first transition | `apps/sales-api` (`sync.service.ts` — dual consumer delivered: routing by event shape into the v1 writer `recordReferredSpend` or the v2 writer `recordReferredSpendV2` from `packages/sales-db`, readers aggregate both schemas; `commerce.service.ts`; `COMMERCE_BASE_URL`) | `docs/sales/SALES_PORTAL.md` |
| `apps/sales-api` (`src/internal.controller.ts`, `/v1/internal/*`) | POST `promo/redeem` is kept for credit/attribution; `partners/referral-discount` and the discount fields are legacy compatibility until the tier-linked personal price is removed | `apps/api` (`promo.service.ts`, `auth.service.ts`; `SALES_API_URL`) | `docs/sales/SALES_PORTAL.md` |

The feed types are duplicated as local zod schemas on both sides; they are not factored out
into `packages/contracts`. Any feed change edits both sides — see the contract protocol in
`AGENTS.md`.

### Other cross-context relationships

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `packages/contracts` | zod schemas of the engine/pricing/auth/checkout contracts, canonical models, and catalog pins; target pricing — global B2C/provider/model rules, B2B, OpenKeys 1:1, service `meter_only`, pricing releases; strict Stage 5/6 admin request/status summaries; engine ledger attribution accepts the expand-only `release_v2` snapshot kind, the `global` rule scope, release lineage fields, and v2 lot funding evidence (`docs/engine/CONTROL_API.md`), the commerce release-v2 writer is wired in a separate checkpoint after its migration | `apps/api`, `apps/worker`, `apps/openkeys`, `packages/db`, `packages/engine-client`. Do NOT import: `apps/web`, `apps/sales-*`, `apps/admin` | `docs/commerce/MULTI-DISCOUNT.md` |
| `apps/api` (public API) | HTTPS `backend.apitoken.sale/v1/*`, cookie session | `apps/web` (`src/lib/api.ts`, `NEXT_PUBLIC_BACKEND_URL`) | `docs/commerce/COMMERCIAL_BACKEND.md` |
| `apps/api` (paying/spender admin producer) | Read-only `GET /v1/admin/finance/paying-users` via the Caddy rewrite `admin.apitoken.sale/admin/*`: omitted funding and `payments|manual|bonus|all` preserve their existing lifetime-money/strict-bonus semantics; additive `spenders` selects every positive `pricing_usage_events` spender in 1/7/30d and labels zero-money non-strict rows `spend_only`; page/count/summary share one half-open cutoff in read-only `REPEATABLE READ`; omitted/false `include_usage` stays DB-only and exposes no engine IDs/usage, while literal `include_usage=true` performs page-wide concurrency-four, five-second abortable usage calls for every distinct event account and adds a minimal exact `(provider, model)` aggregate with safe counters serialized as decimal strings, exact nanoUSD strings, and explicit `complete|partial|unavailable` coverage—never full EngineUsage/account/key/daily detail or provider inference | `apps/admin` (`/paying-users`) consumes `funding=spenders&include_usage=true` by default, with expandable exact provider/model coverage and one-row-per-user×provider×model CSV; wired only after GREEN exact producer SHA `d27033effc237156bce91a38d1ca0ff5b6e66cbd`. Additive bonus was consumed after GREEN `b12a08fe872fb08a88943d7ade0a75a3e567b579`; the original consumer after GREEN `ce92503d1adc0e31967b2dda5853ce05ed480048` | `docs/commerce/COMMERCIAL_BACKEND.md`, `docs/product/ADMIN_PANEL.md` |
| `apps/api` (admin API) | `/v1/admin/*` via the Caddy rewrite `admin.apitoken.sale/admin/*`, header `x-admin-key`; protected Stage 5 dry-run/materialize and Stage 6 status/stage require a verified actor, exact plan digest, and audit mutation reason; exact pre-cutover `/pricing-policy-delivery-repairs` supersedes only a proven dead `strict + legacy_single` job and creates a new audited shadow delivery; `/pricing-catalog-jobs/stage` and `/pricing-switch-jobs/stage` stage delivery for exactly the stored immutable version (no wire payload, exact replay is idempotent) and move only commerce heads; per-service CAS `/service-account-inventory/*` cross-checks the full engine inventory and after cutover completes the exact `meter_only` release-v2 policy/extension before durable registration; the same channel and key on `content-studio.apitoken.sale/v1/*` | the future standalone `apps/admin` Stage 5/6 UI consumer is wired only after GREEN producer; `packages/db` materializer/orchestration; engine Control API — via typed `packages/engine-client`; `apps/content-studio` (`/v1/admin/content/*`) | `docs/ops/DEPLOYMENT.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE6.md`, `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md` |
| `apps/openkeys` (admin API) | `/api/internal/admin/*` via Caddy `admin.apitoken.sale/openkeys-admin/*`, headers `X-OpenKeys-Control-Key` + verified actor; additive read-only `GET /paying-keys` returns every non-removed warehouse or delivered key with explicit `stock|delivered` lifecycle and nullable delivery time, exact nullable lifetime engine spend, global `spent|nominal|created|delivered|status` + `asc|desc` server sorting, bounded batch account reads, page-only live usage for `1|7|30` days, exact nanoUSD/model counters, and row-local unavailable status without secrets | `apps/admin` (`/paying-users`, OpenKeys cohort) consumes sorting/lifetime spend only after GREEN exact producer SHA `65f2160f67f8662ec58fbf336444c0ca8b5ff76a`; warehouse+lifecycle consumption followed GREEN `11aec1b731a5b31b057641982957aa0142eaacf2`, and the original delivered-only consumer followed GREEN `558d4b34896792cfaed5760852f9001feb0d0443` | `docs/product/OPENKEYS.md`, `docs/product/ADMIN_PANEL.md` |
| `apps/openkeys` (pricing inventory producer) | loopback/internal GET `/api/internal/pricing/v2/inventory`, bounded cursor + full `sha256:v2` manifest under `X-OpenKeys-Control-Key`; no secrets/live money | `packages/db` Stage 5/Stage 8 consumers and the activation first-delivery preflight exhaust the cursor twice and require one unchanged full-manifest digest; wired only after GREEN producer SHA | `docs/product/OPENKEYS.md`, `docs/commerce/MULTI_DISCOUNT_STAGE5.md`, `docs/commerce/MULTI_DISCOUNT_STAGE9.md`, `docs/ops/DEPLOYMENT.md` |
| `apps/sales-api` (public + admin API) | `partners.apitoken.sale/v1/*`; `/v1/admin` via Caddy `admin.apitoken.sale/partner-admin/*`, header `x-sales-admin-key` | `apps/sales-web`; `apps/admin` | `docs/sales/SALES_PORTAL.md` |
| `packages/payments` | provider adapters: Platega (default) and Cryptomus — live; DigiSeller — registered but disabled for customers (no entry point, status in the document); webhooks `POST /v1/payments/{platega,cryptomus}/webhook` in `apps/api`; reconcile polling in `apps/worker` | `apps/api`, `apps/worker` (the only consumers) | `docs/commerce/PLATEGA_INTEGRATION.md`, `docs/commerce/CRYPTOMUS_INTEGRATION.md`, `docs/commerce/DIGISELLER_INTEGRATION.md` |

### Devbot (`apps/devbot`)

The Telegram dev bot is a consumer of observability and deploy-perimeter signals; it touches
no database and uses the engine Control API read-only. It has its own release lane
`deploy/devbot` (`/opt/apitoken/devbot-releases`), loopback port `127.0.0.1:3800`.

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| Alertmanager (`observability/alertmanager/alertmanager.yml.template`) | webhook `POST http://127.0.0.1:3800/alerts/{DEVBOT_AM_SECRET}` — receiver `devbot-telegram`, route with `continue: true` next to the email tree (expand-only); the block renders only with a provisioned `DEVBOT_AM_SECRET` from `/etc/apitoken/devbot.env` | `apps/devbot` | `docs/ops/DEVBOT.md` |
| GitHub API | commit statuses `deploy/*`, deployments `production-*` (read-only PAT) | `apps/devbot` (poller, 30–60 s) | `docs/ops/DEVBOT.md` |
| `crates/server` Control API | readonly/control GET (`/pool`, `/codex-subs`, `/gemini-subs`, `/settlement-health`, `/ready` slots) | `apps/devbot` (bot commands) | `docs/engine/CONTROL_API.md` |
| journald | reading the unit journals of the deploy perimeter (prefixes `[watchdog]`, `[admin-deploy]`, etc.) | `apps/devbot` (stage 3) | `docs/ops/DEVBOT.md` |
| `apps/devbot` | node-exporter textfile `devbot_heartbeat_timestamp_seconds` (`/var/lib/apitoken/monitoring/textfile/devbot.prom`, atomically every 60 s) | Prometheus → alert `DevBotHeartbeatMissing` | `docs/ops/MONITORING.md#devbotheartbeatmissing` |
| `apitoken-affinity-redis.service` | two instances under one unit: history `127.0.0.1:6379` (legacy Compose service identity `affinity-redis`, Codex response history, `allkeys-lru`, 512 MiB) and affinity `127.0.0.1:6380` (service `cache-affinity-redis`, cache-lineage L2 + advisory cooling hints `claude-api:cool:v1`, `allkeys-lru`, 128 MiB); the installer performs an additive Compose reconcile up to the two-target monitoring gate and does not stop history; exporters `9121`/`9122` (job `redis`, label `instance_role`); the password is read from `CLAUDE_API_REDIS_PASSWORD` in `server.env` and published as the JSON secret `observability/secrets/affinity_redis_password` | Prometheus → alerts `AffinityRedisDown`, `AffinityRedisEvictingKeys`, `AffinityRedisMemoryHigh` | `docs/ops/MONITORING.md#affinityredisevictingkeys` |
| `crates/forward` billing writer + `crates/server` `/metrics` | histogram `claude_api_billing_pg_command_duration_seconds{op="reserve\|settle\|acquire_capacity"}` (latency around the retry wrapper, 10 buckets 1 ms–1 s) and gauge `claude_api_billing_write_queue_depth` (occupied slots of the 4096-slot writer channel); both operational (no money labels), visible to the readonly key; PostgreSQL-only, the SQLite fallback does not publish the histogram | Prometheus → alerts `BillingPGCommandLatencyHigh`, `BillingWriteQueueBacklog`; Grafana `production-overview` row "Billing writer (PostgreSQL hot path)" | `docs/ops/MONITORING.md#billingpgcommandlatencyhigh` |

## 2. Inside the engine (briefly)

Layers and invariants — `CLAUDE.md` (layer table) and `docs/engine/ARCHITECTURE.md`. Here
is only what is needed to walk the relationships when making changes:

- **`crates/metering` — the engine's price authority.** Hardcoded effective-dated tables
  in nanoUSD: `src/lib.rs` (Anthropic), `src/codex.rs` (OpenAI text), `src/gemini.rs` (Gemini),
  and dormant `src/openai_image.rs` (GPT Image 2). A price/model change is a reviewable commit
  here. Consumers: `crates/forward` (main), `crates/server` (types/tariff identifiers). The image
  tariff is pure dormant official OpenAI API replacement authority; the private CLI uses it only for
  a conservative plan/authorization estimate. It is not ChatGPT native credits, customer billing,
  settlement, product access or a runtime route.
- `crates/registry/src/pricing/` — NOT a price list, but the durable identities of
  multi-discount: catalogs/switches/policies, admission snapshots
  (`docs/commerce/MULTI-DISCOUNT.md`). Fixed provider IDs actual/shadow contract —
  `anthropic|openai|google`; `gemini` is not a durable authority.
- `crates/forward/src/pricing*` — the pricing-resolver and shadow-evaluation pipeline.
  Live: the resolver is called in the admission path of the strict policy (`proxy.rs`)
  and in Codex billing (`codex/billing.rs`); atomic legacy snapshot producers are in the
  Anthropic, Codex, and Gemini billing paths, and the shadow runtime for all three fixed
  planes starts in production (`crates/server`). It does NOT read the DB and does NOT
  compute token cost.
- **Provider data plane (`crates/forward` → `crates/router`).** The planes produce the
  native and universal HTTP surfaces; the router consumes them only through stable
  loopback origins. In particular, `/v1/messages/count_tokens` is available on all three
  planes and is selected by `model`: Anthropic native, the local reserve-grade Codex
  count, or the quota-free Gemini `:countTokens`. The router preserves the universal
  body, so the plane strips its own namespaced prefix before admission; canonical GPT
  Fast aliases are normalized by the Codex plane. At the compatibility boundary the
  router additionally accepts camelCase `serviceTier:"fast"|"priority"` only for the
  executable GPT Chat/Responses chain, removes the alias, and passes the plane the
  canonical `service_tier:"priority"`; conflicting values and non-GPT/surface misuse are
  rejected before the plane is called.
  After dispatch the planes identically fail-closed validate the optional execution
  controls: missing/null stay absence, while a malformed non-null boolean/output limit
  gets a local 400 before reserve/upstream; output alias precedence and the exact OpenAI
  `error.param` are part of the produced universal contract. Translated SSE is also
  fail-closed at the plane boundary: Anthropic requires the full Messages lifecycle up to
  `message_stop`, Gemini requires `finishReason` or `promptFeedback.blockReason` before
  EOF; a malformed/premature stream becomes a lane-shaped terminal error, not a false
  success. The router does not normalize or fix these fields or events — it preserves
  the request body and the streaming response.
  The Codex Messages skin also accepts and strips only the bounded no-op
  `context_management` of the current Claude Code (`edits:[]` or exact
  `clear_thinking_20251015` + `keep:"all"`), and leaves stateful/unknown forms
  fail-closed; it strips the client's exact ephemeral cache markers in favor of
  automatic Codex caching, without accepting extended retention; its GA
  `output_config.effort` and the bounded
  json_schema `format` it translates into equivalent Responses controls, including the
  structured title request of the current Claude Code. For
  harnesses without arbitrary body fields the router accepts the
  `x-apitoken-service-tier: fast|priority` header only on the executable GPT chain,
  turns it into the body `service_tier:"priority"`, and strips the header itself before
  calling the plane; the Codex plane remains the reserve/settlement/effective-tier
  authority.
  The aggregated catalog stays OpenAI-shaped, except for the authenticated Codex-native
  `{models:[]}` overlay by harness identity. Contract — `docs/engine/UNIFIED_ROUTER.md`.
- **The `x-apitoken-execution-state` contract (planes → router, stage 6.1).** Producers —
  `crates/forward` (`proxy.rs`, `anthropic.rs`, `anthropic_responses.rs`, `codex/api.rs`,
  `codex/skin.rs`, `gemini/api.rs`, `gemini/chat.rs`, `gemini/responses.rs`,
  `gemini/skin.rs`): the
  `not_started` header on non-2xx refusals before the started boundary, with a
  refund/cancel-reserve guarantee; universal adapters preserve only the plane's exact
  signal and strip it from errors after 2xx. Consumer —
  `crates/router`: it always strips the header from all public responses, and on the
  explicit off-by-default `models` chain uses the exact signal for the next serial
  attempt; 401/402/client 4xx (except signed 429) are not retried. The second permitted
  proof is TCP `ConnectionRefused`; timeout/generic connect/unsigned 5xx fail closed.
  Contract document — `docs/engine/ROUTING_FENCING.md` §3.
- **Execution group contract (router → provider planes → registry, stage 6.3).** The
  trusted identity producer is `crates/router`: one CSPRNG UUIDv4 and attempts `1..N`
  only for an explicit fallback chain. `deploy/Caddyfile` removes client copies on all
  provider/router vhosts; the router removes them again before its own injection.
  `crates/forward` validates the pair before reserve and passes it through
  `AsyncBilling`; `crates/registry` stores the identity and atomically selects one
  nonzero settlement winner in the shared PostgreSQL authority (SQLite parity for
  rollback/tests). Consumers of the winner result — money/funding settlement and
  `crates/server` `/metrics`; public APIs do not return group identity. Contract —
  `docs/engine/ROUTING_FENCING.md` §4.
- **ClaudeStore emergency transport (`crates/server` → `crates/forward` → ClaudeStore API3).**
  `crates/server/src/config.rs` solely reads the two strict enable/key pairs, and the
  compile-fixed `https://api3.claudestore.store` cannot be replaced by an env URL.
  `crates/forward/src/proxy.rs` consumes the Claude pair only for metered Anthropic
  `POST /v1/messages`; `crates/forward/src/codex` consumes a separate Codex-tier pair
  only for `gpt-5.5`/`gpt-5.4` via `POST /v1/responses`. After a terminal refusal under
  the regular local pre-byte rotation policy, each transport performs at most one
  sanitized external attempt, preserves the original request/reservation identity and
  the customer exact settlement, but does not write local subscription
  spend/quota/calibration/affinity. Systemd fixed-provider units do not inherit another
  plane's switch. Prometheus consumes fixed-cardinality attempts/successes/failures with
  the target provider label; the failure alert and rollback are described in
  `docs/ops/MONITORING.md#claudestorefallbackfailing`. Contract and evidence —
  `docs/engine/CLAUDESTORE_FALLBACK.md` and `research/CLAUDESTORE_GPT_FALLBACK_EVIDENCE.md`.
- **Private GPT Image 2 live gate (`crates/server` canary CLI →
  `crates/forward::codex::images` → existing Codex OAuth pool).** The transport posts typed JSON to
  `{CodexConfig.base_url}/images/generations|edits` with the existing bearer/account/originator/UA/
  version identity plus `x-codex-image-turn-id`; edit references are one to five PNG data URLs.
  Automatic library calls reuse normal pool selection/`TurnSlot`, refresh once, and rotate only on a
  final pre-execution auth/quota rejection; ambiguous outcomes are never replayed. The CLI freezes an
  explicit or first admitted opaque profile, performs the free `/wham/usage` preflight and one
  exact-home attempt. Generation requires an exact SHA and at least `8_560_000` nanoUSD for fixed
  `opaque/low/1024x1024`; edit remains blocked until a normative input-image ceiling exists. The one-shot
  watchdog controller invokes exact deployed engine SHA `3f67d43c0ae541979fee66823d251e2e3eea33e0`
  before advancing processed/overall GREEN. Evidence is exclusive mode-`0600` and requires exact controls
  plus terminal usage. It introduces no image key/origin/env. There is no `AppState`, HTTP/customer,
  router, catalog, defaults, billing/settlement,
  public-doc or publication consumer. Contract and blockers — `docs/engine/CODEX_PROVIDER.md`,
  `docs/ops/GPT_IMAGE_2_CANARY.md`, and `research/GPT_IMAGE_2_EVIDENCE.md`.
- **Policy preflight contract (provider planes → router, phase 6.4a).** The producer is
  the identical `crates/server::router_policy` on every fixed runtime: an authenticated
  loopback-only `POST /internal/router/policy/preflight` reads the customer key and one
  coherent pricing bundle through `AsyncBilling`, applies the engine-owned resolver, and
  returns only a bounded ordered allow-list. The consumer is `crates/router` (6.4b
  implemented): after preset/catalog/preferences, before attempt 1, with exact
  ordered-subset validation, sequential mixed-version origin failover, and with no
  credential/policy cache and no `forward`/`registry` imports. The public provider
  Caddy vhosts do not include `/internal/*` in the allowlist; the stable origins
  8790/8792/8794 are reachable by the router over loopback. The contract and
  mixed-version failure semantics — `docs/engine/ROUTING_FENCING.md` §5.1.
- **Early auth preflight contract (provider planes → router).** The producer is the
  identical `crates/server::router_auth` on every fixed runtime: a loopback-only bodyless
  `POST /internal/router/auth/preflight` verifies the forwarding-admin/customer
  credential through the same `authed`/`AsyncBilling` resolver as live admission, and
  returns only a closed success marker or 401/503 without reserve, pricing/policy read,
  or identity. The consumer is `crates/router`: before materializing the 32 MiB universal
  body it uses bounded hedged Anthropic → OpenAI → Gemini probes. The first starts
  immediately, later origins start at fixed 50 ms intervals without a conclusive result,
  and an inconclusive response with no useful active probe advances immediately. The first
  exact schema-v1 success or terminal 401 wins; mixed-version/transport/5xx remain
  inconclusive, and no credential/result is cached. Dropping outstanding request futures
  does not guarantee cancellation of provider DB work already accepted. The deployment
  startup probe remains concurrent across all three origins. The fail-fast 64 MiB budget
  with a 1 MiB step grows dynamically with the actual chunked
  bytes, has a 15-second idle and a 5-minute absolute body deadline, and does not create
  an execution queue.
  Contract —
  `docs/engine/UNIFIED_ROUTER.md` §"Early auth and the request-body memory boundary".
- **Catalog pricing contract (provider planes → router).** The producer is the identical
  `crates/server::router_pricing` on every fixed runtime: an authenticated loopback-only
  `POST /internal/router/catalog/pricing` resolves the customer/admin credential, reads
  only one coherent pricing bundle for the strict account, and projects the audited
  `crates/metering` rates through the effective payable multiplier into integer
  nanoUSD-per-million strings. The response contains no key, account, balance, policy, or
  rule identity and reserves/charges nothing. The consumer is `crates/router`: after a
  separate producer-first GREEN SHA it validates version/unit/canonical integer strings
  and ordered subset, filters unavailable models, adds `data[].apitoken.pricing`, marks
  the response `private, no-store`, and fail-closed returns 401/503. Catalogs larger than
  256 candidates are cut into deterministic chunks; a failed chunk closes the entire
  overlay. The credential-specific overlay is not cached and never lands in the shared
  catalog TTL cache. The public provider vhosts do not serve `/internal/*`.
- **Catalog runtime metadata contract (provider planes → router).** Anthropic produces
  native `max_input_tokens`/`max_tokens`/`capabilities`; the owned OpenAI and Gemini
  model resources produce expand-only `apitoken.limits`/`apitoken.capabilities`,
  including modalities, tool calling, structured outputs, and streaming; OpenAI may also
  publish a provider-authored `name`. Codex context/name is last-good authenticated
  provider evidence, aggregated conservatively across serving profiles; the
  output/efforts/Fast/adapter capabilities and the Gemini model-specific metadata belong
  to the reviewed runtime contract. The consumer is `crates/router`: after a separate
  producer-first GREEN SHA it strictly validates and normalizes metadata into the unified
  `apitoken`, keeps top-level capability mirrors, and moves a plane with malformed
  metadata to last-good/degraded. It also strips globally conflicting aliases, keeping
  namespaced IDs executable and the private native ID for rewrite/preflight. The pricing
  overlay complements, not replaces, runtime metadata. Pricing rates, account identity,
  and credentials do not travel through this relationship; unknown values are not derived
  from the model id or pricing tables. Contract —
  `docs/engine/UNIFIED_ROUTER.md` §"Models and catalog".
- **Unified catalog contract (router → OpenCode integration).** `crates/router` produces
  the authenticated key-scoped `/v1/models`: authoritative runtime metadata is
  complemented by a personal pricing projection without changing the original model IDs.
  The consumer is the canonical `packages/opencode-router-plugin`: the live response is
  translated into the OpenCode model/variant/Fast schema, and the local schema-v2
  last-good cache contains only encrypted capability records without `pricing` and
  `cost`, is bound to the exact credential/base URL, and is limited by
  schema/TTL/max-stale guards. The cached fallback is always explicitly stale and
  cost-free. The OpenCode transport does not consume Gemini `inlineData`, so the plugin
  does not advertise generated-image output; the native Gemini API remains the supported
  image surface. The router-owned preset publishes live member IDs, conservative
  guarantees, and a variable-price marker, but the plugin deliberately does not turn it
  into an OpenCode model. There are no other consumers of the cache file. Contract —
  `docs/engine/UNIFIED_ROUTER.md` §§"Harness-agent compatibility", "Models and
  catalog".
- **Fallback telemetry (router/provider planes → Prometheus, phase 6.4c).** `crates/router`
  produces an unauthenticated loopback `/metrics`; the Caddy stable origin 8802 directs
  the scrape to the same active router slot 8800/8801 as the public vhost, with exactly
  18 `claude_router_fallback_total{from_namespace,to_namespace,reason}` series plus
  fixed-cardinality admission/auth/catalog/pricing/policy/header-timeout/balance
  telemetry; the public Caddy allowlist does not pass this path. Each fixed
  `crates/server` plane produces three bounded
  `claude_api_execution_not_started_total{plane}` series through the existing
  authenticated `/metrics`, counting only the exact response actually returned by the
  plane. The consumer is `observability/prometheus/prometheus.yml` and the
  recording/alert rules; Alertmanager/operator use the runbooks `RouterMetricsDown`,
  `RouterFallbackRateHigh`, `RouterConnectionRefusedFallback`, `RouterAdmissionFailures`,
  `RouterAuthorityFailures`, and `RouterResponseHeaderTimeout`, while money-regression
  detectors stay separate. Model, credential, account, group, and request identity do not
  travel through this relationship. Contract — `docs/engine/ROUTING_FENCING.md` §§5.3–6
  and `docs/ops/MONITORING.md`.
- `crates/authbot` — access producer outside the layers; OAuth callback on `127.0.0.1:8796`.

## 3. Models and prices — where else they are mirrored

Authority — `crates/metering` (above). Everything below is mirrors that must be touched
together with it (the full walk — the "New model" / "Price change" checklists in
`docs/CHANGE_CHECKLISTS.md`):

- `packages/contracts` — `CURRENT_*_CANONICAL_MODELS`, catalog generations, and pricing
  release schemas. Frozen capability generation 3 preserves the original main
  Anthropic/OpenAI/Gemini set. Immutable generation 4 historically added
  `gemini-3-flash-preview` (`google` is the internal engine provider id), but the old
  public wire gate returned 404, so generation 4 stays rejected/dormant and cannot be
  materialized or activated; its digest is not rewritten. After the complete fresh
  exact-implementation Pro+Ultra gate, the admitted generation 5 repeats the reviewed set
  under a new digest; the Stage 5 main catalog includes Preview. OpenKeys generation 5
  remains an explicit Anthropic/OpenAI subset.
  `B2C_PRICING_TIERS` is a cleanup target, not an authority of the new pricing.
- `apps/web/src/lib/models.ts` — hardcoded SEO model catalog with official prices;
  the file header requires synchronization with `crates/metering/src/{codex,gemini}.rs`.
- `apps/web/src/lib/pricing-tiers.ts` — legacy cleanup target; the storefront must
  read/show the global 50% and the effective provider/model discount without the tier
  ladder.
- `packages/engine-client/src/openkeys-policy.ts` — canonical OpenKeys policy
  identity/digest and the catalog check against the exact reviewed identity of
  generation 1, 2 or 5 (`CURRENT_PRODUCT_CATALOG_ENTRIES` /
  `MULTI_DISCOUNT_GEN2_PRODUCT_CATALOG_ENTRIES` /
  `MULTI_DISCOUNT_GEN5_OPENKEYS_CATALOG_ENTRIES`); generations 1 and 2 are historical
  reviewed identities, generation 5 is the current active production authority;
  `apps/openkeys` and the Stage 5 planner use one builder (fail closed on divergence).
- `apps/admin/src/app/sales/calculator/calculation.ts` — hardcoded `PRODUCT_CATALOG`
  of subscription products (nanoUSD, bigint).
- Model inclusion policy and discount calculation: `docs/commerce/MULTI-DISCOUNT.md` §§2–4;
  customer pricing — `docs/commerce/PRICING.md`.

## 4. Database boundaries

| DB | Package | Opened by |
|---|---|---|
| engine PostgreSQL/SQLite | `crates/registry` | only the engine; from TS — nobody (Control API only) |
| commerce PostgreSQL (`DATABASE_URL`) | `packages/db` | only `apps/api`, `apps/worker` |
| sales (`SALES_DATABASE_URL`) | `packages/sales-db` | only `apps/sales-api` |
| OpenKeys | `packages/openkeys-db` | only `apps/openkeys` |

## 5. Infrastructure relationships

### Caddy (`deploy/Caddyfile`) — domain → upstream

| Domain | Upstream |
|---|---|
| `api.apitoken.sale` | engine `127.0.0.1:8790` (blue-green slots 8787/8788) |
| `openai.api.apitoken.sale` | OpenAI runtime `:8792` (slots 8793/8797) |
| `gemini.api.apitoken.sale` | Gemini runtime `:8794` (slots 8795/8799); `/oauth/callback` → authbot `:8796` |
| (loopback only, no public vhost) | KIMI runtime `:8803` (active/passive slots 8804/8805); a backend-only plane, not part of the router namespace or catalog |
| `router.apitoken.sale` | atomic `router_backend` → claude-router slot `:8800` or `:8801`; the stable loopback `:8802` uses the same backend |
| `backend.apitoken.sale` | commerce `apps/api` `:8791` (slots 3000/3001) |
| `admin.apitoken.sale` | managed auth; data routes → engine 8790/8792/8794/8803, `/admin/*` → commerce 8791, `/openkeys-admin/*` → 3410, `/partner-admin/*` → sales 3100; everything else → `apps/admin` `:3700` |
| `partners.apitoken.sale` | `/v1/*` → sales-api `:3100`; everything else → sales-web `:3200` |
| `openkeys.apitoken.sale` | `apps/openkeys` `:3410` |
| `content-studio.apitoken.sale` | `/v1/*` → commerce 8791; everything else → `apps/content-studio` `:3500` |
| `crm.apitoken.sale` | `/v1/ingest/*`, `/r/*` → crm-api `:3400` (no admin auth); `/v1/*` and everything else → managed auth → crm-api 3400 / crm-web `:3300`. The CRM lives in a separate repository — do NOT delete the routing |
| `monitoring.apitoken.sale` | Grafana `:3600`; `support.apitoken.sale` → Chatwoot `:3010` |
| `mail.apitoken.sale` (+`autodiscover.`, `autoconfig.`) | mail service `127.0.0.1:8080` |
| `sales.apitoken.sale` | 301 redirect to `partners.apitoken.sale` |
| `admin.partners.apitoken.sale` | managed auth; `/v1/*` → sales-api `:3100`; everything else → sales-web `:3200` |

The stable provider origins 8790/8792/8794/8803 synthesize the internal
`X-Apitoken-Execution-State: not_started` only on Caddy `no healthy upstream`; an ordinary
runtime 503 does not get it. The public provider vhosts strip this header, while the
loopback router uses it as a safe fencing proof before the next explicit fallback
attempt.

### systemd (`systemd/`) — service → application

`claude-api-anthropic@` → Anthropic slots 8787/8788 (current unit; `claude-api@` is legacy) ·
`claude-api-openai@` → 8793/8797 · `claude-api-gemini@` → 8795/8799 · `claude-api-kimi@` → 8804/8805 (`claude-api-kimi` → 8804, legacy/anchor singleton; stable origin 8803; the plane is enabled by the argv pin `CLAUDE_API_KIMI_ENABLED=1` in the reviewed units) · `claude-router@` → 8800/8801 (`claude-router` → 8798 only legacy handoff) ·
`claude-authbot` → authbot ·
`apitoken-api[@]` → `apps/api` 3000/3001 · `apitoken-worker` → `apps/worker` ·
`apitoken-admin` → 3700 · `apitoken-content-studio` → 3500 · `apitoken-openkeys` → 3410 ·
`apitoken-devbot` → `apps/devbot` 3800 · `apitoken-sales-api` → 3100 · `apitoken-sales-web` → 3200 · `apitoken-crm-{api,web}` → external
CRM (3400/3300, do NOT delete) · plus infra units: postgres, affinity-redis, deploy-watchdog,
monitoring-collector, candidate-validator, backup, fingerprint · host bootstrap:
`apitoken-{sudoers,sysctl,tmpfiles}-install`.

### Monitoring — the "metric → alert → runbook" loop

`observability/prometheus/rules/{application,operations}.yml` (~64 alerts) — each has the
annotation `runbook: 'docs/ops/MONITORING.md#<alert>'`, and a `## <Alert>` section must
exist in `docs/ops/MONITORING.md`. Consistency is mechanically checked by
`deploy/monitoring-config.test.sh`, which the host runs when validating every merge
candidate (`deploy/watchdog.sh`) — the check covers ALL alerts of both rules files, not
just the ones named individually. This is an exemplary case of a
closed "code ↔ documentation" relationship — new relationships should be set up on the
same principle.

### Delivery

`deploy/agent-merge.sh` — the only path into `master`; path-aware gate (classifiers in
`deploy/watchdog-lib.sh`), machine merge lock, green `deploy/watchdog` on the production
host. Full description — `deploy/README.md`, `CONTRIBUTING.md`.
