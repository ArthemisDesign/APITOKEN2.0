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
  tools; `store:false`, `stream:true`, `include:["reasoning.encrypted_content"]`, tenant-scoped
  `prompt_cache_key` and first-party-shaped `client_metadata`. Headers carry the pinned client
  identity (`originator: codex_cli_rs`, UA and `version` pinned to `CODEX_CLI_VERSION`,
  `ChatGPT-Account-ID` from the envelope) plus stable opaque installation/session/thread/window
  metadata and a per-turn id. Root session and thread ids are equal, as in the official 0.146
  client. Usage and model probes carry only the base auth/client headers. The SSE `response.*`
  stream is translated into the same internal event vocabulary the public adapters always
  consumed, so the public streaming contract is byte-identical to the app-server era.
- **Selection** mirrors the Claude fleet: conversation affinity first, then freshness of quota
  evidence, in-flight envelope, bucketed quota steering above 50% utilisation, and an atomic
  rotation cursor for ties. Cache stickiness is deliberate: tenant-scoped affinity derives stable
  opaque prompt-cache/session/thread/window identities, so one conversation reads as one
  continuous session across pool rotation without exposing the customer key. A home leaves
  rotation on an explicit provider `reached` verdict or
  an explicit provider `limit_reached`/`allowed: false` verdict, and returns a single
  OpenAI-shaped `429 + Retry-After` at the soonest window reset.
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
- **Capacity calibration is an evidence-backed workload blend.** `/wham/usage`, live response
  headers and SSE report decimal percentage utilisation. The gateway parses it without binary
  floating point into `10^-8` fraction units (one unit is `10^-6` percentage point), and every
  served turn credits the profile's exact audited API-catalog cost in integer nanoUSD. Estimator
  v7 calculates
  `capacity_nano = 100_000_000 × ΣΔspend_nano / ΣΔused_fraction_units`. This is the API-dollar
  equivalent of the workload actually served — not the subscription purchase price and not a
  permanent nominal for a plan. That distinction is required by the
  [official Codex pricing documentation](https://learn.chatgpt.com/docs/pricing): consumption
  varies by model, context, reasoning and tools.
  Each complete interval also contributes a conservative ±1-fraction-unit low/high workload
  envelope. `confidence` is deterministic evidence quality
  (`sample maturity × workload stability × quantisation resolution`), not a probability. There
  is no configured prior, EMA, WLS, float-money arithmetic or hidden fallback nominal.
  A cold snapshot alone remains an unpublished anchor, while the first confirmed positive
  utilisation movement is already counted as a complete interval with its quantisation envelope.
  A movement without positive settled spend waits for settlement catch-up. Real resets retain
  accumulated evidence and make the first complete movement of the new window immediately
  eligible; reset timestamp jitter cannot fork a window, and rollback snapshots cannot erase or
  duplicate a high-water interval. Raw observations, exact cumulative legs and CAS state live in
  the engine authority, survive restart/blue-green and are replayed on estimator upgrades.
  Calibration is fed only by wire events — reads never write — and each provider-reported duration
  (normally 5h and weekly) calibrates independently.
- **Health** is the same pure two-axis policy (`health.rs`): account
  (healthy→suspect→dead, durable in the authority) and transport
  (responsive→degraded→wedged, in-memory). A successful turn or probe is the only thing that
  clears a verdict.
- **Pricing** comes only from `metering::codex` (audited, effective-dated). Public `fast` and
  `priority` normalize to the official request value `priority`; Standard omits that value.
  Reservation keeps the conservative Fast hold. After a successful turn the accepted request is
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
- **History.** `store=true` responses persist in the tenant-bound encrypted history store
  (local + optional Redis) and are retrievable/deletable through the public routes. A response
  id from one billed account cannot be replayed by another.

## Failure and stream safety

- Retry is permitted only before the first translated native SSE event reaches the client.
- Client disconnect detaches the upstream read; the turn drains to its authoritative final
  usage before settlement, and the shutdown deadline aborts the read, settles the last snapshot
  and only then releases the background semaphore permit.
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
  metrics stay absent/null rather than publishing a false zero.
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
