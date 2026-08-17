# Request observability contract

> **Status: v1 DECISIONS LOCKED; implementation is incomplete.** Registry S2, forward-core S3A,
> the Caddy logical-ID perimeter, the provider-plane admission/context consumer, and the router
> logical-ID producer are implemented. The only production request-fact producer is metered
> Codex/OpenAI universal `POST /v1/messages/count_tokens`; no private read surface, request-fact
> metric, client classifier, or billable producer exists.
>
> This document is the owner-approved v1 implementation contract. It authorizes only the finite,
> ordered rollout and Definition of Done in §§13-15; it does not claim that those stages are complete.
> Migrations 0053-0054 are deployed; `crates/registry` exposes opt-in PostgreSQL write/lifecycle
> primitives, and `AsyncBilling` transports typed facts through the owning money transactions plus a
> distinct fail-open terminal-at-insert inbox. The Caddy perimeter reserves
> `X-Apitoken-Logical-Request-Id` by deleting internet copies at all four public provider/router
> ingresses while leaving stable loopback origins untouched. Anthropic, OpenAI, Gemini, and Combined
> customer routers consume at most one canonical trusted value before auth/body/reserve/dispatch,
> generate one for direct ingress, remove the wire header, and retain only a typed request extension
> through internal adapters. The provider consumer's exact production SHA is GREEN, and the router
> creates one canonical UUIDv4 after admission and sends it on every executable attempt, reusing it
> across fallback. Codex universal count_tokens consumes the typed extension only after metered
> admission and submits one already-terminal nullable-billing-ID fact through the fail-open PostgreSQL
> inbox. The logical ID remains operator-only. Billable paths, native OpenAI Responses token counting,
> Anthropic, Gemini, and every other plane caller remain absent.

## 1. Purpose

The engine needs request-level evidence for two related goals:

1. understand which customers, API keys, client applications, providers, and models generate load;
2. find incompatibilities between a provider/model implementation and the way clients use it,
   especially streaming, tools, structured output, reasoning controls, retries, and fallback.

The existing billing authority already answers how much successful settled traffic cost and which
token buckets it used. The missing capability is a privacy-minimal lifecycle record that covers
successful requests, refusals, transport failures, mid-stream failures, retries, and non-billable
inference-related calls without storing customer content.

## 2. Current state

`usage_events` is the authoritative settled-usage fact. It stores the engine request identity,
account/key attribution, provider, model, token buckets, web-search usage, and immutable official and
charged nanoUSD. The Control API aggregates it by model/provider, day, and key for customer and
operator dashboards. The schema is defined in `crates/registry/migrations_pg/0001_engine_authority.sql:120-137`
and expanded by `0005_provider_attribution.sql:13-18`.

It is deliberately not a complete request journal. It normally exists only when authoritative usage
reaches settlement, so it does not cover all validation errors, authorization or balance refusals,
provider failures, non-billable calls, or every interrupted stream. It also does not own request
latency, routing attempts, client classification, or general tool-use dimensions.

The dormant S2 PostgreSQL layer now provides privacy-bounded typed admission/terminal records,
exact-replay validation in reservation transactions, delivery tracking in the delivering transaction,
crash-safe terminalization during authoritative settlement-outbox APPLY, reconciliation synthesis,
a bounded terminal batch insert, and request-fact-first retention pruning. Dormant S3A forward-core
plumbing carries opt-in billable facts through the existing single money writer, including crash-safe
fact terminalization when a reserve handoff is lost. A separate 4096-bounded, nonblocking PostgreSQL
inbox persists already-terminal post-auth/nonbillable facts on a lazily connected low-priority thread;
it is fail-open and excluded from money `flush`. Existing methods remain fact-free wrappers, SQLite
money behavior is unchanged and omits analytics, and no production plane caller exercises the new
forms for billable lifecycles yet. The existing authorization snapshot carries the authoritative
non-secret `key_id` beside the raw credential: registry obtains both in the same `key_account`
statement/snapshot, every money path still uses only the raw key, and the first narrow Codex
count_tokens producer exposes only the non-secret identity through a privacy-minimal typed seed.
`billing_outcome` is never accepted in the outbox envelope: APPLY derives it from the authoritative
winner, reconciliation, cancellation, and metered-amount state.

Provider-plane logical identity admission is implemented for customer routes in Anthropic, OpenAI,
Gemini, and the Combined migration bridge. Malformed reserved capabilities fail before auth, body
handling, reserve, or dispatch; direct ingress gets a fresh canonical UUIDv4; the wire header is
removed; and only a typed extension survives through synthesized universal leaf requests. Health,
admin, internal preflight, and backend-only KIMI/GLM/Tripo3D/Suno routes stay outside this MVP. After that
consumer reached production GREEN, the router producer was implemented: client copies are removed,
one fresh canonical ID is generated immediately before the first executable provider attempt, direct
and universal single attempts receive it, and every fallback attempt reuses the same value. Balance,
router preflights/helpers, catalog/health/startup and 404/405 do not receive one.

The HTTP error audit writes one JSON journal event for a terminal non-2xx response to a recognized
metered key; it lives in `crates/server/src/http.rs:774-846` as `audit_customer_error` middleware
(forward only carries the `TerminalErrorReason` extension at `crates/forward/src/proxy.rs:572,653`).
Prometheus and Grafana intentionally exclude customer, key, request, model, credential,
and content identities. These operational surfaces remain useful but are not a durable product
analytics authority.

Request identities are currently fragmented across protocol surfaces. A provider-plane billing ID,
a public response ID, an upstream request ID, a calibration ID, and a router execution group may be
different values. The new contract must make these relationships explicit rather than overloading
one existing ID. Note: `billing_request_id` is the request-fact schema name; the provider-plane
identifier is `engine_request_id` / `request_id` and is generated inside the provider planes (`crates/forward/src/proxy.rs:1292`,
`crates/forward/src/codex/billing.rs:289,394`, `crates/forward/src/gemini/billing.rs:611`) and passed
through `AsyncBilling` (`crates/forward/src/billing.rs:1291-1525,1716-1729`).

## 3. Core invariants

1. **No content collection.** Prompts, message text, images, audio, tool arguments, tool results,
   schemas, arbitrary metadata, session IDs, conversation IDs, IP addresses, raw user agents,
   credentials, full API keys, emails, provider subjects, and raw upstream errors are forbidden.
2. **Billing remains authoritative.** `ledger`, `usage_events`, reservations, and settlement outbox
   remain the sole authorities for money and token usage. Request facts must not become a parallel
   charge or usage ledger.
3. **Observability is not an availability dependency.** A request must not fail, wait, or lose its
   billing settlement because an optional observability write or queue is unavailable.
4. **Streaming transparency remains unchanged.** Instrumentation must not buffer or rewrite response
   bodies, delay the first public byte, broaden retry conditions, or change disconnect draining.
5. **Fixed retry fences remain unchanged.** A provider-plane billing request still spans its current
   internal pre-byte rotation. A router fallback attempt receives its own billing request identity;
   the execution group continues to fence one nonzero winner.
6. **Metrics stay low-cardinality.** Customer, key, request, model, tool fingerprint, provider
   profile, and execution-group identities never become Prometheus labels.
7. **Unknown is explicit.** Missing, unparsed, unsupported, or legacy evidence is `unknown`/`NULL`,
   never a fabricated zero, false, default client, inferred provider, or inferred model.
8. **Retention is explicit and bounded.** Request facts are analytics data, not permanent business
   records. The MVP retention is 30 days and is independent of telemetry retention.

## 4. Identity model

The identities below have different meanings and must remain separate:

| Identity | Meaning | Producer |
|---|---|---|
| `logical_request_id` | One customer request at the public router or direct provider-plane ingress | Router for routed traffic; provider plane for direct traffic |
| `billing_request_id` | One provider-plane money/admission lifecycle, including that plane's internal pre-byte rotation | Provider plane before reserve |
| `execution_group_id` + `attempt` | A router fallback chain and its one-based model/plane attempt | Router, only for an effective chain longer than one |
| `upstream_request_id` | Bounded terminal provider reference, when safely available | Provider plane |
| `calibration_request_id` | Admin-only exact-calibration identity (Gemini and Kimi lanes), canonical UUIDv4 | External admin runbook via `x-apitoken-calibration-request-id` |

The implemented perimeter reserves `X-Apitoken-Logical-Request-Id`: Caddy removes client-supplied
copies on every public provider/router ingress while stable loopback origins preserve the reserved
capability for the trusted internal hop; loopback access alone is not sender authorization. The
implemented provider-process consumer accepts zero values (direct ingress: one
fresh CSPRNG canonical lowercase UUIDv4) or exactly one canonical value, removes the wire header, and
stores only a typed request extension before auth, body handling, reserve, or provider dispatch.
Universal adapters preserve that same extension on synthesized leaf requests without recreating the
wire capability. Malformed identity returns a bounded provider-shaped 400 with `not_started`.
Backend-only KIMI/GLM/Tripo3D/Suno and non-customer routes are outside this MVP. The provider consumer's
exact production SHA is GREEN, so the implemented router producer now creates one CSPRNG UUIDv4 only
at the final executable boundary after auth/body/model/routing/policy admission, removes all inbound
copies again in the common proxy function, and injects the same logical ID into every provider attempt.
Native and universal single attempts receive it; balance and helper/preflight traffic traversing the
common proxy passes no typed ID and therefore only strips. The router neither logs nor publishes it.

The current production request-fact producer is deliberately narrower than the locked v1 matrix. Only the
Codex/OpenAI handler for universal `POST /v1/messages/count_tokens` participates, including Combined
and router universal dispatch that reaches that same handler. Immediately after successful metered
`begin_admission`, it snapshots `pool::now()`, the typed logical ID from request extensions, the
retained execution attempt, and authoritative account/key IDs without exposing the raw key to the
skin. Body/translation/model/prepare/success exits converge through one terminalization call and one
`try_submit_terminal_request_fact`; submission outcomes never affect response status, headers, or
body. Facts use `billing_request_id=NULL`, `billing_outcome=not_applicable`,
`openai`/`universal`/`count_tokens`, stream false, client kind/source unknown, internal attempt count
zero, bounded client model spelling after Messages validation, and canonical public model ID only
after Responses parsing. Deliberately unextracted capability and terminal fields remain NULL. Admin,
unauthorized, missing typed logical context, native OpenAI Responses token counting, billable Messages,
Anthropic, Gemini, and all other surfaces are omitted. SQLite and inbox drops fail open; coverage is
visible only through the existing internal delivery snapshot, not public metrics.

The logical ID is additive, operator-only correlation metadata in v1. It does not replace a
protocol's public response ID, an upstream `request-id`, the billing ID, or the existing
execution-group contract. Existing public ID semantics do not change, and no public response header
exposes the logical ID.

One logical request can therefore produce several request facts when router fallback executes more
than one provider-plane attempt. One request fact represents one provider-plane execution attempt,
not an entire fallback chain and not every internal subscription retry.

**Calibration ID scope.** The separate `calibration_request_id` exists only on the Gemini and Kimi
exact-calibration lanes, where an external admin runbook must correlate a pre-generated UUID with
the resulting immutable turn-evidence row (`crates/registry/migrations_pg/0019_provider_turn_calibration.sql:8`).
On Anthropic and Codex, turn calibration keys on the same plane billing `request_id`
(`crates/forward/src/billing.rs:962`, `persist_anthropic_turn_postgres`), and Codex window-calibration
rows carry no request identity at all. The calibration ID is therefore not a uniform fifth identity
across all planes.

## 5. Customer and client identity

The contract distinguishes three attribution levels:

| Level | Field | Reliability |
|---|---|---|
| Customer/team | engine `account_id` | authoritative |
| Project/integration | non-secret engine `key_id`; key label is resolved only at the presentation boundary | authoritative |
| Client application | normalized `client_kind` plus `client_source` | explicit or heuristic |

The engine remains unaware of commerce `user_id`. Commerce joins an engine account to a person through
its existing `engine_accounts` mapping. Request facts store neither commerce identity nor email.

Client application classification uses the closed v1 vocabulary `opencode`, `claude_code`, and
`unknown`. Every value carries one source:

- `explicit`: a reviewed integration sent the optional untrusted self-report header
  `X-Apitoken-Client: <kind>[/<version>]`;
- `heuristic`: the engine matched a reviewed positive signature from the versioned v1 rules;
- `unknown`: evidence was absent, contradictory, ambiguous, malformed, duplicated, or unsupported.

The explicit header is exactly one total ASCII value no longer than 80 bytes. `kind` is lowercase and
is exactly `opencode` or `claude_code`. When present, `version` is 1-64 characters and matches
`[A-Za-z0-9._+-]+`; an empty version is invalid. The header is attribution only, never authentication
or routing authority. Missing values, multiple field-lines or comma-coalesced values, invalid ASCII or
length/grammar, and unsupported kinds fail open to `client_kind=unknown`, `client_source=unknown`, and
`client_version=NULL`. The provider plane consumes the header before compatibility-header stripping,
retains only the normalized fields, and removes it before any external upstream dispatch.

Heuristic classifier version 1 contains only reviewed positive signatures for `opencode` and
`claude_code`. It persists neither the evidence nor raw headers. A partial, conflicting, or ambiguous
signature is `unknown`; absence of a match is never classified as a generic SDK or custom client.

The only existing inbound heuristic today is a Codex-envelope prefix check on `originator`/`user-agent`
(`crates/forward/src/codex/api.rs:409-416`). There is no `claude_code`, `opencode`, or `cursor`
detection anywhere in `crates/forward` or `crates/server`; the vocabulary above is new code. The
engine deliberately strips client `x-stainless*`, `user-agent`, `anthropic-beta`, `x-claude-code-session-id`,
`x-conversation-id`, and `x-session-id` headers and synthesizes its own Claude-Code fingerprint upstream
(`crates/forward/src/proxy.rs:193-216`), so inbound classification must run before that strip.

Heuristics may inspect headers already required for compatibility, but raw header values are never
persisted. Heuristic evidence must never be presented as certain. Separate labeled API keys remain
the most reliable project-level attribution and must not be replaced by fingerprinting.

## 6. Tool and capability dimensions

The MVP records bounded structural facts, not names or payloads:

- `tools_declared_count`;
- a closed tool-class bitset: `custom_function`, `custom_tool`, `web_search`, `computer`,
  `code_execution`, `mcp`, and `other_reviewed`;
- `tool_choice_mode`: `auto`, `required`, `none`, `named`, or `unknown`;
- `parallel_tools_requested`: nullable boolean;
- `tool_results_in_input`;
- `tool_calls_in_output`;
- request flags for streaming, structured output, reasoning, service tier, and input/output
  modalities where the adapter has already validated them.

Arbitrary tool names, descriptions, JSON schemas, arguments, results, MCP server names, and unknown
tool types are forbidden. A toolset fingerprint is not part of v1. Adding one requires a later
versioned scope decision; it cannot be inferred from this contract.

`web_search_requests` in `usage_events` remains the authoritative billable search counter. A request
fact may describe the presence/class of a web-search tool but must not override settlement usage.

## 7. Durable fact contract

Migrations 0053-0054 establish the current additive SQL envelope. Any further expand-only migration
must preserve this v1 semantic record:

| Group | v1 fields |
|---|---|
| Identity | `fact_id`, `logical_request_id`, nullable unique `billing_request_id`, nullable `execution_group_id`, positive `attempt` |
| Attribution | `account_id`, non-secret `key_id`, `client_kind`, `client_source`, nullable bounded client version |
| Request | provider plane, route/surface class, request class, requested model, executable model, stream flag |
| Capabilities | tool dimensions from section 6, structured-output/reasoning/service-tier/modality flags |
| Lifecycle | `admitted_at`, `delivery_started_at`, `first_public_byte_at`, `terminal_at` |
| Result | exact bounded HTTP status code, provider terminal class, billing outcome, downstream-disconnect observation, bounded upstream request ID |
| Diagnostics | provider-plane internal attempt count, bounded failure class, instrumentation schema version |

The four v1 timestamp points are mandatory when safely observable without changing transport:

- `admitted_at`: successful post-auth customer admission to the scoped leaf route;
- `delivery_started_at`: the existing durable delivery boundary, before upstream execution or a public
  response can be called started;
- `first_public_byte_at`: the first response-body byte made available to the customer, measured without
  buffering or rewriting;
- `terminal_at`: the terminal lifecycle observation or authoritative settlement/reconciliation point.

The read surface safely derives four durations and only when both endpoints are measured and ordered:
admission-to-delivery, admission-to-first-public-byte, delivery-to-first-public-byte, and
admission-to-terminal. A missing, unsafe, or contradictory endpoint yields `NULL`; zero is never
substituted for unmeasured evidence.

Models may be stored only after the owning parser has accepted a bounded canonical string. Request
facts distinguish the model supplied by the client from the model actually executed. They do not
infer provider identity from model spelling.

Outcomes are independent dimensions rather than one misleading `status`:

- `delivery_state`: `not_started`, `started`, `completed`, `interrupted`, `unknown`;
- `provider_terminal_class`: `success`, `client_error`, `quota`, `auth`, `timeout`, `transport`,
  `upstream_error`, `protocol_error`, `unknown`;
- `billing_outcome`: `winner`, `loser`, `zero_metered`, `canceled`, `reconciled`, `not_applicable`,
  `unknown`;
- `http_status_code`: exact bounded status code (100-599); the class is a derived
  projection for reports and dashboards. Storing the exact code preserves diagnostic value
  (for example, 400 vs 422 vs 413) at no meaningful cardinality cost in the database.

There is intentionally no generic `billing_outcome=settled` client-success label. A router loser,
downstream disconnect, provider success, and financial cancellation can coexist in combinations
that one scalar cannot represent.

## 8. Write lifecycle

### 8.1 Billable metered request

The request fact is inserted or exact-replay-validated in the same authority transaction that
creates the reservation. This guarantees that every accepted billable lifecycle has a durable fact
without adding a second hot-path round trip. Before either backend receives the money command, forward
requires the fact's `billing_request_id`, `account_id`, `execution_group_id`, and `attempt` to match the
reservation arguments. The forward authorization result now carries both the raw secret key and its
authoritative non-secret `key_id`, obtained by the existing `key_account` statement in one snapshot.
Current reserve/settle calls still receive the raw key, and no billable request-fact producer uses
`key_id`. The nonbillable Codex count_tokens producer consumes a separate typed seed that contains only
the authoritative non-secret identity. PostgreSQL keeps the final authoritative same-transaction fact
key lookup/comparison, while SQLite intentionally persists no request-fact analytics.

`delivery_started_at` is set in the transaction that marks the reservation delivering. Reserve alone
is admission evidence, not evidence that an upstream execution or public response started.

Terminal fields are updated only inside the transaction that actually applies the settlement outbox,
after winner/loser and effective actual usage are known. Updating the fact when `settle` merely
enqueues the outbox would create false terminal evidence after a crash between enqueue and apply.
Reconciliation and exact replay use the same terminal updater.

The fact stores lifecycle dimensions only. Authoritative tokens and money stay in `usage_events` and
`ledger`, joined by `billing_request_id` when a report needs them.

### 8.2 Post-auth error or non-billable request

An already terminal fact is submitted through a separate low-priority bounded inbox owned by
`AsyncBilling`. It uses non-blocking `try_send`, drops on full/closed, never enters the money FIFO,
and is not part of the mandatory billing shutdown flush. The producer captures immutable
admission-time `account_id` and `key_id`; it must not repeat authorization after the response.

The inbox must be drained by a separate low-priority writer connection or a strictly deferred
batch-insert path, not by the money writer thread itself. `AsyncBilling` already protects money
with a single writer thread and a `Flush` barrier
(`crates/forward/src/billing.rs:49,1291,1522-1524,1716-1729`); a slow observability insert on that thread
would add latency to admission and
settlement. Post-auth facts are terminal at insert, so they use `INSERT ... ON CONFLICT DO NOTHING`
without updates. Connection acquisition may retry before any SQL attempt. Once an insert returns an
error, the connection is discarded; the batch may be replayed only when every row has a non-null
`billing_request_id`, whose unique constraint makes the whole batch idempotent. If any row has a
nullable billing identity, commit status is uncertain and the batch is dropped and counted failed
rather than replayed, preserving at-most-once insertion for rows that have no uniqueness key.

Dropped events increment the existing fixed internal delivery-snapshot counters by bounded reason.
For this first producer those counters are not exposed as public metrics or reads; queue depth, dropped
total, and persistence health remain a later private coverage surface rather than silently inferred
from request rows.

Unauthenticated requests have no customer identity and are outside the customer analytics MVP.
Aggregate auth failures remain operational metrics. A later abuse/security design may define a
separate privacy boundary; it must not be smuggled into this contract.

## 9. Read surfaces

Request-fact reads are private Control API contracts guarded by `control_authed`, not public APIs and
not panel-key metrics. The v1 producer exposes exactly:

1. `GET /admin/request-facts/summary` for bounded fleet and optional exact-account aggregation;
2. `GET /admin/request-facts` for keyset-paginated drilldown;
3. `GET /admin/request-facts/logical/{id}` for every persisted plane attempt of one exact canonical
   logical request ID.

Every window is half-open `[from,to)`, is explicitly supplied, and is no wider than 30 days. Drilldown
orders newest first by `(admitted_at,fact_id)`, uses that tuple as the opaque keyset cursor, and accepts
`limit=1..200` with a default and maximum of 200; offset and unbounded queries do not exist. Summary
axes are the closed client kind/source, provider plane, requested/executed model, route/surface,
streaming, tool class/choice, terminal classes, fallback/retry counts, and the four safely derived
latency distributions from §7.

Coverage is a first-class response object, separate from result rows and aggregates. It reports the
requested window and `scope_version=1`, persisted facts, drops/persistence failures attributable to
that window where durable evidence exists, incomplete/nonterminal facts, and legacy/unknown evidence.
The process-runtime queue/health snapshot is returned separately with its own observation interval and
restart/continuity state; it is never presented as historical window coverage. A denominator, coverage
percentage, or completeness claim is emitted only when an independent scoped admitted-request count
proves it. Missing pre-producer history and process restarts therefore stay explicit unknowns rather
than a false zero or 100% coverage.

Responses return no raw API key, key label, email, prompt, content, provider subject/profile, or raw
error. The admin backend may resolve account/key display metadata through its existing authoritative
mappings. The producer must reach the complete v1 route matrix, pass the producer coverage gate, ship
the fixed operations surfaces, and pass the 24-hour observation gate in §13 steps 8-10 before any
read endpoint or cross-context consumer ships.

The customer-facing usage API is unchanged in v1. There is no customer request-fact API, aggregate,
or request-level UI.

## 10. Metrics and logs

Prometheus publishes only compile-bounded dimensions: provider plane, route/surface class, stream,
status/terminal class, queue outcome, and persistence outcome. The v1 metric set covers lifecycle
totals, the four safely derived duration histograms from §7, request-fact inbox capacity/depth and
drops, persistence health/failures, and facts still nonterminal after one hour. Models, tools, clients,
accounts, keys, logical/billing/upstream IDs, execution groups, and provider profiles are database
report dimensions, never Prometheus labels.

Each alert below requires a matching fixed-cardinality metric, rule, dashboard view, and runbook
anchor in the same metric commit:

| Condition | Alert threshold |
|---|---|
| Request-fact persistence unhealthy | continuously unhealthy for more than 5 minutes |
| Terminal inbox queue pressure | depth at least 75% of fixed capacity for 10 minutes |
| Dropped facts | more than 1% of submissions over 15 minutes, only when submissions are at least 100 |
| Stuck lifecycle | any scoped fact nonterminal for more than 1 hour, sustained for 15 minutes |

Journald/Loki remains an incident surface. Terminal errors and invariant violations carry the logical
request ID once correlation exists, but routine successful facts are not duplicated as high-volume
journal lines. Existing logs that print full subscription email migrate to an opaque home identity as
a related privacy hardening step.

## 11. Retention and indexing

MVP retention is 30 days. Pruning is bounded and independent from reservation/outbox deletion. No
foreign key may cascade facts away with shorter transient lifecycle storage, and facts must not keep
reservations alive past their own retention boundary. Request-fact pruning must respect the existing
validated 30-day minimum enforced by `validate_request_lifecycle_prune_cutoff`
(`crates/registry/src/pricing/snapshots.rs:36-50`), which today guards `maintenance_prune`
deletions of `settlement_outbox`, `reservations`, and `execution_group_winner`. The pruning order
must finalize or prune facts before their corresponding lifecycle rows, never resurrect a pruned
reservation, and never extend a fact past the shared retention window.

The deployed migration contains only query-proven indexes:

- nullable unique `billing_request_id`;
- `(logical_request_id, attempt)`;
- `(account_id, admitted_at DESC, fact_id)`;
- one compact time index for pruning/fleet windows.

Mutable terminal columns are not broadly indexed; keeping them out of indexes allows HOT-style
updates and avoids settlement write amplification. There is no unbounded `all` query or offset
pagination.

## 12. Ownership and affected components

- `crates/registry`: additive PostgreSQL migration, SQLite semantic parity where required by the
  rollback/test contract, insert/update/query/prune primitives;
- `crates/forward`: admission snapshot, provider parsers, tool/capability classification, lifecycle
  updates, low-priority inbox, stream-safe terminal observations. `AsyncBilling` is a single-writer +
  N-reader actor with a bounded 4096-entry FIFO and a `Flush` barrier
  (`crates/forward/src/billing.rs:49,1291,1522-1524,1716-1729`);
- `crates/server`: composition, private Control API reads, bounded telemetry, request correlation.
  The existing Control API aggregates `usage_events` via `GET /admin/account/{id}/usage`,
  `GET /spend-stats`, and `GET /fleet-history` (`crates/server/src/admin.rs:947-1056`,
  `crates/server/src/http.rs:392` (`router`), `:3961` (`spend_stats`), `:4058`
  (`spend_window_json`), and `:4490` (`fleet_history`));
- `crates/router`: trusted logical request ID production and propagation across fallback attempts;
- `deploy/Caddyfile`: implemented owner of the completed security-only perimeter stage; the
  `strip_execution_identity` snippet removes client-supplied logical identity at all four public
  ingresses without changing stable loopback origins;
- `packages/contracts`: additive typed private contract producer only after the engine read producer
  and 24-hour gate; `packages/engine-client` consumes it only after its exact SHA is GREEN;
- `apps/api`: private operator identity join only after the engine-client producer is GREEN;
- `apps/admin`: dedicated Request Analytics UI only after the `apps/api` producer is GREEN;
- `observability/` and `docs/ops/MONITORING.md`: only aggregate health metrics, alerts, dashboards,
  and runbooks.

`crates/pool` does not own this feature. Selection remains there, but persistence, HTTP, and client
analytics must not be introduced into the pool layer.

## 13. Ordered finite rollout

The rollout is finite and ordered; a stage that depends on a producer or contract begins only after
the prerequisite exact SHA is production GREEN.

1. **Storage and S2 primitives — complete.** Migrations 0053-0054, opt-in
   reservation/delivery/terminal lifecycle, reconciliation, bounded terminal inserts, and 30-day
   pruning are deployed. Legacy callers remain fact-free.
2. **Forward-core S3A transport — complete.** Fact-aware money commands use the money actor; already
   terminal post-auth/nonbillable facts use the distinct bounded fail-open PostgreSQL inbox.
3. **Logical-ID perimeter and consumers — complete.** The Caddy trust boundary, Anthropic/OpenAI/
   Gemini/Combined provider consumers/direct generators, and typed adapter propagation are deployed.
4. **Router producer — complete.** Routed executable requests receive one operator-only logical ID,
   reused across fallback attempts and kept separate from billing and execution-group identities.
5. **Freeze v1 classifiers.** Add the exact client-header grammar and fail-open normalization from
   §5, reviewed positive heuristic rule set version 1, closed tool classes from §6, lifecycle clocks
   from §7, and privacy-negative tests. This stage changes no public response contract.
6. **Complete nonbillable producers.** Cover exactly Anthropic native Messages
   `POST /v1/messages/count_tokens`; OpenAI universal Messages `POST /v1/messages/count_tokens` plus
   native Responses `POST /v1/responses/input_tokens`; and Gemini universal Messages
   `POST /v1/messages/count_tokens` plus native `POST /v1beta/models/{model}:countTokens`. The existing
   Codex/OpenAI universal Messages caller is the first completed slice. Discovery, stored-response
   reads, health, balance, catalogs, router/provider preflights, and auth helpers remain excluded.
7. **Complete billable producers.** Cover every customer-facing text-generation leaf route on the
   Anthropic, OpenAI, and Gemini planes, in native and universal protocols, stream and nonstream. This
   includes native Anthropic Messages, OpenAI Responses/Chat, Gemini generateContent/
   streamGenerateContent, and the universal Messages/Responses/Chat adapters that execute on those
   planes. A Combined route creates only the underlying leaf fact and never an extra Combined fact.
   Backend-only KIMI and GLM, Tripo3D, Suno, images, embeddings, files, and batches remain outside v1.
   Future private API/UI responses declare `scope_version=1` for this exact matrix.
8. **Pass the producer coverage gate.** Prove the route manifest mechanically against server/router
   dispatch, prove one and only one leaf fact per scoped plane attempt, prove all exclusions, and run
   the full privacy, lifecycle, stream-transparency, fallback, persistence, and billing-invariant
   suite. No read endpoint, metric completeness ratio, or UI may ship before this gate is GREEN.
9. **Deliver fixed-cardinality operations surfaces.** Add the metrics, alert rules, dashboard panels,
   and runbook anchors from §10 under the new metric checklist.
10. **Run the 24-hour observation gate.** Compare the instrumented exact SHA with its approved
    baseline and apply every threshold from §15. Any breach stops rollout and is fixed or rolled back
    before the private Control API or any cross-context consumer.
11. **Deliver private Control API producers.** Only after step 10 passes, add the three endpoints and
    coverage semantics from §9; update `docs/engine/CONTROL_API.md` and `docs/DEPENDENCIES.md` in the
    same producer commit, then wait for its exact SHA to be production GREEN.
12. **Deliver `packages/contracts`.** Only after the step-11 producer is GREEN, add the typed private
    request-analytics schemas as an additive contract producer; update `docs/DEPENDENCIES.md` in the
    same commit and wait for the exact SHA to be GREEN.
13. **Deliver `packages/engine-client`.** Only after the step-12 contract producer is GREEN, consume
    those schemas in the sole TypeScript Control API transport; wait for the exact SHA to be GREEN.
14. **Deliver `apps/api`.** Only after the step-13 engine-client producer is GREEN, add the private
    admin-backend identity join and request-analytics projection; wait for the exact SHA to be GREEN.
15. **Deliver `apps/admin`.** Only after the step-14 API producer is GREEN, add the dedicated
    **Request Analytics** area, linked from but not mixed into Engine Spend. It consumes only the
    private producer chain. There is no customer API or UI.
16. **Record final proof.** Capture exact producer and consumer SHAs, migration/watchdog verdicts,
    route-manifest coverage, exclusion tests, privacy-negative tests, metric/alert/runbook checks,
    24-hour thresholds, and bounded live smoke results. Live-smoke credentials and budget records may
    remain in the operator evidence system rather than this repository; no secret or customer content
    enters the proof.

Every migration and cross-context contract remains expand-only and producer-first. Completed stages
are not repeated or combined with dependent producer/consumer stages.

## 14. Verification requirements

Implementation is incomplete without tests for:

- PostgreSQL migration, exact replay, pruning, bounded queries, and 30-day retention;
- SQLite/PostgreSQL semantic parity for paths required by the existing authority contract;
- crash between settlement enqueue and actual outbox apply;
- reconciliation and execution-group winner/loser outcomes;
- reservation cancellation before delivery;
- successful non-stream and stream responses on every scoped leaf route;
- first-byte/mid-stream failure, malformed terminal event, and downstream disconnect drain;
- internal provider rotation versus router fallback attempts, including no extra Combined fact;
- post-auth validation, balance, quota, and provider errors;
- the exact nonbillable matrix and every explicit v1 exclusion from §13;
- explicit client-header valid/missing/malformed/duplicate/unsupported cases and positive/ambiguous
  heuristic signatures;
- tool declaration/result/output-call classification on Anthropic, OpenAI, and Gemini shapes, with no
  names, payloads, schemas, or fingerprint persisted;
- stripping and strict validation of internal logical-ID headers and absence of a public logical-ID
  response header;
- all four timestamp points and `NULL` for every unsafe/unmeasured derived duration;
- fail-open behavior and measurable drops of the low-priority inbox;
- absence of forbidden content and identity fields in DB rows, logs, metrics, and API responses;
- Control API window bounds, authorization, `(admitted_at,fact_id)` keyset pagination, maximum 200,
  logical-ID lookup, `scope_version=1`, and honest separation of window coverage from runtime health;
- fixed-cardinality metrics and exact alert thresholds from §10;
- unchanged transparent streaming bytes and existing execution-state retry and billing fences.

## 15. Locked v1 decision record and Definition of Done

The owner-approved v1 decisions are locked:

1. `logical_request_id` is operator-only; v1 adds no public header.
2. The nonbillable and billable route matrices and exclusions are exactly §13 steps 6-7. Combined
   creates no additional fact. Future private API/UI responses declare `scope_version=1`.
3. `X-Apitoken-Client` is an optional untrusted self-report for only `opencode` and `claude_code`,
   with the exact grammar and fail-open behavior in §5. Heuristic v1 contains only reviewed positive
   signatures; ambiguous evidence is `unknown`.
4. Tool analytics uses only the closed structural classes in §6. There is no fingerprint in v1.
5. The four timestamps and four safely derived durations in §7 are mandatory; unmeasured is `NULL`.
6. The operator surface is a dedicated Request Analytics area in `apps/admin`, linked from but not
   mixed with Engine Spend. There is no customer API or UI.
7. Complete producer coverage, fixed operations surfaces, and the 24-hour observation are hard gates
   before the private Control API. The exact private read surface, keyset, limits, windows, and honest
   coverage semantics are §9.
8. Retention is exactly 30 days. The fixed-cardinality metric and alert contract is §10.
9. The rollout order and final evidence bundle are §13. Each cross-context stage proceeds only after
   its previous producer's exact SHA is GREEN. Later providers or modalities require a new versioned
   scope decision rather than silently widening v1.

The MVP is **Done** only when all of the following finite conditions hold:

- §§13.1-13.10 are complete on production-GREEN exact SHAs, including the mechanical producer
  coverage gate, fixed operations surfaces, 24-hour observation gate, and every test in §14;
- the exact scoped route manifest has no missing leaf and no duplicate/Combined fact, while every v1
  exclusion produces no request fact;
- private reads return `scope_version=1`, enforce `[from,to) <= 30d`, keyset ordering and `limit<=200`,
  and never manufacture a coverage denominator from persisted rows or a process-runtime snapshot;
- the metrics, alerts, dashboard, and runbooks implement the four thresholds in §10 with no forbidden
  label cardinality;
- during one continuous 24-hour observation window, admission and first-public-byte latency p99 for
  both scoped billable and scoped nonbillable traffic do not increase by both more than `+5 ms` and
  more than `+10%` of the approved baseline for any continuous 15-minute evaluation; attributable
  request-error rate increases by no more than `0.1` percentage points; dropped/persistence accounting
  has no unexplained gap; and reservation, settlement, execution-group winner, billed usage, and
  charged nanoUSD invariants remain unchanged;
- any attributable error-rate breach, latency breach satisfying both the absolute and relative
  thresholds, persistence-health alert, unexplained fact loss, billing divergence, changed response
  bytes, or changed retry/stream fence blocks the consumer rollout until a new exact SHA passes a
  fresh 24-hour window;
- §§13.11-13.16 are complete in order: the private Control API, `packages/contracts`,
  `packages/engine-client`, `apps/api`, and dedicated `apps/admin` UI each follow their previous GREEN
  producer, and the final proof records exact SHAs, production verdicts, route/exclusion/privacy
  coverage, observation results, and bounded live smoke evidence.

Passing unit tests alone, a partial provider matrix, rows without honest coverage, or a UI over the
current single Codex count-token producer is not completion.

## 16. Historical review findings and code confirmations (2026-08)

The following conclusions were verified against `origin/master` at `916dee0d` in read-only worktrees.
They are retained as source observations; where an earlier recommendation differed, the locked v1
record in §15 is authoritative.

### 16.1 Confirmed by code

- `usage_events` is the authoritative settled-usage fact: `request_id` (unique), `account_id`, `key`,
  `model`, five token buckets, `web_search_requests`, `real_nano`, `charge_nano`, `provider`
  (`crates/registry/migrations_pg/0001_engine_authority.sql:120-137`,
  `0005_provider_attribution.sql:13-18`). The only runtime write is inside the settlement outbox
  apply transaction (`crates/registry/src/pg.rs:1677-1691`); losing execution-group attempts and
  model-less reconciliation charges do not produce a row (`crates/registry/src/pg.rs:1558-1574,1653-1693`).
- The HTTP error audit lives in `crates/server/src/http.rs:774-846` as `audit_customer_error`
  middleware; `crates/forward` only carries the `TerminalErrorReason` response extension
  (`crates/forward/src/proxy.rs:572,653`).
- Request identities are fragmented: the billing-plane identifier is `engine_request_id`/`request_id`
  generated in the provider planes (`crates/forward/src/proxy.rs:1292`,
  `crates/forward/src/codex/billing.rs:289,394`, `crates/forward/src/gemini/billing.rs:611`) and
  passed through `AsyncBilling`; provider implementations expose their established public request-ID
  semantics independently, and the upstream reference is `BillCtx.reference`
  (`crates/forward/src/meter.rs:39-40`).
- `AsyncBilling` is a single-writer + N-reader actor with a bounded 4096-entry money FIFO and a
  `Flush` barrier (`crates/forward/src/billing.rs:49,1291,1522-1524,1716-1729`).
- Settlement outbox enqueue and apply are distinct steps; winner/loser is decided only at apply
  (`crates/registry/src/pg.rs:1558-1574`).
- The router creates an execution group only when `attempt_count > 1`
  (`crates/router/src/routing.rs:532-539`); single-attempt traffic sends no group header.
- `control_authed` is defined in `crates/forward/src/proxy.rs:348`, not in `crates/server`; existing
  Control API aggregation endpoints are `GET /admin/account/{id}/usage`, `GET /spend-stats`, and
  `GET /fleet-history` (`crates/server/src/admin.rs:947-1056`,
  `crates/server/src/http.rs:392` (`router`), `:3961` (`spend_stats`), `:4058`
  (`spend_window_json`), and `:4490` (`fleet_history`)).
- Prometheus `/metrics` uses fixed compile-bounded series with no per-request or per-customer labels
  (`crates/server/src/http.rs:401` and the `metrics` handler at `:1130`, including its billing
  aggregate block at `:1240-1262`).
- Retention today is 30 days for ledger/usage_events (`LEDGER_RETENTION_DAYS = 30`) and 30 days for
  reservations/settlement_outbox (`REQUEST_LIFECYCLE_RETENTION_DAYS = 30`), enforced by separate
  prune loops (`crates/server/src/main.rs:35-39`, `crates/server/src/poller.rs:97-160`) with a
  validated 30-day minimum (`crates/registry/src/pricing/snapshots.rs:36-50`).
- SQLite/PostgreSQL semantic parity is a registry contract requirement
  (`crates/registry/CLAUDE.md:140-149`), with mirrored primitives for reserve, settle, execution-group
  winner, and exact replay.
- Client classification is essentially greenfield: the only existing inbound heuristic is the Codex
  envelope prefix check on `originator`/`user-agent` (`crates/forward/src/codex/api.rs:409-416`).
  The engine strips client `x-stainless*`/`user-agent`/`x-conversation-id`/`x-session-id` and
  synthesizes its own upstream fingerprint (`crates/forward/src/proxy.rs:193-216`).
- The `x-apitoken-*` internal capability header convention already exists
  (`x-apitoken-execution-group`, `x-apitoken-attempt`, `x-apitoken-execution-state`,
  `x-apitoken-service-tier`, `x-apitoken-calibration-*`), with Caddy stripping at public ingress
  (`deploy/Caddyfile:76-79`) and fail-closed validation in the plane.
- The streaming and pre-byte retry fences described in §3(4-5) are real and tested
  (`crates/forward/src/meter.rs:124` TeeMeter, `crates/forward/src/proxy.rs:2046-2072`,
  `docs/engine/ROUTING_FENCING.md`).

### 16.2 Corrections and clarifications to this document

- The HTTP error audit is owned by `crates/server`, not by `crates/forward` (§2, §12).
- `billing_request_id` is the request-fact schema name, not the provider-plane variable name; the
  actual plane-generated identifier is `engine_request_id`/`request_id` (§2, §4).
- A separate `calibration_request_id` exists only on the Gemini and Kimi exact-calibration lanes
  (`crates/forward/src/gemini/billing.rs:13`, `crates/forward/src/kimi/gateway.rs:100`); on Anthropic
  and Codex, turn calibration keys on the same plane billing `request_id`, and Codex window
  calibration rows carry no request identity (§4).
- `control_authed` is defined in `crates/forward/src/proxy.rs:348` (§9, §12).
- The router creates an execution group only for chains longer than one attempt; if the logical
  request ID is sent to every attempt, that is a new convention requiring extension of the existing
  strip and validation discipline (§4, §13 step 4).
- Request-fact pruning must respect the existing validated 30-day minimum enforced by
  `validate_request_lifecycle_prune_cutoff` and must not resurrect or extend pruned lifecycle rows
  (§11).

### 16.3 Risks carried into the locked contract

1. **Writer contention.** The low-priority observability inbox stays off the `AsyncBilling` money
   writer. Its separate connection/path and fail-open behavior are mandatory so analytics cannot add
   admission or settlement latency.
2. **Hot-transaction amplification.** Billable admission inserts a fact in the reservation
   transaction. The dual absolute/relative latency thresholds and fresh 24-hour gate in §15 decide
   whether that cost is acceptable.
3. **Exact HTTP evidence.** The durable row stores a bounded exact status code (100-599); reports may
   derive a class without discarding the diagnostic distinction.
4. **Client attribution.** The explicit header and heuristic classifier obey §5. No raw evidence,
   generic-client fallback, or tool/client fingerprint is permitted.
5. **Public identity.** The logical request ID remains operator-only under §15; existing public
   `x-request-id` semantics do not change.
