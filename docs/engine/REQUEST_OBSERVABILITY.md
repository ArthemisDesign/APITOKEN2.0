# Request observability contract

> **Status: PROPOSED; dormant registry S2 and forward-core S3A plumbing implemented, no plane producer or read surface.**
> This document records the target design and rollout constraints for discussion. Migrations 0053-0054
> are deployed; `crates/registry` exposes opt-in PostgreSQL write/lifecycle primitives, and
> `AsyncBilling` now transports those typed facts through the owning money transactions plus a distinct
> fail-open terminal-at-insert inbox. No provider-plane/server/router caller, logical-ID wire capability,
> read API, metric, or end-to-end coverage guarantee exists yet; all current production callers continue
> to omit request facts. Producer stages remain pending and require the producer-first changes below.

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
forms yet. `billing_outcome` is never accepted in the outbox envelope: APPLY derives it from the
authoritative winner, reconciliation, cancellation, and metered-amount state.

The HTTP error audit writes one JSON journal event for a terminal non-2xx response to a recognized
metered key; it lives in `crates/server/src/http.rs:667-739` as `audit_customer_error` middleware
(forward only carries the `TerminalErrorReason` extension at `crates/forward/src/proxy.rs:564,648-649`).
Prometheus and Grafana intentionally exclude customer, key, request, model, credential,
and content identities. These operational surfaces remain useful but are not a durable product
analytics authority.

Request identities are currently fragmented across protocol surfaces. A provider-plane billing ID,
a public response ID, an upstream request ID, a calibration ID, and a router execution group may be
different values. The new contract must make these relationships explicit rather than overloading
one existing ID. Note: the symbol `billing_request_id` does not exist in code; the real identifier
is `engine_request_id` / `request_id` generated inside the provider planes (`crates/forward/src/proxy.rs:1288`,
`crates/forward/src/codex/billing.rs:171,276`, `crates/forward/src/gemini/billing.rs:611`) and passed
through `AsyncBilling` (`crates/forward/src/billing.rs:1346-1376`).

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

Four identities have different meanings and must remain separate:

| Identity | Meaning | Producer |
|---|---|---|
| `logical_request_id` | One customer request at the public router or direct provider-plane ingress | Router for routed traffic; provider plane for direct traffic |
| `billing_request_id` | One provider-plane money/admission lifecycle, including that plane's internal pre-byte rotation | Provider plane before reserve |
| `execution_group_id` + `attempt` | A router fallback chain and its one-based model/plane attempt | Router, only for an effective chain longer than one |
| `upstream_request_id` | Bounded terminal provider reference, when safely available | Provider plane |
| `calibration_request_id` | Admin-only exact-calibration identity (Gemini and Kimi lanes), canonical UUIDv4 | External admin runbook via `x-apitoken-calibration-request-id` |

The router must create one CSPRNG UUIDv4 `logical_request_id` before the first executable attempt and
send it to every attempt through a new internal capability header. Caddy removes client-supplied
copies on every public ingress; the router removes them again before injecting its own value. A
provider plane strictly validates the internal value. A direct request with no trusted internal
value receives a fresh plane-generated logical ID.

The logical ID is additive correlation metadata. It does not replace a protocol's public response ID,
an upstream `request-id`, the billing ID, or the existing execution-group contract. Existing public
ID semantics must not silently change. A later explicit decision may expose the logical ID in a new
response header, but the MVP may keep it operator-only until compatibility is proven.

One logical request can therefore produce several request facts when router fallback executes more
than one provider-plane attempt. One request fact represents one provider-plane execution attempt,
not an entire fallback chain and not every internal subscription retry.

**Calibration ID scope.** The separate `calibration_request_id` exists only on the Gemini and Kimi
exact-calibration lanes, where an external admin runbook must correlate a pre-generated UUID with
the resulting immutable turn-evidence row (`crates/registry/migrations_pg/0019_provider_turn_calibration.sql:8`).
On Anthropic and Codex, turn calibration keys on the same plane billing `request_id`
(`crates/forward/src/billing.rs:928`, `persist_anthropic_turn_postgres`), and Codex window-calibration
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

Client application classification uses a closed, versioned vocabulary such as `claude_code`,
`opencode`, `codex_cli`, `cursor`, `sdk`, `custom`, and `unknown`. Every value carries one source:

- `explicit`: a reviewed integration sent a bounded client-identification header;
- `heuristic`: the engine classified existing protocol headers using a versioned rule;
- `unknown`: evidence was absent, contradictory, malformed, or unsupported.

The only existing inbound heuristic today is a Codex-envelope prefix check on `originator`/`user-agent`
(`crates/forward/src/codex/api.rs:409-416`). There is no `claude_code`, `opencode`, or `cursor`
detection anywhere in `crates/forward` or `crates/server`; the vocabulary above is new code. The
engine deliberately strips client `x-stainless*`, `user-agent`, `anthropic-beta`, `x-claude-code-session-id`,
`x-conversation-id`, and `x-session-id` headers and synthesizes its own Claude-Code fingerprint upstream
(`crates/forward/src/proxy.rs:193-216`), so inbound classification must run before that strip.

An explicit header is stripped before external upstream dispatch and has bounded ASCII kind/version
values. Heuristics may inspect headers already required for compatibility, but raw header values are
never persisted. Heuristic evidence must never be presented as certain. Separate labeled API keys
remain the most reliable project-level attribution and must not be replaced by fingerprinting.

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
tool types are forbidden. A toolset fingerprint is not part of the MVP. If later required, it must
be a versioned, domain-separated keyed digest of canonical validated structure; an ordinary hash is
not sufficient because low-entropy toolsets can be recovered by dictionary matching.

`web_search_requests` in `usage_events` remains the authoritative billable search counter. A request
fact may describe the presence/class of a web-search tool but must not override settlement usage.

## 7. Proposed durable fact

The exact SQL shape is deferred to the migration review, but the semantic record is:

| Group | Proposed fields |
|---|---|
| Identity | `fact_id`, `logical_request_id`, nullable unique `billing_request_id`, nullable `execution_group_id`, positive `attempt` |
| Attribution | `account_id`, non-secret `key_id`, `client_kind`, `client_source`, nullable bounded client version |
| Request | provider plane, route/surface class, request class, requested model, executable model, stream flag |
| Capabilities | tool dimensions from section 6, structured-output/reasoning/service-tier/modality flags |
| Lifecycle | admitted, delivery-started, first-public-byte when safely measurable, and terminal timestamps |
| Result | HTTP status class, provider terminal class, billing outcome, downstream-disconnect observation, bounded upstream request ID |
| Diagnostics | provider-plane internal attempt count, bounded failure class, instrumentation schema version |

Models may be stored only after the owning parser has accepted a bounded canonical string. Request
facts distinguish the model supplied by the client from the model actually executed. They do not
infer provider identity from model spelling.

Outcomes are independent dimensions rather than one misleading `status`:

- `delivery_state`: `not_started`, `started`, `completed`, `interrupted`, `unknown`;
- `provider_terminal_class`: `success`, `client_error`, `quota`, `auth`, `timeout`, `transport`,
  `upstream_error`, `protocol_error`, `unknown`;
- `billing_outcome`: `winner`, `loser`, `zero_metered`, `canceled`, `reconciled`, `not_applicable`,
  `unknown`;
- `http_status_class`: exact bounded status code (100-599), not a class; the class is a derived
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
reservation arguments. The forward layer has only the raw secret key and cannot safely compare it to
the non-secret fact `key_id`; PostgreSQL therefore keeps the authoritative same-transaction key lookup
and comparison, while SQLite intentionally persists no request-fact analytics.

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
with a single writer thread and a `Flush` barrier (`crates/forward/src/billing.rs:1668-1674`,
`:1474-1476`); a slow observability insert on that thread would add latency to admission and
settlement. Post-auth facts are terminal at insert, so they use `INSERT ... ON CONFLICT DO NOTHING`
without updates. Connection acquisition may retry before any SQL attempt. Once an insert returns an
error, the connection is discarded; the batch may be replayed only when every row has a non-null
`billing_request_id`, whose unique constraint makes the whole batch idempotent. If any row has a
nullable billing identity, commit status is uncertain and the batch is dropped and counted failed
rather than replayed, preserving at-most-once insertion for rows that have no uniqueness key.

Dropped events increment a fixed-cardinality counter by a bounded reason. The system must expose
queue depth, dropped total, and persistence health so data coverage is measurable rather than
silently assumed.

Unauthenticated requests have no customer identity and are outside the customer analytics MVP.
Aggregate auth failures remain operational metrics. A later abuse/security design may define a
separate privacy boundary; it must not be smuggled into this contract.

## 9. Read surfaces

Request-fact reads are private Control API contracts guarded by `control_authed`, not public APIs and
not panel-key metrics. The first producer should support:

1. bounded fleet summary for a half-open `[from,to)` window no wider than 30 days;
2. bounded account summary over the same dimensions;
3. keyset-paginated drilldown, maximum 200 rows per page;
4. lookup by exact `logical_request_id` returning every plane attempt;
5. explicit coverage metadata: persisted, dropped, incomplete, and legacy/unknown counts.

Suggested summary axes are client kind/source, provider plane, requested/executed model, route/surface,
streaming, tool class/choice, terminal classes, fallback/retry counts, and latency distributions.
Responses return no raw API key, key label, email, prompt, content, provider subject/profile, or raw
error. Commerce or the admin backend may resolve account/key display metadata through their existing
authoritative mappings.

The customer-facing usage API is not changed in the MVP. After operator validation, selected
aggregates may be added to the existing account usage response producer-first. Request-level
drilldown is not automatically a customer contract.

## 10. Metrics and logs

Prometheus may publish only compile-bounded dimensions such as plane, route class, surface, stream,
status class, terminal class, queue outcome, and persistence outcome. Candidate metrics include:

- request lifecycle totals;
- end-to-end, response-header, first-byte, and stream-duration histograms where measured without
  buffering;
- request-fact inbox depth and dropped events;
- persistence failures;
- nonterminal facts older than a fixed threshold.

Models, tools, clients, accounts, keys, logical/billing/upstream IDs, execution groups, and provider
profiles are database report dimensions, never Prometheus labels.

Journald/Loki remains an incident surface. Terminal errors and invariant violations should carry the
logical request ID once correlation exists, but routine successful facts should not be duplicated as
high-volume journal lines. Existing logs that print full subscription email should migrate to an
opaque home identity as a related privacy hardening step.

## 11. Retention and indexing

MVP retention is 30 days. Pruning is bounded and independent from reservation/outbox deletion. No
foreign key may cascade facts away with shorter transient lifecycle storage, and facts must not keep
reservations alive past their own retention boundary. Request-fact pruning must respect the existing
validated 30-day minimum enforced by `validate_request_lifecycle_prune_cutoff`
(`crates/registry/src/pricing/snapshots.rs:36-50`), which today guards `maintenance_prune`
deletions of `settlement_outbox`, `reservations`, and `execution_group_winner`. The pruning order
must finalize or prune facts before their corresponding lifecycle rows, never resurrect a pruned
reservation, and never extend a fact past the shared retention window.

The migration should start with only query-proven indexes:

- nullable unique `billing_request_id`;
- `(logical_request_id, attempt)`;
- `(account_id, admitted_at DESC, fact_id)`;
- one compact time index for pruning/fleet windows.

Mutable terminal columns should not be broadly indexed; keeping them out of indexes allows HOT-style
updates and avoids settlement write amplification. There is no unbounded `all` query or offset
pagination.

## 12. Ownership and affected components

- `crates/registry`: additive PostgreSQL migration, SQLite semantic parity where required by the
  rollback/test contract, insert/update/query/prune primitives;
- `crates/forward`: admission snapshot, provider parsers, tool/capability classification, lifecycle
  updates, low-priority inbox, stream-safe terminal observations. `AsyncBilling` is a single-writer +
  N-reader actor with a bounded 4096-entry FIFO and a `Flush` barrier (`crates/forward/src/billing.rs:39`,
  `:1668-1681`, `:1474-1476`);
- `crates/server`: composition, private Control API reads, bounded telemetry, request correlation.
  The existing Control API aggregates `usage_events` via `GET /admin/account/{id}/usage`,
  `GET /spend-stats`, and `GET /fleet-history` (`crates/server/src/admin.rs:947-1056`,
  `crates/server/src/http.rs:341,3947-4039,4383`);
- `crates/router`: trusted logical request ID production and propagation across fallback attempts;
- `deploy/Caddyfile`: removal of client-supplied internal correlation capability headers, following
  the existing `strip_execution_identity` snippet pattern (`deploy/Caddyfile:76-79`);
- `packages/contracts` and `packages/engine-client`: typed consumers only after the engine producer
  is deployed GREEN;
- `apps/api` and `apps/admin`: commerce identity join and operator analytics UI after the producer;
- `observability/` and `docs/ops/MONITORING.md`: only aggregate health metrics, alerts, dashboards,
  and runbooks.

`crates/pool` does not own this feature. Selection remains there, but persistence, HTTP, and client
analytics must not be introduced into the pool layer.

## 13. Staged delivery

1. Agree this contract's vocabulary, privacy boundary, client classifier, public-ID decision, and
   MVP provider/surface scope.
2. Merge an expand-only engine migration first. It introduces dormant storage and pruning support
   without a runtime dependency. Wait for GREEN `deploy/migration` and `deploy/watchdog`.
3. Deliver the dormant registry S2 runtime primitives: opt-in reservation/delivery lifecycle,
   outbox-terminal APPLY, reconciliation synthesis, bounded terminal inserts, and retention pruning.
   Keep all legacy callers fact-free and add no read surface. **Implemented; not yet a production
   producer.**
4. Deliver dormant forward-core S3A transport: fact-aware money commands remain in the existing
   money actor; terminal-at-insert events use a separate bounded fail-open PostgreSQL inbox. Preserve
   all legacy callers and add no wire/read/metric surface. **Implemented; no plane caller yet.**
5. Deliver the provider-plane producer for the optional trusted logical-ID capability and wire the
   dormant forward-core forms. Update the engine contract documentation in the same commit and wait
   for GREEN.
6. Deliver the router consumer/producer after the plane capability is GREEN. Caddy and router both
   strip client copies before trusted injection.
7. Instrument Anthropic, Codex, and Gemini native and universal surfaces. Keep existing body,
   response, stream, retry, settlement, and execution-group behavior unchanged.
8. Deliver private aggregate and drilldown Control API producers. Update `docs/engine/CONTROL_API.md`
   and `docs/DEPENDENCIES.md` in the same commit.
9. After the exact producer SHA is GREEN, deliver `packages/contracts`,
   `packages/engine-client`, `apps/api`, and the operator UI as consumer commits.
10. Add fixed-cardinality health metrics and their alert/runbook/dashboard changes under the new
   metric checklist.
11. Run a coverage and load observation period with a pre-agreed rollback threshold for admission
   latency before exposing selected aggregates to customers or increasing retention.

Every migration and cross-context contract remains expand-only and producer-first. This proposal
does not authorize combining migration, dependent runtime, and consumers into one rollout.

## 14. Verification requirements

Implementation is incomplete without tests for:

- PostgreSQL migration, exact replay, pruning, and bounded queries;
- SQLite/PostgreSQL semantic parity for paths required by the existing authority contract;
- crash between settlement enqueue and actual outbox apply;
- reconciliation and execution-group winner/loser outcomes;
- reservation cancellation before delivery;
- successful non-stream and stream responses;
- first-byte/mid-stream failure, malformed terminal event, and downstream disconnect drain;
- internal provider rotation versus router fallback attempts;
- post-auth validation, balance, quota, and provider errors;
- non-billable inference-related calls included by the final MVP scope;
- tool declaration/result/output-call classification on Anthropic, OpenAI, and Gemini shapes;
- stripping and strict validation of internal logical-ID headers;
- fail-open behavior and measurable drops of the low-priority inbox;
- absence of forbidden content and identity fields in DB rows, logs, metrics, and API responses;
- Control API window bounds, authorization, keyset pagination, and coverage metadata;
- unchanged transparent streaming bytes and existing execution-state retry fences.

## 15. Decisions still open for discussion

1. Should `logical_request_id` be returned to customers in a new additive response header, or remain
   operator-only for the MVP?
2. Which non-billable calls belong in request analytics: `count_tokens` only, or also model discovery
   and stored-response reads?
3. Which client kinds receive explicit first-party classification in the first version, and which
   header name/schema will integrations send?
4. Are normalized tool classes sufficient, or is a keyed toolset fingerprint required after the
   first analytics review?
5. Which latency points are mandatory for MVP: total, response headers, first public byte, stream
   duration, or all four?
6. Is the first operator surface a new Request Analytics page, an extension of Engine Spend, or both
   summary plus dedicated drilldown?
7. Should the first implementation cover only Anthropic/Codex/Gemini, or every currently implemented
   provider plane before any UI consumer ships?

Until these decisions are resolved, this document is the discussion baseline, not implementation
authorization.

## 16. Review findings and code confirmations (2026-08)

The following conclusions were verified against `origin/master` at `916dee0d` in read-only worktrees
and are recorded here as fixed observations.

### 16.1 Confirmed by code

- `usage_events` is the authoritative settled-usage fact: `request_id` (unique), `account_id`, `key`,
  `model`, five token buckets, `web_search_requests`, `real_nano`, `charge_nano`, `provider`
  (`crates/registry/migrations_pg/0001_engine_authority.sql:120-137`,
  `0005_provider_attribution.sql:13-18`). The only runtime write is inside the settlement outbox
  apply transaction (`crates/registry/src/pg.rs:1436-1451`); losing execution-group attempts and
  model-less reconciliation charges do not produce a row (`pg.rs:1419`).
- The HTTP error audit lives in `crates/server/src/http.rs:667-739` as `audit_customer_error`
  middleware; `crates/forward` only carries the `TerminalErrorReason` response extension
  (`crates/forward/src/proxy.rs:564,648-649`).
- Request identities are fragmented: the billing-plane identifier is `engine_request_id`/`request_id`
  generated in the provider planes (`crates/forward/src/proxy.rs:1288`,
  `crates/forward/src/codex/billing.rs:171,276`, `crates/forward/src/gemini/billing.rs:611`) and
  passed through `AsyncBilling`; the public `x-request-id` is the same value
  (`crates/forward/src/proxy.rs:654`); the upstream reference is `BillCtx.reference`
  (`crates/forward/src/meter.rs:39-40`).
- `AsyncBilling` is a single-writer + N-reader actor with a bounded 4096-entry money FIFO and a
  `Flush` barrier (`crates/forward/src/billing.rs:39`, `:1668-1681`, `:1474-1476`).
- Settlement outbox enqueue and apply are distinct steps; winner/loser is decided only at apply
  (`crates/registry/src/pg.rs:1316-1337`).
- The router creates an execution group only when `attempt_count > 1`
  (`crates/router/src/routing.rs:532-539`); single-attempt traffic sends no group header.
- `control_authed` is defined in `crates/forward/src/proxy.rs:348`, not in `crates/server`; existing
  Control API aggregation endpoints are `GET /admin/account/{id}/usage`, `GET /spend-stats`, and
  `GET /fleet-history` (`crates/server/src/admin.rs:947-1056`,
  `crates/server/src/http.rs:341,3947-4039,4383`).
- Prometheus `/metrics` uses fixed compile-bounded series with no per-request or per-customer labels
  (`crates/server/src/http.rs:316-1023`, `:1136-1147`).
- Retention today is 30 days for ledger/usage_events (`LEDGER_RETENTION_DAYS = 30`) and 30 days for
  reservations/settlement_outbox (`REQUEST_LIFECYCLE_RETENTION_DAYS = 30`), enforced by separate
  prune loops (`crates/server/src/main.rs:35-39`, `crates/server/src/poller.rs:97-160`) with a
  validated 30-day minimum (`crates/registry/src/pricing/snapshots.rs:36-50`).
- SQLite/PostgreSQL semantic parity is a registry contract requirement
  (`crates/registry/CLAUDE.md:134-136`), with mirrored primitives for reserve, settle, execution-group
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
  (`crates/forward/src/meter.rs:124` TeeMeter, `crates/forward/src/proxy.rs:2044-2072`,
  `docs/engine/ROUTING_FENCING.md`).

### 16.2 Corrections and clarifications to this document

- The HTTP error audit is owned by `crates/server`, not by `crates/forward` (§2, §12).
- The identifier `billing_request_id` does not exist in code; the actual plane-generated name is
  `engine_request_id`/`request_id` (§2, §4).
- A separate `calibration_request_id` exists only on the Gemini and Kimi exact-calibration lanes
  (`crates/forward/src/gemini/billing.rs:13`, `crates/forward/src/kimi/gateway.rs:100`); on Anthropic
  and Codex, turn calibration keys on the same plane billing `request_id`, and Codex window
  calibration rows carry no request identity (§4).
- `control_authed` is defined in `crates/forward/src/proxy.rs:348` (§9, §12).
- The router creates an execution group only for chains longer than one attempt; if the logical
  request ID is sent to every attempt, that is a new convention requiring extension of the existing
  strip and validation discipline (§4, §13.4).
- Request-fact pruning must respect the existing validated 30-day minimum enforced by
  `validate_request_lifecycle_prune_cutoff` and must not resurrect or extend pruned lifecycle rows
  (§11).

### 16.3 Design risks and recommendations

1. **§8.2 writer contention.** The low-priority observability inbox must not be drained by the
   `AsyncBilling` money writer thread. A separate writer connection or strictly deferred batch-insert
   path is required so that a slow observability insert does not add latency to admission and
   settlement. Post-auth facts are terminal at insert, so they use `INSERT ... ON CONFLICT DO NOTHING`
   without updates.
2. **§8.1 write amplification.** Inserting the request fact into the reservation transaction adds
   bytes to the hottest transaction in the system without a second round trip. A pre-agreed rollback
   threshold for admission latency is required during the coverage and load observation period (§13.9).
3. **§7 `http_status_class`.** Store the exact bounded status code (100-599), not a class; the class
   is a derived projection. Exact codes preserve diagnostic value at no meaningful database
   cardinality cost.
4. **§5 client classification.** Explicit identification should start with the first-party
   `opencode` plugin, followed by `claude_code`. The versioned heuristic v1 should cover the existing
   Codex envelope check and `anthropic-version`/`openai-beta` presence; all other values remain
   `unknown`. The header should be a bounded ASCII `x-apitoken-client` kind/version pair, stripped at
   ingress like other internal capability headers.
5. **§4 public response header.** The logical request ID should remain operator-only for the MVP.
   The existing public `x-request-id` is already the billing/reservation identity and is relied on
   by clients; a second public ID would be confusing. If exposure is later required, it must use a
   new header name after compatibility is proven.
