# CLAUDE.md — crates/router (claude-router)

The single stateless entry point for all provider planes — stage 1b of
`docs/engine/UNIFIED_ROUTER.md`. A separate bounded context OUTSIDE the layers
`registry ← pool ← forward ← server`: the `claude-router` binary, which talks to the
planes only over HTTP via stable loopback origins (8790/8792/8794/8803).

## Boundaries (do NOT violate)

- **No imports** of `pool`/`forward`/`registry`/`metering` — all contact with the
  engine is HTTP to stable origins. A new "cool" capability that requires importing an
  engine crate means it belongs to the plane, not to the router.
- **Billing lives only in the plane.** The router does not reserve, does not charge, does not
  know `request_id`. The client's key is passed to the plane verbatim
  (`proxy::AUTH_HEADERS`); the router has no env secrets.
- **Fail-closed retry.** Native lanes and ordinary single-model universal requests
  make exactly one attempt. With `CLAUDE_ROUTER_FALLBACK_ENABLED` on,
  the next model of the effective chain is permitted only after an exact non-2xx
  `x-apitoken-execution-state: not_started` (except 401/402/other client
  4xx; a signed 429 is a capacity refusal) or a proven TCP `ConnectionRefused`.
  Timeout, DNS/generic connect error, unsigned 5xx, reset/abrupt close and a response after
  the headers are never retried (`docs/engine/ROUTING_FENCING.md` §3.3).
- **Execution identity is a capability, not client input.** For every effective chain
  longer than one model,
  the router generates a CSPRNG UUIDv4 once and injects
  `x-apitoken-execution-group` + a positive attempt `1..N`. Before injection, client copies are always
  removed; the native and universal single-attempt paths send neither header. Caddy independently
  strips them on the external ingress. The identity is admissible only in internal router→plane requests and
  is never returned to the client.
- **Logical request identity is likewise router-owned.** After auth/body/model/routing/policy admission
  and immediately before the first executable provider attempt, every customer request gets one canonical
  lowercase CSPRNG UUIDv4 in `x-apitoken-logical-request-id`. Native and universal single attempts receive
  it too; a fallback chain reuses the identical value on every attempt. The common proxy function removes
  all client copies before optional typed injection, so `/balance` and non-executable helper/preflight calls
  never receive an ID even when a spoofed value arrives. The header is stripped from provider responses,
  never returned publicly, and does not replace `x-request-id`, billing identity, or execution group/attempt.
- **No execution queues, semaphores, circuit breakers, rate limits** (invariant 3).
  The only exception is a 4 GiB fail-fast estimated-RSS budget and a 16 GiB spool budget in 1 MiB
  steps on buffered universal request bodies: a known `Content-Length` is rounded up, an unknown/chunked
  one starts at 1 MiB and
  fail-fast adds units as bytes are actually read. Read deadline — 60 seconds without
  progress and 5 minutes of absolute time.
  The ordinary single-model path drops the parsed tree and releases the permit after the outbound upload;
  extended routing holds the permit until terminal response headers, because the parsed template
  is needed for the next attempt. The budget never waits and never holds a native/SSE response body.
  Readiness (`/health`, `/live`, `/ready`) is router-local, never a
  conjunction of the planes' health; there are no synchronous health checks on the request path.
- **SSE is not buffered.** Request and response bodies are streams
  (`Body::wrap_stream`/`Body::from_stream`); reqwest is built without auto-decode
  (default-features off) so that bytes and Content-Encoding pass unchanged.
  The only exception is the shared `routing.rs`: the REQUEST body of
  `/v1/chat/completions`, `/v1/responses` and
  `/v1/messages{,/count_tokens}` is read in full (256 MiB limit) for the sake of the
  `model` field; the response body stays a stream. An additional exception is the router-owned
  `x-apitoken-service-tier: fast|priority` and the OpenAI-compatible body alias
  `serviceTier:"fast"|"priority"`: on executable GPT Chat/Responses requests the router
  normalizes the selector into the body's `service_tier:"priority"`; the alias and the header
  never reach the plane. A body alias on Messages/count_tokens and any Fast selector on non-GPT
  are rejected fail-closed.
  A client disconnect must transitively tear down the connection to the plane
  (TeeMeter drain): that is why there are no detached tasks around the response body.
- **Internal execution semantics are not relayed to the client.** The
  `x-apitoken-execution-state` header (contract `docs/engine/ROUTING_FENCING.md` §3, stage 6.1) is an
  engine↔router contract: the planes set it on no-execution refusals
  (`not_started`), and the router must strip it from ALL transit responses before handing them to
  the client (`proxy.rs` `EXECUTION_STATE_HEADER`). Only the engine itself is responsible for the
  header's conditions — the router checks the signal only inside the fallback engine and does not
  relay it. Clients must not depend on the engine's internal state.
- **Money amounts — integers only**: the router does not touch money at all; if amounts ever
  appear — nanoUSD strings, no float.

## What lives here

- `config.rs` — the only place env is read (`CLAUDE_ROUTER_*`), including
  the strict off-by-default flag `CLAUDE_ROUTER_FALLBACK_ENABLED` (`0|1|false|true`). The dormant
  body settings use `api-limits` strict decimal MiB/seconds parsing and fail startup on malformed,
  zero, inconsistent, or above-current values. Production pins the 256 MiB request, independent 4 GiB
  estimated-RSS and 16 GiB spool budgets, 8 MiB spill threshold, and 120/1800-second deadlines. A
  required absolute `CLAUDE_ROUTER_BODY_SPOOL_ROOT` is opened as a private directory capability;
  systemd gives every slot a separate mode-0700 StateDirectory on disk. Request
  `Content-Encoding` other than `identity` is 415 before the body is read. Ordinary namespaced
  single-model JSON extracts only routing selectors (`model` / `models` / `provider` / `serviceTier`)
  and skips unknown fields through `IgnoredAny`; a full `serde_json::Value` is built only for alias,
  Fast, or advanced `models`/`provider` rewrite.
- `auth.rs` — uncached bodyless early-auth client: before reading the universal body it probes fixed
  origins in hedged Anthropic → OpenAI → Gemini order. Anthropic starts immediately; each later
  origin starts after a 50 ms hedge only without a conclusive result, or immediately when an
  inconclusive response leaves no useful active probe. The first exact schema-v1 success or terminal
  401 wins; mixed-version/transport/5xx remain inconclusive. Dropping outstanding client futures does
  not claim cancellation of provider DB work already accepted. Also here: the exact unauthenticated
  probe for loopback-only `/startup`, which still probes all three origins concurrently for blue-green
  verification of the real provider data path.
- `proxy.rs` — byte-for-byte proxying of native lanes, auth passthrough and classification of
  a single attempt down to the public headers: exact `not_started` / source-chain
  `ConnectionRefused`. The final common outbound function always removes inbound logical/execution
  identity, then injects only explicitly supplied typed router identity; balance/helper traversals pass none.
  The data plane has no router-owned deadline either before response headers or after
  them: a non-stream plane legitimately responds only after generation completes, and the lifetime
  is bounded by the client disconnect and the plane itself. A separate two-second header deadline
  remains only on the read-only `/balance` failover. Internal execution-state and logical-ID headers are
  stripped before the response is assembled. Before any plane, the public router capability header
  `x-apitoken-service-tier` is stripped as well.
- `routing.rs` — shared model dispatch and serial fallback for all universal
  surfaces. It first performs the bodyless auth, then dynamically reserves the actual
  body size in the shared 4 GiB estimated-RSS budget; overload/slow body return lane-shaped 503/408 without a billable call.
  An ordinary request without `models` and `provider` keeps the original bytes and direct
  namespaced dispatch. The extended planner obtains one aggregate catalog snapshot, canonically
  deduplicates the explicit chain, applies provider filters/order and `allow_fallbacks`, then the
  account-policy preflight.
  Only after that does it remove `models`/`provider`, substitute the chosen `model` and
  execute the retry matrix. Every nonempty final plan owns one logical-request UUIDv4 across all
  attempts; an effective chain longer than one element separately owns one CSPRNG execution-group UUIDv4
  and a monotone attempt per model. After filtering down to one model, logical identity is still injected
  but execution group/attempt are not.
  Attempt logs contain only surface/index, the public catalog ID, lane,
  status and a bounded retry reason — no URLs, headers, credentials or request bodies.
  Compatible Fast selectors are validated here as well: the header for Chat/Responses/Messages and
  the camelCase body alias only for OpenAI-compatible Chat/Responses. They are allowed only for GPT,
  not token counting; conflicting `serviceTier`/`service_tier`/Messages `speed` are rejected before
  the plane is called. The GPT-only check runs after preferences/policy, i.e. it evaluates only
  executable attempts.
- `policy.rs` — the closed schema of OpenRouter-shaped `provider` preferences and a bounded
  client of the engine-owned `/internal/router/policy/preflight`: all auth-header values are passed
  verbatim, fixed origins are iterated sequentially, `401` is terminal,
  malformed/mixed-version responses fail closed. Credentials and policy responses are not cached.
- `pricing.rs` — a bounded client of the engine-owned `/internal/router/catalog/pricing`: the credential
  of the current catalog request is passed verbatim, the catalog is deterministically cut into chunks of at most
  256 candidates, fixed origins are iterated sequentially for each chunk, and any failed
  chunk/`401` closes the whole overlay. Schema version/unit/canonical integer strings/ordered subset
  are checked fail closed. Personal rate cards exist only in request memory and are not
  cached.
- `metrics.rs` — fixed-cardinality telemetry admission/auth/catalog/pricing/policy/balance-header-timeout/
  balance and a compile-bounded `claude_router_fallback_total` (exactly 18 series). Large-payload
  baseline records only fully materialized universal bodies in fixed Chat/Responses/Messages/count
  surfaces, plus fixed oversized/read-timeout/admission-overload rejections; partial/native stream
  bytes are not fabricated. Model, credential, path, account, group and request identity are
  forbidden in labels.
- `chat.rs` and `responses.rs` — thin OpenAI-shaped entrypoints into `routing.rs`.
- `messages.rs` — thin Anthropic-shaped entrypoint for `POST /v1/messages` and
  `POST /v1/messages/count_tokens`: namespaced `openai/*` goes to the Codex plane
  (where the Messages→Responses adapter lives, `crates/forward/src/codex/skin.rs`),
  `anthropic/*` — to the Anthropic plane as a native lane, `kimi/*` — to the dedicated KIMI
  origin on the Anthropic Messages lane (Chat/Responses keep their client paths; origin 8803
  mounts the Anthropic-plane adapters), `google/*` — to the Gemini
  plane under the shared namespace rule (the Messages→generateContent skin is implemented
  in `crates/forward/src/gemini/skin.rs`). For `count_tokens` the same
  plane is chosen: Anthropic native, reserve-grade local counting for Codex, or
  the quota-free native `:countTokens` of Gemini.
- Stored responses endpoints (`/v1/responses/input_tokens`, `/v1/responses/{id}`,
  `.../input_items`) use no dispatch — they remain a native OpenAI lane
  (stored responses only for `openai/*`, decision 5). The Images API
  (`POST /v1/images/generations`, `/v1/images/edits`) is likewise a native OpenAI lane:
  a byte-faithful proxy to the OpenAI plane, which owns admission, billing, and the proved
  narrow GPT Image 2 contract. Gemini `/v1beta/*` and `/upload/v1beta/*` are byte-faithful native
  wrappers to the Gemini plane only. These native wrappers create logical identity immediately before
  their single proxy attempt; `/balance`, health, catalog, startup, and fallback 404/405 do not.
- `catalog.rs` — the unified `/v1/models`: aggregation of the three planes, namespaced IDs
  + only globally unambiguous aliases, per-plane singleflight refresh sharing the result of a
  successful or failed in-flight attempt, a deterministically
  skewed TTL of 27/30/33 s, last-good on plane failure and the degradation marker
  `x-apitoken-catalog-degraded`. A plane response is limited to 4 MiB, 1,024 models and 256 bytes per
  ID/display name; surrounding whitespace, duplicate IDs and hostile metadata fail closed and never
  enter the cache/logs. After the same aggregate-auth check, `main.rs` answers
  Codex `originator`/User-Agent with the backend-native overlay `{models:[]}` (the CLI merges it with
  its built-in metadata), without changing the OpenAI list for other clients. The consumer strictly
  normalizes the Anthropic native `max_input_tokens`/`max_tokens`/thinking/effort matrix and the owned
  OpenAI/Gemini `apitoken.limits/capabilities`; it publishes them in `apitoken` and in the previous top-level
  capability mirrors. A standalone `reasoning` is not derived from an empty effort: for Anthropic it comes from
  native `thinking.supported`, for an owned producer — from an explicit bool or the authoritative effort list.
  Producer-authored release dates are mandatory: Anthropic RFC 3339 `created_at` is normalized to
  Unix seconds, and owned planes supply positive Unix seconds directly. Missing/zero/malformed dates
  and unknown/duplicate `apitoken.endpoints` fail the plane refresh to last-good/degraded; the closed
  endpoint vocabulary is the two Images API paths. Other missing legacy metadata is not guessed. An
  alias collision removes the alias from all participants, but the namespaced ID and separate native
  ID for body rewrite/pricing remain functional. Also here — shared `pub(crate) namespace_lane` for
  universal dispatches (direct plane selection without a catalog fetch for requests without
  fallback). `main.rs` separately obtains the key-scoped pricing ordered subset, filters out
  unavailable models, publishes
  exact nanoUSD/M strings in `apitoken.pricing` without overwriting runtime metadata, and sets
  `Cache-Control: private, no-store`;
  a pricing-authority error yields a 503 with no zero/stale fallback.
- `error.rs` — the router's synthetic errors in the envelope of the corresponding
  provider (plane errors are proxied byte-for-byte and do not end up here).
- `main.rs` — the public contract's route table + composition.
  `/balance` — bodyless read-only failover Anthropic → OpenAI → Gemini: transport/header-timeout/
  5xx continue, 401 and any non-5xx are terminal. The loopback-only `GET /startup` and `/metrics` are not
  in the public allowlist; Prometheus and deploy use the stable Caddy origin
  `127.0.0.1:8802`, which follows the same single-active backend as the public vhost.

## Verification

```bash
cargo test -p claude-router   # unit + integration (mock planes on TCP)
cargo build                   # whole workspace green before commit
cargo build && bash tests/router_fallback_smoke.sh  # concurrent 6.4c mock-load + metric deltas
```

The integration tests bring up mock planes on real loopback sockets and
cover: bodyless early auth ahead of an unfinished large body, terminal 401 and mixed-version,
dynamic weighted 4 GiB overload without a queue, the slow-body deadline, permit release on parse error,
outbound EOF with an open SSE,
absence of a data-plane pre-header deadline and a bounded `/balance` deadline without retry, passthrough
of body/headers, unbuffered SSE, transitive
disconnect, strict normalization/degradation/staleness of the capability catalog, removal of conflicting aliases,
uncached key-scoped pricing for two
keys over one shared cache, terminal pricing 401/503, canonical wire validation, alias
resolution of models, 404/405, model-based dispatch of chat-, responses- and
messages- and messages/count_tokens requests (namespaced without a catalog fetch,
alias via the catalog, 400 on an invalid body, 413 on an oversized body, unbuffered
chat SSE), as well as off-by-default fallback, preflight of the whole chain, the exact
retry matrix (`not_started`, 429, 4xx/5xx, ConnectionRefused/timeout), rewrite
of the per-attempt body, provider preferences, removed-sort rejection, mixed-version policy
failover, terminal `401`, strict subset/empty `403`, Fast after policy filtering, and logical-ID spoof
removal, canonical single/native injection, cross-fallback reuse, distinct external-request values,
preflight/balance absence, and public-response stripping. No live PostgreSQL or subscriptions are needed.
The full router→engine→mock upstream chain — `tests/universal_chat_smoke.sh`. The separate
`tests/router_fallback_smoke.sh` brings up three deterministic TCP planes and proves
parallel `not_started → success`, provider+strict-policy fencing before attempt 1, terminal unsigned
503, last-good catalog + killed origin → `ConnectionRefused` and the exact deltas of the 18-series metric.

After the telemetry package rolls out onto an exact GREEN SHA, the live canary is run via
`tests/router_fallback_live_canary.sh` with an already-existing `APITOKEN_API_KEY` and an explicit
`APITOKEN_CANARY_EXPECTED_SHA`. The wrapper executes a separate router process from the actually running
immutable production release, passes the key only via SSH/curl stdin and always removes the temporary
shim/response files. It fail-closed requires a strict subset and two permitted provider planes,
checks the signed/unsigned/ConnectionRefused matrix and the absence of growth in double-winner, balance
divergence and settlement backlog. Until a green result, the unit flag is not changed.

## Operations

The `systemd/claude-router@.service` unit runs in two fixed slots `127.0.0.1:8800/8801`;
`deploy/router-bluegreen.sh` is the sole owner of their lifecycle. It checks the inactive slot against the
exact immutable binary, atomically flips Caddy via a root-owned runtime snippet and only then
stops the old slot with a bounded graceful drain. The legacy `claude-router.service:8798` exists
only for the first handoff/rollback horizon. The public boundary is the Caddy vhost
`router.apitoken.sale` (see `deploy/CADDY.md`); multi-host HA is not claimed hereby.

After rollout, fallback stays off: absence of the env flag is the contractual
default. The canary enables it only via an explicit
`CLAUDE_ROUTER_FALLBACK_ENABLED=1`; any presence of `models` or `provider` with the flag off
receives a lane-shaped `400` before any catalog/policy/network work.
