# Codex (ChatGPT) OAuth subscription provider

The optional Codex provider serves the OpenAI-compatible text surface at
`https://openai.api.apitoken.sale/v1` from a pool of sealed ChatGPT OAuth profiles. It is the
Codex counterpart of the Gemini provider: native HTTPS to the ChatGPT Codex backend, encrypted
credential roster, single-flight token refresh, evidence-based window calibration — and no child
processes, pinned sidecar binaries, or ownership fences of any kind.

Public contract (unchanged from the app-server era):

| Public route | Status |
|---|---|
| `POST /v1/responses` | supported, streaming and non-streaming |
| `GET`/`DELETE /v1/responses/{id}` | supported for `store=true` responses within the history TTL |
| `GET /v1/responses/{id}/input_items` | supported |
| `POST /v1/responses/input_tokens` | supported; estimates input tokens without running a turn |
| `POST /v1/chat/completions` | supported adapter, streaming and non-streaming |
| `GET /v1/models`, `GET /v1/models/{model}` | supported; last-good live intersection with the pinned billing catalog |

Everything else on the OpenAI hostname returns an OpenAI-shaped `404`; nothing is ever forwarded
to Anthropic from it. The lenient SDK-compatibility rules (ignored sampling/store/unknown
fields, degraded forced `tool_choice`/`strict`, client-side `stop`/`max_tokens` enforcement,
reasoning summaries as `reasoning_content`, heartbeat SSE every 15 s, `x-ratelimit-*` headers on
non-stream responses) are unchanged.

## Accepted subscriptions

Sealing requires a **paid ChatGPT plan**: the id-token `chatgpt_plan_type` claim must map to
`chatgpt_plus` (Plus), `chatgpt_pro` (Pro) or `chatgpt_business` (Business/Team/Enterprise).
Free and API-key logins are rejected at purchase and at roster load. The OAuth application
identity is pinned to the official Codex public client
(`app_EMoamEEZ73f0CkXaXp7hrann`, token endpoint `https://auth.openai.com/oauth/token`); a
credential sealed under any other client id fails closed.

## Purchase and publication flow (authbot)

1. The seller completes the official device flow (`codex login --device-auth` in a PTY) through
   the same proxy the account will serve with. The bot never sees a password or second factor.
2. `codex login status` must report a ChatGPT login (API-key logins are rejected).
3. The bot reads the staging `auth.json` exactly once, extracts `access_token`, `refresh_token`,
   `account_id`, plan (id-token claim) and expiry, seals them with the account proxy into an
   AEAD envelope (`crates/codex-credential`, XChaCha20Poly1305, profile id as associated data),
   writes `<roster>/credentials/<id>.json` (0600) and republishes `<roster>/profiles.json`
   atomically (tmp+rename). The staging directory is deleted: after sealing, no plaintext token
   exists on disk, in logs, or in Telegram.
4. The engine rescans the roster on every health tick and admits the new profile on the same
   pass — no restart, no config edit, no root.

Profile ids are opaque slugs derived from the account id — never an email and never a path.

## Encrypted roster contract

```text
/srv/claude-api/data/codex/profiles.json            roster: [{id, credential_file}]
/srv/claude-api/data/codex/credentials/<id>.json    AEAD envelope (0600)
```

Layout is enforced exactly like Gemini: `credential_file` must equal
`<roster>/credentials/<id>.json`, ids must match `[A-Za-z0-9_-]{1,64}` and be unique. The
runtime and the authbot share one keyring (`CLAUDE_API_CODEX_CREDENTIAL_KEYS` /
`AUTH_BOT_CODEX_CREDENTIAL_KEYS` + `AUTH_BOT_CODEX_CREDENTIAL_ACTIVE_KID`); old keys remain
readable during rotation.

**Refresh rotation (the load-bearing difference from Gemini).** OpenAI rotates the refresh
token on every refresh with strict family reuse detection. The pool therefore:

- serializes expiry-check and refresh under the profile's credential mutex (single-flight;
  a 401 burst reuses the winner instead of refreshing per rejected request);
- re-seals the rotated envelope atomically **before** releasing the lock, so a crash never
  strands the family on an invalidated token;
- on `invalid_grant`, reloads the envelope from disk exactly once (a blue-green peer may have
  rotated first) and retries once with the winner's material.

## Environment

| Variable | Default | Purpose |
|---|---|---|
| `CLAUDE_API_CODEX_ENABLED` | `0` | provider kill switch (OpenAI-shaped disabled envelope stays stable) |
| `CLAUDE_API_CODEX_PROFILES_FILE` | `/srv/claude-api/data/codex/profiles.json` | roster location |
| `CLAUDE_API_CODEX_CREDENTIAL_KEYS` | — (required when enabled) | AEAD keyring `kid:64hex[,...]` |
| `CLAUDE_API_CODEX_BASE_URL` | `https://chatgpt.com/backend-api/codex` | native backend (loopback only with explicit opt-in) |
| `CLAUDE_API_CODEX_CLI_VERSION` | `0.146.0` | pinned official-client wire identity |
| `CLAUDE_API_CODEX_MODELS` | `gpt-5.6,gpt-5.6-sol,gpt-5.6-terra,gpt-5.6-luna,gpt-5.5,gpt-5.4` | enabled ids from the pinned price catalog |
| `CLAUDE_API_CODEX_REQUEST_TIMEOUT_MS` | `15000` | connect/control bound (`CLAUDE_API_CODEX_RPC_TIMEOUT_MS` is accepted as a legacy alias) |
| `CLAUDE_API_CODEX_TURN_TIMEOUT_MS` | `600000` | total turn bound |
| `CLAUDE_API_CODEX_TURN_SILENCE_TIMEOUT_MS` | `180000` | "is this profile still there" bound |
| `CLAUDE_API_CODEX_HEALTH_INTERVAL_SECS` | `10` | usage sweep + roster rescan cadence |
| `CLAUDE_API_CODEX_RESERVE_OVERHEAD_TOKENS` | `16384` | conservative reserve allowance |
| `CLAUDE_API_CODEX_HISTORY_*` | unchanged | tenant-bound encrypted history |

## Runtime behavior

- **Wire.** One `POST {base}/responses` per turn: Responses body with explicit base
  instructions (`""` when the client supplied none), replayed history, new input and client
  tools. Codex 0.146 emits function, Lark custom and client-executed `tool_search` definitions in
  top-level `tools`; the public parser accepts the same bounded forms as the legacy
  `additional_tools` item and translates them to one internal dynamic-tool vocabulary. Hosted
  `web_search` remains unsupported and fails closed, so custom-provider harnesses disable it in
  their isolated config. The upstream request keeps `store:false`, `stream:true`,
  `include:["reasoning.encrypted_content"]`, tenant-scoped
  `prompt_cache_key` and first-party-shaped `client_metadata`. Headers carry the pinned client
  identity (`originator: codex_cli_rs`, UA and `version` pinned to `CODEX_CLI_VERSION`,
  `ChatGPT-Account-ID` from the envelope) plus stable opaque installation/session/thread/window
  metadata and a per-turn id. Root session and thread ids are equal, as in the official 0.146
  client. Usage and model probes carry only the base auth/client headers. The SSE `response.*`
  stream is translated into the same internal event vocabulary the public adapters always
  consumed, so the public streaming contract is byte-identical to the app-server era.
- **Selection** mirrors the Claude fleet: conversation affinity first, then freshness of quota
  evidence and in-flight envelope. Within that normal health/load class, a new unpinned conversation
  first seeds every home that has no immutable calibration turn; bucketed quota steering above 50%
  utilisation and an atomic rotation cursor resolve the remaining ties. Seeding never overrides a
  resolved conversation affinity. Cache stickiness is deliberate: tenant-scoped affinity derives
  stable opaque prompt-cache/session/thread/window identities, so one conversation reads as one
  continuous session across pool rotation without exposing the customer key. A home leaves
  rotation on an explicit provider `reached` verdict or
  an explicit provider `limit_reached`/`allowed: false` verdict, and returns a single
  OpenAI-shaped `429 + Retry-After` at the soonest window reset.
- **Concurrency.** User turns have no process, per-home or per-account request ceiling or wait path.
  In-flight is only a load-balancing signal. An unlimited RAII task tracker registers every detached
  turn immediately and is closed only during graceful shutdown, when it becomes the drain barrier.
- **Blame classification.** 429/usage-limit → account fault (cooling until reset, rotation does
  not spend the transport budget); first 401 → one forced refresh + one retry on the same
  profile, second → auth quarantine; timeout/5xx/EOF → transport axis
  (responsive→degraded→wedged, wedged rebuilds the client); 400/context → client fault, never
  rotated. Nothing is ever retried after the first byte reached the client.
- **Quota evidence (verified live 2026-07-31).** `/wham/usage` returns
  `rate_limit.{allowed, limit_reached, primary_window, secondary_window}` where the provider's
  `allowed`/`limit_reached` is the ONLY hard stop: a window at `used_percent=100` with
  `allowed: true` still serves (the percentage can include usage outside this gateway). The
  background sweep reads it selectively: busy homes are fed by live traffic (response headers —
  verified names `x-codex-{primary,secondary}-used-percent / -window-minutes / -reset-at /
  -reset-after-seconds`), healthy idle homes are probed at a slow floor cadence, and only
  stale/suspicious/unprobed homes cost a request every tick — bounded in parallel so the sweep
  never becomes an upstream burst. Stale evidence never rejects and never wins a tie;
  never-arrived evidence ranks equal to fresh.
- **Soft window reserve (weekly-limit discipline).** As with the Claude fleet, selection never
  routes above `1 − base` of a window (`CLAUDE_API_CODEX_RESERVE_5H`, default 0.10 → ~90% of the
  5h window; `CLAUDE_API_CODEX_RESERVE_7D`, default 0.03 → ~97% of the weekly window; both default
  to the fleet-wide `CLAUDE_API_RESERVE_5H/7D` keys). Thresholds are jittered deterministically
  per profile (`CLAUDE_API_CODEX_RESERVE_JITTER`, default 0.02), so the fleet does not cut at one
  percent and does not look like an automaton maxing quota to zero. A home past its cap returns at
  that window's reset; under peak, when every home is past its soft cap, the filter relaxes to the
  provider's own wall (fail open: serving beats a synthetic 429).
- **Capacity calibration is an evidence-backed dual-ledger workload blend.** `/wham/usage`, live
  response headers and SSE report decimal percentage utilisation. The gateway parses it without
  binary floating point into `10^-8` fraction units (one unit is `10^-6` percentage point). Every
  successful billable turn creates one immutable event for the exact home that served it, model,
  effective Standard/Fast tier, provider-reported tier and all token legs. Registry advances in one
  transaction both exact cumulative ledgers: API replacement cost in integer nanoUSD and native
  ChatGPT subscription consumption in integer nanocredits. Estimator v10 calculates
  `native_capacity_nanocredits = 100_000_000 × ΣΔnanocredits / ΣΔused_fraction_units`; separately,
  `capacity_nano = 100_000_000 × ΣΔnanoUSD / ΣΔused_fraction_units` remains the API-dollar
  equivalent of the workload actually served. Native capacity is the like-for-like unit for
  comparing equal subscriptions; API USD per native credit is the profitability metric by model
  and tier. API USD capacity is not the subscription purchase price and cannot be a fixed nominal
  for a plan, because two identical plans serving different model/token mixes correctly produce
  different API-dollar equivalents. That distinction is required by the
  [official Codex pricing documentation](https://learn.chatgpt.com/docs/pricing): consumption
  varies by model, context, reasoning and tools.
  Storage precision is not treated as provider measurement precision. The live provider commonly
  emits whole percentages: `40%` therefore has resolution `1_000_000` fraction units (one percentage
  point), while `12.5%` has `100_000` and `12.125%` has `1_000`. Estimator v10 derives that
  conservative resolution from each fixed-point endpoint and applies half of both endpoint
  resolutions to every interval denominator. A movement no larger than its rounding uncertainty
  has no finite upper bound. `confidence` is deterministic evidence quality (`sample maturity ×
  workload/envelope stability × quantisation resolution`), not a probability. There is no
  configured prior, EMA, WLS, float-money arithmetic or hidden fallback nominal.
  A cold snapshot alone remains an unpublished anchor, while the first confirmed positive
  utilisation movement is already counted as a complete interval with its quantisation envelope.
  A movement without positive settled spend waits for settlement catch-up. The credit cutover
  starts one shared anchor for both ledgers; pre-cutover API evidence is retained as historical
  state and is never reinterpreted as zero native-credit spend. Real resets retain
  accumulated evidence and make the first complete movement of the new window immediately
  eligible. A rolling weekly reset is recognized by the joint signal of a material forward
  reset-at shift and utilisation rollback even when the shift is below half the nominal window;
  bounded reset timestamp jitter alone cannot fork a window, and rollback snapshots cannot erase
  or duplicate a high-water interval. Raw observations, immutable turn rows, exact cumulative legs
  and CAS state live in the engine authority, survive restart/blue-green and are replayed on
  estimator upgrades. Exact event retry is idempotent by an internal request id stable across
  home/transport retries. A semantic replay conflict quarantines that row without blocking later
  FIFO entries. During an estimator rebuild, an incomplete legacy API-only raw snapshot found after
  native-credit cutover remains in authority but is skipped because it has no authoritative credit
  denominator; the next tracked cumulative snapshot safely spans it. A tracked-to-untracked
  regression on the live incremental path still fails closed. Calibration is fed only by wire
  events — reads never write — and each
  provider-reported duration (normally 5h and weekly) calibrates independently. A transient writer
  failure leaves the event in a bounded FIFO, which every health sweep retries even when no new
  customer request reaches that home. On recovery the event and cumulative ledgers are persisted
  before the cached post-turn quota snapshot is replayed; retire also performs a final flush.
  Pending, dropped and persistence state are explicit in `/codex-subs`. Usage accepts current
  `cache_write_tokens` and legacy `cache_creation_tokens` as aliases and never adds both.
  The control-authenticated `/codex-subs` projection includes the reviewed non-secret paid-plan
  identity (`chatgpt_plus|chatgpt_pro|chatgpt_business`) so the admin sales calculator can aggregate
  like-for-like profiles. Its `plan_cohorts` groups exact `plan + window_minutes` identities and
  publishes one pooled native capacity per home:
  `100_000_000 × Σobserved_spend_nanocredits / Σobserved_fraction_units`. That common value is
  applied to the current unused fractions for fleet remaining capacity, so equal subscriptions no
  longer receive different commercial capacities merely because their whole-percent endpoints
  rounded differently. `measured_homes` and `homes_total` expose coverage; conservative low/high
  are the union of contributing home envelopes, and a missing upper bound remains `null`.
  Individual home estimates and immutable turn evidence remain unchanged for audit. API-dollar
  capacity is deliberately not pooled because it remains workload-dependent. Full email, account
  id, OAuth and proxy remain sealed.
- **Health** is the same pure two-axis policy (`health.rs`): account
  (healthy→suspect→dead, durable in the authority) and transport
  (responsive→degraded→wedged, in-memory). A successful turn or probe is the only thing that
  clears a verdict.
- **API replacement cost and subscription consumption are separate ledgers.** Both audited rate
  cards live in `metering::codex`, but they never convert into each other implicitly. Public API
  nanoUSD uses the effective-dated API schedule; ChatGPT quota uses the official Codex credit card
  (input / cached input / output). Cached input is a subset of input, and reasoning is a subset of
  output, so neither is double-counted. The subscription card publishes no cache-write premium or
  long-context multiplier, and the implementation does not invent either. Since 2026-07-30 the API
  rates for GPT-5.6 Terra are `$2 / $0.20 / $2.50 / $12` and Luna
  `$0.20 / $0.02 / $0.25 / $1.20` per million fresh / cached / cache-write / output tokens. The
  same effective epoch changed GPT-5.6 API Fast to `2x`, while ChatGPT subscription Fast remains
  `2.5x` credits (`2x` for GPT-5.4). This distinction prevents the API-dollar workload equivalent
  from being mistaken for a plan's fixed dollar size. Sources:
  [Codex credits](https://learn.chatgpt.com/docs/pricing#what-are-tokens-and-credits),
  [Fast mode](https://learn.chatgpt.com/docs/agent-configuration/speed#fast-mode), and the
  [2026-07-30 API changelog](https://developers.openai.com/api/docs/changelog#july-2026).
- **Request pricing** comes only from that effective-dated catalogue. Public `fast` and `priority`
  normalize to the official request value `priority`; Standard omits that value. Reservation keeps
  the audited API Fast hold. After a successful turn the accepted request is
  the effective product tier used by settlement, ledger, capacity spend and the public response:
  requested `priority` remains Fast, while Standard remains `default`. The private ChatGPT backend's
  completed `response.service_tier` is stored separately as diagnostic
  `provider_reported_tier`; it commonly says `default` for measurably accelerated Fast and must not
  drive money or placement. This behavior is confirmed by the official maintainer response in
  `openai/codex#14204` and matching reports `#30413`/`#32191`. Production A/B on 2026-08-01 measured
  `67.36` vs `102.02` median output tok/s (`1.514x`) while both paths reported `default`; all four
  Pro profiles successfully served priority turns. Model discovery retains `service_tiers` and
  legacy `additional_speed_tiers` per profile, and Fast routing prefers catalogue-supported homes
  without allowing the non-authoritative completed field to break affinity or demote a profile.
  This normalization applies to every GPT execution surface: native/universal Responses and Chat
  accept `service_tier: "fast"|"priority"`; the Anthropic Messages skin additionally maps native
  `speed: "fast"` (and the same `service_tier` aliases) to `priority`, including
  `usage.service_tier` in its Anthropic-shaped response. On `router.apitoken.sale`, harnesses that
  cannot add body fields may instead send `x-apitoken-service-tier: fast|priority`; the router
  converts it to canonical body `priority` and strips the header, so this same admission,
  reservation, settlement and effective-tier path remains authoritative.
- **History.** `store=true` responses persist in the tenant-bound encrypted history store
  (local + optional Redis) and are retrievable/deletable through the public routes. A response
  id from one billed account cannot be replayed by another.

## Failure and stream safety

- Retry is permitted only before the first translated native SSE event reaches the client.
- Native Responses events are deduplicated only by their monotonic `sequence_number`. The first
  identity observed for an `output_index` is canonical through delta and completion; a missing or
  drifted upstream `item_id` is normalized to that identity. Content is never compared or removed,
  so intentionally repeated text with distinct event sequence numbers remains byte-for-byte intact.
- Client disconnect detaches the upstream read; the turn drains to its authoritative final
  usage before settlement, and the shutdown deadline aborts the read, settles the last snapshot
  and only then releases the background task guard.
- Public errors are OpenAI-shaped and carry no pool/profile/proxy/upstream internals; the
  regression gate is `codex::api::tests::public_errors_never_leak_internal_architecture`.
- Blue-green is trivial compared to the app-server era: generations own no children and no
  home directories, so they overlap freely; candidate readiness (credential opens, token
  refreshes, one usage read per profile) is the admission gate.

## Operations

- **Provisioning** is the authbot flow above. The bot's device flow needs an official `codex` CLI
  on the host (`AUTH_BOT_CODEX_BIN`, default `/srv/claude-api/data/codex/bin/codex`); install any
  current official release there — it is used only for `login --device-auth` and `login status`,
  never for serving. Manual sealing of an existing `CODEX_HOME` (app-server era) is
  `claude-api codex-seal --home <dir> --roster <dir> --keys <spec> --active-kid <kid>
  [--delete-home]`; `deploy/codex-homes-migrate.sh --check|--apply` discovers the legacy locations
  and runs it. The legacy binary, systemd app-server units and daemon reconciliation are removed:
  `tools/codex-app-server/` no longer exists.
- **Status** stays at `GET /codex-subs` (control plane) and the Prometheus
  `claude_api_codex_*` series; `process_live` now means "credential opened and transport built",
  `ready_published` means this generation proved the profile works. The control-authenticated JSON
  includes a bounded email hint (the first four local-part characters, without the domain) beside
  the opaque home id so an operator can identify the purchased account; the full email, account id,
  OAuth and proxy remain sealed and never enter responses, logs or metrics. Each home's `fast_tiers`
  separates catalogue availability/support and effective `served_tier` from diagnostic
  `provider_reported_tier`/`observed_at`; a misleading completed `default` therefore remains
  visible without being mistaken for a Fast downgrade. Capacity windows expose canonical
  nanoUSD values as decimal strings (`capacity_nano`, remaining and low/high variants), exact
  fraction/evidence counters, `workload_dependent:true`, source `workload_blend` and rounded USD
  compatibility fields. Before the first complete interval, capacity/remaining and their dollar
  metrics stay absent/null rather than publishing a false zero. Each window also exposes
  `measurement_resolution_fraction_units`; `plan_cohorts` is the like-for-like native-credit
  planning surface, while per-home and `window_totals` remain diagnostic evidence views.
- **Runbook alerts** are unchanged in name (`CodexNoAvailableHomes`, `CodexHomeUnauthenticated`,
  `CodexHomeQuotaSnapshotStale`); their meaning maps to sealed profiles.
- **Wire verification** before enabling in production and after any `CODEX_CLI_VERSION` bump:
  run `tools/codex-native/probe-live.py` on a throwaway account and record findings in
  `research/CODEX_NATIVE_WIRE.md` (HTTP vs WS, exact rate-limit header names, current client
  version, ClientHello acceptance).

## Deprecation note

This provider replaces the pinned `codex app-server` transport (see git history for
`docs/CODEX_APP_SERVER.md`). The earlier document's direct-backend audit conclusion is
superseded: direct native access is now implemented with the Gemini discipline — encrypted
roster, pinned official client identity, single-flight refresh with durable rotation, and no
impersonation beyond what the official client itself presents.
