# Codex fleet failure isolation plan

**Status:** accepted implementation plan; not implemented by this document.

**Scope:** the native Codex provider in `crates/forward/src/codex/*`, its fixed-cardinality
observability in `crates/server`, and Codex-specific regression/live gates. This plan does not change
the public API, billing authority, router fallback contract, credential format, or provider roster.

**Primary safety invariant:**

> A failure observed while executing one customer request is evidence about that request first. It
> cannot, by itself or through retries of the same request, remove multiple homes from shared fleet
> rotation.

The implementation must isolate failure at the narrowest proven boundary:

- request rejection affects only the request;
- model availability affects only the `home × model` pair, or the model-level circuit when proved
  fleet-wide;
- transport failure affects only the transport generation that failed;
- account quota or authentication affects only the provider-confirmed home;
- provider outage affects the plane, without fabricating dead accounts or exhausted quota.

Unknown evidence fails closed for the current request and conservatively for shared health: the turn
may fail, but the fleet does not acquire a durable or hard exclusion verdict from an unclassified
customer-path event.

## 1. Problem statement

The current Codex path collapses distinct failure domains into shared health:

1. Upstream terminal events such as `response.failed`, `response.incomplete`, or `error` can arrive
   without an error code recognized by `codex_error_info`.
2. The accepted terminal event can then become `ProcessError::Protocol`, even when its framing and
   transport were valid and the result may be deterministic for one model/request shape.
3. `CodexHome::note_turn_error` sends `Protocol` through the generic transport-error branch, which
   maps it to `HealthSignal::TransportClosed` and immediately makes the home `wedged`.
4. Before public model output, the runner may rotate the same deterministic failure to a second
   home. Independent retries by Codex or another SDK can repeat the same poisoning across the
   remainder of the fleet.
5. `preflight_capacity` maps every `HomeSelection::Unavailable` result to
   `UsageLimitExceeded`, regardless of whether the candidates were rejected for provider quota,
   transport, authentication, model eligibility, shutdown, or a mixture. The public result is a
   synthetic 429 even when no provider reported a limit.
6. The streaming capacity preflight acquires and immediately releases a slot, then the turn selects
   again. A state change in between can still open HTTP 200/SSE before discovering that execution
   cannot begin.

This permits one request-dependent provider failure to remove the entire shared fleet and makes the
secondary outage look like customer or subscription quota exhaustion. Nested retries amplify the
blast radius: the engine has its own home/transport retry policy, while Codex CLI and SDKs may retry
an interrupted Responses stream.

The document records the target design and gates. It is not evidence that the current behavior has
already changed.

## 2. Non-negotiable invariants

The implementation is complete only when all of these are mechanically enforced:

1. **One request cannot poison the fleet.** All internal attempts belonging to one logical execution
   contribute at most one provisional customer-path health vote and cannot hard-exclude more than one
   home.
2. **No quota without quota evidence.** A public 429 is permitted only when every otherwise eligible
   execution path is blocked by explicit, fresh provider quota evidence. A transport-only or mixed
   outage is never called quota.
3. **Terminal model events are not transport failures.** A syntactically valid terminal SSE event
   with unknown or absent provider code does not mutate transport or account health.
4. **Hard health needs independent corroboration.** Customer traffic can make a transport suspect
   and request an immediate probe; hard `wedged`, durable account quarantine, and fleet/model circuit
   state require authoritative evidence defined below.
5. **Last capacity is protected, not blindly used.** Provisional evidence may not remove the final
   eligible home. The failing request is stopped and a clean probe is requested; the system does not
   resend that request merely to keep the fleet numerically non-empty.
6. **Model failures are model-scoped.** A home that cannot serve Sol may remain eligible for Luna,
   Terra, or another catalog-confirmed model.
7. **No replay after delivery.** Existing first-public-byte and delivery fencing remains authoritative:
   no transport, home, model, or external fallback retry begins after public model output.
8. **Money remains exact.** Every failed or refused execution cancels/refunds its reservation under
   the existing authority. Health and retry changes do not introduce a second billing decision.
9. **Privacy remains bounded.** No prompt, message content, tool name/schema/arguments/result, raw
   provider error text, API key, OAuth token, email, account ID, proxy, or arbitrary model/request
   string enters logs or metric labels.
10. **Fail-open health is not fail-open execution.** Unknown shared-health evidence leaves existing
    fleet state unchanged, but the current request still returns a typed failure rather than being
    guessed successful or replayed across the fleet.

## 3. Target failure taxonomy

Replace the overloaded meaning of `ProcessError::Protocol` with typed evidence. Exact Rust names may
vary, but the semantic classes and policies must not.

| Failure class | Required evidence | Request result | Retry policy | Shared health effect |
|---|---|---|---|---|
| `RequestRejected` | Valid terminal provider event whose code means client/request rejection, or whose code is unknown/absent | Honest client/provider-shaped terminal error | None across homes | None |
| `ModelUnavailable` | Explicit model-specific provider verdict or live-catalog ineligibility | 503 `model_unavailable` | At most another catalog-eligible home, under the single execution budget | `home × model` only |
| `ContextExceeded` | Reviewed explicit context verdict | 400 | None | None |
| `AccountQuota` | Fresh structured provider quota/limit verdict | 429 only if no non-quota route remains | Rotate once per distinct eligible home | Cool only that home until bounded reset |
| `AccountAuth` | Reviewed 401/403/auth code | 401/503 according to public contract | One token refresh on the same home; rotation only after confirmed home fault | Suspect first; quarantine only after corroboration |
| `TransportConnect` | Connection could not be established and no execution began | 503 | At most one safe transport/home retry | Suspect that transport generation |
| `TransportTimeout` | Reviewed phase deadline before terminal provider evidence | 503 or in-stream failure | At most one safe retry before output | Provisional vote; hard state only after corroboration |
| `TransportClosed` | EOF/reset before a terminal event | 503 or in-stream failure | At most one safe retry before output | Provisional vote; clean probe/recycle policy |
| `WireMalformed` | Invalid SSE framing/UTF-8/JSON/sequence or impossible reviewed protocol state | 503 or in-stream protocol failure | No cross-home replay by default | Protocol incident; no immediate hard wedge |
| `ProviderOutage` | Reviewed upstream 5xx or fleet-level corroboration | 503 | Bounded plane/model retry policy | Plane/model circuit, not account death |
| `ConfigurationFault` | Local invalid immutable configuration | 503/readiness failure | None | Deployment fault; never represented as account quota |
| `Shutdown` | Local drain/abort state | 503 | None | No health mutation |

### 3.1 Default for unknown terminal provider events

A valid terminal `response.failed`, `response.incomplete`, `error`, or failed `turn/completed` whose
provider code is absent or unrecognized defaults to `RequestRejected`, not `TransportClosed` and not
`AccountQuota`. The error remains sanitized publicly. A new provider code can be promoted into a
narrower class only after wire evidence and a regression fixture establish its semantics.

### 3.2 Wire corruption is not model rejection

Malformed framing and valid framing carrying an unknown terminal result must have separate variants.
Only the former is protocol-integrity evidence. Even then, one customer turn cannot immediately
hard-wedge a home; the implementation first rebuilds or probes the transport and seeks independent
corroboration.

## 4. Health evidence and corroboration

### 4.1 Evidence sources

Health transitions record their source in a closed internal enum:

- `customer_turn` — weakest, request-dependent;
- `clean_probe` — independent request shape owned by the control plane;
- `provider_snapshot` — structured quota/catalog evidence;
- `local_transport` — objective connection/client construction state;
- `operator` — explicit administrative action, if supported.

The source is operational metadata only. It carries no customer identity.

### 4.2 Provisional customer-path vote

A retry chain has one request-scoped evidence identity. Existing typed logical request/execution
identity should be reused where present; direct attempts need an internal request-scoped identifier.
No prompt digest is required or permitted.

For one execution identity:

- repeated failures on the same home count once;
- a retry on another home does not create independent corroboration;
- at most one home can move from `responsive` to a provisional `suspect/degraded` state;
- no home moves to durable `dead` or hard `wedged` solely from that chain;
- terminal request/model rejection contributes no health vote at all.

Separate HTTP retries from the same external client may not share a trustworthy public identity.
Safety therefore cannot depend only on correlating them: hard transitions still require a clean probe
or multiple independent requests separated by a minimum time window.

### 4.3 Hard transport transition

A customer-path EOF/timeout/connect failure may request an immediate clean probe and may rebuild the
local transport handle when that is non-destructive. Hard `wedged` requires at least one of:

1. a failed clean probe after the customer-path signal;
2. the configured number of independent execution identities failing over the configured minimum
   interval with no successful turn/probe between them;
3. an objective local failure to construct or operate the transport generation.

A successful turn or probe clears provisional transport evidence. Configuration constants remain
compile-fixed unless an operator-controlled tuning need is separately justified.

### 4.4 Account transitions

- Provider quota requires structured `allowed:false`, `limit_reached:true`, or an audited equivalent
  code. Percent utilization alone is not a hard stop.
- Authentication becomes suspect after the first clean rejection. One forced refresh is allowed on
  the same home. Durable/hard quarantine requires a repeated clean auth verdict or an explicit
  permanent subscription verdict.
- Unknown 401/403-shaped environment errors do not become durable account death without
  corroboration.

### 4.5 Last-home fleet fuse

Before applying a provisional transition, compute the remaining eligibility for the requested model.
If the transition would remove the final eligible home and lacks authoritative provider/probe evidence:

1. stop the current request;
2. keep the last home out of further attempts for that request;
3. leave shared hard health unchanged or mark only a non-excluding suspect state;
4. request an immediate clean probe;
5. return 503 with a bounded retry hint when no execution path remains.

The fuse prevents fleet poisoning; it does not force traffic through a home that just failed and does
not convert unknown evidence into success.

## 5. Selection and capacity-result semantics

### 5.1 Preserve rejection reasons

`HomeSelection::Unavailable` must carry a fixed-cardinality summary instead of only `ready_at`.
At minimum it distinguishes:

- all eligible homes explicitly provider-limited;
- all eligible homes unavailable on transport;
- no home eligible for the requested model;
- all eligible homes unavailable on authentication/account state;
- shutdown/configuration unavailable;
- mixed reasons.

The summary includes only counts/booleans from closed reasons and the earliest authoritative reset;
it contains no home, account, customer, or request identity.

### 5.2 Public mapping

| Aggregate selection result | Public status |
|---|---|
| Every otherwise eligible home has fresh explicit provider quota evidence | 429 with reset-derived `Retry-After` |
| Requested model has no currently eligible home | 503 `model_unavailable` |
| Transport-only, provider outage, shutdown, or mixed unavailability | 503 `service_unavailable` |
| Confirmed request/context rejection | Appropriate 400-class error |
| Authentication of the customer API key fails | Existing public 401 |

A 429 with zero explicit provider-limit homes is an invariant violation and increments a dedicated
tripwire counter. The implementation must not infer quota from `ready_at`: cooldown deadlines also
exist for transport/auth states.

### 5.3 Eliminate the streaming preflight race

The streaming path must not acquire a home/slot only to release it before the turn selects again.
Preferred design:

1. perform selection once;
2. return an owned `SelectedTurn`/capacity lease containing the home and RAII slot;
3. carry that lease through delivery marking into `run_turn`;
4. open public SSE only after the lease exists and the execution is ready to submit;
5. release it exactly once on completion/error/drop.

If model/execution preparation can still fail before submission, it fails before HTTP 200. A state
change after the provider submission remains an in-stream failure, with no replay after output.

## 6. Model-scoped eligibility and circuits

### 6.1 `home × model` eligibility

Selection must intersect every text request, Standard and Priority, with the last-good authenticated
model catalog of each home. Priority/Fast capability remains an additional requirement/rank, not the
only path where model support matters.

- A home lacking the executable model is skipped for that model only.
- It remains eligible for other catalog-confirmed models.
- Stale catalog evidence follows a separately documented fail-open/last-good policy; it cannot be
  silently treated as fresh proof of absence.
- Public aliases are normalized to the executable upstream model before eligibility is checked.

### 6.2 Model-specific rejection

An explicit model-unavailable verdict temporarily blocks only `home × executable_model`. It must not
set account dead or transport wedged. Recovery comes from a refreshed catalog, a successful
model-specific half-open probe/turn, or expiry of a short compile-fixed cooldown.

### 6.3 Model/plane circuit breaker

Correlated failures for the same executable model across independently healthy homes open a bounded
model circuit. Correlated provider failures across models may open a plane circuit. Circuits:

- return 503, never synthetic quota;
- have compile-fixed cooldown and single half-open execution;
- do not mutate account health;
- expose fixed-cardinality state and transition metrics;
- preserve unrelated models whenever the evidence is model-scoped.

This circuit is a failure-domain boundary, not an execution queue, concurrency limit, or hidden
fallback mechanism.

## 7. Retry containment

Retries are granted by proof, not by generic retryability:

| Failure | Engine retry budget before public output |
|---|---|
| Request rejection, unknown valid terminal event, context/bad request | 0 |
| Model unavailable | At most one other catalog-eligible home; no revisit |
| Connect/timeout/EOF before provider output | At most 1 total transport/home retry |
| Explicit quota | Each distinct eligible non-limited home at most once, bounded by roster width |
| First auth rejection | One forced refresh on the same home |
| Confirmed auth failure after refresh | Other distinct eligible home at most once each |
| Malformed wire protocol | 0 cross-home by default; rebuild/probe outside the request |
| Any error after first public model byte | 0 |

Additional rules:

- a home is never revisited within one execution chain;
- internal retry counts continue to feed request facts with checked conversion;
- retries from the same chain share one provisional health vote;
- router fallback remains governed solely by `docs/engine/ROUTING_FENCING.md` and exact
  `not_started` proof; this plan does not broaden it;
- external client retries are untrusted and unbounded, so fleet safety must remain true even when a
  client retries indefinitely;
- changing public text or asking clients to lower retry counts is mitigation, not the safety fix.

## 8. Public errors and correlation

Generate the public request ID before provider admission and return the same safe ID on every
pre-stream 4xx/5xx as well as success. It must be the ID used by `customer_http_error` and bounded
request-fact correlation, while remaining distinct from raw API keys, upstream request IDs, billing
identity, logical execution identity, and router execution groups.

Responses streams retain their existing public response identity. In-stream failures keep HTTP 200
and terminate with the protocol's error/failed lifecycle; they cannot retroactively add
`Retry-After`. Pre-stream 503 may carry a short bounded retry hint; 429 carries only an authoritative
provider reset-derived hint.

Public errors remain sanitized and must not reveal roster size, profile/home identifiers, proxy,
account state, or provider error bodies.

## 9. Observability and automatic tripwires

### 9.1 Fixed-cardinality metrics

Add or extend metrics with only closed labels:

- `codex_health_transition_total{axis,from,to,source,reason}`;
- `codex_request_failure_total{class,delivery}`;
- `codex_public_refusal_total{status,reason}`;
- `codex_transport_recycle_total{reason}`;
- `codex_model_circuit_state{state}` aggregated across models; exact model identity remains only in
  authenticated bounded control output and never becomes a Prometheus label;
- `codex_synthetic_quota_violation_total` for any attempted public 429 without explicit fleet quota
  evidence;
- existing total/available/authenticated/wedged/limited gauges remain.

Customer, key, request, home, arbitrary model, provider message, and prompt data never become labels.
Per-home operational detail stays on the existing authenticated bounded control surface where its
privacy contract permits it.

### 9.2 Alerts

Add runbook-backed alerts for:

1. available homes falling from nonzero to zero within 30–60 seconds while upstream quota/auth
   evidence remains zero;
2. `codex_synthetic_quota_violation_total > 0`;
3. multiple hard health transitions sourced only from customer turns before a clean probe;
4. model or plane circuit remaining open beyond its bounded recovery horizon;
5. repeated transport recycle without a successful probe.

A runtime fuse may temporarily reject further customer-path hard transitions while a fleet-poisoning
signature is active. It must still allow clean probes, successful recovery, explicit quota/auth
verdicts, and shutdown; it is not a mechanism to keep sending customer traffic into a known failure.

### 9.3 Request facts

Use closed `provider_terminal_class`, delivery state, and a bounded `failure_class` to distinguish at
least:

- request rejected;
- model unavailable;
- provider quota;
- auth;
- transport connect/timeout/closed;
- wire malformed;
- provider outage;
- capacity preflight by aggregate cause.

The fact stores no raw provider message. `internal_attempt_count=0` remains the proof that a
preflight refusal never reached upstream.

## 10. Mandatory regression and integration gates

The following tests block implementation completion:

1. **Unknown terminal event across five homes.** Replaying a valid `response.failed` with unknown or
   absent code never wedges any home and never fans the request across all profiles.
2. **Multi-tenant isolation.** Client A repeatedly sends a deterministic failing request while a
   simple request from client B continues to obtain healthy capacity.
3. **One execution, one health vote.** Multiple internal attempts and duplicate errors from one
   execution cannot create independent hard evidence.
4. **Last-home fuse.** Provisional evidence cannot remove the final eligible home; the failed request
   is not resent to it, and a probe is requested.
5. **Transport-unavailable response.** All homes wedged/degraded returns 503, not 429.
6. **True quota response.** All eligible homes explicitly provider-limited returns 429 with the
   earliest authoritative reset.
7. **Mixed reasons.** Any non-quota terminal cause among an otherwise empty selection returns 503.
8. **Model eligibility.** Sol present on one of five homes routes only there; the other four remain
   available for their catalog-confirmed models.
9. **Model failure isolation.** A Sol-specific rejection neither affects Luna nor changes account or
   transport health.
10. **Malformed wire vs valid terminal failure.** Malformed SSE takes the protocol-integrity path;
    valid unknown terminal SSE takes request rejection. Their health effects differ.
11. **Preflight lease.** No streaming response opens before the selected lease is owned; the same
    slot reaches the actual submission.
12. **No retry after output.** Every error class remains terminal once a model delta has crossed the
    public boundary.
13. **Billing fencing.** Every failed/refused path cancels or refunds exactly once and cannot create a
    winner/charge without provider execution evidence.
14. **Public correlation.** Every pre-stream Codex error has the same `x-request-id` in the response
    and sanitized audit event.
15. **Property-based health state machine.** Arbitrary events from one execution cannot hard-exclude
    multiple homes, create quota without quota evidence, or mutate account health from an unknown
    provider event.
16. **Pinned Codex CLI compatibility.** Test the minimum supported and current pinned clients against
    SSE terminal failure, generic 503, and true 429, asserting actual HTTP submission counts rather
    than relying on the CLI error wording.
17. **Mock rotation smoke.** Existing universal Chat/Responses behavior and router execution fencing
    remain green.

Live verification, if needed, uses a throwaway provider account and the safe provider live-runner
rules. It must not discover semantics by exposing a customer request or by repeatedly poisoning the
production roster.

## 11. Delivery sequence

### Stage 1 — immediate containment

Implement as one production-ready behavior change with tests and same-commit contract updates:

1. classify valid unknown terminal provider events as request rejection;
2. stop cross-home retry and health mutation for that class;
3. preserve selection rejection causes;
4. return 503 for transport/mixed/model absence and 429 only for explicit quota;
5. add the synthetic-quota tripwire;
6. cover the five-home poisoning and multi-tenant regression.

**Acceptance:** one deterministic bad request cannot reduce `homes_available`; public 429 requires
positive explicit quota evidence; existing successful Codex traffic and exact billing remain green.

### Stage 2 — transport corroboration and atomic streaming admission

1. introduce evidence source and request-scoped provisional vote;
2. require clean-probe/independent corroboration for hard wedge;
3. add the last-home fuse;
4. carry the selected RAII slot into the real streaming submission;
5. return public request IDs on all pre-stream errors.

**Acceptance:** a customer-path error can request recovery but cannot independently hard-remove the
fleet, and streaming preflight has no select/release/reselect race.

### Stage 3 — model isolation and circuits

1. enforce per-home model eligibility for Standard and Priority;
2. add `home × model` temporary state;
3. add bounded model/plane circuits and half-open recovery;
4. add fixed-cardinality metrics, request-fact failure classes, alerts, and runbook sections.

**Acceptance:** a model rollout/rejection cannot reduce capacity for unrelated models, and operators
can distinguish request, model, transport, account, and provider incidents without reading content.

### Stage 4 — production canary and closeout

On the exact implementation SHA:

1. run all targeted and workspace gates;
2. deploy through the watchdog only;
3. verify all homes remain authenticated/available under the deterministic unknown-terminal fixture
   or equivalent isolated candidate test;
4. verify synthetic quota tripwire remains zero;
5. verify true quota and transport-unavailable status mapping;
6. verify no charge on every failed fixture;
7. observe a bounded canary horizon with no fleet-wide customer-path transitions.

The document may be marked implemented only after those exact-SHA conditions are recorded in the
normal deployment evidence. A passing unit test or one successful request is not sufficient.

## 12. Implementation map

Expected ownership, without pre-approving an oversized diff:

- `crates/forward/src/codex/transport.rs` — preserve terminal/wire evidence without collapsing valid
  unknown failures into protocol corruption;
- `crates/forward/src/codex/runner.rs` — typed retry policy, one-execution budget, no request rejection
  rotation, selected-turn lease handoff;
- `crates/forward/src/codex/health.rs` — evidence sources, provisional/corroborated transitions, fuse
  invariants;
- `crates/forward/src/codex/mod.rs` — model eligibility, aggregate selection cause, probes/recycle,
  model/plane circuit composition;
- `crates/forward/src/codex/api.rs`, `chat.rs`, `skin.rs` — honest public status/reason mapping and
  public request ID propagation;
- `crates/forward/src/codex/billing.rs` — terminal fact classification while preserving exact
  reserve/cancel/settlement semantics;
- `crates/server/src/http.rs` and monitoring config/runbook — fixed-cardinality metrics, audit
  correlation, alerts;
- `docs/engine/CODEX_PROVIDER.md` and this document — same-commit behavioral contract updates when
  implementation lands.

Changes remain inside the existing `registry ← pool ← forward ← server` dependency direction.
Selection and forwarding policy stay in `forward`; environment remains in `server`; registry remains
the persistence authority; router fallback semantics do not move into the plane.

## 13. Explicit non-solutions

The following may reduce symptoms but do not satisfy the plan:

- increasing roster size;
- lowering Codex CLI or SDK retry counts;
- adding a generic retry around `Protocol`;
- shortening `WEDGED_COOL_SECS`;
- restarting the OpenAI slot when all homes disappear;
- returning a friendlier 429 message without fixing evidence classification;
- preserving one home by continuing to send the known failing request to it;
- recording raw upstream error messages, prompts, or tool payloads for diagnosis;
- disabling health transitions entirely;
- treating all unknown errors as success;
- relying on client-provided identity to deduplicate poisoning;
- broadening router fallback without exact `not_started` proof.

The durable fix is failure-domain isolation plus corroborated shared-health evidence. Client behavior
and fleet size must not be part of the safety proof.

## 14. Definition of Done

This plan is complete when:

- the target taxonomy is represented by typed code rather than message matching;
- valid unknown terminal events do not mutate shared health or rotate across homes;
- one execution cannot hard-exclude multiple homes;
- hard transport/account state requires authoritative independent evidence;
- last-home protection and atomic selected-turn leasing are tested;
- Standard and Priority selection both honor per-home model eligibility;
- only explicit provider quota can produce public 429;
- every pre-stream error carries a public correlation ID;
- fixed-cardinality metrics and runbook-backed alerts distinguish the failure domains;
- all listed regression/integration gates are green;
- exact billing cancellation/settlement remains green;
- production canary evidence on the exact implementation SHA confirms no false quota and no
  fleet-wide customer-path poisoning;
- `docs/engine/CODEX_PROVIDER.md` describes the implemented behavior without retaining obsolete
  failure semantics.
