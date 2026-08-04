# Onboarding a new subscription provider to GA

This is the canonical playbook for adding to Claude_API a new AI provider whose capacity comes
from user/corporate subscriptions, OAuth profiles, service accounts, or a similar model.
The goal is not "learn to send one request", but to bring a separate provider-plane up to the
Claude/Codex/Gemini level: safe replenishment via Auth Bot, sticky parallel pool, exact money,
evidence-based calibration, a complete admin panel, blue-green, monitoring, and a live production
audit.

This document answers the question **what must be proven**. The question **what exactly to edit** —
exact files, symbols, commit order, and traps that have already fired — is answered by
`docs/engine/PROVIDER_WIRING_CHECKLIST.md`. Read both: principles without the map give a correct
but slow traversal; the map without principles gives a fast but wrong provider.

This document is mandatory together with the root `AGENTS.md`, `CLAUDE.md`, `BRANCHES.md`,
`docs/CHANGE_CHECKLISTS.md`, and `docs/DEPENDENCIES.md`. The local `CLAUDE.md` of every affected
crate/app is read before editing. If this document diverges from the current code or a local
instruction, the current checkout is authoritative; the divergence must be fixed in the same
change.

## 1. What counts as a provider and what counts as GA

A new model inside an already finished provider-plane goes through the "New model" checklist. A new
payment method goes through a separate payment-provider checklist. This playbook applies when new
credentials, upstream transport, subscription quota/credits, a pool, or a separate runtime appear.

Integration states:

- **research** — facts are being gathered; there are no production promises;
- **preview** — additive/runtime code may be in production behind a disabled flag, but unknown
  plans, models, money, or calibration are explicitly visible;
- **GA** — every applicable gate of this document is proven on the exact production SHA.

A merge, a green build, a mock 200, a model row in the catalog, or one live non-stream request do
not by themselves mean GA.

### Terminal GA criterion

The provider is GA when all of the following hold simultaneously:

1. Subscription names, models, tier availability, official pricing, OAuth/API contract, and
   limits are dated and confirmed by sources; there are no unresolved contradictions.
2. For every published plan × model × tier/capability row there is applicable live
   evidence: auth/refresh, non-stream, a real incremental stream, authoritative usage, quota, and
   sanitised errors.
3. The credential is sealed, atomically published, rotated, and survives restart/blue-green without
   plaintext, duplicate identity, or refresh-family race.
4. Auth Bot fully walks a newcomer from purchase/proxy/plan activation to atomic publication;
   cancel, retry, batch, crash, and restart neither pay out nor move someone else's deal.
5. Every request starts immediately: no process/account semaphore, queue, slot wait, or
   artificial concurrency reject. Sticky affinity, provider quota, and honest cooling are preserved.
6. Retry/rotation is possible only before the first public byte. A disconnect drains the upstream
   to usage and settlement; after the first byte account replay is forbidden.
7. The money lifecycle is durable: reserve → delivering → exact settlement/refund; idempotency
   survives retry, disconnect, writer failure, and restart.
8. Official API replacement cost and native subscription consumption are accounted separately.
   Window calibration is built from immutable raw evidence without an invented plan nominal/prior/EMA.
9. Admin UI, catalog, commerce/OpenKeys/web consumers, monitoring, alerts, runbook, rollback, and
   provider docs are complete.
10. All selected/full gates are green, the exact master SHA has `deploy/watchdog GREEN`, and the
    post-deploy smoke through the public production endpoint has passed.

## 2. Agent work and Goal mode

For a long integration, a single goal is created only when the user explicitly asks to work in Goal
mode. The objective must contain the terminal criterion: "provider X brought to verified production
GA per `docs/engine/PROVIDER_ONBOARDING.md`". The goal must not be closed after research, merge, or
preview.

The agent maintains a plan along at least these phases:

1. official/GitHub/live research and capability manifest;
2. architecture, dependencies, migration/contract rollout;
3. credential + Auth Bot;
4. runtime/pool/streaming;
5. billing + calibration;
6. admin/product surfaces;
7. observability/blue-green/tests;
8. production deploy + live GA audit.

A missing Ultra/Enterprise subscription or vendor approval blocks only the dependent live gate.
The agent continues all safe mock/code/docs/test tasks, then reports the exact missing evidence
and the minimal human action. Do not carry over a result from another tier or invent
availability. Goal `complete` is set only after the terminal criterion; `blocked` — only per
the Goal mode tool's rules, not because the task is large.

## 3. Research: do not work blind

### 3.1 Evidence hierarchy

Every significant claim gets one label:

- `official` — current provider-owned docs/schema/model card/pricing/plan/OAuth/terms/changelog;
- `live` — sanitised observation on a subscription we own, with date, plan, region, and version;
- `oss-hypothesis` — hypothesis from a pinned third-party source;
- `decision` — our architectural decision, tied to evidence;
- `unknown` — not established;
- `not-applicable` — ruled out with a reason.

Trust order: official normative contract → our own live wire → multiple independent OSS
implementations → community issues. GitHub helps understand the implementation, but it is not an
authority on pricing, plans, permission, or GA.

### 3.2 How to study GitHub safely

1. Search not only the provider name, but the exact endpoint, rare header, wrapper key, OAuth
   client id, model alias, and error text.
2. Choose, where possible, at least two independent active implementations; a fork/copy counts
   once.
3. Record the repo URL, full commit SHA, license, last activity, relevant paths, and the concrete
   hypothesis the code confirms.
4. Clone read-only into `mktemp -d`, read via `rg`; do not run anyone else's install/postinstall,
   binary, curl, or script.
5. Compare field-by-field: URL/query/method, headers, OAuth/PKCE/redirect/refresh, body wrapper,
   streaming framing, terminal usage, quota/reset, model translation, and errors.
6. Delete the temporary clone/capture after the research. Copy nothing into the product worktree
   without understanding the license and a security review.

You must not store access/refresh tokens, cookies, email/account/subject/project, authenticated
proxies, raw prompts, or full private errors. Live tests — only on owned/permitted accounts;
no bypassing CAPTCHA, access control, or provider limits.

### 3.3 Capability manifest

At the start, create `docs/engine/<PROVIDER>_PROVIDER.md` with the following tables.

**Product/plan:** exact marketing names, regions, account types, cadence, quotas/credits,
models by tier, automation/redistribution/API terms, review date.

**Credential:** grant, issuer, auth/token endpoints, official client, scopes, redirect, PKCE/state,
refresh rotation, duplicate identity, proxy/geography, revocation.

**Model admission:**

| Public model | Native model/control | Official plans | Live-tested plans | Price epoch | Non-stream | Incremental stream | Usage | Quota | Decision |
|---|---|---|---|---|---|---|---|---|---|

**Wire:**

| Operation | URL/query | Headers | Body/wrapper | Framing | Usage | Errors/retry |
|---|---|---|---|---|---|---|

**Money/quota:** disjoint usage legs, overlap rules, official rates, native credits, buckets,
duration/reset, scale/resolution, hard-stop signal, stale behavior.

Critical distinctions:

- a model and price existing in the Developer API does not prove the subscription route;
- a quota/catalog row does not prove generation;
- a non-stream 200 does not prove streaming;
- `stream=true` with a single buffered frame is not an incremental stream;
- Pro success does not prove Ultra/Enterprise, and vice versa;
- a private route that technically works does not cancel the terms/compliance review.

An unknown future model/tier always fails closed. The minimal dependent surface is blocked, but
not the entire provider, if identity, money, and the safety of the remaining rows are already proven.

## 4. Architecture and boundaries

Engine layers are unchanged:

```text
registry <- pool <- forward <- server
```

- `registry` — sole owner of engine PostgreSQL and durable authority; no HTTP/provider network;
- `pool` — selection/affinity/cooling state machine without HTTP/network;
- `forward` — provider transport, protocol translation, stream lifecycle, billing/calibration
  orchestration; does not own PostgreSQL;
- `server` — composition, the only reader of engine env, fixed plane, HTTP/control/readiness;
- `metering` — pure integer math/JSON, only exact pricing;
- `<provider>-credential` — pure AEAD envelope, no network;
- `authbot` — credential producer ahead of runtime; does not import pool/forward/server;
- `router` — stateless HTTP to stable origins, without registry/billing/retry/queue/provider health;
- `apps/admin` — HTTP consumer without its own DB/secrets.

Before designing, compare the current:

- `docs/engine/CODEX_PROVIDER.md`, `docs/engine/GEMINI_PROVIDER.md`;
- `crates/forward/src/{codex,gemini}/**`, `crates/pool/**`, `crates/registry/**`;
- `crates/metering/src/{codex,gemini}.rs`;
- `crates/authbot/src/{codex_login,gemini_oauth,setup_token}.rs`;
- `apps/admin/src/app/subscriptions/**`;
- `docs/engine/UNIFIED_ROUTER.md`, `crates/router/**`;
- `deploy/engine-bluegreen.sh`, `deploy/watchdog*.sh`, `deploy/Caddyfile`, `systemd/**`;
- `observability/**`, `docs/ops/MONITORING.md`.

A new distinct auth/quota/backend usually gets its own fixed provider-plane, two slots,
a stable loopback origin, and a router namespace. Its outage/readiness must not stop other
planes or turn router `/health` into a conjunction of all providers.

## 5. Delivery order and compatibility

Schema and cross-context contract changes are expand-only, producer-first. Typical order:

1. research/provider-doc skeleton;
2. additive engine migration + dormant registry API → merge command → exact watchdog GREEN;
3. credential crate + metering;
4. disabled runtime producer/control DTO → watchdog GREEN;
5. dependent contracts/engine-client/commerce consumers → watchdog GREEN;
6. Auth Bot and admin/product surfaces;
7. systemd/Caddy/blue-green/observability behind a disabled switch;
8. controlled live preview/calibration;
9. reviewed catalogue/policy/provider activation;
10. public smoke and GA report.

Never modify an existing migration. A new field/route is added before its consumer. The old runtime
must survive the expanded schema. Removal/rename/semantic replacement is a separate final phase.

The new provider default stays disabled until the preview gate. Do not change global Claude/Codex/
Gemini semantics for the convenience of a new transport. A shared refactor must preserve all their
tests and public bytes.

## 6. Credential and atomic roster

For OAuth/secret material, create `crates/<provider>-credential` and a local `CLAUDE.md`.
The minimal contract:

- versioned XChaCha20-Poly1305 envelope (or the currently approved AEAD), explicit `kid`;
- profile id + credential kind in the AAD;
- the keyring reads old keys, writes with the active key, and supports online rewrap;
- bounded strict fields; secret types have no leaking `Debug`/errors;
- envelope 0600, directory 0700, correct owner; symlinks/alternate paths are forbidden;
- temp file on the same filesystem, fsync + atomic rename;
- the roster contains only opaque id and the exact credential file;
- the envelope is written completely first, then the atomic roster;
- a bad reload preserves the last-good pool.

Stable provider subject/account identity is the quota/dedup authority. Raw identity is sealed.
Only the opaque id/permitted short mask is published. Reject: duplicate identity, duplicate
authenticated proxy (if isolation is needed), wrong issuer/audience/client/kind/plan, unknown tier,
permissive mode/path escape.

A rotating refresh family requires per-profile single-flight: the winner atomically re-seals the
token before releasing the lock. In a race between two blue-green generations the loser re-reads
the envelope once and uses the winner; the old refresh token must never be reused without control.

## 7. Auth Bot: full-fledged onboarding

A separate offer/handoff kind is added without weakening the Claude/Codex/Gemini state machines:

- exact plan menu and offer validation;
- single/batch seller lock and item generation;
- proxy selection/issuance and credential-free preflight;
- newcomer instructions: set up the proxy before opening the account, do not change profile/IP,
  activate the exact plan;
- official OAuth/device flow with state + PKCE and isolated staging;
- plan/issuer/audience/identity validation;
- seal + atomic publication before payout completion;
- cancel/retry/expiry/pause/resume/restart/crash semantics;
- admin jobs/status without secrets.

The seller never sends the operator a password, 2FA, cookie, card data, OAuth token, or proxy URL.
The bot does not print secrets/private errors. The callback form is one-time/no-store/bounded.
A failed, expired, or wrong-plan flow leaves no credential/roster row and does not complete the
payout. A retry gets a new generation but keeps the exact seller/job/item and the assigned egress.

Live acceptance is performed for every supported plan: acquire → publish → runtime refresh →
remove/revoke, including one restart on an unfinished flow.

## 8. Runtime and the sticky pool

### 8.1 Request lifecycle

One internal CSPRNG `request_id` lives through all pre-byte attempts:

1. authenticate the customer, canonical model/tier, exact conservative hold;
2. durable reserve;
3. sticky affinity or selection of an eligible profile;
4. single-flight credential refresh;
5. native request and startup classification;
6. retry/rotation until the client has received a public byte;
7. atomic mark `delivering` before the first public byte;
8. incremental translation;
9. client disconnect stops delivery, but a bounded task drains the upstream;
10. terminal authoritative usage → immutable evidence → exact settlement;
11. release guards and health/quota update.

Shutdown closes admission first, then waits for detached drains. On deadline: abort the read,
conservatively settle the last documented state, cross the task barrier, then flush the billing
writer.

### 8.2 No local concurrency limits

Process/per-profile/per-account semaphores, admission queues, wait-for-slot, and synthetic 429 are
forbidden. Every admitted request immediately starts an upstream attempt. `inflight` is only a
placement signal. The real limits remain provider `allowed/limit_reached`, quota reset,
auth/transport/model health, and per-request memory/body/time bounds.

The concurrency test uses a barriered mock: N independent upstream requests must start before
the mock releases even one response. Sequential successful completion does not prove parallelism.

### 8.3 Selection and affinity

Order of preference:

1. healthy tenant-scoped conversation affinity;
2. account/model eligibility and explicit provider wall;
3. fresh quota above stale, never-seen neutral;
4. account/transport/model health;
5. inflight;
6. coarse quota steering only near the wall;
7. atomic rotation cursor.

New sessions are spread; sticky preserves cache/thread continuity. The soft reserve is jittered
deterministically per profile, but if all working profiles have crossed the reserve and the
provider still permits work, the pool fails open until the real wall.

### 8.4 Health and retry

Separate the axes:

- durable account/auth `healthy → suspect → dead`;
- in-memory transport `responsive → degraded → wedged`;
- model generation streak/cooldown, if one model can break independently;
- provider quota bucket + reset, not generic health.

Typical policy, refined by live evidence:

- first 401 → forced refresh + same-profile retry; repeated 401/403 → auth quarantine + rotate;
- 429 quota → cool the exact model/account scope until the parsed reset, rotate without transport
  budget;
- timeout/network/408/409/425/5xx before bytes → bounded transport/model rotation;
- deterministic context/schema/safety 4xx → client/provider semantic error, no rotate/blame;
- malformed stream/wrapper → fault before bytes or a sanitised terminal error after bytes.

Only a successful generation or an equivalent provider probe clears the corresponding fault.
`countTokens` does not rehabilitate the generation route if those are different backend paths.

### 8.5 Streaming

You must separately prove the endpoint/query/Accept/body variant, multiline/partial framing,
incremental arrival, terminal usage, and mid-stream error. Bounds: startup time/bytes/chunks and
accounting-only silence after the first event. After the first public byte, replay/account switch is
forbidden. Truncation cannot become a synthetic clean completion. A buffered fallback is labelled as
non-stream/buffered, not as a real stream.

## 9. Exact money and settlement

`crates/metering/src/<provider>.rs` is the sole authority on the official rate card:

- effective-dated model/tier/geography schedule;
- integer nanoUSD (`1 USD = 1_000_000_000`), checked rational/rounding where needed;
- disjoint fresh/cached/cache-write/output/reasoning/audio/image/search/tool/long/speed legs;
- explicit subset rules so cached/reasoning are not counted twice;
- official URL, review date, canonical aliases, and exact vector tests;
- unknown price/model/tier fails closed before reserve.

Money lifecycle: reserve a conservative hold → same reservation through pre-byte rotation → mark
delivering → terminal exact cost → durable settlement outbox exactly once. On failure before
delivery, refund; after delivery, missing usage uses the documented conservative hold/last-snapshot
policy and an operational counter. RAII and restart must not leave money in an undefined state.

Upstream/client request ids are audit metadata, not money identity. An exact replay of one semantic
event is idempotent; a different payload under the same internal id is a typed conflict.

## 10. Calibration — Claude/GPT level

Calibration is a backend evidence system, not a frontend formula and not a one-off benchmark. Its
job is to answer how much official API replacement cost actually fits into the provider window for
the observed workload, and — when native consumption exists — also how many native units the window
contains. The subscription's purchase price does not affect this calculation.

Before implementing, read the current:

- `crates/forward/src/anthropic_calibration.rs` — the Claude estimator without invented native
  credits;
- `crates/forward/src/codex/calibration.rs` — the GPT dual-ledger estimator and quantisation
  envelope;
- `crates/registry/src/provider_calibration.rs` and the PostgreSQL parity — immutable turn ledger,
  cumulative subject spend, observations, and CAS;
- `tools/claude_calibration/{run_live.py,test_run_live.py}` and
  `docs/ops/CLAUDE_CALIBRATION.md` — the safe live calibration runner;
- `docs/engine/CODEX_PROVIDER.md` — native-credit cohorts and the separation of API/native
  economics.

### 10.1 Choosing the ledger model

Official API replacement cost and native subscription consumption are different quantities:

- A **Claude-like provider** publishes a quota fraction but not native consumption. An exact API
  nanoUSD ledger is stored, and the estimator publishes only the realized workload blend. Native
  credits are absent — they are not computed from dollars.
- A **GPT-like provider** publishes both the fraction and authoritative native consumption. API
  nanoUSD and native units run as two independent cumulative ledgers; one is never reconstructed
  from the other.
- An **unknown provider unit** is stored as separate raw evidence until live evidence proves its
  semantics. `remaining_amount`, provider credits, and API tokens must not be treated as
  interchangeable.

API-dollar capacity does not equal the subscription price and need not match across identical
plans if they have a different model/token/tool mix. Identical subscriptions must be compared by
native capacity if the provider genuinely publishes native consumption; otherwise only
like-for-like observed workload with an honestly stated blend is compared.

### 10.2 Exact turn evidence and durability

Every successful billable turn with authoritative terminal usage creates one immutable event:

- provider and opaque subject/profile;
- the internal CSPRNG request id, unchanged through all pre-byte retries;
- canonical requested/served model, accepted effective tier, and provider-reported tier separately;
- inference geography/capability modifiers;
- effective-dated tariff schedule id and priced/completed timestamps;
- all non-overlapping usage legs: fresh/audio/cache read/cache write TTL/output/image/search/tool;
- subset counters (reasoning/thinking/tool prompt) with explicit invariants;
- exact official API nanoUSD legs and total; authoritative native legs, if they exist.

First, metering checks overlap and integer bounds. Then a single authority transaction inserts the
event and advances the cumulative subject ledgers. An exact replay of the same payload is
idempotent; a different payload under the same request id is a typed conflict. The aggregate/report
is built from immutable rows but never replaces them. The customer discount/multiplier does not
enter the calibration event.

Between the stream finalizer and the authority there must be a bounded FIFO:

1. the event is enqueued before the post-turn quota probe runs;
2. a transient writer failure leaves the head pending, so a later turn or a free poll does
   not see quota relative to an outdated cumulative spend;
3. an exact ambiguous database reply is safely replayed through immutable idempotency;
4. a semantic replay conflict is quarantined, increments dropped, and does not block the whole
   tail;
5. the health sweep repeats the flush even without new customer traffic; retire and graceful
   shutdown perform a final flush;
6. projection/metrics publish `pending_events`, `dropped_events`, `persistence_ok`, authority
   availability, and the queue limit. While delivery is degraded, sellable capacity is not
   considered fresh.

### 10.3 Exact quota observations

Provider utilization is parsed from a decimal string/header into fixed point, not through binary
float:

```text
FRACTION_SCALE = 100_000_000
0%   = 0
100% = 100_000_000
```

Alongside the value, the endpoint's real resolution is stored. For example, `40%` has a resolution
of `1_000_000` fraction units, `12.5%` — `100_000`, `12.125%` — `1_000`. PostgreSQL bigint
precision does not make a coarse whole-percent snapshot precise.

Each immutable observation contains the exact subject, authoritative paid plan, provider bucket,
window kind/duration, reset evidence, used fraction, measurement resolution, observed timestamp,
cumulative API/native ledgers, source (`response` or free `poll`), source request id, and estimator
version. 5h, 7d, and any provider-native durations live independently. Reads never create
observations.

### 10.4 Interval state machine

- The first snapshot is an anchor, not a sample.
- The first subsequent positive fraction movement with a positive settled delta already publishes
  an estimate; there is no need to wait for an arbitrary number of samples.
- Response quota arriving before settlement holds the anchor until ledger catch-up.
- Repeated quota-only movement becomes `unattributed_fraction_units` and is not attributed to
  gateway spend.
- A rollback with a real reset starts a new interval but does not erase the complete history. A
  rolling reset is determined by joint utilisation rollback and material reset advance; bounded
  timestamp jitter by itself does not fork the window.
- Returning to an old high-water after a rollback is not new spend.
- A new native-ledger cutover sets a common new anchor for both current estimators. Old API
  evidence remains history and is not interpreted as zero-native spend.
- Stale/duplicate observations do not change state. Invalid regression/identity/duration/resolution,
  negative delta, and integer overflow fail closed.
- When the estimator version changes, state is deterministically rebuilt from the immutable
  observation history; a stored old derived value is not considered authority.

### 10.5 Capacity, uncertainty, and maturity

For each subject + plan + bucket + duration separately:

```text
capacity_nanoUSD = round_half_up(
  FRACTION_SCALE * Σ(delta_api_spend_nanoUSD) / Σ(delta_used_fraction_units)
)

native_capacity = round_half_up(
  FRACTION_SCALE * Σ(delta_native_consumption) / Σ(delta_used_fraction_units)
)
```

The native formula exists only with an authoritative native ledger. All operations are checked
integer/i128/rational math. Subscription-price priors, plan nominals, EMA/WLS, float money, and
hidden fallbacks are forbidden.

On each interval the denominator is widened by half the resolution of both endpoints. Low uses
`delta + uncertainty`, high — `delta - uncertainty`. If the movement does not exceed the
uncertainty, a finite high is not mathematically proven and `null` is published, not a guessed
ceiling. The overall estimate uses all complete intervals; the envelope conservatively covers the
contributing samples. `confidence` is deterministic maturity × envelope stability × quantisation
quality, not a probability.

The projection must publish decimal integer strings: current capacity/remaining, low/high,
samples, observed fraction/spend/native consumption, resolution, confidence/maturity, last measured,
reset, source/version, unattributed, and persistence state. Cold/unknown is `null`, not `$0`.
A fresh exact fraction without a reset may update **current remaining**, but it does not prove the
next horizon/reset; a stale fraction must not look sellable.

### 10.6 Cohorts and identical subscriptions

Like-for-like aggregation is allowed only for the exact paid plan + native bucket/duration/schedule:

```text
pooled_native_capacity = FRACTION_SCALE
  * Σ(native_consumption)
  / Σ(used_fraction_units)
```

Equal plans get one shared cohort capacity applied to their current unused fraction. This
removes false scatter from whole-percent rounding and different sample counts. Different plans are
never mixed; a missing plan blocks cohort aggregation. Per-home raw evidence and bounds are
preserved. Workload-dependent API-dollar capacity is not turned into a promised plan nominal.

### 10.7 Deterministic test gate

Estimator/authority tests must cover:

- cold anchor and the first complete interval;
- exact fractional evidence and whole-percent unbounded high;
- every admissible true boundary inside the quantisation envelope;
- mixed model/tier/token/tool workload and disjoint leg totals;
- quota-before-settlement and repeated unattributed movement;
- reset, rolling rollover, reset jitter, rollback/high-water, and independent durations;
- native-ledger cutover and legacy incomplete-history rebuild;
- estimator-version replay from immutable history;
- exact event replay, changed-payload conflict, CAS/idempotency, and SQLite/PostgreSQL parity;
- transient FIFO failure/recovery, conflict quarantine, overflow/drop health, and shutdown flush;
- remaining/bounds from the exact current fraction;
- invalid identity/window/resolution, negative/regressing cumulative values, and overflow fail
  closed.

### 10.8 Safe live calibration runner

For a new provider, create `tools/<provider>_calibration/run_live.py`, offline tests, and an ops
runbook following the example of `tools/claude_calibration`. The runner is part of calibration
acceptance, not a one-off script:

- dry-run by default; paid traffic only with `--execute`;
- integer `--budget-usd`, a hard maximum no higher than explicitly permitted by the user, a
  worst-case bound, and a budget guard for every possible serving profile;
- exact admin-only target/session without spill/rebind; hard provider wall/cooling/dead remain
  insurmountable;
- baseline `pending=0`, `dropped=0`, persistence/authority healthy, and an authoritative plan;
- free count/preflight, if the provider has one; the bound includes a full cache miss, max output,
  server-side tool/search payload, and all per-call units;
- the full model × supported tier × context × token/cache/media/tool matrix; proven unavailability
  is recorded, not hidden;
- unique run id and cache salt; only the expected write/read share a cache key;
- after a paid response, attribution waits for exactly one new immutable event with the exact
  request id, profile/model/tier, and full usage/cost vector. Concurrent traffic is ignored by id,
  and ambiguity fails closed;
- retry only read-only discovery/count/capacity. A paid request is not automatically repeated after
  transport ambiguity;
- the report contains the exact spend per profile, before/after fraction for each window, records,
  coverage, unavailable capabilities, profile stops, final capacity, and profitability only for a
  positive observed delta.

Runner tests cover budget/rebind, exact attribution against background third-party traffic,
ambiguity, cost-vector integrity, capability coverage, alias/global ceiling, cache isolation, safe
retry, secret containment, incomplete report, and profitability ordering. Mock tests prove the
guards; the real provider contract is proven only by a controlled run on owned subscriptions.

## 11. Current subscriptions control-room — the UI reference

The main admin panel after `ea5a07a` is deliberately not a calibration laboratory. Before adding a
provider, re-read the current `origin/master`:

- `apps/admin/src/app/subscriptions/fleet-capacity-overview.tsx`;
- `apps/admin/src/app/subscriptions/provider-board-ui.tsx`;
- `apps/admin/src/app/subscriptions/{claude,codex,gemini}-capacity-board.tsx`;
- `apps/admin/src/app/subscriptions/{provider,codex}-calibration.ts` and tests;
- `apps/admin/src/app/subscriptions/types.ts`, `page.tsx`, `page.test.tsx`;
- `apps/admin/src/app/globals.css` and `docs/product/ADMIN_PANEL.md`.

Reference information hierarchy:

1. At the top, a single control-room of provider cards. Each shows only two truly comparable rails
   (currently 5h/7d): current API-$ remaining / full calibrated window, used share, ready/total
   identities, and measured coverage. A provider with other durations shows its real windows,
   not artificial 5h/7d.
2. Below, each provider has one compact account/profile table. On the left, a sticky bounded email
   hint and plan/state; then quota/reset and exact remaining/full API-$ over the real windows. GPT
   can additionally show remaining native credits and two brief API-$ scenarios computed through
   the authoritative native/API rate cards.
3. A filled quota bar means the **already used** share. Next to it, the exact display percent;
   below, the reset.
4. A dead/non-routable row says `вне ротации` ("out of rotation") and is not included in capacity.
   Pending/stale evidence says `сохраняется` ("persisting"), `обновляем` ("refreshing"), or
   `ждём данные` ("waiting for data"); `null` is not turned into zero/prior.
5. UUIDs, full emails, raw ledgers, schedules, transport/proxy, private quota buckets,
   token-capacity, and profitability matrices are not shown in the primary UI. The backend keeps
   them for audit/replay.
6. One identity yields one row regardless of the number of models/windows. Money arrives as decimal
   strings and is processed with BigInt; model availability stays a compact count, if the operator
   needs it.
7. The visual language reuses `FleetCapacityOverview`, `ProviderSection`,
   `ProviderQuotaMeter`, `TableCard`, the existing colors/spacing/sticky column, and responsive
   horizontal overflow. A new provider does not create a separate design system.

For GPT, the exact conversions remain when a brief summary/home needs them:

```text
token_capacity = remaining_native_nanocredits / native_nanocredits_per_token
api_value_nanoUSD = round_half_up(
  remaining_native_nanocredits * api_nanoUSD_per_token
  / native_nanocredits_per_token
)
```

Context/speed multipliers are applied on the same integer half-up boundaries as server metering.
Cache write may use native fresh-input credits at a separate API cache-write price;
reasoning may be a subset of output. The front does not invent rates and mirrors the exact overlap
rules only for compact money values, rather than unfolding analytical matrices.

SSR/component tests are mandatory for exact values, the privacy mask, null/stale/dead/pending,
coverage, ordering, duplicate prevention, and the explicit absence of removed analytics. After the
build, a visual review of desktop/mobile is performed; long tables must not break the page width
or lose the left identity.

## 12. Admin, catalogue, and product surfaces

The provider read/control projection contains only privacy-safe data:

- opaque id and the permitted bounded mask;
- the exact paid plan;
- auth/live/readiness, account/transport/model health;
- quota/cooling/reset and inflight as activity;
- calibration windows, resolution, native/API capacities, bounds/confidence/samples;
- exact plan+duration cohorts;
- bounded transport/runtime attestations.

Forbidden: full email, external account/subject/project/org, token/cookie/proxy, credential path,
private error/trace.

The UI must correctly show null, zero, 100-but-allowed, stale, pending, reset, unbounded high,
model failure, cohort coverage, and duplicate input. The same profile must not multiply because of
joins on windows/models. Component/calculation/page tests, responsive/mobile,
accessibility, and `docs/ops/FRONTEND_VISUAL_QA.md` are mandatory.

Check all mirrors from `docs/CHANGE_CHECKLISTS.md`:

- metering/canonical models;
- `packages/contracts`, versioned product catalog, and provider switches/policies;
- router namespace/aliases/`/v1/models`;
- the OpenKeys fail-closed catalog;
- commerce worker/pricing policies;
- customer web/docs/SEO/integration builder;
- admin subscriptions and sales calculator.

A new model does not become saleable automatically. Catalogue/switch/policy are prepared immutable,
runtime capability pins are verified, the producer deploy goes first, and activation happens after
live evidence.

## 13. Blue-green, readiness, and rollback

A separate provider-plane gets two slot units/ports, a stable loopback Caddy origin, a shared sealed
roster/keyring + PostgreSQL authority, but no shared process-local state. Promotion starts the
candidate in the inactive slot and does not stop the active one beforehand.

Candidate readiness fails closed on invalid env/base/keyring/permissions, an unavailable authority,
zero live authenticated homes, total auth/refresh failure, a missing model/rate catalog,
helper/client attestation mismatch, or inability to settle correctly. Quota exhaustion is a capacity
state, not process death. One usable subscription is real capacity; an arbitrary minimum fleet is
forbidden.

Rollback is the previous tested immutable release, without schema rollback. The deployment
selector, candidate validation, and exact-SHA verification include the new plane. Only the
watchdog/blue-green controller performs production migrations.

## 14. Metrics, alerts, and runbook

Only fixed/low-cardinality metrics:

- profiles by account/transport/model health;
- attempts/success/bounded failure classes;
- pre-byte rotation and post-byte terminal error;
- inflight/detached drains;
- quota freshness/exhaustion/model cooling;
- refresh/roster reload;
- reserve/settlement/refund/outbox/missing usage;
- calibration observations/samples/pending/dropped/conflict/unattributed/version;
- stream startup/first-event/turn latency.

No raw profile/customer/email/account/project/proxy/prompt/request id/provider error labels.

Alerts and same-named anchors in `docs/ops/MONITORING.md` are added in a single change: zero usable
homes, auth spike, all quota exhausted/stale, settlement backlog/failure, missing usage,
roster/refresh/key rotation failure, calibration loss/conflict/unattributed, stream error/latency,
blue-green mismatch. Runbook: impact, safe diagnosis, provider status, kill switch, replacement,
rollback, evidence.

## 15. Full test matrix

- credential seal/open/AAD/key/version/mode/path/symlink/rewrap/duplicates;
- OAuth state/PKCE/replay/expiry/wrong issuer/audience/client/plan;
- Auth Bot single/batch/locks/cancel/retry/pause/resume/restart/publication faults;
- metering exact rates, aliases, overlap, long/speed/media, and overflow;
- registry SQLite/PostgreSQL parity, migration, immutable replay/conflict/CAS/outbox;
- request/response/error/stream fixtures for all operations;
- barriered burst of 50+ simultaneous starts, sticky + unbound spread;
- 401/429/5xx/network/malformed pre-byte rotation budget;
- one-byte boundary: after it, retry/account switch = 0;
- disconnect drain, missing usage, exactly-once settlement, and shutdown deadline;
- roster reload/remove/refresh race/blue-green overlap;
- router catalogue/native/adapted routes and provider outage isolation;
- admin BigInt calculations/null/cohorts/duplicates/visual QA;
- systemd/Caddy/deploy/monitoring regressions;
- the full existing Claude/Codex/Gemini regression gate.

Mocks prove deterministic edges; live tests prove the real contract. One must not replace the other.

## 16. Production live matrix

For every advertised plan/model/tier/capability on owned subscriptions, record the date, region,
client/runtime version, and code SHA:

- auth + refresh;
- catalogue/native route;
- non-stream + canonical model + non-zero authoritative usage;
- incremental stream (several public arrivals, if the output allows);
- token count/estimate, if advertised;
- quota movement/reset/hard stop;
- representative deterministic 4xx and exhaustion classification;
- official-rate settlement;
- proxy/geography behavior.

After landing, the test goes through the public production hostname/router with a dedicated test
key/account: models list, non-stream, stream, sticky, parallel burst, safe client error, exact
charge, admin/metrics, calibration persistence, and exact release origin. Quota burn is minimised;
immutable money/calibration evidence is not deleted.

## 17. Final report

The report contains:

- the exact master SHA and the `deploy/watchdog` verdict;
- enabled/disabled plans/models/surfaces with reasons;
- the dated live matrix;
- official schedule ids/epochs and the estimator version;
- calibration maturity/coverage/unknowns;
- Auth Bot acceptance;
- concurrency/stream/settlement fault results;
- units/slots/origins/readiness/rollback;
- metrics/alerts/runbook;
- residual risks and intentionally unsupported capabilities.

Declaring GA is forbidden with an unknown advertised tier/model, missing authoritative usage, fake
streaming, a secret leak, local concurrency wait, retry after bytes, lost disconnect settlement,
nominal/EMA calibration, an unfinished Auth Bot rollback, missing blue-green/monitoring, or while
the exact production watchdog/public smoke are not green.
