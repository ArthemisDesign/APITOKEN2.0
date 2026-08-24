# Request observability contract

> **Status: v1 COMPLETE under the owner-approved 21-hour observation exception.** Registry S2, forward-core S3A,
> the Caddy logical-ID perimeter, provider-plane logical/client context admission, router logical-ID
> production, and the client-attribution slice of classifier stage 5 are implemented. The production
> request-fact surfaces are metered Codex/OpenAI/Gemini universal Messages count,
> OpenAI native Responses input-token count, Anthropic native Messages count, native Gemini
> countTokens, OpenAI/Codex generation through native Responses, native Chat Completions, and universal
> Messages, and billable native/universal Gemini text generation; all consume normalized client
> attribution. The closed structural classifier contract is implemented: Anthropic, OpenAI native,
> both Gemini counting routes, native/universal Gemini text generation, and Anthropic-plane OpenAI
> Chat/Responses consume their owning classifiers. A typed once-only lifecycle
> carrier, transparent final-public-body observation seam, and nonwaiting atomic terminal seal are
> active. All counting producers capture safely ordered first-byte evidence or freeze `NULL` before
> any later outer body poll. Stage 7 now covers the native billable Anthropic `POST /v1/messages` leaf
> and Anthropic-plane universal OpenAI Chat `POST /v1/chat/completions` and Responses
> `POST /v1/responses`, including stream/nonstream settlement and the configured ClaudeStore
> Anthropic-wire fallback. The OpenAI/Codex generation slice admits facts only with PostgreSQL billing tickets,
> shares one overflow-checked count across every actual generation POST (including retries and the
> configured ClaudeStore fallback), and seals success, provider failure, cancellation, explicit
> downstream disconnect, and reviewed tool-call output evidence conservatively. Runner join/panic
> terminalization retains an exhaustive observed attempt count but leaves provider, delivery, HTTP,
> and tool evidence unknown; a frame-send timeout alone is not a disconnect. If a completed
> nonstream turn cannot durably mark delivery, its authoritative success/usage/tool evidence still
> settles exactly once while the client receives a conservative 503 and delivery remains unknown.
> Native Gemini text generation follows the same PostgreSQL-owned reserve/delivery/settlement contract,
> sharing one physical-send observer across helper restart, 401 resend, and profile rotation. Its
> background stream drain seals terminal usage/tool/disconnect evidence independently of public body
> polling. Gemini universal Chat, Responses, and Messages now hand a typed privacy-bounded origin to
> the synthesized native leaf, which emits exactly one fact under the original public route semantics.
> Legacy/admin, SQLite, image, batch, counting, and missing-context generation paths remain fact-free.
> The v1 producer matrix, private read surface, metrics/alerts/runbooks, typed consumer chain and
> dedicated operator UI are complete; excluded providers/modalities remain fact-free by contract.
>
> This document is the owner-approved v1 implementation contract and final evidence record. The finite,
> ordered rollout and Definition of Done in §§13-16 are complete.
> Migrations 0053-0054 are deployed; `crates/registry` exposes opt-in PostgreSQL write/lifecycle
> primitives, and `AsyncBilling` transports typed facts through the owning money transactions plus a
> distinct fail-open terminal-at-insert inbox. The Caddy perimeter reserves
> `X-Apitoken-Logical-Request-Id` by deleting internet copies at all four public provider/router
> ingresses while leaving stable loopback origins untouched. Anthropic, OpenAI, Gemini, and Combined
> customer routers consume at most one canonical trusted value before auth/body/reserve/dispatch,
> generate one for direct ingress, remove the wire header, and retain only a typed request extension
> through internal adapters. The provider consumer's exact production SHA is GREEN, and the router
> creates one canonical UUIDv4 after admission and sends it on every executable attempt, reusing it
> across fallback. These counting routes consume typed extensions only after metered route/body
> admission and submit one already-terminal nullable-billing-ID fact through the fail-open PostgreSQL
> inbox, persisting normalized explicit client attribution or unknown when malformed/absent. Anthropic
> additionally publishes its privacy-bounded Messages classifier only after upstream success proves
> native shape acceptance; Gemini classifiers are parser-gated at their owning seams. The logical ID
> remains operator-only. The native Anthropic billable Messages, Anthropic-plane universal OpenAI Chat/Responses, and
> OpenAI/Codex native Responses, native Chat Completions, and universal Messages slices, plus billable
> native and universal Gemini text generation, now admit facts in the PostgreSQL reserve transaction,
> mark delivery with money, and seal terminal evidence through their authoritative settlements. Every
> other billable plane caller remains absent.

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
statement/snapshot, every money path still uses only the raw key, and the three narrow nonbillable
producers expose only the non-secret identity through privacy-minimal typed seeds.
`billing_outcome` is never accepted in the outbox envelope: APPLY derives it from the authoritative
winner, reconciliation, cancellation, and metered-amount state.

Provider-plane request-context admission is implemented for customer routes in Anthropic, OpenAI,
Gemini, and the Combined migration bridge. Malformed reserved logical capabilities fail before auth,
body handling, reserve, or dispatch; direct ingress gets a fresh canonical UUIDv4. The optional
public client header is consumed at that same early boundary and malformed/duplicate/unsupported or
absent evidence fails open to typed unknown without changing HTTP semantics. Both raw headers are
removed, and only typed extensions survive through synthesized universal leaf requests. Health,
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

The current production request-fact producers remain deliberately narrower than the locked v1 matrix.
Codex/OpenAI universal and Anthropic native `POST /v1/messages/count_tokens` participate, including
Combined/router dispatch that reaches those leaf paths. After successful metered admission, each
snapshots `pool::now()`, typed logical/lifecycle context from request extensions, the retained execution
attempt, and authoritative account/key IDs without exposing the raw key. Missing either typed logical
or lifecycle context omits the fact as an instrumentation gap. Every later terminal response converges
through one nonblocking `try_submit_terminal_request_fact`; sealing and submission outcomes never
change response status, headers, frames, body, or polling. Both use `billing_request_id=NULL`,
`billing_outcome=not_applicable`, stream false and normalized typed client kind/source/version (or
unknown/unknown/NULL when malformed, absent, or the typed client extension is missing). Codex records
`openai`/`universal`/`count_tokens`, internal attempt count zero, bounded client model spelling after
Messages validation, and canonical public model only after Responses parsing.

Anthropic records `anthropic`/`native`/`count_tokens` through one request-scoped RAII guard spanning all
subscription rotations. It is created only after metered auth, target selection, bounded body read,
JSON object ownership and typed logical/lifecycle context. The original client JSON is classified
before namespace/identity mutation into a closed content-free candidate; structural fields are emitted
only when an upstream 2xx proves the owning native shape accepted, while any rejected/local outcome
discards the candidate. The guard retains no JSON, prompt, tool name/schema/argument, credential or
header. Requested model is treated as an untrusted bounded printable-ASCII candidate and is emitted
only after upstream 2xx acceptance; rejected/local/transport outcomes keep it NULL. Executable model
remains NULL without exact execution proof. Actual upstream send attempts use checked `usize`→`i32`
when exhaustively known; the final upstream response alone may contribute exactly one printable ASCII
`request-id` up to the registry bound. A returned upstream response is terminalized at headers with
honest delivery `started`, never `completed` merely because a body exists; local pre-send is
`not_started`, post-send local/transport exhaustion is `unknown`, and cancellation/panic safety uses
NULL HTTP status plus unknown class/delivery and attempt count. Submission happens before returning the
response and never depends on body polling, so a never-polled body remains covered. This header-time
handoff proves only the provider HTTP outcome plus delivery `started`, not response-body consumption or
transport completion; `downstream_disconnect` remains NULL. Safely ordered `first_public_byte_at` is
captured by the atomic terminal seal; a prior valid outer observation wins, otherwise NULL is frozen
before later DATA. Admin, unauthorized, unsupported/model routes, missing typed logical/lifecycle
context, native OpenAI Responses token counting, billable Messages, Gemini and
all other surfaces are omitted. SQLite and full/closed inbox drops fail open; coverage remains visible
only through the internal delivery snapshot, not public metrics.

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

The implemented producer-side classifier admits exactly this grammar, removes all header values,
retains only a private-field typed value with redacted `Debug`, and emits only the closed producer
values `opencode`, `claude_code`, or `unknown`; it does not narrow the deployed registry vocabulary.
It runs before auth/body/dispatch, and malformed explicit evidence is terminal for classification,
not an HTTP error and not a reason to try heuristics.

Heuristic classifier version 1 currently has no reviewed positive signatures for `opencode` or
`claude_code`, so it always yields `unknown`. It does not reuse or reclassify the existing Codex-envelope
prefix check. Heuristics persist neither evidence nor raw headers; absence of a match is never
classified as a generic SDK or custom client.

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

The stage-5 definition is now frozen in `crates/forward/src/request_classification.rs`: a private-field,
redacted-`Debug`, non-serializable typed value; a checked `usize`→`i32` declared-count conversion; the
registry's seven closed bits; closed choice/tier/modality types; and pure classifiers for already
validated client shapes: Anthropic Messages, OpenAI Chat/Responses and canonical native Gemini
GenerateContent. Universal Chat/Responses use their OpenAI classifiers before translation; Messages
uses its Anthropic classifier before translation. A producer must therefore preserve explicit client
intent and never classify a degraded or gateway-injected translated body. None inspect transport bytes. Only explicit tool types reviewed by the owning parser get bits. Unknown kinds stay
unclassified rather than becoming `other_reviewed`. Current reviewed uses of that fallback bit are
Gemini `urlContext` and an explicit client-declared Codex `tool_search`; its gateway-produced dynamic
function name is discarded and is not counted again. An accepted Codex `web_search` declaration records
client intent even though admission drops that hosted descriptor. A Codex namespace remains one
top-level declared count and contributes only the reviewed function/custom classes of its validated
children. Native Responses classifies its single validated `input.additional_tools.tools` list when
present (including beside an explicit empty top-level list), and never counts its later synthetic
dynamic functions. Anthropic's ordinary absent/`custom` JSON-schema tool is `custom_function`; only the exact
reviewed native server-tool versions receive their corresponding bits, and no currently unproven MCP
prefix is inferred. When any accepted declaration or namespace child is unreviewed, its count stays
known but the entire class bitset is `NULL`, never a misleading partial set. A named choice becomes only
`named`. Missing, malformed, unsupported or unmeasured evidence stays `NULL`; an explicit validated
empty tool array or modality shape may become zero, and booleans are set only where the owning shape proves the value: a reviewed tool result is existential
`true`, while `false` requires an exhaustively reviewed input; structured output is classified only
from exact reviewed formats. Input modality bits come only from validated strings or content parts; image-only
input and Gemini functionResponse-only input do not implicitly become text. The Anthropic native
count_tokens producer releases its content-free candidate only after upstream success supplies the
native owning-validation proof. OpenAI native input_tokens releases its candidate after the owning
Responses parser succeeds and retains it across a later local preparation error. OpenAI Chat and
Gemini count-token producers consume the reviewed classifiers at their owning parser boundaries;
other Gemini producers remain dormant for Stage 6/7. `tool_calls_in_output` remains later terminal
producer evidence, and lifecycle clocks remain outside this structural slice.

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

The active server seam creates one typed clock for each successfully admitted customer provider
request, preserves it through synthetic leaf adapters, and is the sole observer of the first non-empty
successful DATA frame from the final public `http_body::Body`. It forwards data, trailers, errors,
frame boundaries, cancellation, `is_end_stream`, `size_hint` and backpressure unchanged. Empty DATA,
trailers-only, errors, EOF and drop-before-data leave the clock unset. The terminal primitive
atomically seals an open clock with an internal, never-exposed no-byte sentinel and linearizes with
that observer: an earlier first byte wins, while an earlier terminal seal prevents any later body poll
from creating post-terminal evidence. It never waits, notifies, polls, spawns, performs I/O or delays
terminal work, and repeated seals are idempotent. Existing evidence is returned only when safely
ordered inside inclusive `[admitted_at, terminal_at]`; invalid bounds or out-of-order evidence produce
`NULL` without clamping or fabrication, and invalid bounds still seal an open clock. This seam does
not itself write facts or settle money. The nonbillable Codex/OpenAI/Gemini universal Messages count,
OpenAI native Responses input-token count, Anthropic/Gemini native count fact producers, native
billable Anthropic Messages, Anthropic-plane universal OpenAI Chat/Responses, OpenAI/Codex
generation, and native/universal Gemini text generation consume it. Remaining scoped producers still
keep overall coverage incomplete.

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
Reserve/settle calls still receive the raw key, while native billable Anthropic Messages admits the
authoritative `key_id` only through its privacy-bounded fact. The nonbillable Codex count_tokens
producer consumes a separate typed seed that contains only
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

Migration 0061 exposes two private read-only daily aggregate views for the managed Grafana
PostgreSQL datasource. `request_fact_usage_daily` groups the normalized client/source, requested and
executable model, route, request class, bounded tool-count bucket, tool choice, and nullable capability
flags, then joins authoritative token/search/nanoUSD values by billing request ID. The separate
`request_fact_tool_usage_daily` expands only the seven closed tool-class bits. Both views expose the
opaque engine `account_id` and non-secret `key_id` so the operator can answer which customer/project
produced the load. Neither view exposes email, key label, raw API key, request, execution-group,
provider-profile, content, tool-name, schema, argument, or result fields. Migration 0062 adds four
narrow top-level rollups for the always-visible Production Overview cards; they group one
axis at a time and avoid the broad parallel hash aggregation that can exceed the monitoring PostgreSQL
shared-memory budget. Grafana overview panels 435-438 query those 0062 views. The drilldown dashboard
keeps the 0061 views. The monitoring role receives `SELECT` only on granted reporting views. Queries remain capped to the 30-day retention horizon and bounded
dashboard result sets. This does not weaken the Prometheus label prohibition.

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
5. **Freeze v1 classifiers — complete.**
   The exact client-header grammar, fail-open normalization, empty reviewed-positive heuristic v1,
   typed adapter propagation, Codex count_tokens consumption, privacy-negative tests, closed structural
   contract, and pure already-validated Anthropic/OpenAI/Gemini shape classifiers are implemented.
   Structural classifiers have owning consumers for every scoped count and generation route.
   `tool_calls_in_output` remains terminal evidence only. The typed once-only lifecycle carrier, transparent final-public-body
   observer and nonwaiting atomic terminal seal are active. The seal closes an unobserved clock against
   later DATA without indefinite holding and accepts only safely ordered first-byte evidence. Native
   Anthropic Messages passes its carrier through TeeMeter settlement; the scoped OpenAI and Gemini
   producers consume the same frozen contract without exposing a public response.
6. **Complete nonbillable producers — complete.** Cover exactly Anthropic native Messages
   `POST /v1/messages/count_tokens`; OpenAI universal Messages `POST /v1/messages/count_tokens` plus
   native Responses `POST /v1/responses/input_tokens`; and Gemini universal Messages
   `POST /v1/messages/count_tokens` plus native `POST /v1beta/models/{model}:countTokens`. The
   Codex/OpenAI universal Messages, OpenAI native Responses, Anthropic native Messages, Gemini
   universal Messages, and Gemini native countTokens callers are complete. Discovery, stored-response
   reads, health, balance, catalogs, router/provider preflights, and auth helpers remain excluded.
7. **Complete billable producers — complete.** Native
   Anthropic `POST /v1/messages` plus Anthropic-plane universal OpenAI Chat
   `POST /v1/chat/completions` and Responses `POST /v1/responses` now cover stream/nonstream local
   subscription attempts and the configured ClaudeStore Anthropic-wire fallback. Their immutable
   admission shares the reservation transaction; delivery and TeeMeter terminal evidence share the
   existing money transitions. Each outer OpenAI adapter classifies the original accepted client JSON
   before translation/injection, then creates one typed content-free carrier only after its owning
   translator accepts. It carries route `universal`, exact request class `chat` or `responses`, the
   original bounded requested model, accepted stream boolean, and closed classifier fields—never raw
   content, tool names/schemas/arguments or headers. The inner Messages leaf consumes the carrier and
   suppresses native classification, producing exactly one fact. Missing typed context, unauthorized,
   malformed adapter input, KIMI/GLM and non-Anthropic planes remain fact-free. OpenAI/Codex native
   Responses, native Chat, and universal Messages are covered by the same reservation-owned pattern.
   Native Gemini `generateContent`/`streamGenerateContent` and its universal Chat/Responses/Messages
   adapters are also covered: each accepted outer adapter passes one typed content-free public origin
   into the synthesized native leaf, which admits one `universal` fact and never an extra native fact.
   This completes the locked v1 text-generation leaf matrix. A Combined route creates only the
   underlying leaf fact and never an extra Combined fact.
   Backend-only KIMI and GLM, Tripo3D, Suno, images, embeddings, files, and batches remain outside v1.
   Future private API/UI responses declare `scope_version=1` for this exact matrix.
8. **Pass the producer coverage gate — complete and production GREEN.**
   `deploy/request-observability-coverage.test.sh` pins the exact 15-scope manifest against
   server/router dispatch, owning producer markers and explicit exclusions. Existing real-PostgreSQL
   matrices prove one fact per reservation/leaf, no Combined duplicate, privacy and billing fences.
   No read endpoint, metric completeness ratio, or UI ships in this stage.
9. **Deliver fixed-cardinality operations surfaces — complete and production GREEN.**
   `/metrics` exports closed lifecycle totals, the four safely derived duration histograms, fixed inbox
   capacity/depth/outcomes/persistence health and the PostgreSQL one-hour stuck count. The four exact
   alerts, Grafana panels and runbook anchors from §10 ship under the new metric checklist.
10. **Run the observation gate — accepted after 21 hours by direct owner decision.** Corrective
    production-GREEN SHA `f35c841d1f82abe962ded6381da8b6e53f6109a6` began at
    `2026-08-20 08:32:34 UTC` (`1787219554`). At 21 hours the window contained 43k+ scoped terminal
    lifecycles; all three public planes had continuous healthy persistence, zero stuck/lost facts,
    zero RequestFact alerts, zero double winners and zero balance divergence. Later active engine SHAs
    remained descendants and did not change the request-fact producer/metrics modules. Historical
    Caddy latency/error series were sparse and produced threshold excursions, so they are recorded as
    owner-accepted residual uncertainty rather than a fabricated GREEN comparison. On 2026-08-21 the
    owner explicitly approved 21 hours as sufficient and directed the rollout to finish. The runner
    accepts only this explicit `75600`-second exception via
    `REQUEST_OBSERVABILITY_OWNER_APPROVED_21H=1`; its default remains 24 hours.
11. **Deliver private Control API producers — complete, production GREEN `899bb0a1`.** After the accepted gate,
    the three endpoints expose the §9 coverage semantics, bounded summaries, keyset drilldown and
    logical-attempt lookup. This commit updates `docs/engine/CONTROL_API.md` and
    `docs/DEPENDENCIES.md`; consumers still wait for its exact production GREEN SHA.
12. **Deliver `packages/contracts` — complete, production GREEN `1541c1fe`.** After production-GREEN engine
    producer `899bb0a10e8b977aa775f996cced53491264a39c`, add typed private summary/page/logical schemas
    with bounded rows/axes and honest nullable coverage/runtime semantics; update
    `docs/DEPENDENCIES.md` and wait for this exact SHA to be GREEN.
13. **Deliver `packages/engine-client` — complete, production GREEN `97ef2f2f`.** After production-GREEN
    contracts producer `1541c1fefcaa84c4e87ecd5b2d1a0a67b2b21138`, add the three validated GET
    methods with local window/limit/cursor/UUID bounds; wait for this exact SHA to be GREEN.
14. **Deliver `apps/api` — complete, production GREEN `3fe2baeb`.** After production-GREEN engine-client
    consumer `97ef2f2f9f7bf5feb2c90b9fb9d0522487370b91`, expose three guarded no-store admin routes
    that validate bounds and forward only through EngineClient. Engine rows intentionally omit account/key
    identities, so v1 performs no fabricated commerce identity join; wait for this exact SHA to be GREEN.
15. **Deliver `apps/admin` — complete, production GREEN `76386e90`.** After production-GREEN apps/api producer
    `3fe2baebfe8ccca22a974974154a83cb2af27600`, add the dedicated **Request Analytics** area,
    linked from but not mixed into Engine Spend. It consumes only the private producer chain and
    preserves unknown coverage/runtime evidence. There is no customer API or UI.
16. **Record final proof — complete in this document.** Exact producer/consumer SHAs, GREEN verdicts,
    route/exclusion/privacy gates, the owner-approved 21-hour observation exception and bounded live
    smoke are recorded below. No secret or customer content enters the proof.

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
- explicit client-header valid/missing/malformed/duplicate/unsupported cases, plus proof that the
  empty reviewed-positive heuristic v1 leaves absent or malformed evidence unknown;
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

The MVP is **Done** under the direct owner-approved 21-hour exception recorded in §16. The following
finite conditions are satisfied, with the latency/error baseline uncertainty explicitly accepted rather
than silently converted into a pass:

- §§13.1-13.10 are complete on production-GREEN exact SHAs, including the mechanical producer
  coverage gate, fixed operations surfaces, 24-hour observation gate, and every test in §14;
- the exact scoped route manifest has no missing leaf and no duplicate/Combined fact, while every v1
  exclusion produces no request fact;
- private reads return `scope_version=1`, enforce `[from,to) <= 30d`, keyset ordering and `limit<=200`,
  and never manufacture a coverage denominator from persisted rows or a process-runtime snapshot;
- the metrics, alerts, dashboard, and runbooks implement the four thresholds in §10 with no forbidden
  label cardinality;
- during the owner-approved continuous 21-hour observation window (the default remains 24 hours),
  admission and first-public-byte latency p99 for
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

## 16. Final v1 evidence bundle (2026-08-21)

The completed producer-first chain is:

| Stage | Exact production-GREEN SHA | Evidence |
|---|---|---|
| complete Gemini universal producer matrix | `92b53c1a4af4916717d2d2af16278bacbe77e728` | native/universal PostgreSQL ownership matrix, one fact per reservation, no synthesized double fact |
| mechanical coverage + operations | `942e5309c5cc89453942fc81ecd4f3fca028bc02` | 15-scope manifest gate, fixed metrics, four alerts, dashboard and runbooks |
| fresh-inbox health correction | `f35c841d1f82abe962ded6381da8b6e53f6109a6` | fresh PostgreSQL inbox healthy-without-failure; SQLite series absent |
| private Control API | `899bb0a10e8b977aa775f996cced53491264a39c` | three control-key routes, Repeatable Read summary/page/logical reads, 30-day/200 bounds, privacy-safe DTOs |
| `packages/contracts` | `1541c1fefcaa84c4e87ecd5b2d1a0a67b2b21138` | `scope_version=1` Zod envelopes with bounded axes/rows and nullable honest evidence |
| `packages/engine-client` | `97ef2f2f9f7bf5feb2c90b9fb9d0522487370b91` | sole typed transport, local bounds, 15 package tests |
| `apps/api` | `3fe2baebfe8ccca22a974974154a83cb2af27600` | AdminGuard/no-store producer, 162 API tests passed in its gate |
| `apps/admin` | `76386e90103bbbc05a94d0b44dddb3af2b7ceb2b` | dedicated Request Analytics route, build/typecheck and 427 tests, host admin watchdog GREEN |

The owner explicitly accepted a 21-hour continuous observation window instead of the locked 24 hours.
The accepted interval began at `1787219554`. It contained more than 43,000 scoped lifecycle events,
continuous healthy persistence on Anthropic/OpenAI/Gemini, zero stuck facts, zero measured drops or
persistence failures, zero RequestFact alerts, zero execution-group double winners and zero balance
divergence. Historical Caddy latency/error comparisons were sparse and had threshold excursions; the
owner accepted that residual uncertainty rather than treating absent/weak baseline evidence as GREEN.
The default runner remains 24 hours and the exception requires its explicit one-time flag.

The mechanical gate `deploy/request-observability-coverage.test.sh` pins all 15 v1 leaves, server/router
dispatch, producer ownership markers and excluded image/batch/files/embeddings/Combined surfaces.
PostgreSQL suites cover reserve/delivery/settlement/reconciliation, exact replay, pruning, keyset pages,
logical attempts and privacy-negative persistence. The monitoring gate pins fixed-cardinality export,
alert thresholds, dashboard consumers and every runbook anchor. Full Rust and path-selected TypeScript
gates passed on each producer SHA before merge.

A bounded production smoke after the private producer chain returned `200` for summary and a two-row
page over one hour: `scope_version=1`, 2,423 persisted facts, `coverage.status=unknown`, process runtime
health and a non-null next cursor. The commerce admin producer was ready and rejected the same route
without managed credentials with `401`. Credentials remained on the host and no response content or
identity entered this evidence.

The customer API/UI remains unchanged. Operator analytics is isolated from Engine Spend and exposes no
prompt/content, raw API key, key label, email, provider subject/profile, account/key/billing/execution/
upstream identity or raw failure prose. Coverage denominator and historical inbox loss stay `null` until
an independent durable admitted-request authority exists.

## 17. Historical review findings and code confirmations (2026-08)

The following conclusions were verified against `origin/master` at `916dee0d` in read-only worktrees.
They are retained as source observations; where an earlier recommendation differed, the locked v1
record in §15 is authoritative.

### 17.1 Confirmed by code

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

### 17.2 Corrections and clarifications to this document

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

### 17.3 Risks carried into the locked contract

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
