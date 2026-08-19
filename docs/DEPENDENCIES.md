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

## 0. Retired: the per-account pricing policy chain

The engine↔commerce pricing-policy relationship is gone. Price is `accounts.mult_bp` plus
optional `account_provider_discounts` rows; commerce records intent and delivers it through
`engine_pricing_jobs`. There is no policy document, catalog/switch generation, release head,
shadow rollout or strict binding on either side, and no runtime manifest for a slot to publish.

Engine migrations `0044`/`0045` drop what the retired design still enforced at runtime: the
constraint triggers on the money tables, and the two `engine_instances` triggers that gated the
owner lease (`engine_instances_policy_runtime_floor`, `engine_instances_release_v2_epoch_fence`).
Their tables and columns stay as immutable history under the expand-only rule. Nothing may start
reading them again. Exact manifests, retention boundary and drop gates:
`docs/ops/PRICING_RETIREMENT.md`; live model: `docs/commerce/PRICING_MODEL.md`.

## 1. Contracts between contexts

Format: producer → contract/channel → consumers. The contract document is where the
relationship is described in subject terms.

### Deployment compatibility (commerce ↔ engine)

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `deploy/engine-commerce-compatibility.contract` | closed release marker `.engine-commerce-compatibility-v1`: `commerce_requires` and `engine_provides` capability sets; known markerless scalar-transition anchors use bounded exact Git-ancestry classification and older/unreadable history fails closed | `deploy/deploy.sh`, `deploy/rollback.sh`, `deploy/api-bluegreen.sh`, `deploy/engine-bluegreen.sh`; every transition is checked against immutable releases resolved from active API/worker/Control API PIDs, plus the selected target pair | `deploy/RELEASES.md`, `docs/ops/DEPLOYMENT.md` |

### Engine Control API (engine → commerce, OpenKeys)

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `crates/server` (`src/http.rs`, `src/admin.rs`) | HTTP `/admin/*` under `x-api-key: CLAUDE_API_CONTROL_KEY`; account creation/readback, status, keys, ledger, scalar default pricing and canonical per-provider overrides. Account and override multipliers are `0..10000`; a charge ledger row keeps full billed `amount_nano` and additively exposes non-negative `uncollected_nano` (missing means zero), so consumers derive the collected debit and never commission pool-funded shortfall. Other additive fields remain ignorable. | `packages/engine-client` is the only TypeScript transport; `apps/api`, `apps/worker` and `apps/openkeys` consume it. Direct TypeScript calls to `/admin/*` outside the client remain forbidden. | `docs/engine/CONTROL_API.md`, `docs/commerce/PRICING_MODEL.md`, `docs/product/OPENKEYS.md` |
| `crates/server` Gemini status/discovery producer + `crates/forward` native Gemini producer | additive protected `/gemini-subs.credential_generation_digest`, an opaque path-independent identity of the exact encrypted credential set loaded by the process; equality is the only supported interpretation. Published `gemini-3.7-flash` maps only to the credential-bound `gemini-3.7-flash-tiered` wire/quota row, while ordinary discovery, JSON/SSE responses, billing and affinity expose only the public id. The retained admin exact-profile/UUIDv4/deadline evidence lane preserves raw upstream `modelVersion`, one-shot dispatch attestation and no-replay semantics; it is not required by ordinary traffic | native Gemini clients, unified router discovery/pricing, router manifest, admin quota views and the public web/docs consume the public identity. Production-GREEN runtime `c4f0773a…` completed one count and one no-retry paid SSE generation with exact output, terminal usage, 8/7 incremental frames, confirmed raw alias and reconciled `$0.0018075`; the authorization is consumed and may not be replayed. The retired root bridge, trigger, admission unit and permanent canary remain absent. Explicit thinking levels `low`, `medium` and `high` were live-admitted per level on production-GREEN `916dee0d…` (2026-08-15, exact output, terminal STOP/usage, positive thinking class, aggregate `$0.00546`) and are advertised in native and unified discovery; `minimal` stays rejected locally. The 2026-08-15 full-capability matrix on `35153abe…` additionally admitted function calling, JSON structured output, image input and implicit prompt caching (8,170 authoritative cached tokens) for an aggregate `$0.0190`, and those capabilities are advertised the same way. Search grounding was admitted the same day on `ed63dc0f…` under the explicit 10-query reserve contract (one executed query, `$0.014921` reconciled). The 2026-08-16 media matrix on `fc556402…` admitted inline audio (WAV), video (MP4) and PDF inputs with mandatory content-perception markers (`$0.00119025` aggregate), and `audio`/`video`/`pdf` are advertised in `input_modalities`. The 2026-08-16 fleet media matrix (20 legs across the remaining eight models, `$0.0182` aggregate) admitted the same modalities per model with the same marker contract; only 2.5-flash and 2.5-flash-lite audio remain unpublished (no authoritative AUDIO token class on those routes), and 2.5-flash has no official PDF claim. Ordinary panel/Auth Bot consumers ignore the additive status field, and ordinary Gemini responses never carry private headers or the tiered alias | `docs/engine/CONTROL_API.md`, `docs/engine/GEMINI_PROVIDER.md`, `docs/ops/GEMINI_CALIBRATION.md`, `research/GEMINI_3_7_FLASH.md` |
| `packages/engine-client` | TS client `EngineClient`, strict Zod validation from `@claude-api/contracts`, money amounts as `json-bigint` strings; account creation and readback fail closed outside `mult_bp=0..10000`, and scalar/provider pricing writes use the same bound. | `apps/api`, `apps/worker`, `apps/openkeys` | `docs/engine/CONTROL_API.md`, `docs/commerce/PRICING_MODEL.md`, `docs/product/OPENKEYS.md` |
| `crates/server` operator routes + `crates/authbot` proxy lifecycle | read-only `/overview /capacity /metrics /subs /spend-stats /fleet-history /settlement-health /glm-subs` (→ 8790; GLM is a backend inside the Anthropic runtime), `/codex-subs` (→ 8792), `/gemini-subs` (→ 8794), `/kimi-subs` (→ 8803, a dedicated default-off KIMI plane; the Anthropic-embedded gateway is dev/test only), plus authbot `GET /proxy-admin/inventory` and explicit idempotent `POST /proxy-admin/renew` (→ loopback 8806) via Caddy `admin.apitoken.sale`, authenticated by `X-Proxy-Admin-Key` from the stable `root:root` `0600` non-symlink `/etc/apitoken/proxy-admin.key` (exactly 64 lowercase hex plus optional LF), atomically provisioned below the root-owned, non-deploy-writable `/etc/apitoken` parent before unit/Caddy installation; the installer removes one exact legacy `AUTH_BOT_PROXY_ADMIN_KEY` assignment from `authbot.env`, fails on malformed/duplicate/divergent input, and rejects either proxy-admin key or key-file setting in `server.env`. `LoadCredential=proxy-admin.key:/etc/apitoken/proxy-admin.key` gives only authbot a private copy; after all env files load, the `ExecStart=/usr/bin/env` command assignment pins `AUTH_BOT_PROXY_ADMIN_KEY_FILE=%d/proxy-admin.key` (not `Environment=`), so env files cannot redirect it. The bounded Rust parser accepts only that file, and sibling services receive no value. On Linux, after any operator subcommand has returned and before daemon secrets are loaded, authbot calls `prctl(PR_SET_DUMPABLE, 0)`, blocking same-UID `ptrace`, `process_vm_readv`, and sensitive `/proc` memory access; `ProtectProc=invisible` and `ProcSubset=pid` remain. Code already executing inside authbot itself is in the same trust boundary, and no defense can protect secrets from code already executing there. The root Caddy installer and renderer use only the `/etc` raw path, match the live `X-Proxy-Admin-Key` header name case-insensitively, and fail on duplicate or mismatched live values; Caddy additionally injects shared `x-api-key` only for the previous binary during mixed-version rollout/rollback, while new authbot ignores it and uses `CLAUDE_API_CONTROL_KEY` only for outgoing sanitized `/codex-subs` and `/gemini-subs` runtime status reads by opaque id, then produces a fail-closed inventory containing only subscription-backed durable exact IPRoyal bindings with liveness other than `dead`; GPT is public `gpt` over durable `codex`; one exact legacy `gpt` local-id/order/allocation-IP binding migrates in place while ambiguous or mismatched rows stay untouched, and Codex status accepts exactly `healthy|suspect|dead` with schema drift closing the source. The additive inventory item `account_email` is the sole full-identity exception, confined to the closed `managed_admin_auth` `/proxies` response with `no-store`/in-memory handling; raw proxy identity/credentials and other secrets remain absent. Additive `operator_renewable` ignores only local subscription expiry, while the optional `allow_inactive_subscription` request flag defaults false and is persisted in the idempotent selection. Exact binding, authoritative liveness, provider-order, overlap and uncertain-result safeguards remain mandatory. IPRoyal receives the non-empty selected canonical IPs in `proxies`, so one allocation may be renewed without its siblings; already-disabled auto-extend is not toggled again, while an enabled setting is disabled and exact-refetched before the paid request. Only paid transport/5xx/post-confirmation ambiguity remains indeterminate. Pending/in-progress selections are exclusive across UUIDs (`409 renewal_selection_busy` before enqueue on overlap; claim atomically terminalizes legacy overlapping pending siblings as `indeterminate`; disjoint selections proceed). All new IPRoyal orders have auto-extend disabled and the background guard disables it on every existing order without performing a paid renewal | `apps/admin` consumes the additive full `account_email` as the sole identity exception after matching the producer's strict ASCII grammar; it searches and renders that value without persistence, while recursively rejecting generic/nested identity or secret fields. The consumer independently drops every `dead` or non-`bound` row, marks non-null subscription/proxy expiries in separate cells at the exact inclusive 72-hour boundary, and fail-closed parses `operator_renewable === true` for its count, bulk selection, checkbox and row action. Every newly created renewal request sends `allow_inactive_subscription=true`, and uncertain replays retain that exact request and UUID, so local subscription expiry alone cannot block an operator-selected proxy. There is no engine-client and the app owns no secrets. `/metrics` is also scraped by Prometheus directly over loopback, bypassing Caddy (`observability/prometheus/prometheus.yml`), including the KIMI origin 8803 with the target label `provider: kimi` | `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md`, `crates/authbot/CLAUDE.md` |

Live Control API endpoint groups are accounts, idempotent credit/debit and ledger cursors, usage,
keys, scalar account/provider pricing and hot tariff overrides. The additive compiled-tariff dump
publishes time-relative `has_future_epoch` and whole-schedule `seed_safe`; seed validates every
selected family and returns atomic 400 before authority access when any schedule has multiple
epochs, while explicit current/future override rows remain the append-only rollout path. The former catalog/switch/policy
and release-v2 routes have been removed from the server, engine client and shared contracts; their
database objects are retained evidence, not callable expand-only contracts
(`docs/ops/PRICING_RETIREMENT.md`).

Legacy ledger provider recovery stays producer-owned: first the exact immutable
`usage_events.provider` of the same `account_id + request_id` pair is used; for request-less
history only a strict account/key/amount/ref/model/time settlement fingerprint with a single
non-empty provider across all candidates is acceptable. Conflict is fail-closed, ambiguity stays
unknown and model-name inference is forbidden. Recovery version `2` prevents endless rescans after
either exact evidence or a terminal no-evidence result. Strict fingerprint producer SHA
`d5f3d6bccdaa5015a443500d2530f1430596362b` was watchdog-GREEN before the commerce consumer shipped.
### Sales feed (commerce ↔ sales)

A bidirectional perimeter under one key `SALES_CONTROL_KEY` (header `x-api-key`).

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `apps/api` (`src/sales-feed.controller.ts`, `/v1/internal/sales/*`) | GET feeds `attributions` / `usage-events` / legacy `topups` / additive `topups-v2` / additive `payment-reversals` (cursors `after_id`). Password/OAuth signup commits its referral row with the new account; promo redemption commits attribution before credit. The live usage producer emits only the scalar form: `amountNano=real_funded_nano`, optional informational `providerId`, and null retired attribution/release fields. Usage/topup/reversal eligibility starts at the durable attribution timestamp. Legacy topup pages fail closed at a split equal-`paid_at` group; `topups-v2` uses the fenced commit-ordered `payments.feed_seq`, advances over the full source page before referral filtering, and keeps a verified deposit replayable after a later refund. A terminal payment transaction also appends a fenced immutable reversal audit row; all three money feeds expose a committed `sourceHead`, including rows still inside visibility lag. | `apps/sales-api` (`sync.service.ts`) consumes scalar usage through `recordReferredSpend`, storing the informational `providerId` in the separate authority-free `partner_usage_events.spend_provider_id` (migration 0022) so earnings can be reported per provider without touching the retired attribution tuple; its v1/v2 parsers/writers remain only for expand-only replay of historical usage rows. It consumes `topups-v2` for analytics and an independent immutable funding-lot replay, allocates usage/commissions by causal payment FIFO, and consumes `payment-reversals` only after both causal feeds prove their current head. Reversal evidence, exact negative entries and its cursor commit together under `SERIALIZABLE`; crash replay is idempotent and a conflict/incomplete allocation leaves the cursor behind. Gross, signed adjustments, net, debt and payable are separate read fields. Payout prepare/send drain all causal feeds, hold the sync mutex, take the shared accounting lock, re-probe Commerce heads, and keep both fences through commitment/signing; any head/cursor/allocation/reversal mismatch fails closed. The legacy timestamp cursor remains unchanged. The consumer rejects non-canonical/out-of-range bigint values, repeats the attribution-time gate in storage, and globally fences one `commerce_event_id` across recorded and pending v1/v2 stores. `commerce.service.ts` uses `COMMERCE_BASE_URL`. | `docs/sales/SALES_PORTAL.md` |
| `apps/sales-api` (`src/internal.controller.ts`, `/v1/internal/*`) | POST `promo/redeem` supplies credit/referral evidence; Commerce persists first-touch attribution before credit, accepts an exact-owner replay, and fails with 409 on a different existing owner. `partners/referral-discount` atomically claims a one-time link and returns its promised `discountBps` only to the winning user. `partners/external-referral-alias` idempotently maps one trusted `(source, externalRef)` to an opaque public alias owned by an active ordinary partner; the immutable alias has no price-marker semantics and resolves through the same attribution/commission path. | `apps/api` (`promo.service.ts`, `auth.service.ts`, CRM bridge; `SALES_API_URL`) | `docs/sales/SALES_PORTAL.md`, `docs/commerce/CRM_BRIDGE.md` |

The feed types are duplicated as local zod schemas on both sides; they are not factored out
into `packages/contracts`. Any feed change edits both sides — see the contract protocol in
`AGENTS.md`.

### CRM referral bridge (CRM → commerce → sales/engine)

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `apps/api` (`src/crm-bridge.*`, `/v1/internal/crm/*`) | Dedicated `CRM_CONTROL_KEY`; idempotent referral-link ensure and profile read strictly by CRM UUID `externalRef`. Commerce obtains the partner-owned opaque alias through Sales, joins registrations only by that alias, returns all candidates plus actual scalar/provider pricing and exact decimal-string money (`customerFundedSpentNano` is the post-scalar `pricing_usage_events.real_funded_nano` sum), and batch-reads nullable live balance/status through `packages/engine-client`. No retired pricing evidence or arbitrary email/user lookup exists. | Standalone CRM server only; the browser never receives this key or calls Commerce directly. | `docs/commerce/CRM_BRIDGE.md` |
| Standalone CRM | Opaque local `/r/:token`, local click evidence and server-to-server `COMMERCE_BRIDGE_URL`/`COMMERCE_BRIDGE_KEY`. Creation/copy is not `link_sent`; only an outgoing archived message ID proves sent. An attributed email remains a candidate and is not auto-written into CRM contact channels. | CRM operator UI; public redirect to fixed `https://apitoken.sale/?ref=<alias>` | `docs/commerce/CRM_BRIDGE.md` |
| `apps/web` (`src/proxy.ts`, root `[0-9][a-z0-9]{6}` gateway) | Server-side resolution through standalone CRM `/r/:code`; accepts only `302/303` to the exact attributed `https://apitoken.sale/` destination, forwards no browser credentials and fails closed without caching. Named site routes are outside the matcher. | Public CRM short links such as `https://apitoken.sale/3kgj45g`; CRM remains click authority. | `apps/web/README.md`, standalone CRM `COMMERCE_BRIDGE.md` |

### Other cross-context relationships

| Producer | Contract / channel | Consumers | Contract document |
|---|---|---|---|
| `packages/contracts` | Zod schemas for engine account/key/ledger/usage, auth and checkout boundaries plus canonical model pins. The live scalar account contract is `mult_bp=0..10000` for both creation input and account readback; money remains integer/decimal-string safe. | `apps/api`, `apps/worker`, `apps/openkeys`, `packages/db`, `packages/engine-client`. Do NOT import: `apps/web`, `apps/sales-*`, `apps/admin` | `docs/engine/CONTROL_API.md`, `docs/commerce/PRICING_MODEL.md` |
| `crates/authbot` (`tripo3d_roster.rs`, `AUTH_BOT_TRIPO3D_{DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID}`) | Sealed Tripo3D roster at `/srv/claude-api/data/tripo3d` (`profiles.json` + `credentials/<id>.json`, XChaCha20-Poly1305 envelopes AAD-bound to profile id): atomic publish/replace after the free balance probe and cohort corroboration; the engine only ever reads | `crates/forward` (`tripo3d/roster.rs`) on the dedicated default-off `ProviderMode::Tripo3d` plane (`CLAUDE_API_TRIPO3D_*`); `crates/server` read-only `GET /tripo3d-subs` (control key) projects it to the admin surface — privacy-safe, no key/subject/proxy; `apps/admin` (subscriptions control room) consumes `GET /tripo3d-subs` via same-origin fetch — the Caddy route is intentionally not installed while the plane is dormant, so the board renders its null state | `docs/engine/TRIPO3D_PROVIDER.md` |
| `crates/authbot` (`suno_roster.rs`, `AUTH_BOT_SUNO_{DIR,CREDENTIAL_KEYS,CREDENTIAL_ACTIVE_KID}`) | Sealed Suno roster at `/srv/claude-api/data/suno` (`profiles.json` + `credentials/<id>.json`, XChaCha20-Poly1305 envelopes AAD-bound to profile id): atomic publish/replace after Clerk session discovery → JWT mint → free billing probe → plan corroboration; the engine reads and additionally RE-SEALS a profile's envelope when a `set-cookie` rotation arrives (single-flight, manifest §2) | `crates/forward` (`suno/roster.rs`, `suno/session.rs`) on the dedicated default-off `ProviderMode::Suno` plane (`CLAUDE_API_SUNO_*`); `crates/server` read-only `GET /suno-subs` (control key) projects it to the admin surface — privacy-safe, no cookie/session/subject/proxy; `apps/admin` (subscriptions control room) consumes `GET /suno-subs` via same-origin fetch — the Caddy route is intentionally not installed while the plane is dormant, so the board renders its null state | `docs/engine/SUNO_PROVIDER.md` |
| `apps/api` (public API) | HTTPS `backend.apitoken.sale/v1/*`, cookie session | `apps/web` (`src/lib/api.ts`, `NEXT_PUBLIC_BACKEND_URL`) | `docs/commerce/COMMERCIAL_BACKEND.md` |
| `apps/api` (paying/spender admin producer) | Read-only `GET /v1/admin/finance/paying-users` via the Caddy rewrite `admin.apitoken.sale/admin/*`: omitted funding and `payments|manual|bonus|all` preserve their existing lifetime-money/strict-bonus semantics; additive `spenders` selects every positive `pricing_usage_events` spender in 1/7/30d and labels zero-money non-strict rows `spend_only`; page/count/summary share one half-open cutoff in read-only `REPEATABLE READ`; omitted/false `include_usage` stays DB-only and exposes no engine IDs/usage, while literal `include_usage=true` performs page-wide concurrency-four, five-second abortable usage calls for every distinct event account and adds a minimal exact `(provider, model)` aggregate with safe counters serialized as decimal strings, exact nanoUSD strings, and explicit `complete|partial|unavailable` coverage—never full EngineUsage/account/key/daily detail or provider inference | `apps/admin` (`/paying-users`) consumes `funding=spenders&include_usage=true` by default, with expandable exact provider/model coverage and one-row-per-user×provider×model CSV; wired only after GREEN exact producer SHA `d27033effc237156bce91a38d1ca0ff5b6e66cbd`. Additive bonus was consumed after GREEN `b12a08fe872fb08a88943d7ade0a75a3e567b579`; the original consumer after GREEN `ce92503d1adc0e31967b2dda5853ce05ed480048` | `docs/commerce/COMMERCIAL_BACKEND.md`, `docs/product/ADMIN_PANEL.md` |
| `apps/api` (admin API) | `/v1/admin/*` via the Caddy rewrite `admin.apitoken.sale/admin/*`, header `x-admin-key`; additive `GET /events` fans commit-bound `commerce_admin_changes` into resource-prefix SSE invalidations with initial/reconnect resync. `/users/:id/provisioning-repair` reconciles only an existing mapped account after two fresh Control API readbacks: live `active` status and the exact default multiplier must match both commerce copies, then a row-locked CAS admits that exact `pending|error` mapping and writes one replay-idempotent audit event; it never creates/imports/reprices and fails closed for disabled or drifting state. Scalar B2B pricing uses atomic `PATCH /business-users/:id/pricing` plus the fenced durable delivery queue. `GET /admin/pipeline-health` exposes the live pipeline summaries. The same authenticated channel and key are used on `content-studio.apitoken.sale/v1/*`; retired release-cycle/policy/catalog/switch firing routes are not contracts and must not be restored. | engine Control API — via typed `packages/engine-client`; `apps/admin`; `apps/content-studio` (`/v1/admin/content/*`) | `docs/commerce/COMMERCIAL_BACKEND.md`, `docs/commerce/PRICING_MODEL.md`, `docs/product/ADMIN_PANEL.md`, `docs/engine/CONTROL_API.md` |
| `apps/openkeys` (admin API) | `/api/internal/admin/*` via Caddy `admin.apitoken.sale/openkeys-admin/*`, headers `X-OpenKeys-Control-Key` + verified actor; additive `GET /events` fans commit-bound `openkeys_admin_changes` into resource-prefix SSE invalidations with initial/reconnect resync. Read-only `GET /paying-keys` returns every non-removed warehouse or delivered key with explicit `stock|delivered` lifecycle and nullable delivery time, exact nullable lifetime engine spend, global `spent|nominal|created|delivered|status` + `asc|desc` server sorting, bounded batch account reads, page-only live usage for `1|7|30` days, exact nanoUSD/model counters, and row-local unavailable status without secrets | `apps/admin` (`/paying-users`, OpenKeys cohort) consumes sorting/lifetime spend only after GREEN exact producer SHA `65f2160f67f8662ec58fbf336444c0ca8b5ff76a`; warehouse+lifecycle consumption followed GREEN `11aec1b731a5b31b057641982957aa0142eaacf2`, and the original delivered-only consumer followed GREEN `558d4b34896792cfaed5760852f9001feb0d0443` | `docs/product/OPENKEYS.md`, `docs/product/ADMIN_PANEL.md` |
| `apps/openkeys` (seller issuance API) | Session-protected `GET/POST /api/admin/batches`; `GET` exposes display-only models under `issuanceAuthority.supportedModels`, and successful `POST` additively returns the same `supportedModels` beside the new keys, face value and historical `apiType`. The array guides the customer handover text but never authorizes a model or price. | OpenKeys `/admin`; standalone CRM's server-side OpenKeys bridge consumes the additive POST field only after this producer is production-GREEN and continues to tolerate older responses without it. | `docs/product/OPENKEYS.md` |
| `apps/sales-api` (public + admin API) | B2B grant: `PATCH /v1/admin/partners/:id` and `POST /v1/admin/invites` accept `b2bEnabled`/`b2bMaxDiscountBps`, storing an admin-set ceiling on the partner row (migration 0023, off by default); the grant is authority for a partner to price their OWN referrals as B2B and does not alter commission, which stays a percentage of the customer's own money for both classes. `partners.apitoken.sale/v1/*`; `/v1/admin` via Caddy `admin.apitoken.sale/partner-admin/*`, header `x-sales-admin-key`; additive `GET /events` fans commit-bound `sales_admin_changes` into resource-prefix SSE invalidations with initial/reconnect resync | `apps/sales-web`; `apps/admin` | `docs/sales/SALES_PORTAL.md`, `docs/product/ADMIN_PANEL.md` |
| Engine/Authbot admin invalidations | Engine provider processes expose authenticated `/admin-events`, rewritten by Caddy to `/events/{engine,openai,gemini,kimi}`; Authbot exposes `/proxy-admin/events`. Payloads are invalidation hints only, with initial/lag resync and transport keepalive. Engine billing invalidations are emitted after the successful authority write and evict matching server response caches before the browser refetches. | `apps/admin` shared request cache | `docs/product/ADMIN_PANEL.md`, `docs/engine/ARCHITECTURE.md` |
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
| `apps/devbot` | loopback `/metrics` (`127.0.0.1:3800/metrics`, Prometheus job `devbot`): `devbot_telegram_send_failures_total`, `devbot_last_webhook_seconds` | Prometheus → alerts `DevBotTelegramSendFailures`, `DevBotWebhookSilent`, `DevBotMetricsDown` | `docs/ops/MONITORING.md`, `docs/ops/DEVBOT.md` |
| `apitoken-affinity-redis.service` | two instances under one unit: history `127.0.0.1:6379` (legacy Compose service identity `affinity-redis`, Codex response history, `allkeys-lru`, 512 MiB) and affinity `127.0.0.1:6380` (service `cache-affinity-redis`, cache-lineage L2 + advisory cooling hints `claude-api:cool:v1`, `allkeys-lru`, 128 MiB); the installer performs an additive Compose reconcile up to the two-target monitoring gate and does not stop history; exporters `9121`/`9122` (job `redis`, label `instance_role`); the password is read from `CLAUDE_API_REDIS_PASSWORD` in `server.env` and published as the JSON secret `observability/secrets/affinity_redis_password` | Prometheus → alerts `AffinityRedisDown`, `AffinityRedisEvictingKeys`, `AffinityRedisMemoryHigh` | `docs/ops/MONITORING.md#affinityredisevictingkeys` |
| `crates/forward` billing writer + `crates/server` `/metrics` | histogram `claude_api_billing_pg_command_duration_seconds{op="reserve\|settle\|acquire_capacity"}` (latency around the retry wrapper, 10 buckets 1 ms–1 s) and gauge `claude_api_billing_write_queue_depth` (occupied slots of the 4096-slot writer channel); both operational (no money labels), visible to the readonly key; PostgreSQL-only, the SQLite fallback does not publish the histogram | Prometheus → alerts `BillingPGCommandLatencyHigh`, `BillingWriteQueueBacklog`; Grafana `production-overview` row "Billing writer (PostgreSQL hot path)" | `docs/ops/MONITORING.md#billingpgcommandlatencyhigh` |
| `crates/server` Anthropic `/metrics` | gauges `claude_api_anthropic_quota_last_observation_timestamp_seconds` (newest exact provider quota snapshot across the routable fleet) and `claude_api_anthropic_quota_snapshot_subscriptions` (routable subscriptions holding any snapshot); aggregate-only, no per-subscription label, and no timestamp is published before the first observation so a never-probed fleet cannot fire | Prometheus → alert `AnthropicQuotaSnapshotStale` (fires when the newest snapshot exceeds the same 900 s freshness bound `/capacity` uses to price current remaining) | `docs/ops/MONITORING.md#anthropicquotasnapshotstale` |

## 2. Inside the engine (briefly)

Layers and invariants — `CLAUDE.md` (layer table) and `docs/engine/ARCHITECTURE.md`. Here
is only what is needed to walk the relationships when making changes:

- **`crates/elog` — the engine's unified error log.** A leaf crate (no dependencies) that
  every runtime layer that logs (`forward`, `server`, `router`, `authbot`, `registry`,
  `pool`) consumes. All runtime diagnostic lines flow through `elog::error/warn/info`
  with one line format `[LEVEL][category] message` and one central secret scrubber;
  `metering` stays pure and does not log. Contract — `crates/elog/CLAUDE.md`.
- **`crates/metering` — the engine's price authority.** Hardcoded effective-dated tables
  in nanoUSD: `src/lib.rs` (Anthropic), `src/codex.rs` (OpenAI text), `src/gemini.rs` (Gemini),
  and `src/openai_image.rs` (GPT Image 2). A price/model change is a reviewable commit here.
  Consumers: `crates/forward` (main), `crates/server` (types/tariff identifiers). The GPT Image 2
  tariff prices authoritative five-leg usage from the sealed Codex OAuth-pool runtime; customer
  admission uses the same scalar/provider multiplier path as every other model. It is not ChatGPT
  native-credit accounting and introduces no reseller, external image key, fallback, or
  environment setting.
- **Retired pricing modules.** `crates/registry/src/pricing/snapshots.rs` owns the live scalar
  reserve/settlement identity types and `crates/forward/src/pricing.rs` contains scalar pricing
  helpers; neither is a policy/catalog/release resolver or a reader of the retired tables. The hot
  tariff authority remains live in `crates/registry/src/pricing/tariffs.rs`. Exact retirement
  boundary: `docs/ops/PRICING_RETIREMENT.md`.
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
- **Logical request identity contract (Caddy perimeter → router → provider planes).** All four
  public provider/router vhosts remove the reserved internal `X-Apitoken-Logical-Request-Id`, while
  stable loopback origins preserve the reserved capability for the trusted router→plane hop;
  loopback access alone is not sender authorization. The provider consumer's exact production SHA is
  GREEN. `crates/router` is now the routed-traffic producer: its final common proxy removes all inbound
  copies, and only after auth/body/model/routing/policy admission it creates one canonical lowercase
  CSPRNG UUIDv4 immediately before the first executable attempt. Native and universal single attempts
  receive it; fallback reuses it byte-for-byte across attempts while execution group/attempt remains
  separate. Balance/helper/preflight traffic strips but never injects, and the router strips response
  copies so the ID stays private. `crates/server` consumes exactly one canonical trusted value on
  Anthropic, OpenAI, Gemini, and Combined customer routes, removes the wire capability, and stores a
  typed `crates/forward` extension; direct provider ingress with zero values creates its own ID.
  Universal Anthropic/Gemini adapters preserve the typed logical, client-attribution, and shared
  request-lifecycle-clock context on synthesized leaf requests. At the same early boundary,
  `crates/server` creates the typed once-only clock and wraps the final public response body: only its
  first non-empty successful DATA frame is observed, while data, trailers, errors, frame boundaries,
  cancellation, hints and backpressure remain unchanged. No request-fact producer consumes this clock
  yet; terminal handoff remains a later producer-owned slice and unmeasured evidence stays `NULL`.
  `crates/server` also consumes all optional public `x-apitoken-client` values into privacy-bounded
  `crates/forward::ClientAttribution`: exactly one
  valid `opencode[/version]` or `claude_code[/version]` is explicit, while absent, malformed,
  duplicated, unsupported, or case-variant evidence fails open to unknown. The raw value never reaches
  dispatch/upstream; heuristic v1 has no reviewed positive signature and does not reuse the Codex
  envelope heuristic. Health, admin/internal preflight and backend-only KIMI/Tripo3D/Suno remain
  outside this MVP. The first and only production request-fact producer is Codex/OpenAI universal
  `POST /v1/messages/count_tokens`: after successful metered admission it consumes the typed logical
  and client context plus authoritative non-secret account/key and execution identities, then submits
  exactly one already-terminal nullable-billing-ID fact through the fail-open PostgreSQL inbox.
  Missing typed client context remains unknown without suppressing the fact. Router fallback reuses
  the logical ID while each plane attempt keeps its distinct execution attempt. `crates/forward` also
  defines dormant, privacy-bounded pure structural classifiers for already-validated client shapes:
  Anthropic Messages, OpenAI Chat/Responses and canonical native Gemini GenerateContent. Universal
  Chat/Responses classify the OpenAI client shape before translation and Messages classifies its
  Anthropic client shape before translation. They retain only closed registry bits/counts/flags and
  discard names and request content; no handler or request-fact producer consumes
  them yet, so Stage 6/7 owns the producer connection and lifecycle/output evidence. Admin/unauthorized/
  missing-logical-context traffic is omitted. Billable paths, native Responses token counting,
  Anthropic/Gemini, read APIs, public metrics, and a public logical-ID header remain absent; `x-request-id`
  and response availability are unchanged.
  Contract —
  `docs/engine/REQUEST_OBSERVABILITY.md` §§4, 13; perimeter details — `deploy/CADDY.md`.
- **ClaudeStore-compatible emergency transports (`crates/server` → `crates/forward` → external relay origins).**
  `crates/server/src/config.rs` solely reads the two strict enable/key pairs, and the
  compile-fixed origins cannot be replaced by env URLs: Claude Messages uses
  `https://api.llmsrelay.com`, while dormant GPT/Codex remains on
  `https://api3.claudestore.store`.
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
- **GPT Image 2 producer chain (`crates/server` OpenAI routes → `crates/forward::codex` →
  existing sealed Codex OAuth pool → `crates/metering`/`registry` settlement).** The native transport
  posts typed JSON to `{CodexConfig.base_url}/images/generations|edits` with the existing bearer,
  account and first-party client identity; no image API key, reseller origin, or new env exists.
  Private generation SHA `df58715abb4f1ac52b6c46b1ea6f830c6e11178f` and one-reference edit SHA
  `1c48e3769f0fe775e650f60ea3c5839458e5dfe2` have watchdog-GREEN exact-home PNG/usage evidence;
  edit delivery `8357ec764d1cdddff652ae4b5d6221267eb14f4e` is accepted only by watchdog-GREEN non-network
  verifier `354832bc86c3a8365e713faf0f35ad2c239c7087`, which would fail a terminal withdrawal.
  The producer-first `POST /v1/images/generations|edits` authenticates before buffering, freezes and
  preflights one home, permits only one `opaque/low/auto` PNG (edit: exactly one strict PNG reference),
  reserves a typed immutable image snapshot, and settles authoritative five-leg token cost. Successful
  malformed evidence and ambiguous post-dispatch errors retain the hold and never claim `not_started`.
  Public gate delivery `0dbbfdda054a1a7bda709434c8678b192bf12276` is RED and permanently fences
  producer `d2e345f2de75e0ee6c72797fdf315f12ab4bbeb6`. Non-network inspector delivery
  `5a16ce96e2d1aef242055e88aa5d38f152d0ecd5` observed exact `preflight`, both dispatch flags false, and
  both request identities null, proving that no paid image operation was dispatched. The fixed
  `deploy/gpt-image-2-public-smoke-gate.sh <producer-sha> --inspect` consumer can accept this exact safe
  withdrawal or complete retained success; it has no credential/environment loader or image dispatch. The
  successor producer `d42fc0e3290c0042a16797626326c250e0f6721c` is watchdog-GREEN. Its private CLI has a
  separate free `--preflight-only` consumer of only the engine PostgreSQL DSN, existing `crm-parsing`
  meter-only credential and authenticated public `/v1/models`. Delivery
  `737d0234fc7d016c31c5b9c56a27e16aef134d83` is RED and fences its root, but this mode has no image POST
  and every valid stage has false dispatch flags and null request identities. Corrective inspect-only delivery
  `77cf6791c92840dc1e45c1aba252820506f63fd4` reported the retained `credential_selecting` stage without
  environment, credential, binary, or network access; it did not rerun credential selection. Selector
  hardening SHA `6629ecd7b3725bcd7306ef7a1dc8675ef9160a43` resolves the assignment through the active policy's
  `owner_type=service`/`owner_id=crm-parsing`, while retaining canonical meter-only resolution, the OpenAI
  master switch, and exactly-one-active-unexpired-key fencing. Corrective deploy SHA
  `b0c67351bb25437316afb61d18cd4462c57ef27b` made the lowercase `deploy/*` status allowlist accept digits
  and is watchdog-GREEN; it performed no image request. The separate
  `deploy/gpt-image-2-public-preflight-v2-gate.sh` was pinned to producer
  `6629ecd7b3725bcd7306ef7a1dc8675ef9160a43`, used the fresh
  `gpt-image-2-public-preflight-v2` root, inherited only the active OpenAI slot's PostgreSQL DSN, and ran only
  `openai-image-public-smoke --preflight-only`. Delivery `267744a02d664f24ca072326fa06771b54188de4`
  stopped at `credential_selecting`, with both dispatch flags false and null request identities; it performed
  no image POST and permanently fenced that root. Corrective selector
  `63972f2ddfd5906d7c30a87406053eb3782f4223` identifies the unique engine account by handle
  `crm-parsing` while independently requiring its active assignment and linked policy to remain
  service/meter-only; service policy owner metadata is authorization, not engine account identity. Its
  trusted-host gate was GREEN, but the overall delivery was RED because the historical v2 trigger revisited
  its already-fenced failed root; no selector or image request was rerun. Delivery
  `2f11cd62ed0f78b97cf31d2287fa660907975aad` retired the v2 trigger and is watchdog-GREEN. The successor
  `deploy/gpt-image-2-public-preflight-v3-gate.sh` is pinned to producer
  `63972f2ddfd5906d7c30a87406053eb3782f4223`, uses a fresh v3 root, inherits only the production PostgreSQL
  DSN, and can run only `openai-image-public-smoke --preflight-only`. Delivery
  `825b3596983e7420a038feb3e883b11f0ebabba7` completed authenticated discovery with the sole terminal
  `preflight_success` journal, false dispatch flags and null request identities, but was overall RED when
  process teardown crossed the controller deadline; no image POST occurred. The successor controller accepts
  only that strict terminal shape after a nonzero timeout and uses the SHA fence without network replay;
  incomplete states, image artifacts, dispatch flags, request identities and extra arguments remain RED.
  Corrective delivery `df924a10edff41b0d047805d18abe16a397b4809` validated the retained terminal preflight
  without replay and is exact watchdog-GREEN. The separate
  `deploy/gpt-image-2-public-paid-smoke-gate.sh` pins the same producer to the fresh
  `gpt-image-2-public-paid-smoke` fence, inherits only the production PostgreSQL DSN, and runs exactly one
  `--execute`: free discovery, one generation, authoritative settlement, then one one-reference edit and
  authoritative settlement. Existing output prevents every replay; no reseller origin, fallback, image API
  key or additional credential path exists. Delivery `d2216bfa276d9fe195b0d1f0c8f4f137612bed5a` is RED at
  `generation_received`: generation returned a bounded decoded PNG, no complete settlement evidence was
  persisted, and edit was not dispatched. That paid root is permanently fenced and its execute trigger is
  retired. The separate `deploy/gpt-image-2-public-paid-inspect-gate.sh` is credential-, environment-, CLI-
  and network-free; it accepts only the exact generation-only two-file withdrawal and emits dimensions,
  bytes and SHA-256. This inspection is not a generation+edit success and no publication consumer can proceed.
  Watchdog-GREEN producer `ab3b4e557f7b870b93f62a88a53e87e46b49fb4c` temporarily exposed the
  identifier-free settlement diagnostic used by the separately pinned one-shot controller. It read the
  fenced request's PostgreSQL stages in one read-only snapshot and published only a bounded status. The
  watchdog-GREEN controller
  `d66e25babba5e55ef96ebec51971962656a4badf` reported `terminal_evidence_present` for that request:
  reservation `settled`, outbox `done` on attempt 1 without error, usage present, `real_nano=7_045_000`, and
  `charge_nano=0`. This proves the old smoke stopped while waiting for evidence rather than a failed
  settlement; it still does not prove edit or authorize publication. The successor observer uses an explicit
  150-second wall-clock deadline instead of a fixed iteration count. Each database statement is independently
  bounded to 15 seconds by the PostgreSQL session; observer timeout remains terminal without paid replay.
  Producer `853fdc6c8d5be486c371b23df6772eeaf7a48029` is exact watchdog-GREEN. Its sole paid successor
  consumer is `deploy/gpt-image-2-public-paid-smoke-v2-gate.sh`, pinned to that binary and a fresh v2 evidence
  root; it inherited only the PostgreSQL DSN, permitted one `--execute`, and required strict generation+edit
  PNG, usage, settlement, and unchanged-money evidence. Delivery
  `2efcfbf69b672e531b62b8602a74d7fb76ee1fae` withdrew at
  `generation_received:g=true:e=false`; the v2 root is permanently fenced. A second one-shot diagnostic
  reused the exact GREEN `853fdc6c...` binary and read only that request's durable
  settlement state. Neither diagnostic had image HTTP, credential selection, dispatch, retry, or mutation.
  Diagnostic delivery
  `f1fb47c3e6e75c219f7b9f6f229db693e54197f5` is exact watchdog-GREEN with the same terminal
  `settled`/`done/1`, `real_nano=7_045_000`, `charge_nano=0` evidence. It isolated the repeated stop to the
  smoke runner: synchronous `PgStore` was invoked inside its Tokio runtime even though the synchronous
  `postgres` client uses an internal runtime for query and drop. The corrected producer keeps PostgreSQL
  observation and teardown outside Tokio and enters the network runtime only for each HTTP future; neither
  fenced request is replayed. Both diagnostic statuses are immutable historical evidence; after the later v3
  gate succeeded, the diagnostic CLI/controllers/sudo grants were retired, so no current runtime consumer
  reads `pricing_request_snapshots_v2`. Corrective producer `8b68d73a2a6ba6ffae2f24692b283059f15b7c63` is exact
  watchdog-GREEN. Its sole paid consumer is the separately delivered
  `deploy/gpt-image-2-public-paid-smoke-v3-gate.sh`, pinned to the fresh v3 evidence root and allowed exactly
  one `--execute`; every partial result permanently fences replay. The direct OpenAI plane and header-gated Combined
  bridge produce these routes; the unified router proxies both image routes to the OpenAI plane as a
  native lane, and its preset lists the snapshot id; `/v1/models` deliberately does not publish the
  image model. The v3 one-shot public generation+edit smoke (delivery
  `d172c6fd0116ba73b051fc5aa02193a4885de5da`) is overall watchdog-GREEN, so the model is published:
  generation-6 pricing catalogs activated the immutable `gpt-image-2-2026-04-21` snapshot (release
  head 41), and the router preset, site catalog and public docs list it (publication commit
  `3917a31b333899aed87396acd6e8e83e403cd3e6`). The separately delivered
  `deploy/gpt-image-2-surface-probe-gate.sh` is the control-surface probe: pinned to the
  probe-capable producer `d69868fb700aaeb9b6723d8780bb29be4aab9c0d`, it runs one medium and one
  high generation plus one two-reference edit, each under its exact official authorization ceiling
  in its own fenced root, and publishes sanitized honored/normalized/rejected verdicts. Verdicts:
  medium and high normalize to low on the wire (admission keeps rejecting them); the two-reference
  edit is honored, so the producer admits up to five references with the per-reference envelope. Contract and blockers —
  `docs/engine/CODEX_PROVIDER.md`, `docs/ops/GPT_IMAGE_2_CANARY.md`, and
  `research/GPT_IMAGE_2_EVIDENCE.md`.
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
  startup probe remains concurrent across all three origins. The fail-fast 128 MiB budget
  with a 1 MiB step grows dynamically with the actual chunked
  bytes, has a 60-second idle and a 5-minute absolute body deadline, and does not create
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
  translated into the OpenCode model/variant/Fast schema; because OpenCode 1.18 requires
  both `context` and `output` inside any native `model.limit`, a validated partial router limit is
  retained in the encrypted capability record but omitted from both live and stale OpenCode cards
  instead of aborting client startup or inventing a ceiling. The local schema-v2 last-good cache
  contains only encrypted capability records without `pricing` and `cost`, is bound to the exact
  credential/base URL, and is limited by
  schema/TTL/max-stale guards. The cached fallback is always explicitly stale and
  cost-free. The installed integration is a stable loader: a pinned Ed25519 public key,
  exact GitHub release origin/path, monotonic channel sequence, SHA-256 and atomic writes guard
  automatic runtime updates; current/previous/installer fallback keeps startup available, and a
  rollback republishes known-good bytes at a higher signed sequence. The OpenCode transport does not
  consume Gemini `inlineData`, so the plugin
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
  under a new digest; the Stage 5 main catalog includes Preview. The OpenKeys catalog
  generations pin release-manifest lineage only; admission follows the runtime, so Google
  traffic bills 1:1 like every other runtime-priceable model.
  `B2C_PRICING_TIERS` was removed with the post-cutover progressive cleanup; the flat global
  50% policy in the active release is the only B2C price authority.
- `apps/web/src/lib/models.ts` — hardcoded SEO model catalog with official prices;
  the file header requires synchronization with `crates/metering/src/{codex,gemini}.rs`.
- `apps/web/src/lib/pricing-tiers.ts` — the flat B2C 50% storefront constants
  (`B2C_DISCOUNT_PERCENT`); no tier ladder remains.
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
| `admin.apitoken.sale` | managed auth; data and `/events/{engine,openai,gemini,kimi}` routes → engine 8790/8792/8794/8803, `/admin/*` → commerce 8791, `/openkeys-admin/*` → 3410, `/partner-admin/*` → sales 3100, `/proxy-admin/*` → Authbot 8806; everything else → `apps/admin` `:3700` |
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
attempt. Separately, the logical-ID perimeter makes the four public provider/router vhosts remove the
reserved internal `X-Apitoken-Logical-Request-Id`; stable loopback origins retain the reserved
capability for the trusted router→plane hop; loopback access alone is not sender authorization.
Anthropic/OpenAI/Gemini/Combined customer routers consume that capability or generate a direct-ingress
ID before dispatch, remove the wire header, and keep only typed context. They also consume the optional
public `x-apitoken-client` into fail-open normalized attribution before auth/body/dispatch and propagate
only the typed value through synthesized leaves. After the strict logical consumer reached production
GREEN, the unified router producer began injecting one private canonical ID per admitted customer request
across every executable attempt. Codex universal Messages count_tokens is the only request-fact caller
and persists this attribution; other producers remain absent.

### systemd (`systemd/`) — service → application

`claude-api-anthropic@` → Anthropic slots 8787/8788 (current unit; `claude-api@` is legacy; the
GLM backend preview is argv-pinned off on this public unit and both combined rollback anchors until
owned live evidence plus a compliant private boundary or written permission) ·
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

`.github/workflows/production-uptime.yml` is an independent, credential-free consumer of the
public engine/provider/router health routes, Commerce and Sales readiness, OpenKeys readiness and
the Vercel status surface. It runs outside the production-host failure domain every five minutes;
`.github/scripts/production-uptime.sh` owns exact response validation and reconciles one GitHub
incident issue until recovery. The workflow has only `contents: read` and `issues: write`, and the
controlled `simulate_failure` dispatch proves issue delivery without mutating production.

### Delivery

`deploy/agent-merge.sh` — the only path into `master`; path-aware gate (classifiers in
`deploy/watchdog-lib.sh`), machine merge lock, green `deploy/watchdog` on the production
host. Full description — `deploy/README.md`, `CONTRIBUTING.md`.
