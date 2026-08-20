# ROUTING_FENCING.md — detailed design of UNIFIED_ROUTER stage 6 (routing + attempt fencing)

Status: phases 6.1–6.3, the policy/preferences consumer 6.4b and the telemetry/mock-load part of
6.4c are implemented; the phase 6.4 contract was fixed on 2026-08-02. Serial fallback remains off by
default: ahead are a post-deploy live canary on the exact GREEN SHA and a separate production unit flag.
The implementation follows this document; any deviation requires revising it.

Fact-base date: 2026-08-02 (re-audit of production after phases 6.1–6.3).
References like `proxy.rs:1880` mean `crates/forward/src/proxy.rs` unless stated otherwise.

## 1. Stage 6 scope

Per `UNIFIED_ROUTER.md` item 6: OpenRouter-grade routing — provider preferences, explicit
model fallback lists, attempt fencing (execution group / single billable winner), per-account
policy, and telemetry. NOT included: quorum/parallel attempts (racing
multiple models), cross-provider response storage, changes to the universal dictionaries
of stages 3–5.

## 2. Fact base (audit 2026-08-01)

### 2.1. There is NO double billing inside a plane — by construction

One `engine_request_id` (UUIDv4, CSPRNG) is created BEFORE the first rotation attempt and is
shared by all in-plane retries: Anthropic `proxy.rs:955`, Codex `codex/billing.rs:98`, Gemini
`gemini/api.rs:2180`. Exactly-once for money: `UNIQUE INDEX ledger_request_once ON
ledger(kind, request_id)` + outbox PK + reservation PK (`registry/src/lib.rs:1517-1520`);
a repeated settle with a different actual is a hard error, not a silent duplicate (`pg.rs:1907`).

### 2.2. The hole is strictly at the plane/model boundary

Any retry ABOVE the plane (router fallback to another model/plane) creates a NEW
request_id and a NEW reserve: if the first plane actually executed the request (and the router saw
timeout/5xx/disconnect), both attempts are billable. The router today is stateless and CANNOT safely
retry: no attempt state is available from outside the plane (`router/src/proxy.rs:66-114`
— one send, zero retries, 2 s connect timeout, plane 5xx passed through). The only
safe signal without a new contract is TCP connect-refused (the request physically never left).

### 2.3. The "started" boundary differs across the four lanes today

| Lane | Durable "started" | What the router sees on failure BEFORE started |
|---|---|---|
| Anthropic | upstream 2xx headers → `mark_delivering` BEFORE the first byte to the client (`proxy.rs:1880`) | non-2xx (plane returned an internal 5xx/429 after exhausting rotations) |
| Codex stream | `emitted` flag — first delta into the client channel (`runner.rs:360-363`); `mark_delivering` is set BEFORE the attempt (`api.rs:302`), pre-delta refund via `HoldGuard` settle(hold,0) | non-2xx, or 200 with an error event inside the stream |
| Codex non-stream | after the full turn (`api.rs:321`) | non-2xx |
| Gemini stream | first translated public event (`api.rs:1728`, bounded prelude :2305-2365) | non-2xx |
| Gemini non-stream | after success (`api.rs:2579`) | non-2xx |

Conclusion: a non-2xx from a plane TODAY almost always means "not delivered, money refunded" —
but that is observed behavior, not a contract: no lane guarantees it explicitly, and for
Codex stream even 200 does not mean billable. Without an explicit signal the router cannot tell
"never started" from "started and failed" (fact #4: plane-level 5xx before delivery = refund,
200 + mid-stream `event: error` = billable delivering — the distinction exists only inside the plane).

### 2.4. Existing substrates (we extend them, we do not rebuild)

- Per-request reservation state: `reserved → delivering → [settlement_pending] →
  settled/canceled` + lease + reconcile; reconcile cancels `reserved` without charging —
  de facto today's "not_started" (`lib.rs:4824-4835`).
- "delivering ⇒ billable" is covered by crash recovery: an expired lease in `delivering` → charge
  the full hold, only when the owner epoch is provably dead (`pg.rs:2162-2191`).
- Fail-closed fencing precedent: a failed durable `mark_delivering` → no "free"
  usage (full hold with reference `delivery-marker-failed`, 503 to the client, `proxy.rs:1880-1905`).
- An embryo of (group, attempt) identity in the capacity plane: `capacity_lease_id =
  "{request_id}:{attempt}"` (`proxy.rs:1632`), PG `capacity_leases` with exact-replay
  semantics (`pg.rs:2201-2233`).
- Stable per-turn IDs that survive rotation: Codex `cal_*` (`runner.rs:272-274`), Gemini
  `upstream_request_id` (`api.rs:2133-2136`), Anthropic `engine_request_id`.
- Telemetry: `apitoken_balance_divergence_nano` (a direct detector of excess charges),
  `apitoken_engine_settlement_pending` + alerts `EngineSettlementBacklog`,
  `EngineExpiredLeasePresent`, per-plane counters `upstream_{429,auth,5xx}`,
  `gemini_stream_start_failures_total` (pre-byte failures).

## 3. The `execution_state=not_started` contract (MVP fallback, phase 6.1)

### 3.1. Semantics

A plane sets the HTTP header `x-apitoken-execution-state: not_started` on a response
when ALL of these conditions hold:

1. Not a single byte of the public response has gone to the client (the same criterion as the in-plane retry
   boundary: Anthropic — before upstream 2xx headers; Codex — before `emitted`; Gemini — before the first
   translated public event).
2. The reserve for this request_id WILL NOT be metered: a refund has been durably recorded
   (settle(hold, 0) / cancel reserve), or the reserve is guaranteed to be cancelled by reconcile as
   `reserved` without a charge.
3. The response is non-2xx. On 2xx the header is NEVER set: 2xx is always the end of the discussion
   (a mid-stream error event is ambiguous, the client's decision, `UNIFIED_ROUTER.md` "Fallback
   semantics": no automatic retry on another model).

The header is an internal router↔plane contract: the router MUST strip it before handing the response
to the client (clients must not depend on internal engine state).

### 3.2. Plane obligations (per-plane emission points)

- **Anthropic** (`proxy.rs`): rotation budget exhaustion → final non-2xx responses
  (429/5xx/503 exhausted, network-fail outcomes) — all of them before `mark_delivering`, the reserve still in
  `reserved`: the header is set provided that this lane's settle is refund/cancel. Responses
  AFTER `mark_delivering` (including SseErrorTail inside 200) — without the header.
- **Codex** (`api.rs` + `runner.rs`): pre-delta failures with `HoldGuard` refund (stream) and
  non-stream failure before turn end — header; any response after `emitted` — without it.
- **Gemini** (`api.rs`): failures in the bounded prelude (provider_error before the first public event),
  non-stream failure — header; after the first public event — without it.
- A single per-plane unit contract: "a response with the header ⇒ the ledger does not and will not
  contain a charge for the request_id" (verified at the settle-outcome level in lane tests).
- Universal Chat/Responses adapters (`anthropic.rs`/`anthropic_responses.rs`,
  `gemini/chat.rs`/`gemini/responses.rs`) are covered since 2026-08-02: local pre-request
  failures receive `not_started`, a rebuilt non-2xx preserves only the exact authoritative
  plane signal, and parse/assembly errors after 2xx explicitly strip it, because a charge
  is already possible. The Gemini Messages skin (`gemini/skin.rs`) follows the same rule for its own
  surface. A missing or unknown signal remains fail-closed: retry is forbidden
  (§3.3).
- **Stable Caddy origins** (8790/8792/8794) synthesize the same exact `not_started` only when
  the reverse-proxy handler itself returns `503 no healthy upstream`: no health-gated runtime
  accepted the request. Runtime-produced HTTP 503 does not enter `handle_errors` and receives no signal.
  External provider vhosts strip the header on the outer hop; the router sees it only over loopback.

### 3.3. Router obligations (phase 6.2)

A retry on the next model of the fallback list is permitted in EXACTLY two cases:

1. The plane response is non-2xx WITH the `x-apitoken-execution-state: not_started` header (the header
   is stripped; the client receives the response of the last attempt). The router logs only bounded
   attempt metadata; request/response headers and bodies never enter the log.
2. TCP connect-refused to the plane (the request physically never left).

Forbidden: retry on timeout, on 5xx WITHOUT the header, on a disconnect after headers, on 402
(account balance — retrying on another model of the same account is pointless), on client 4xx.
The exception inside the 4xx range is `429` with the exact `not_started`: that is a plane
capacity refusal, not a client-fixable error. Exact means the single value `not_started`;
different case, multiple values, and unknown values fail closed.

## 4. Execution group / attempt identity (mature version, phase 6.3)

The §3 MVP contract closes the race "the second attempt started while the first is billable" only when
the signal works. The durable guarantee against a bug/desync is group identity:

- **The router generates** a `group_id` (UUIDv4) per logical request with a fallback list and sends
  the plane `x-apitoken-execution-group: <group_id>` + `x-apitoken-attempt: <N>` (N = 1,2,…
  in list traversal order). Without a fallback list the headers are not set — the plane
  works as today (group = request_id).
- **Trust boundary:** Caddy strips both headers on public provider vhosts and on
  `router.apitoken.sale`. The router additionally removes client-supplied copies before each attempt and
  only then injects its own CSPRNG UUIDv4/ordinal attempt. The plane accepts either a fully absent
  pair or exactly one canonical value of each; partial, duplicate,
  non-lowercase/non-v4 UUID, and non-canonical positive decimal fail closed before reserve.
- **Registry (expand-only migration):** `reservations` gains a nullable `group_id TEXT` and
  `attempt INTEGER NOT NULL DEFAULT 1`. A PostgreSQL default cannot reference another column,
  so `group_id IS NULL` is the explicit compatible representation of an old/direct attempt, and the effective
  group at runtime is determined as `COALESCE(group_id, request_id)`. The new table
  `execution_group_winner(group_id TEXT PRIMARY KEY, winner_request_id TEXT NOT NULL,
  decided_at BIGINT NOT NULL)` stores one insert-first-wins row per effective group.
- **Settle path:** a nonzero settle (charge > 0) atomically (in the same DB transaction) performs
  `INSERT INTO execution_group_winner … ON CONFLICT DO NOTHING` and reads the winner:
  - winner == my request_id → normal settle;
  - winner != my request_id → double execution detected durably: the charge is forced to
    0 (refund), a fatal structured event `execution_group_double_winner` + metric
    (must be 0 always; >0 = §3 contract bug, incident).
  A refund-settle (charge == 0) does not assign a winner.
- **Strict-policy loser:** the original outbox payload (`actual`, usage, disposition) remains
  unchanged for exact-replay audit, but money processing is performed as an internal `cancel` with
  effective actual 0 and without usage/charge rows. The reservation and funding allocations record a zero
  charge and full release. Exact replay derives the effective actual from the durable winner row.
- **Retention:** the winner is deleted only after the bounded terminal-prune of the last reservation with the
  same effective group. As long as at least one reservation/replay record of the group
  exists, the winner is retained, even if the winner's own reservation has already become eligible for deletion.
- **The exactly-once invariant is not weakened:** the existing `UNIQUE ledger(kind, request_id)`
  remains per-attempt; the winner rule adds "exactly one nonzero winner per group".
- Migrations are expand-only, in two commits per `AGENTS.md`: first a schema compatible with the old
  writer (nullable group identity, attempt with default, new table); code only after green
  `deploy/migration` + `deploy/watchdog`.

## 5. Router routing interface (phase 6.2)

- A new optional request field `models: [<catalog id>, …]` (an OpenRouter-compatible
  convention; an expand-only change to the universal endpoint contract — old clients unaffected).
  `model` remains required and is treated as the first element of the chain; `models` defines
  the continuation. An empty list/duplicates/unknown ids → `400` in the ingress-path lane envelope.
  The `CLAUDE_ROUTER_FALLBACK_ENABLED` flag is strict (`0|1|false|true`) and defaults to false;
  with the flag off, the mere presence of `models` yields `400` before any network call.
- A request without `models` keeps the previous contract: original body bytes are unchanged,
  a namespaced ID selects the plane directly even with an unavailable catalog, an alias uses
  the cached aggregate catalog. An explicit fallback chain is fully validated against a single
  aggregate snapshot BEFORE the first attempt; an alias and a namespaced ID of the same catalog entry count as
  a duplicate. Then `models` is removed and `model` is replaced for each attempt.
- The router buffers only the request body (as today, 64 MiB), selects the plane for each
  attempt independently (namespace/alias — the existing `catalog::namespace_lane`); retry —
  only per the §3.3 rules; the client gets the last attempt's response (its success or its error);
  the in-flight response is NOT buffered (the byte-passthrough invariant is untouched: retry
  is possible only before the first byte).
- A `provider` preferences object — NOT in this phase; a separate package after live fallback
  telemetry. Its later contract contains filters, explicit provider order and `allow_fallbacks`,
  but no router-owned price/latency sorting.
- Per-account policy: the existing substrate `crates/registry/src/pricing.rs` (provider
  switches, account policy) will filter the fallback chain BEFORE the first attempt in phase
  6.4; phase 6.2 does not read policy.

### 5.1. Policy preflight (6.4a contract)

Policy remains engine-owned and is not moved into the stateless router. Every fixed
provider plane adds the same internal `POST /internal/router/policy/preflight`:

```json
{
  "schema_version": 1,
  "candidates": [
    {
      "id": "anthropic/claude-sonnet-5",
      "provider_id": "anthropic",
      "canonical_model_id": "claude-sonnet-5"
    }
  ]
}
```

The response contains only the version, the mode `unrestricted|strict`, and an ordered subset of the input `id`s in
the `allowed` field; account ID, policy/rule/digest, prices, and refusal reasons never leave the plane.
The body is limited to 64 KiB, the list to 32 unique candidates, identifiers to 256 bytes;
unknown fields and unknown provider IDs are rejected. The credential is passed with the same auth headers as the
executed request. Env-admin gets `unrestricted`; an invalid credential gets `401`, an authority
error gets `503`.

For an active strict binding the handler reads `KeyAuth` and one coherent
`PricingReadBundle`, builds the runtime manifest only via
`RuntimePricingManifest::from_evidence`, and calls the existing `resolve_pricing` for each
candidate. `Resolved` admits; any typed rejection forbids the model. Google candidates
for a strict account are forbidden as long as the Gemini plane itself fail-closed rejects strict admission;
preflight has no right to promise executability that does not exist on the money path. Unbound,
legacy-scalar, and shadow bindings remain `unrestricted`: their live admission does not change.

The router performs exactly one preflight per logical chain after catalog/preferences validation
but before attempt 1. It tries stable origins sequentially without binding authority to
one provider; `404/405`, transport/`5xx`, and malformed responses allow trying the
next plane, but the absence of at least one valid response ends in a lane-shaped
`503` without execution. `401` is terminal. Decisions are neither cached nor indexed by key:
policy is mutable, and the credential must not linger in memory beyond the request. The response must be an
exact subset of the original list without duplicates; anything else is a producer-contract failure and `503`.
An empty strict subset → lane-shaped `403 policy_restricted` before the first attempt.

The producer endpoint was implemented on 2026-08-02 in `crates/server/src/router_policy.rs` and registered
on all runtime modes before provider-specific route composition. Public Caddy allowlists do not
pass `/internal/*`; the router reaches it only through stable loopback origins. The endpoint
rolls out and passes `deploy/watchdog` before the consumer router. This ordering makes the expand-only
rollout safe; the consumer still understands the mixed-version window and fail-closed iterates
planes instead of depending on the Anthropic origin.

The consumer is implemented in `crates/router/src/policy.rs`: after building the effective chain the router
first tries the origin of the first candidate lane, then the remaining candidate/fixed origins without
repeats; request and response are limited to 64 KiB. All `x-api-key`, `x-goog-api-key`, and
`authorization` values are preserved (engine auth OR-semantics), but no other headers are copied. For
`unrestricted` only the full original list is accepted; for `strict` — an exact ordered subset;
unknown/duplicate/out-of-order ID, unknown field/mode/version, or an oversized body count as
producer-contract failure. The TCP integration matrix covers `404`, `5xx`, malformed and
transport failover, terminal `401`, strict filtering before attempt 1, and empty `403` without execution.

### 5.2. Provider preferences (6.4b contract)

Implemented in `crates/router/src/routing.rs` and `policy.rs`; rollout remains default-off until
6.4c. The universal body accepts an optional OpenRouter-shaped `provider` object with only these
fields:

- `order`, `only`, `ignore`: arrays of unique namespaces `anthropic|openai|google|kimi`;
- `allow_fallbacks`: boolean; `false` keeps only the first permitted candidate after filters/order.

Unknown fields — including the removed `sort` — unknown values, duplicates, an intersection of
`only` and `ignore`, or an empty chain after filtering produce a lane-shaped `400`. The
transformation order is strict: one aggregate catalog snapshot → canonical dedup → `only`/`ignore`
→ explicit `order` (unlisted namespaces keep their relative order afterwards) →
`allow_fallbacks` → policy preflight. The `provider` field, like `models`, is removed before sending
to the plane.

Router-owned `preset/*` IDs, their compiled manifest, and price/latency rank sorting were removed.
The caller owns the explicit ordered `model` + `models` chain; the router does not silently replace
or expand it. A former preset spelling is now an ordinary unknown model: direct single-model use
returns model-not-found, while its presence in an advanced chain returns a validation `400`.

Any presence of `models` or `provider` is governed by the single rollout flag. While
`CLAUDE_ROUTER_FALLBACK_ENABLED=false`, the request is rejected before catalog/policy/network work;
single-model requests without these fields keep the byte-identical behavior of phases 1–5.

### 5.3. GA rollout (6.4c contract)

Telemetry and a reproducible harness are implemented default-off: router/plane counters, loopback scrape,
recording/alert rules with runbooks, concurrent mock-load, and a stdin-only live-canary runner. The live
canary itself runs only after this package is deployed; neither its result nor the production flag is
anticipated by documentation.

The router exports `/metrics` without authorization on loopback. A fallback continuation increments
`claude_router_fallback_total{from_namespace,to_namespace,reason}` exactly once before
the next attempt; label sets are compile-fixed (3×3 namespaces, two reasons). The plane
increments `claude_api_execution_not_started_total{plane}` exactly for the exact `not_started` actually
returned externally on a non-2xx. The same fixed dimensions cover active body units,
overload/read timeout, auth outcomes/latency, catalog refresh/degradation, pricing/policy failure,
response-header timeout, and read-only `/balance` failover. Large-payload baseline additionally
histograms only fully materialized universal bodies across four fixed surfaces and counts three fixed
rejection reasons; partial/native streaming bytes are not inferred. Credential/model/path/account/
group/request IDs are forbidden in metrics; `RouterAdmissionFailures`,
`RouterBodyOversizePressure`, `RouterAuthorityFailures`, and `RouterResponseHeaderTimeout` map to the
eponymous runbook sections.

Activation order: producer 6.4a → consumer 6.4b at default-off → telemetry/Prometheus 6.4c
at default-off → mock-load and live canary in a separate router process → unit flag in production.
The canary must prove policy filtering before attempt 1, serial continuation, no retry on
ambiguous outcomes, zero growth of double-winner and balance divergence, and an acceptable settlement
backlog. The production flag is enabled only by the final reviewed commit; rollback is returning the
flag to false in a new commit, without removing the expand-only contract.

## 6. Telemetry and verification

- Phase 6.2 writes a bounded attempt log: surface, ordinal/chain size, public
  canonical catalog ID, lane, HTTP status, and reason (`not_started`/`connect_refused`/none).
  URL/query, auth headers, credentials, and request/response bodies are forbidden.
- Phase 6.4 counters: `claude_router_fallback_total{from_namespace,to_namespace,reason}`
  (reason: `not_started`/`connect_refused`), `claude_api_execution_not_started_total{plane}`,
  fixed-cardinality admission/auth/catalog/pricing/policy/balance-header-timeout series, and phase 6.3
  already exports `claude_api_execution_group_double_winner_total`. The critical
  `ExecutionGroupDoubleWinner` fires on any growth over 5 minutes; runbook —
  `docs/ops/MONITORING.md#executiongroupdoublewinner`. `RouterMetricsDown` covers the loss of
  a single scrape, `RouterFallbackRateHigh` — a sustained rate >1 continuation/s, and
  `RouterConnectionRefusedFallback` — any transport proof over 5 minutes. The recording rules
  `claude_router:fallback_rate5m` and `claude_api:execution_not_started_rate5m` keep only
  bounded namespace/plane dimensions.
- Group labels are NOT added to existing money series (cardinality); after phase 6.3 group_id is
  allowed only in structured attempt logs, not in metric labels.
- Regression detectors: `apitoken_balance_divergence_nano` (existing),
  `EngineSettlementBacklog`, `EngineExpiredLeasePresent` — must pass a load period with
  fallback enabled before GA.
- Rollout flag: fallback is off by default (`CLAUDE_ROUTER_FALLBACK_ENABLED=false`), enabled by a
  config flag on canary; the `deploy` checklist includes measuring the share of ambiguous outcomes
  (timeouts without the header) before and after enabling.
- Phase 6.2 verification: a TCP integration router with two mock planes proves serial
  not_started/ConnectionRefused retry and fail-closed ambiguous outcomes; per-plane phase 6.1
  tests prove refund of the signaling attempt. Phase 6.3 is covered by a SQLite and real-PostgreSQL matrix:
  reverse settlement order, zero settlement, exact loser replay, strict funding refund, and exactly
  one charge per group; forward tests separately verify durable group/attempt for all planes.
- Phase 6.4c verification: `tests/router_fallback_smoke.sh` provides concurrent exact-signal load, strict and
  provider filtering before execution, unsigned-terminal and cached-catalog ConnectionRefused cases with
  exact counter deltas. `tests/router_fallback_live_canary.sh` runs exactly the deployed router
  binary as a separate process, uses only the existing stdin-delivered key, and repeats the matrix
  on real secondary attempts; the GA flag is forbidden until clean double-winner/divergence/backlog
  evidence exists.

## 7. Phasing (each phase is a separate package through the merge pipeline)

1. **6.1 — `not_started` contract in the planes — IMPLEMENTED 2026-08-01** (header-strip in the
   router for transit responses even without fallback, unit/contract lane tests with a real
   reserve, documentation `crates/forward/CLAUDE.md` + `crates/router/CLAUDE.md`). Rollout
   with fallback disabled is safe: the client never sees the header. The Gemini Messages skin
   and the four universal Chat/Responses adapter surfaces are covered by the §3.2 rules; the
   signal-propagation gap is closed before enabling fallback in 6.2.
2. **6.2 — router fallback engine — IMPLEMENTED 2026-08-02:** the `models` field, a single
   preflight/rewrite engine for Chat/Responses/Messages/count_tokens, the §3.3 retry matrix,
   safe attempt logging, feature-flag off-by-default; two-plane TCP mock tests cover the
   exact signal, 429, unsigned 5xx, 400/402, ConnectionRefused,
   timeout, malformed/duplicate/unknown models, and internal header stripping.
3. **6.3 — group identity in registry/billing — IMPLEMENTED 2026-08-02:** migration-first
   schema 0021, trusted router headers, group-aware scalar/legacy/strict reserve, transactional
   insert-first-wins settle in SQLite/PostgreSQL, safe retention, fault matrix, and an always-zero alert.
4. **6.4 — policy/preferences + telemetry GA — 6.4a–6.4b AND THE TELEMETRY/MOCK-LOAD PART
   OF 6.4c IMPLEMENTED 2026-08-02:**
   the producer-first policy preflight is uniformly available on all fixed planes and covered by
   bounded validation, auth-lattice, and real-SQLite strict-policy tests; the router consumer applies
   provider preferences and the exact policy subset before attempt 1. Counters, Prometheus alerts/runbooks,
   mock-load, and a credential-safe live runner are ready. Remaining: the post-deploy live canary and a
   separate production flag enablement; until then fallback remains default-off.

## 8. Rejected alternatives

- **Retry on timeout/5xx without a plane signal** — a direct path to double charging
  (`UNIFIED_ROUTER.md`: "a silent retry on timeout is a path to double charging").
- **Buffering the response in the router to determine started independently** — violates
  the byte-passthrough invariant and inflates the router into a second engine (decision 1).
- **A single request_id across planes (one attempt overwrites another's reservation)** —
  breaks the exactly-once ledger and attempt audit; the group/attempt model is strictly a superstructure.
- **Quorum/hedged requests** — out of scope (§1): consumes capacity and balance on every request.
