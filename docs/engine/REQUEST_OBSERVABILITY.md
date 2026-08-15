# Request observability contract

> **Status: PROPOSED, not implemented.** This document records the target design and rollout
> constraints for discussion. It does not describe a live API, schema, metric, retention guarantee,
> or deployed behavior. Implementation requires the staged producer-first changes below.

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
operator dashboards.

It is deliberately not a complete request journal. It normally exists only when authoritative usage
reaches settlement, so it does not cover all validation errors, authorization or balance refusals,
provider failures, non-billable calls, or every interrupted stream. It also does not own request
latency, routing attempts, client classification, or general tool-use dimensions.

The HTTP error audit writes one JSON journal event for a terminal non-2xx response to a recognized
metered key. Prometheus and Grafana intentionally exclude customer, key, request, model, credential,
and content identities. These operational surfaces remain useful but are not a durable product
analytics authority.

Request identities are currently fragmented across protocol surfaces. A provider-plane billing ID,
a public response ID, an upstream request ID, a calibration ID, and a router execution group may be
different values. The new contract must make these relationships explicit rather than overloading
one existing ID.

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
- `http_status_class`: exact bounded status or class, according to the API projection being built.

There is intentionally no generic `billing_outcome=settled` client-success label. A router loser,
downstream disconnect, provider success, and financial cancellation can coexist in combinations
that one scalar cannot represent.

## 8. Write lifecycle

### 8.1 Billable metered request

The request fact is inserted or exact-replay-validated in the same authority transaction that
creates the reservation. This guarantees that every accepted billable lifecycle has a durable fact
without adding a second hot-path round trip.

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
reservations alive past their own retention boundary.

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
  updates, low-priority inbox, stream-safe terminal observations;
- `crates/server`: composition, private Control API reads, bounded telemetry, request correlation;
- `crates/router`: trusted logical request ID production and propagation across fallback attempts;
- `deploy/Caddyfile`: removal of client-supplied internal correlation capability headers;
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
3. Deliver the provider-plane producer for the optional trusted logical-ID capability, reservation,
   delivery, outbox-terminal lifecycle, and low-priority terminal inserts. Update the engine contract
   documentation in the same commit and wait for GREEN.
4. Deliver the router consumer/producer after the plane capability is GREEN. Caddy and router both
   strip client copies before trusted injection.
5. Instrument Anthropic, Codex, and Gemini native and universal surfaces. Keep existing body,
   response, stream, retry, settlement, and execution-group behavior unchanged.
6. Deliver private aggregate and drilldown Control API producers. Update `docs/engine/CONTROL_API.md`
   and `docs/DEPENDENCIES.md` in the same commit.
7. After the exact producer SHA is GREEN, deliver `packages/contracts`,
   `packages/engine-client`, `apps/api`, and the operator UI as consumer commits.
8. Add fixed-cardinality health metrics and their alert/runbook/dashboard changes under the new
   metric checklist.
9. Run a coverage and load observation period before exposing selected aggregates to customers or
   increasing retention.

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
