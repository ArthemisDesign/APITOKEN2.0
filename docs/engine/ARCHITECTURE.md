# claude-api architecture

A pool of Claude subscriptions as an **Anthropic-compatible `/v1` API** with an explicit subscription
OAuth request-persona adapter. One Cargo workspace, layered structure — each layer knows only about
the layers below. Rules for agents — `CLAUDE.md` (root and per-crate).

## Request flow

```
Client (Anthropic SDK / curl)  ──POST /v1/messages (our api-key)──►  claude-api
                                                                        │
  server::http (router) ── authenticates the client, serves fallback ──►  forward::forward
                                                                        │
  forward: automatically derives cache-lineage (native session header   │
          or canonical history prefixes), L1→Redis affinity;            │
          pool::route_affinity → pin / placement / wait / spill; retries→│
          pool::pick (least loaded),                                    │
          subscription Bearer + its proxy                               ▼
                                                            api.anthropic.com
                                                                        │
  response (incl. SSE) ◄──────────── byte-for-byte ──────────────────  │
  on 429/5xx: pool::mark_cooling + next subscription (before stream start)

  metered POST /v1/messages: only after terminal of the entire local pre-byte
  rotation/smooth-wait ── one default-off attempt ──► api.llmsrelay.com
  (no OAuth/persona headers; same reserve + exact usage settlement)

  GPT /v1/responses|chat|skin ──► local Codex home rotation/retry ──► ChatGPT backend
                                  │ terminal before model output, gpt-5.5/5.4 only
                                  └─ one separate-key default-off /v1/responses
                                                           ──► api3.claudestore.store
```

## Layers (dependency direction — strictly downward)

```
┌────────────────────────────────────────────────────────────┐
│ server (bin claude-api)  — COMPOSITION                      │
│   config(env→ProxyConfig) · http(router) · poller · main    │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ forward  — Claude + Codex adapter + native Gemini gateway   │
│   AffinityStore · Clients · Codex native pool · Gemini pool │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ pool  — pool + rotation (in-memory)                        │
│   Pool · Live · route · pick · place_best · mark_* · …     │
└───────────────┬────────────────────────────────────────────┘
                ▼
┌────────────────────────────────────────────────────────────┐
│ registry  — engine-owned PostgreSQL authority              │
│   reservations/outbox · capacity leases · epochs · CRUD    │
└────────────────────────────────────────────────────────────┘
```

## Responsibility zones (where code goes)

| Changing… | Crate | Owning branch |
|---|---|---|
| subscription storage/reads, DB schema | `registry` | `comp/registry` |
| subscription selection, rotation, cooling, limit state | `pool` | `comp/pool` |
| forwarding, identity injection, poller, streaming | `forward` | `comp/forward` |
| env config, CLI, router, background loops, wiring | `server` | `comp/server` |
| subscription purchasing and pool replenishment (Telegram bot) | `crates/authbot` | `comp/authbot` |

The admin invalidation stream is composition-owned. Every fixed engine process exposes the same
authenticated `GET /admin-events` SSE route backed by one bounded `tokio::broadcast` channel.
Provider maintenance publishes only when the operator-visible fingerprint changes; Anthropic
roster/probe transitions publish directly. Successful billing-writer settlements and durable
provider-turn records publish after the authority write, so spend, settlement health and the
owning provider projection never wait for the next maintenance sweep.
The minute metrics writer invalidates `/fleet-history` only after its SQLite snapshot commits.
The feed contains resource prefixes and a bounded reason only, never provider or customer data.
Caddy rewrites it to the plane-specific same-origin `/events/{engine,openai,gemini,kimi}` routes so
one browser can subscribe without learning runtime credentials.

**Pool replenishment (outside the API layers).** `crates/authbot` — a Rust Telegram bot: purchases Claude,
ChatGPT, and Gemini access, writes Claude tokens via `registry::authority`, atomically publishes completed Codex
device flows as separate `CODEX_HOME`s, and verified paid Antigravity OAuth
subscriptions as AEAD-encrypted profiles. It sits BEFORE `registry` as a producer and does not import
`pool`, `forward`, or `server`.

## Key decisions

- **Claude: HTTP forwarding with a subscription persona, not CLI.** The proxy sends HTTP to
  api.anthropic.com on the subscription OAuth token. Before send, it intentionally injects or rewrites
  Claude Code persona attribution and provider identity headers and can cap `max_tokens` to balance.
  The upstream response, including SSE, is relayed without a CLI wrapper or response buffering.
- **Codex: a separate strict boundary.** The optional `/v1/responses`, `/v1/chat/completions`, and
  OpenAI model-discovery on `openai.api.apitoken.sale` are served by a native HTTPS pool of sealed
  ChatGPT OAuth profiles (like Gemini's); this is a compatible text subset, not transparent
  OpenAI Platform forwarding.
  `api.apitoken.sale` remains exclusively the Claude plane: provider auth headers do not
  select it. Anthropic runs in blue-green `claude-api-anthropic@8787/8788`, OpenAI in
  `claude-api-openai@8793/8797`, and native Gemini in active/passive
  `claude-api-gemini@8795/8799` behind `gemini.api.apitoken.sale`. The backend-only KIMI plane is the
  fourth fixed plane: active/passive slots `claude-api-kimi@8804/8805` behind the stable
  loopback origin `127.0.0.1:8803` (singleton `claude-api-kimi` on 8804 — rollback/anchor only),
  without a public vhost. The unified router namespace `kimi/*` is the customer path and
  goes to that origin; the plane is enabled by the argv pin
  `CLAUDE_API_KIMI_ENABLED=1` in reviewed units (disabling is the reverse reviewed change). All use one fenced
  PostgreSQL billing authority, but not a shared
  HTTP process, router, credential pool, or health state. Gemini profiles are separate encrypted
  Google OAuth identities with a Cloud Code project, their own proxy/refresh/cooling; the private
  wrapper and identity never reach the public boundary. The Codex patch removes local
  instructions/tools/context, leaving only explicit
  client context. The transport does not read the auth store, requires the ChatGPT account type, attests the binary
  SHA/version, and does not change the Claude path. One pre-provisioned process-wide lock under root-owned
  `/run/apitoken` fences the entire set of homes: two processes cannot split the pool between themselves, and a
  rename/replacement of an individual `CODEX_HOME` does not create a second lock inode.
- **Identity injection** — the price of running on a subscription token; it lives in config, tunable without a rebuild.
- **Rotation before the stream** — the response status is checked before the body is handed out, so switching
  subscriptions on 429/5xx does not break the client stream.
- **ClaudeStore-compatible fallback — not a new provider plane.** These are two compile-pinned
  default-off emergency transports with different keys and origins. The Claude transport uses
  `https://api.llmsrelay.com` for one metered `/v1/messages` after the terminal local
  rotation/smooth-wait and does not send local OAuth, identity/billing block, persona, proxy, or
  subscription identity. The GPT transport remains on `https://api3.claudestore.store` and likewise
  permits one `/v1/responses` after the normal Codex
  rotation/retry, only for `gpt-5.5`/`gpt-5.4`; the public id replaces the private local slug, and
  `chatgpt-account-id`, originator, OAuth, proxy, and calibration identity never leave. Both
  use the original customer reserve and authoritative terminal usage, without changing local pool
  spend/quota/calibration/affinity. GPT requires a separate key on the ClaudeStore Codex tier and remains
  blocked until the authenticated live gate. The full contract is
  [`CLAUDESTORE_FALLBACK.md`](CLAUDESTORE_FALLBACK.md).
- **Client dispatch without concurrency wait/reject.** Claude, Codex, and Gemini accept any fan-out and
  immediately start independent upstream attempts: there is no process/per-account/per-profile request
  semaphore. In-flight is only a routing/observability signal and durable lifecycle accounting, not an
  admission cap. Real provider quota/cooling remains a separate honest `429 + Retry-After`.
- **env only in server** — lower layers are purely functional and testable without an environment.
- **Redis — only shared cache-affinity.** No client-supplied session ID: the native harness ID
  is used automatically; the regular API is bound by rolling hashes of canonical history prefixes.
  A large/explicitly cache-controlled shared system/tools root can suggest a warm home for a new conversation,
  after which it immediately gets its own lineage and does not cross-bind rebinds of different dialogues.
  Keys and values are keyed BLAKE3 digests (no prompt/API key/account/subscription ID). Local L1
  always remains; Redis timeout/failure/eviction is fail-open and affects only the prompt-cache hit rate.
  Affinity lives in its OWN Redis instance (`CLAUDE_API_AFFINITY_REDIS_URL`, 6380), separate from Codex
  response history (`CLAUDE_API_REDIS_URL`, 6379). `maxmemory` and `maxmemory-policy` in Redis are set
  per instance, so a shared instance gave them no independent budgets: large conversations evicted
  affinity, and affinity churn deleted paid conversations. Losing affinity is safe by construction;
  losing history is not — which is why affinity was the one that moved.
- **PostgreSQL — durable authority.** Generated request IDs own exact reservation rows. Settlement
  first lands in a durable outbox, then atomically closes that exact reservation, updates the account,
  and inserts a charge unique on `(kind, request_id)`. SQLite is retained only as the guarded import
  source and rollback-era audit snapshot.
- **Fencing, not distributed hope.** Every engine process holds a monotonic PostgreSQL owner epoch;
  stale epochs cannot reserve money, persist pool state, or acquire capacity. Subscription admission
  is one transaction (cooldown/utilization validation + durable lease/inflight increment); tracked
  in-flight does not limit parallelism. Polling uses one
  PostgreSQL lease-epoch leader; there is no Redlock path.
- **Proven overlap gate.** Real-PostgreSQL fault injection and a two-owner end-to-end test gate the
  blue/green path. PostgreSQL mode may overlap two engine slots because money, delivery, capacity,
  pool writes, and poller leadership are fenced. SQLite fallback still takes the OS singleton lock.

The full request lifecycle scheme, fencing, cutover, and operational invariants are described in
[`docs/engine/STAGE2_POSTGRES_AUTHORITY.md`](STAGE2_POSTGRES_AUTHORITY.md). Production runbook —
[`docs/ops/DEPLOYMENT.md`](../ops/DEPLOYMENT.md).

The compatibility boundary, sealed roster, refresh discipline, authorization, and rollback of the Codex provider
are described separately in [`docs/engine/CODEX_PROVIDER.md`](CODEX_PROVIDER.md).

Configuration details — `config.env.example` / `server.env.example`. Deployment — a single provider cohort:
`systemd/claude-api-anthropic@.service`, `systemd/claude-api-openai@.service`,
`systemd/claude-api-gemini@.service` and `deploy/engine-bluegreen.sh` (legacy singleton units remain
only for rollback to releases before the corresponding blue-green marker).

## Commerce perimeter (separate from the engine)

```text
future Next.js web → apps/api → whole-USD checkout_sessions → commerce PostgreSQL
                           └── Control API → Rust claude-api
payment provider → apps/api (verified webhook) → engine_credits outbox → apps/worker → Control API
engine charge ledger → apps/worker cursor → funding/referral attribution ───────────┘
```

`apps/api` owns the future browser-facing API boundary and the intake of signed webhooks.
The user enters an arbitrary whole number of USD as a string; there is no product catalog.
Browser identity is determined only by an opaque server-side session; email/Google identities and
sessions live in commerce PostgreSQL, details in `docs/commerce/AUTHENTICATION.md`.
`apps/worker` picks up durable credit jobs from PostgreSQL via `FOR UPDATE SKIP LOCKED` and
idempotently calls `/admin/account/{id}/credit`. Shared schemas/repositories/engine client live
in `packages/contracts`, `packages/db`, `packages/engine-client`. Commerce applications never receive
the engine DSN and have no direct DB code; they talk to the engine only through the Control API. The full map is
`docs/commerce/COMMERCIAL_BACKEND.md`.
Dashboard routes read authoritative balances, ledger rows and per-key spend through the Control API.
Key creation returns the usable secret once; later revocation uses a stable non-secret engine `key_id`.
B2C/B2B pricing state lives in commerce PostgreSQL; the worker synchronizes immutable policy and
release data through durable jobs. Target B2C is global 50% with provider/model overrides; tiers and
retention are removed. Full rules are in `docs/commerce/PRICING.md`.
