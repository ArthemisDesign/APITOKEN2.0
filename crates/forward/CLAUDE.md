# crates/forward — CLAUDE.md

**Role:** transparent forwarding of Claude `/v1/*` to api.anthropic.com (Step B) + limits poller;
separately — an optional strict OpenAI-compatible text adapter on top of the encrypted ChatGPT OAuth roster
(native HTTPS to the Codex backend) and a native Gemini surface on top of the encrypted Code Assist OAuth pool.
Never mix the three provider paths.

**Owner branch:** `comp/forward`.

**Boundaries (hard):**
- Depends on `pool`, `registry`, `metering`, `axum`, `wreq`, `redis`, `serde_json`, `futures-util`,
  `bytes`, `tokio`[sync,rt] + `anyhow` (for the billing DB actor). `hound` is used only for
  strict local PCM WAV verification in the bounded Gemini 3 Flash Preview audio-accounting fallback;
  there is no network media fetch and no general codec stack in the crate.
- Does NOT read env and does NOT contain CLI/management routes (`/health`, `/pool`, `/balance`) — that is `server`.
- Receives its config ready-made: [`ProxyConfig`] is populated by `server::config`; billing is the async DB actor `Option<Arc<AsyncBilling>>` in `AppState` (1 writer + N readers).
- `api-limits` is the dependency-free checked payload contract. Current provider caps remain distinct:
  Anthropic text 32 MiB, Codex text 8 MiB, Gemini text 32 MiB/media 20 MiB, translated response
  32 MiB and Gemini native response 64 MiB. Hard 256 MiB ceilings are not runtime enablement.
  Native Anthropic Messages plus the Anthropic Chat/Responses universal adapters use `bounded-body`:
  independent 2 GiB raw-storage and estimated-memory admission, the 32 MiB threshold keeps current
  requests memory-backed, and ownership spans parse/translation/reserve/rotation. Native auth remains
  before body read; universal adapters keep their existing outer auth semantics through `forward`.
  Codex native Responses uses the same shared bounded primitive under its narrower 8 MiB cap;
  input_tokens/Chat/Messages skins retain their prior readers until separate route-specific commits.
  Native Gemini generate/stream/count paths use the shared bounded reader under their existing
  32 MiB text and 20 MiB image/media caps; universal Gemini adapters remain separate.

**Gemini typed executor boundary.** Universal Chat, Responses and Messages adapters translate the
validated public request once into `NativeGeminiRequest` and pass the native `serde_json::Value`
through trusted request extensions. The shared native executor performs auth/reserve/affinity/
rotation/wrapper/settlement without serializing and reparsing a synthetic HTTP body. Typed logical
ID, lifecycle, attribution and fixed public error/SSE adapters remain on the surrounding HTTP shell.

**Gemini helper IPC.** Rust and the embedded SHA-pinned Node helper use one binary framed protocol:
13-byte big-endian headers (`kind`, request id, payload length), control JSON capped at 1 MiB,
request bodies capped at the dormant 256 MiB envelope, and response data chunks capped at 1 MiB.
Body/data bytes are raw, never base64; one writer mutex keeps request metadata+body atomic while
response IDs preserve multiplexing, cancellation tombstones, and backpressure. There is no v1
compatibility branch because helper source ships inside the same immutable engine binary.

**Three authorization classes (secret separation, `proxy.rs`):** `authed` (forwarding-admin: `api_keys`
/loopback) ⊂ `control_authed` (+`control_keys` — for commerce `/admin/*`) ⊂ `readonly_authed`
(+`panel_keys` — read-only dashboards). All comparisons are constant-time (`ct_eq`, fold without short-circuit). A control
key does NOT forward `/v1` (neither admin nor metered → 401). `AsyncBilling` is extended with control commands
(`create_account`/`issue_key`/`account_status`/`key_status_by_id`) through the SAME single-writer (no races).
Pricing sync uses the same actors: multiplier writes go through the writer and cursor ledger reads
through a reader; HTTP code never opens the authority directly.
Account discount writes follow the same ownership: the per-provider override write goes through the
single writer, the control-plane listing through a bounded reader. Hot
tariff overrides
follow the same ownership: `list_tariff_overrides` uses a bounded reader and
`insert_tariff_override` the single writer; both delegate validation/sequencing to the
PostgreSQL-only registry authority and fail closed on SQLite.
Credentials in `x-api-key`, `x-goog-api-key` and `Authorization: Bearer` have OR semantics with no
header priority: any single valid one is sufficient. This is critical for Claude Code,
which may send a stale `ANTHROPIC_API_KEY` and a current `ANTHROPIC_AUTH_TOKEN` at the same time.

**Pricing (`pricing.rs`, `pricing/tariff_book.rs`):** an account is priced by one number — its
discount — overridden per provider where a customer negotiated different terms. Authorization
carries both (`KeyAuth::provider_mult_bp`) and every plane resolves the same way through
`Authz::mult_for(provider_id)`; a provider without an override keeps the account default. Model and
control surface — `docs/commerce/PRICING_MODEL.md`.

There is no policy resolver, no catalog/switch lineage, no release generation and no shadow
evaluation. They were deleted on 2026-08-09: strict admission priced from a per-account policy and
funded from `funding_buckets`, while normalization had moved funding into `funding_lots_v2`, so 166
of 168 strict accounts read zero available funds and every request was refused with 402 while the
balance was intact. Money has exactly one authority — the account balance — and price has exactly
one — the account discount. `pricing.rs` keeps only the engine-owned request identity helpers used
by the provider quote builders, plus the hot tariff book below.

**Billing (async, `billing.rs` + tee-metering `meter.rs`):** authorization (`authorize`, async):
the env admin is checked FIRST in memory; otherwise the client key → `key_account` (JOIN key→account)
→ ACCOUNT balance (a non-positive balance rejects only a positive multiplier). The metered auth result
carries both the raw secret `key` for every existing reserve/settle flow and the authoritative non-secret
`key_id`. The request-fact producers consume only that non-secret identity for metered
Codex/OpenAI/Gemini universal Messages count, OpenAI native Responses input-token count, Anthropic
native Messages count, native Gemini countTokens, and billable native/universal Gemini text generation; every
money path still receives the raw key,
and the two identities must never be substituted
for one another. A zero multiplier is
free but metered on every provider: admission holds zero, settlement debits zero and still persists
the authoritative usage event. Balance/reserve/markup live on the account (shared across all of a user's keys).
All DB operations go through `AsyncBilling` (DB actors: 1 writer + N readers; sync PostgreSQL/legacy
SQLite live on dedicated threads, NOT on async workers). The generated request ID is created before reserve;
successful delivery is marked durable before the stream is handed over; finalize puts an idempotent settlement into the
outbox, and the writer retries it until commit. The RAII cancel closes exactly this request ID.
Every provider reserve pins the fixed provider id and `Authz::mult_for(provider)` value in the same
money transaction, so a concurrent admin edit cannot reprice an in-flight response. Reserve uses a
conservative hold with `max_tokens` clamped down (`cap_to_balance`), but authoritative provider
usage may exceed that estimate. Settlement keeps full billed usage while the shared account-row
fence limits collection to the aggregate −$1 floor; any remainder is explicit pool-funded
`uncollected_nano`, never a silent clamp or a second per-request overdraft. 4xx/errors/rotation are
NOT metered.

**Request-fact producer plumbing (S3A plus four narrow producers):** `AsyncBilling` has opt-in
typed fact-aware reserve, delivery, and terminal settlement commands. On PostgreSQL they stay in the
existing single money writer and delegate to the registry S2 same-transaction methods; legacy APIs
pass no fact. Production callers are the nonbillable Codex/OpenAI universal Messages count, OpenAI
native `POST /v1/responses/input_tokens`, Anthropic native Messages count, Gemini universal Messages
count, native Gemini `:countTokens`, and the first billable Stage-7 slices: native Anthropic
`POST /v1/messages` plus the Anthropic-plane universal OpenAI Chat `POST /v1/chat/completions` and
Responses `POST /v1/responses`, on a real Claude subscription or the configured Anthropic-wire
ClaudeStore fallback, plus native Gemini text `:generateContent`/`:streamGenerateContent` and the
Gemini-plane universal Chat, Responses, and Messages adapters. Gemini facts enter only with a
successful PostgreSQL money reservation and follow fact-aware delivery and settlement; SQLite, admin,
image, count, batch, catalog, health, and missing logical/lifecycle context stay outside this billable
producer. Each accepted Gemini universal adapter creates a typed content-free generation origin after
its owning parser accepts the original request. The synthesized native leaf consumes that origin to
produce exactly one fact under the public `universal` route and `chat|responses|messages` request
class instead of producing a second native fact. Missing client attribution is not an exclusion: an
otherwise valid fact records `client_kind=unknown` and `client_source=unknown`. Each
accepted universal adapter creates a private typed synthesized-Messages carrier only after its owning
parser/translator accepts the original client shape. The carrier holds
only route `universal`, exact request class `chat` or `responses`, bounded original model spelling,
accepted stream boolean, and the pure original OpenAI structural classifier; raw content, tool
names/schemas/arguments and headers are absent by type. The inner Messages proxy consumes it instead
of reclassifying the translated body, so one reserve admits exactly one universal fact and suppresses
native double emission. KIMI/GLM aliases are separate backends and never produce an Anthropic fact.
Each producer starts only
after successful metered route/body admission, captures the typed logical/lifecycle context plus
authoritative account/key and execution identities, and converges every later exit through one
nonblocking terminal submission. Anthropic owns one request-scoped RAII guard across subscription
rotation: send count is checked `usize`→`i32`, the terminal upstream response alone may contribute a
validated bounded `request-id`, returned headers use honest `started` rather than body completion,
and cancellation/panic fallback has no fabricated HTTP status or exhaustive attempt count. A lost
fact-aware reserve handoff is canceled with a
privacy-empty `unknown`/`not_started` terminal envelope so the admitted fact cannot leak as nonterminal.
Already-terminal post-auth/nonbillable facts alone use the PostgreSQL-only 4096-slot, nonblocking
`try_send` inbox and its lazily connected `request-facts-pg-writer` thread. That connection is
analytics-only and drops fail-open with fixed atomic health/drop counters. Connection acquisition may
retry within the bounded deadline before SQL. After an uncertain insert error the cached connection
is always discarded; a batch is replayed only when every fact has a non-null `billing_request_id`,
whose S2 uniqueness makes the whole batch idempotent. Any nullable fact makes the batch at-most-once:
the uncertain batch is dropped and counted `persistence_failed` rather than risking duplicate rows.
It does not enter the money FIFO or detached-settlement tracker, and `AsyncBilling::flush` deliberately
neither waits for nor guarantees this inbox; when all senders drop the worker may drain buffered facts,
but there is no join/durability shutdown gate. Fact-aware reservation validates the fact's billing ID,
account ID, execution group, and attempt against the money command before dispatch on both authorities.
A raw secret key cannot be compared with the fact's non-secret `key_id` in forward; PostgreSQL retains
the authoritative same-transaction key lookup/comparison, while SQLite deliberately omits analytics.
SQLite keeps the existing money semantics, starts no fact thread, persists no analytics, and reports
`UnsupportedAuthority` for terminal-at-insert submission. Provider-process request-context admission
is implemented separately: the reserved logical-ID header and optional public client-attribution
header are consumed into typed request extensions. All four nonbillable producer owners use only typed
logical/lifecycle values; either value's absence after metered auth omits analytics rather than
creating or guessing identity or time. A missing typed
attribution extension still records client `unknown`. Codex records
`openai`/`universal`/`count_tokens`, no upstream attempt, and bounded requested/canonical executable
models. OpenAI native input-token counting records `openai`/`native`/`input_tokens`, no upstream
attempt, and publishes bounded model plus content-free structural candidates only after its owning
Responses parser accepts them; a later preparation/history error retains that accepted evidence.
OpenAI/Codex generation owns PostgreSQL fact-aware reserve/delivery/settlement for native Responses,
native Chat, and universal Messages. Its shared overflow-checked observer counts actual generation
POSTs across retry/fallback. Runner join/panic terminalization preserves an exhaustive exact count
when representable but leaves HTTP/provider/delivery/tool evidence unknown; only an explicitly closed
receiver is a downstream disconnect, never a frame timeout. A completed nonstream turn whose durable
delivery marker fails still settles authoritative success/usage/tool evidence once, returns a
conservative 503, and keeps delivery unknown.
Anthropic records `anthropic`/`native`/`count_tokens`; bounded client model and structural classifier
evidence remain untrusted candidates until an upstream 2xx proves the owning native shape accepted.
Rejection discards both, executable model stays absent without exact execution proof, and no producer
retains raw JSON. Gemini count facts are owned once at native execution: the universal parser passes only typed
accepted client intent and receives an ownership handoff for the final public mapping; native parsing
classifies only after validation. A content-free transport observer counts profile rotation, 401
resends and helper replay at the actual submission seam, with checked conversion and conservative
NULL on uncertainty. Native Gemini text generation reuses that observer across its PostgreSQL-owned
reservation lifecycle, seals nonstream and background-drained stream terminal evidence through money
settlement, and explicitly terminalizes every exhausted ordinary post-reservation return with its exact
representable send count. Drop owns only cancellation, panic, or otherwise non-exhaustive ownership
loss. The producer records only explicit receiver closure as disconnect and preserves success usage
when a delivery marker fails while returning 503 with unknown delivery. Its universal origin retains
only bounded requested model, accepted stream flag, exact public request class, and the original
privacy-bounded classifier; executable model and all terminal evidence still come from the native
execution owner. Admin, unauthorized, Gemini image, batch/catalog/health, metrics, read APIs, and every
other production request-fact surface remain absent. Dropped
coverage is visible
only through the existing internal delivery snapshot, not a public metric.
For policy keys the cap takes the minimum of the account balance and the remaining lifetime limit. Such keys
bypass the auth TTL cache; expiry and limit are re-checked inside the atomic reserve transaction.
Every recognized metered 402 is audited after the response against a fresh authority read. The
low-cardinality `claude_api_positive_balance_402_total` counter advances when that live balance is
still positive; provider comes only from the fixed scrape target and customer/key identifiers never
enter Prometheus labels.

**Hot tariff overrides (`pricing/tariff_book.rs`):** the process-wide `TariffBook` (OnceLock +
`RwLock<Arc<TariffBookSnapshot>>`, installed once by `server` from `CLAUDE_API_TARIFF_OVERRIDES`,
default on) is the runtime consumer of the append-only `pricing_tariff_overrides` authority
(registry, migrations 0036/0037; compiled `metering` constants are the implicit v1 of a family,
rows are v2+). A background refresher in server main re-reads the whole tiny table through the
billing reader actor every 5s and atomically swaps the snapshot; a refresh failure keeps the last
good snapshot (startup: empty), a payload that fails typed parsing rejects the whole swap, and the
kill switch makes the book answer empty. The book never blocks and never fails a request. The
contract at the money paths:
- **Reserve pins.** Every reserve/preflight price resolution first computes the compiled family
  via the `metering::*_matched_tariff_at` helpers, then `tariff_book::reserve_base` resolves the
  override effective at the priced ts. An override replaces ONLY the base price vector — the
  speed/geo/long-context premiums, off-peak credit schedule and multipliers stay code-applied on
  top — and the request pins `<family>/v<version>` (`PinnedTariff`), which the durable admission
  snapshots carry as `tariff_schedule_id` (the compiled `<…>/v1`-style ids stay exactly as before
  when no override applies). A model no branch recognizes has no family and keeps its conservative
  MAX_PRICES-style fallback untouched; overrides exist only for known families.
- **Settlement replays the pin.** Charge paths (`meter.rs` finalize, codex/gemini `settle`,
  glm/kimi `price_turn_settlement`) call `tariff_book::charge_base`: the pinned family+version is
  looked up exactly (never the newest), a cross-family serve reprices by the SERVED family's
  override at the pinned priced ts, and no pin (or `<…>/v1`) is byte-identical compiled behavior.
  A pinned version missing from the book is a hard integrity error (elog::error + the local
  failure semantics: Anthropic/Codex/Gemini leave the reservation to the reconciler, KIMI falls
  to the fleet unknown-usage policy, GLM settles its documented conservative hold) — never a
  silent reprice at compiled, because the
  table is append-only and the pinning reserve read the row from this process. The async GLM/KIMI
  paths get one bounded refresh retry (`version_payload_refreshed`); the sync Anthropic/Codex/
  Gemini settle paths read the cache only. Calibration/capacity events price from the same
  resolution and record the override schedule id (codex `price_calibration_event` covers both the
  USD and the `chatgpt/codex-credits/<upstream>` credit ledger; GLM both `zhipu/glm/*` and
  `zhipu/glm-credits/*`).
- metering stays pure: the registry-mirror → metering converters live in `tariff_book.rs` and are
  a deliberate textual duplicate of the opposite-direction converters in
  `crates/server/src/tariff_admin.rs`.
- Router pricing parity: `/internal/router/catalog/pricing` (`server/src/router_pricing.rs`)
  resolves the same book, so router quotes never diverge from engine charges.

**Execution-state contract (stage 6.1 docs/engine/ROUTING_FENCING.md, `proxy.rs`
`EXECUTION_STATE_HEADER` + helpers `with_not_started`/`without_not_started`):** planes
set `x-apitoken-execution-state: not_started` on a response only when ALL three
conditions hold — non-2xx, not a single byte of the public response has gone to the client (per-plane delivery boundary:
Anthropic — before `mark_delivering`, Codex — before `emitted`/first SSE frame, Gemini — before the first
public event), the reserve under request_id is guaranteed to go to refund/cancel. The money
invariant "response with the header ⇒ ledger does not and will not contain a charge under request_id"
is proven by per-plane tests with a real reserve; branches where a charge is possible (legacy-scalar
full-hold, dropped TeeMeter on the 2xx path, fallback SSE assemblies after admission has started)
must strip the header via `without_not_started`. Setting points: Anthropic —
`local_err_for` and `stream_back` (non-2xx without metering), Codex — `ApiError::into_response` and
`skin_error`, Gemini — `ApiError::into_response` and the Messages-skin `skin_error`. On 2xx the header
is never allowed (including SseErrorTail inside a 200). The Universal Chat/Responses adapters
(`anthropic.rs`/`anthropic_responses.rs`, `gemini/chat.rs`/`gemini/responses.rs`) set the
signal on local pre-request failures, preserve only the exact authoritative signal when
rebuilding a plane's non-2xx, and explicitly strip it from parse/assembly errors already after 2xx, when
a charge is possible. The Gemini Messages skin follows the same rule for its surface. The router
must strip the header from transit responses (see crates/router/CLAUDE.md). The public
`is_exact_not_started_response` is the single predicate for router-proof and server telemetry: only
non-2xx with exactly one exact lowercase value. `crates/server` increments the bounded per-plane
counter only for a response that the fixed plane actually returned externally; a malformed duplicate
header and 2xx are not counted.

**Execution-group capability (stage 6.3):** `x-apitoken-execution-group` and
`x-apitoken-attempt` are router→plane only. Caddy removes client-supplied values on every public
ingress. Admission parses the pair exactly once before money mutation: both absent → direct execution;
both present exactly once → canonical lowercase UUIDv4 + canonical positive decimal;
partial/duplicate/malformed/noncanonical → fail closed. Anthropic parses in `proxy::forward`,
Codex/Gemini — in `begin_admission`; identity passes through the reserve in `AsyncBilling`. When sending to the external Anthropic upstream both internal headers
are removed. A plane never generates or repairs identity on its own.

**Request-observability identity and client admission:** `execution.rs` owns the typed
`LogicalRequestId`, privacy-bounded `ClientAttribution`, the reserved lowercase
`x-apitoken-logical-request-id`, and the optional public lowercase `x-apitoken-client`. At the
central server admission layer, an absent logical ID gets one fresh CSPRNG canonical lowercase
UUIDv4 through `fresh_request_id`; exactly one canonical trusted value is consumed; duplicate,
coalesced, non-UTF8, whitespace, uppercase, non-v4/variant, and every other noncanonical logical ID
fail closed. Client attribution is always fail-open: exactly one ASCII value no longer than 80 bytes
may be `opencode` or `claude_code`, optionally followed by one slash and a 1..64-byte version whose
characters are ASCII alphanumeric or `._+-`. Duplicate field-lines, comma, unsupported/case-variant
kinds, invalid bytes/control/grammar, empty, and over-limit values normalize to
`unknown`/`unknown`/no version. A malformed explicit value never falls through; heuristic v1 has no
reviewed positive signatures, so absent evidence also stays unknown and the existing Codex-envelope
heuristic is not reused. Both raw headers are removed before auth/body/dispatch. Universal
Anthropic/Gemini adapters copy both typed extensions to synthesized leaf requests without recreating
wire headers; native requests retain them. The Codex/Gemini universal and Anthropic/Gemini native count_tokens fact
producers persist normalized attribution (or unknown when the typed attribution extension is absent)
and consume the lifecycle terminal seal:
they capture safely ordered outer first-byte evidence or freeze `NULL` before any later body poll. A
missing typed lifecycle extension, like a missing logical extension, omits the fact rather than
fabricating evidence. No metric, public response/header identity, billing identity, execution-group
semantics, or billable producer changes in this slice.

**Request-observability structural definitions (stage 5 complete):**
`request_classification.rs` freezes the privacy-bounded v1 value contract and pure classifiers for
already validated client shapes: Anthropic Messages, OpenAI Chat/Responses and canonical native
Gemini GenerateContent. Anthropic native count_tokens classifies original client JSON before
namespace/identity mutation but publishes those closed fields only after an upstream 2xx proves the
native owning shape accepted. Gemini universal Messages consumes the Anthropic classifier only after
its owning parser accepts; native Gemini consumes its canonical classifier only after native
validation. Universal Chat/Responses use their OpenAI classifiers before translation; Messages uses
its Anthropic classifier before translation. Every scoped Stage 6/7 producer preserves that
client-intent boundary rather than classifying a degraded or gateway-injected translated shape. Evidence is limited to
checked `i32` tool count, the seven registry-defined closed bits, closed choice mode with named choices
stripped of the name, explicit parallel/result/structured/reasoning flags, reviewed Standard/Priority
tier and validated modality bits. Result `true` is existential, while `false` requires exhaustive
review; structured output requires an exact reviewed format. Missing, malformed, unsupported or unmeasured evidence stays
`None`; explicit empty arrays may be zero. The declaration count stays measurable when a lenient
parser accepts an unreviewed type, but the whole class bitset becomes `None` rather than a misleading
partial set. Unknown tool kinds are never promoted to `other_reviewed`. Current reviewed uses of that
bit are Gemini `urlContext` and an explicit Codex `tool_search` client descriptor; its later synthetic
dynamic function name is discarded and is not counted as a second client declaration. Codex
`web_search` records the accepted client intent even though admission drops that hosted descriptor,
and a namespace contributes only its reviewed function/custom child classes while remaining one
top-level count. Native Responses selects a single validated `input.additional_tools.tools` list when
present (including beside an explicit empty top-level list); it is classified once and the later
synthetic dynamic functions are never counted. The classifier stores no content, names, schemas, arguments, results, MCP labels,
raw JSON, headers, metadata or other arbitrary strings; its fields are private, `Debug` is redacted and it has no
serialization implementation. Every scoped count and text-generation producer is wired and preserves
response/upstream bytes. Lifecycle clocks and output tool-call evidence stay separate from the pure
classification values and are supplied only at their owning terminal seams.

**Request lifecycle clock carrier (`execution.rs`, `crates/server/src/http.rs`):** each successfully
admitted customer provider request owns a typed, redacted, once-only clock shared through the same
synthetic-leaf extension propagation as logical identity and client attribution. The server wraps the
final public Axum body after provider translation and remains the sole observer of the first non-empty
successful DATA frame in `http_body::Body::poll_frame`; empty frames, trailers, errors, EOF and
drop-before-data stay unobserved. A crate-visible terminal primitive atomically seals an open clock
with an internal never-exposed sentinel, linearizing against that outer-body observation: first byte
wins before the seal, or terminalization wins and any later DATA observation is ignored. It never
waits, polls, spawns or holds terminalization for a body consumer. It returns existing evidence only
inside the inclusive `[admitted_at, terminal_at]` bounds; invalid or out-of-order evidence stays
`NULL`, never clamped or fabricated, while the open state is still sealed. Repeated seals are
idempotent. The wrapper and the outer active-request guard delegate complete frames, errors,
`is_end_stream` and `size_hint` without buffering, rewriting, eager polling, I/O or settlement.
The production consumers are the nonbillable Codex/OpenAI/Gemini universal Messages count, OpenAI
native Responses input-token count, and Anthropic/Gemini native count fact producers; each seals at
terminal-fact construction without waiting for a later body poll. Gemini universal first transfers
its still-unsubmitted native owner through a typed response extension and seals only after outer
response conversion. Billable integration remains incomplete: native Anthropic Messages,
Anthropic-plane universal OpenAI Chat/Responses, Codex/OpenAI native Responses, native Chat and
universal Messages, plus native and universal Gemini text generation are the Stage-7 generation
producers. The rest of the locked route matrix remains future work, so producer coverage is still
incomplete.

**Claude capacity calibration (`anthropic_calibration.rs`, `billing.rs`, `meter.rs`):** every
successful Anthropic turn, including unmetered admin traffic, after authoritative usage builds one
immutable event with the internal request ID, subject/email, model, Standard/Fast, inference geography,
tariff schedule, exact input/cache-read/cache-write-5m/cache-write-1h/output/search counters and
the corresponding API nanoUSD legs. The event first advances cumulative subject spend and only
then do response quota snapshots observe the new total. Poll snapshots are free: they read durable
spend, but never increase it. After the authoritative turn event is enqueued into the FIFO, `TeeMeter`
immediately marks the serving subscription for a backend count-tokens probe and wakes the server poller:
live response headers can no longer keep `polled_ts` fresh and defer post-turn pairing.
The `Pool::request_probe` debounce limits probes to at most once per 15 seconds per subscription;
the writer poll command first drains the pending turn FIFO before observation, so backpressure cannot
reorder quota ahead of spend. A response snapshot and a fast post-turn poll may share the same
second: the FIFO remains the order of truth, an equal timestamp with a changed quota is processed, and
an exact quota/reset/resolution duplicate is ignored. Decimal quota fractions are parsed into `10^-8` units without float;
the real resolution of each endpoint is stored separately. The response and the free count-tokens probe
also publish the exact fraction without reset as an ephemeral `pool::QuotaSnapshot`: the server uses it
only for a fresh current remaining. Durable `observe_anthropic_window`, interval history and the
estimator still require a real reset — the runtime never invents window identity.

The 5h and 7d windows have independent identity/history/reset and are estimated without the subscription's
nominal value, via prior/EMA/WLS: `capacity_nano = 100_000_000 × Σobserved_spend_nano /
Σobserved_fraction_units`. The first quota-only movement waits for one snapshot ledger catch-up; a repeat
without spend becomes unattributed and does not inflate capacity. The raw history is fully replayed when
the estimator version changes. The runtime never averages a noisy account as a commercial nominal: the server
pools exact evidence only within the same plan + duration.

Turn evidence delivery is a bounded FIFO 4096 in `AsyncBilling` state, drained sequentially
by the billing writer. After the PostgreSQL operation retry is exhausted, the head stays pending and
blocks later Claude events and poll snapshots; the next event/poll or a graceful
`AsyncBilling::flush` retries it first. Exact request replay
is safe; a permanent semantic conflict quarantines only the conflicting row and does not block the queue.
Overflow/conflict increment the dropped counter. `pending_events`, `dropped_events` and
`persistence_ok` are published via `/capacity` and Prometheus; while delivery is pending/degraded the current
remaining fails closed, while accumulated historical capacity evidence stays visible.

An operator live-runner may address a bounded four-character profile hint via the
`x-apitoken-calibration-profile` header, but only under `Authz::Admin` (forwarding-admin/trusted
loopback). Metered/control/panel credentials ignore this header; it is always stripped before Anthropic.
The pool accepts the target only on exactly one match, bypasses the soft Reserve, but
keeps hard cap/cooling/auth-dead and forbids spill/rebind. A PostgreSQL lease gets hard-cap
semantics for the pinned continuation. This ties exact API-nanoUSD and quota delta to one subscription
without opening manual profile selection to clients. Attribution of the test turn itself is taken not from the aggregate
delta, but from a bounded set of new immutable event request IDs with exact profile/model/tier/token
vector; customer traffic in the same aggregate row therefore does not contaminate the result.

**What's inside:** `ProxyConfig`, `AppState`, `Clients` (http-client cache per proxy),
`limits_from_headers`/`Limits` (unified-ratelimit from a response), `poll_sub` (active polling of idle),
`detect_plan` (tariff from /api/oauth/profile), `forward` (axum handler), `authed`;
`validation.rs` — the single fail-closed parser of optional JSON controls for universal adapters.
A missing field and an explicit `null` mean absence/default; any present non-null
value must have the exact type and an allowed domain. `stream` and
`stream_options.include_usage` accept only a JSON boolean. Output limits
`max_completion_tokens`/`max_tokens`/`max_output_tokens` accept only a positive integer
representable as `u64`: zero, negative, fraction, string, object/array and overflow get a
local lane-shaped 400 before reserve/upstream. For aliases the first non-null spelling takes
priority: `null` on the preferred spelling allows the legacy fallback, while a malformed preferred spelling
is terminal and is not masked by a valid legacy value. This contract is identical for Anthropic and
Gemini Chat/Responses, native Codex Chat/Responses and the Codex/Gemini Messages skin; OpenAI surfaces
return the exact `error.param`, while the Anthropic envelope still keeps the parameter name only in the
message. The wiring is pinned by contract tests of each adapter, not just helper tests;
`anthropic.rs` — universal Chat Completions→Messages adapter (stages 3.1–3.4b
docs/engine/UNIFIED_ROUTER.md): translates a chat request into Messages JSON (strips the
`anthropic/` prefix BEFORE admission; when the client omits an output cap, the `max_tokens` required by
Messages is materialized from the model's native ceiling (64k for Claude ≤4.5,
128k for 4.6+/5) and may then only be reduced by balance admission; merges same-role
messages and runs of tool responses, a capability matrix of 16 rules with `400 unsupported_parameter`
for non-default penalties/logprobs/seed) and calls the shared `forward()` — auth, reserve,
rotation, identity injection, tee-metering and settle unchanged; the response is translated
ON THE WAY OUT (Messages SSE → `chat.completion.chunk`, JSON → `chat.completion`), and all
errors of this path (including `local_err` and upstream passthrough) are converted into the
OpenAI envelope preserving the HTTP status (402 LowBalance too) and `Retry-After`.
Tools (3.2): chat `tools`/`functions` → Messages `tools[]` (`parameters`→`input_schema`),
`tool_choice`/`function_call`/`parallel_tool_calls` → `tool_choice` (+`disable_parallel_tool_use`),
history `tool_calls`/tool roles ↔ `tool_use`/`tool_result` blocks (legacy id —
deterministic `callu_<name>`), in the response `tool_use` ↔ `message.tool_calls`
(non-stream) and tool_calls chunks from `content_block_start`/`input_json_delta` (SSE, tool
ordinal is numbered separately from the Messages block index); the event dictionary is pinned by
contract tests in the module. Multimodality and structured output (3.4a):
image_url parts of user messages → Messages image blocks (data: → base64 source,
http(s) → url source, `detail` != auto → 400), `response_format` json_schema →
GA `output_config.format` (schema only; json_object is rejected by the matrix).
Reasoning (3.4b/3.4c): `reasoning_effort` accepts the compatible
minimal|low|medium|high|xhigh|max and is translated into GA `output_config.effort`
(minimal clamps to low, an invalid value → `400 invalid_request`; `effort` sits next to
`format` in the same `output_config`). The exact native matrix is model-specific: Claude 4.6 —
low|medium|high|max, Claude 4.7+/5 — low|medium|high|xhigh|max; a level outside the compatible
model's matrix is rejected locally before reserve/upstream. The adapter also injects
`thinking: {type:"adaptive", display:"summarized"}` — without it adaptive is off, and the default
display=omitted sends empty thinking blocks; an explicit client `thinking` is not overridden.
On models before 4.6 the upstream rejects both fields, so a valid effort degrades to the model
default without them; an explicit legacy `thinking` is preserved. Effort does not create a separate metering
modifier: thinking is already included in the shared Anthropic `output_tokens`, and reserve bounds the whole
output via `max_tokens`.
Response thinking blocks/thinking_delta → `message.reasoning_content`/reasoning_content deltas
(signature/redacted_thinking are never exposed). On replay this field is display-only: the adapter never
turns it into unsigned native thinking; an assistant turn with only a non-empty `reasoning_content`
is dropped so a standard AI SDK round-trip does not poison the next send, while a genuinely empty
assistant still gets a 400. The same display-only omission covers an assistant turn whose only
payload is whitespace-only text (`content:""` or whitespace without tool calls — the replay shape
of an empty AI SDK step); it never becomes a model-visible text block.
Translated Messages SSE is validated by the shared state machine before conversion: known events
must have a matching `data.type` and the order `message_start` → lifecycle of all content blocks →
`message_delta` with stop reason/usage → `message_stop`; a malformed known event, an impossible order
or EOF before `message_stop` become a terminal OpenAI error without `[DONE]`. An unknown named
event is ignored for forward compatibility, and the last valid frame without a terminating blank
line is parsed at EOF.
The adapter's synthetic OpenAI errors are born ONLY through its `chat_error` (with
`TerminalErrorReason`, like `local_err`) and likewise never carry pool internals.
`anthropic_responses.rs` — universal Responses→Messages adapter (stages 4.1–4.2
docs/engine/UNIFIED_ROUTER.md, route `POST /v1/responses` in `ProviderMode::Anthropic`)
following the same scheme as the chat adapter: a Responses request is translated into Messages JSON
(`instructions`/system/developer items → top-level `system`, input items → messages with
same-role merging, `input_text`/`output_text` → text blocks, `input_image` →
image blocks via the shared translation, replay of tool history (4.2): function_call items →
assistant `tool_use` blocks (`call_id` → `id`, the `arguments` JSON string is parsed into
`input`, invalid → `400 invalid_request`), function_call_output items → user
`tool_result` blocks (`call_id` → `tool_use_id`; output string as-is or a merge of
text parts via \n, non-text parts → 400), tool_use/tool_result pairing is not
validated — as in chat adapter 3.2, `tools` → `input_schema` (non-function tool → 400),
`tool_choice`/`parallel_tool_calls` → Messages `tool_choice`, `max_output_tokens` →
`max_tokens`; a missing client cap materializes the same native 64k/128k ceiling,
not a separate universal-lane default; `reasoning.effort` → the same model-specific matrix
`output_config.effort` + inject `thinking: {type:"adaptive", display:"summarized"}` as 3.4c
(on earlier models the hint degrades to the model default), `text.format` json_schema
→ `output_config.format`, capability matrix of 9 rules + open list) and calls the shared
`forward()` unchanged; the response is translated ON THE WAY OUT — Messages SSE → Responses SSE
via the 4.1 dictionary + reasoning 4.2 (`response.created`/`in_progress` → per-block
item/part/text|arguments deltas; thinking block → reasoning item `rs_*` +
`response.reasoning_summary_part.added` → `response.reasoning_summary_text.delta`*
(signature and empty deltas are dropped) → `…_text.done`/`…_part.done` → item.done →
`response.completed`; ping → `: ping`; `event: error`/premature EOF →
`response.failed`; output_index — a dense counter including thinking blocks,
redacted_thinking — without a position; `output_tokens_details` of message_delta are proxied),
JSON message → Response object (text into one message item at the position of the first
text block, thinking → reasoning items `rs_*` in block order (empty thinking — no
item, redacted_thinking is skipped), tool_use → function_call items, usage with
cache/reasoning details, status from stop_reason); errors — the `convert_error_response`
shared with the chat adapter (OpenAI envelope, status 402 and `Retry-After` preserved).
Shared helpers (`chat_error`, `invalid_request`, `unsupported_parameter`,
`convert_error_response`, `image_block` and `translate_reasoning_effort` with the parameter
name, `translate_tool_function`, `merge_or_push`, limit constants) —
`pub(crate)` in `anthropic.rs`. Temporary limitations (after 4.2): input reasoning items
are discarded (signatures and encrypted content are never exposed — decision 4),
`store:true`/`previous_response_id`/`item_reference` → `400 documented_limitation`
(stored responses — openai/* only, decision 5).
 `codex/` contains the native HTTPS transport (`transport.rs`), profile pool (`mod.rs`),
 Responses/Chat adapters, tenant-bound history, Codex admission/settlement and reconstruction SSE
 events. The GPT Chat adapter's assistant-history replay follows the same display-only contract as
 the universal Anthropic/Gemini adapters: a non-empty `reasoning_content` is never turned into
 unsigned provider reasoning, a reasoning-only turn is dropped, and a whitespace-only assistant
 turn (`content:""` or whitespace without tool calls — an empty AI SDK step) is dropped the same
 way; only `content:null`/absent with no payload at all gets a 400.
 `codex/skin.rs` — Anthropic Skin (stage 5.1 docs/engine/UNIFIED_ROUTER.md, routes
`POST /v1/messages` and `/v1/messages/count_tokens` in `ProviderMode::OpenAi`, dispatch by
model — in the router): a Messages request is translated into Responses JSON (strips the
`openai/` prefix, `speed:"fast"` and compatible `service_tier:"fast"|"priority"` are normalized into the canonical
Responses `priority` before admission; the effective tier is returned in `usage.service_tier`,
top-level `system` → `instructions` with text blocks merged via \n\n, user text/image →
`input_text`/`input_image` via the shared `canonical_image_part`, tool history replay — the mirror of 4.2:
`tool_use` → `function_call`, `tool_result` → `function_call_output`, input
thinking/redacted_thinking are dropped — decision 6; `tools[]` → function tools, `tool_choice`
→ default/required/none/named + `parallel_tool_calls`, `thinking` → `reasoning.effort`
lossy by thresholds <4096 → low / <16384 → medium / otherwise high, <1024 → 400; capability
matrix: stateful/unknown `cache_control` anywhere, stateful/unknown
`context_management`, `mcp_servers`, `container` → `400 invalid_request_error`. The exact
Claude Code `cache_control:{type:"ephemeral"}` on system/content/tools is accepted and stripped:
Codex prompt caching is automatic, while an extended retention policy stays fail-closed. The current bounded
no-op Claude Code `context_management` (`edits:[]` or exactly
`clear_thinking_20251015` + `keep:"all"`) is accepted and ignored: this stateless adapter drops input
thinking blocks anyway, and any extension of the form stays fail-closed.
Messages GA `output_config.effort` low/medium/high is honestly translated into
`reasoning.effort`, and exact `output_config.format` json_schema with a schema object — into
Responses `text.format`; unknown keys and unrepresentable forms are fail-closed. This covers
both parallel requests of Claude Code 2.1.220: the structured title and the main adaptive turn.
`metadata` (including `user_id`), sampling controls and unknown fields are accepted and
ignored — the same leniency as chat.rs, otherwise Claude Code with its
`metadata.user_id` would break) and goes
through the SAME turn pipeline as chat.rs (admission, affinity, reserve, run, settle);
the response is translated ON THE WAY OUT — output items → Messages content blocks (message → text block
at the position of the first message item, function_call → tool_use, reasoning → thinking WITHOUT
signature), usage → Messages usage with cache/thinking details, stop_reason
tool_use/max_tokens/stop_sequence/end_turn; `stop_sequences` and the output budget of ~4
chars/token are honestly applied on delivered text via the `StopFilter`/`enforce_output_limits`
shared with chat.rs (the transport never truncates generation upstream); SSE —
`message_start` with zero usage (authoritative usage only in `message_delta`) → dense
content_block start/delta/stop → `message_delta` → `message_stop`, heartbeat `event:
ping`, mid-stream failure `event: error`, a client disconnect never kills the turn before settlement;
all endpoint errors use the Anthropic envelope preserving the status and `Retry-After` (503 →
529 `overloaded_error`, 402 is preserved). `count_tokens` — the same parse +
`parse_responses_request`/`prepare_turn` → a reserve-grade estimate of `input_tokens` without network
(`max_tokens` is optional); there is no end-to-end coverage for the Codex plane — coverage is
contract tests of the module, as 3.3/4.3; `gemini/` — native route allowlist, encrypted OAuth pool, Code Assist translation and
settlement; `gemini/chat.rs` — universal Chat Completions→generateContent adapter (stages 3.3–3.4b
docs/engine/UNIFIED_ROUTER.md) following the same scheme as `anthropic.rs`: a chat request is translated into
GenerateContentRequest JSON (system/developer → `systemInstruction`, merging of same-role contents
and runs of functionResponse; `maxOutputTokens` is passed only with an explicit client cap,
and without it the shared `gemini_api()` takes the model's native output limit and clamps
it only by balance; tool/function history ↔ functionCall/
functionResponse with the name restored from tool_call_id, `tool_choice` → `functionCallingConfig`,
a capability matrix of 18 rules (the same 16 as the Anthropic plane, plus `parallel_tool_calls`
and `user`) PLUS a closed list of top-level fields — an unknown field
→ `400 unsupported_parameter`, because the Code Assist wrapper would silently discard it), strips the
`google/` prefix BEFORE admission; calls the shared `gemini_api()` via a synthesized internal
request to `/v1beta/models/{model}:generateContent|streamGenerateContent?alt=sse` — admission,
reserve, affinity, rotation, wrapper, tee-metering and settle unchanged; the response is translated
ON THE WAY OUT (GenerateContentResponse data-only SSE → `chat.completion.chunk` with role/content/finish/
usage chunks and functionCall as a single tool_calls chunk, JSON → `chat.completion` with synthesized
ids `callu_<name>[_N]`), Google-envelope errors are converted into the OpenAI envelope preserving the
status (402 too) and `Retry-After`, native `400 API_KEY_INVALID` → `401 authentication_error`.
Multimodality and structured output (3.4a): image_url parts of user messages → `inlineData` parts
(only data: URLs are accepted — the plane has no outbound fetch for external images, so
an http(s) image URL → `400 invalid_request`; `detail` != auto → `400 unsupported_parameter`),
`response_format` json_object/json_schema → `generationConfig.responseMimeType`/`responseSchema`
(the name/strict wrapper is stripped). Generated images are NOT dropped: an image-MIME
`inlineData` part in the candidate becomes an `image_url` content part with a data URL
(non-stream — array-form content; stream — one `content` array delta), so billed media reaches
the caller; non-image inline media has no OpenAI representation and is not fabricated. The shared `gemini_schema` translator is mandatory for tool parameters and
structured-output schemas on Chat, Responses and the Messages skin. It accepts only the exact
supported Google `Schema` subset, inline-expands bounded local `$ref`/`$defs`, translates
representable `const`/nullable union/exclusive bounds/true-contains and strips annotations.
An unrepresentable or unknown validation keyword must produce a local 400 with the exact schema path;
silently dropping a constraint or passing it to the private Code Assist parser is forbidden. Limits — 4096
expanded nodes and depth 64. Same-name keys inside `properties` stay parameter names.
Reasoning (3.4b): `reasoning_effort` → `generationConfig.thinkingConfig`
(`thinkingLevel` is proxied as-is — mapping the level into the wire model id is done by the
plane; `includeThoughts: true`; an invalid value → `400 invalid_request`),
response thought parts → `message.reasoning_content`/reasoning_content deltas
(`thoughtSignature` is never exposed). On replay this field is display-only: without an opaque signature it never
becomes a native thought; a model turn with only a non-empty `reasoning_content`
is dropped so an OpenAI-compatible AI SDK can continue the history after a thought-only response, while a genuinely empty
assistant still gets a 400. A whitespace-only assistant turn (`content:""` or whitespace-only text without
tool calls — an empty AI SDK step) is dropped the same way and never becomes a visible text part.
Tool history replay works statelessly: every reconstructed functionCall part gets the
confirmed Code Assist marker
`thoughtSignature:"context_engineering_is_the_way_to_go"`. Real opaque response signatures
per decision 4 are still never exposed and never persisted; synthetic ids and public response
shapes are unchanged. One helper is mandatory for Chat, Responses and the Messages skin.
All three translated Gemini SSE surfaces use a shared fail-closed check of source frames:
unknown extra JSON fields are allowed, but malformed JSON and wrong types of the known
`candidates`/`content.parts`/`functionCall`/`usageMetadata`/`promptFeedback`/`error` terminate
the client stream with a protocol error. A clean EOF is successful only after provider terminal evidence —
a non-empty `finishReason` or `promptFeedback.blockReason`; an EOF before that never becomes
`[DONE]`, `response.completed` or `message_stop`. The last valid data-frame without `\n\n`
is processed before the terminal check.
`gemini/responses.rs` — universal Responses→generateContent adapter (stage 4.3
docs/engine/UNIFIED_ROUTER.md, route `POST /v1/responses` in `ProviderMode::Gemini`) —
the Gemini mirror of `anthropic_responses.rs`: the Responses side of the 4.1+4.2 dictionary (item forms,
SSE events, usage, status/incomplete_details) is identical to the Anthropic adapter (module contract
tests on the same tabular expectations), request translation and response parsing — per the
rules of `gemini/chat.rs`: `instructions`/system/developer items → `systemInstruction`,
input items → contents with same-role merging, `input_image` → inlineData via the shared
translation (only data: URLs, http(s) → 400), tool history replay: function_call items →
functionCall parts of model content (`arguments` JSON string → `args`), function_call_output
items → functionResponse parts of user content (the name is restored from the
call_id→name map — functionResponse references by name, output without a pair →
`400 invalid_request`; unlike the Anthropic mirror, pairing IS validated), `tools` →
functionDeclarations (flat descriptor, `strict` is stripped), `tool_choice` →
`functionCallingConfig`, `max_output_tokens` → `maxOutputTokens`; without a cap the field is not
inserted, and the shared admission uses the model's native output limit,
`reasoning.effort` → `thinkingConfig` (minimal is NOT clamped — a difference from Anthropic),
`text.format` → `responseMimeType`/`responseSchema` (generateContent does have json_object),
capability matrix — the same 9 rules as the Anthropic mirror, plus `parallel_tool_calls`
(default true only), PLUS a closed list of top-level fields (unknown →
`400 unsupported_parameter`); `store:true`/`previous_response_id`/`item_reference` →
`400 documented_limitation` (decision 5). Response: thought parts → reasoning items `rs_*` and
reasoning_summary events of the 4.2 dictionary (thoughtSignature is never exposed), functionCall →
function_call items `fc_*` with synthesized call_ids `callu_<name>[_N]` and exactly one
arguments delta (functionCall arrives whole), generated image-MIME `inlineData` parts →
`output_image` items `img_*` with a data URL (non-stream and stream: item.added → item.done), usage input=`promptTokenCount` /
output=`candidatesTokenCount`+`thoughtsTokenCount` (thoughts → `reasoning_tokens`),
finishReason/blockReason → status via the shared `map_finish_reason` (MAX_TOKENS →
incomplete `max_output_tokens`, SAFETY etc. → incomplete `content_filter`); stream —
data-only SSE → Responses SSE, `finishReason`/`blockReason` + clean EOF →
`response.completed`, a malformed frame or EOF without terminal evidence → `response.failed`,
a mid-stream error frame → `response.failed`; errors — the `convert_error_response` shared
with the chat adapter (400 API_KEY_INVALID → 401). Shared helpers (`chat_error`,
`invalid_request`, `unsupported_parameter`, `convert_error_response`, `merge_or_push`,
`gemini_image_part`/`translate_reasoning_effort`/`parse_tool_arguments` with the parameter
name, `function_declaration`, `code_assist_schema`, `replayed_function_call_part`,
`function_response_value`, `synthetic_call_id`,
`map_finish_reason`, limit constants) — `pub(crate)` in `gemini/chat.rs`; the schema translator itself
is isolated in `gemini_schema.rs` and called by all three adapters.
`gemini/skin.rs` — Anthropic Skin (stage 5.2 docs/engine/UNIFIED_ROUTER.md, routes
`POST /v1/messages` and `/v1/messages/count_tokens` in `ProviderMode::Gemini`, dispatch by
model — in the router) — the Gemini mirror of `codex/skin.rs`: the Messages side of the dictionary is identical to 5.1
(contract tests on equivalent input), translation and parsing — per the rules of `gemini/chat.rs`:
top-level `system` → `systemInstruction` (merge via \n\n, non-default `cache_control` → 400),
messages → contents via the shared `merge_or_push` (assistant → model role; `tool_use` →
functionCall with `args` OBJECT — not a JSON string, a difference from the Codex side; `tool_result` →
functionResponse, pairing via the id→name map IS validated — the 3.3/4.3 pattern), image: only
base64 → inlineData (url source → 400 — generateContent accepts no links), input thinking
is dropped; `tools`/`tool_choice` → functionDeclarations/functionCallingConfig
(`disable_parallel_tool_use:true` → 400 — Gemini has no analogue); `thinking` →
thinkingConfig by the 5.1 thresholds (<1024 → 400) + `includeThoughts:true`; sampling
(temperature/top_p/top_k) and `stop_sequences` are proxied into generationConfig (natively
supported — a plane-level difference from 5.1, where they are ignored; stop_reason stop_sequence
is indistinguishable → end_turn); capability matrix — the same 4 rules of 5.1 + a closed list of
top-level fields (unknown → 400, as chat.rs). Response: text parts → one text block,
thought parts → thinking blocks WITHOUT signature (thoughtSignature-only is skipped),
functionCall → `tool_use` with a synthesized `toolu_<name>[_N]`, usage input=
`promptTokenCount` / output=`candidatesTokenCount`+`thoughtsTokenCount` (thoughts →
`output_tokens_details.thinking_tokens`, cached → `cache_read_input_tokens`); SSE — the
same 5.1 skeleton (message_start with zero usage → dense content_block_* → message_delta →
message_stop only after provider `finishReason`/`blockReason` + EOF, heartbeat `event: ping`,
malformed/premature EOF and mid-stream failure → `event: error`); errors —
Anthropic envelope (400 API_KEY_INVALID → 401, 503 → 529 `overloaded_error`, 402 and
Retry-After preserved). Handlers go through the shared `gemini_api()` via an internal Request to
`generateContent|streamGenerateContent?alt=sse|:countTokens` — admission, reserve,
affinity, rotation, wrapper, settle without a single change; `count_tokens` — the native
`:countTokens` (quota-free, no reserve), `max_tokens` is optional there. Tool schemas and replayed
tool history use the same shared sanitizer/context-engineering marker as Chat/Responses;
real opaque response signatures stay hidden per decision 4.
Env for both is read only by `server::config`.

**Default-off Gemini Batch public producer (`gemini/batch_handlers.rs`):** when composition supplies
`AppState.gemini_batch`, exact native batch and Files routes authorize before reading any body,
atomically submit inline item holds through the dedicated authority actor, scope every read/mutation
by account, and encrypt/decrypt file chunks through the Stage 3 keyring. The facade remains absent
by default. A terminal GET must read the account-scoped `result`/`error` blob, authenticate and
decrypt it with the exact account/job/item/kind identity, and return the real Gemini response or
Google-shaped per-item error in input order; missing, malformed or tampered terminal blobs are
`DATA_LOSS`, never request-id placeholders. Stage 6 composes the facade only in
`claude-api-gemini@.service`; native ListModels/GetModel add `batchGenerateContent` only while
`AppState.gemini_batch` contains that public facade, and never for the image-output model.

**Cache-first routing without client opt-in (`affinity.rs`):** tenant = metered `account_id` (all keys
of the account share the cache) or a separate admin scope. `AffinityStore::infer` is computed BEFORE identity injection.
Strong session IDs are accepted from Claude Code/generic session-conversation-thread headers and the same-named
top-level/`metadata` body fields; they are normalized into a keyed digest and are a HARD boundary: a new ID
never inherits the transcript/root of another session. Without an ID, rolling keyed hashes of each
canonical message-prefix are built together with the cache shape (`model/system/tools/thinking/context_management`),
so a growing classic API history finds the deepest known prefix without parsing the response.
A large/explicitly cache-controlled shared `system+tools` is stored SEPARATELY as a soft cache-root → a set of
warm homes (5m/1h TTL from cache_control), written only after an upstream 2xx and never resolving a
conversation. The pool first warms two competing homes, then picks a warm one while its free
capacity is no worse than 70% of the best; this keeps shared system/tools from collapsing independent sessions onto one account.
Local L1 is always on; the optional Redis L2 shares TTL bindings and the ZSET of warm homes across slots. Redis
stores only keyed digests of tenant/native/transcript/subscription and fails open: network/timeout/eviction never
participate in auth, money or capacity. First attempt = `pool.route_affinity`
(place/pin/immediate spill/rebind), retries = `pool.pick`. The PostgreSQL capacity lease below stays
authoritative. SSE remains byte-for-byte.
L2 is covered by the gate, not just on paper: the watchdog brings up a disposable Redis and passes
`CLAUDE_API_TEST_REDIS_URL` together with `CI=1`. Under this marker `redis_shares_opaque_affinity_across_processes`
must run — with Redis unavailable it fails rather than skips; locally without the variable
the test is skipped with a message. The key shape is pinned separately without infrastructure
(`redis_keys_expose_only_opaque_digests`): only a constant prefix, hash-tag and 64-character hex —
any raw scope/session/prompt in a key fails the test.
In-flight is held for the whole life of the stream: success → `mark_healthy`, `end_stream` from tee-metering (`meter.rs`)
releases the slot on completion/abortion; 4xx → `mark_ok`.

**Advisory cross-slot cooling hint (same `affinity.rs`, keyspace `claude-api:cool:v1`):** a fresh
429 of one slot reaches `pool_state.cooling_until` only via debounced persistence (≥1 s + CAS);
in that window `acquire_capacity` on a neighboring slot still issues a lease on the just-rate-limited
subscription. `publish_cooling_hint` is called detached on every `mark_cooling` of the live path (429,
network failure, broken proxy) and writes the subscription's keyed digest (the same `home_id`; the raw email never
reaches Redis) with max-merge (a short hint never overwrites a long one) and TTL = the cooling deadline; hints < 3 s are not
published. Candidate selection in proxy.rs checks `cooling_hint` before the authority gate and by hint may
ONLY rotate to the next candidate — never grant capacity; `acquire_capacity` remains
the sole authority, a stale hint costs at most one rotation. Fail-open is shared with affinity:
a network/timeout error → `claude_api_cooling_hint_{lookup,publish}_errors_total` and selection without the hint.
The poller's probe-cooling is deliberately not published: its signal does not need sub-second propagation.
The Codex plane has its own in-memory cooling (`codex/health.rs`) and is not part of this contract.

**Rotation/limits (pool resilience):**
- **Passive collection:** on EVERY upstream response we extract unified-ratelimit (`limits_from_headers`)
  → `pool.set_util`. This keeps util/reset always fresh from live traffic; the active `poll_sub` (server)
  only tops up idle subscriptions (the updated `polled_ts` gates this itself). Saves quota.
- **Fault classification (don't chill a subscription for someone else's fault):**
  - `429` → subscription quota → `mark_cooling(cool_secs_429)`: `Retry-After` → culprit window
    (`util7d≥0.95` → `reset7d`, otherwise `reset5h`) → burst default. We don't chill for 5h when the 7d window was hit.
  - live request `401/403` may be caused by the customer model/beta/path/scope. It is counted and
    retried on at most one distinct subscription without cooling either one; a repeated rejection is
    returned byte-for-byte. Only clean background probes feed the durable healthy → suspect → dead
    machine (two rejections spread over at least five minutes), and only `dead` excludes a credential.
  - `5xx/408/409/425/network` → UPSTREAM's fault → `mark_done` (slot −1 WITHOUT cooling: the subscription is healthy)
    + `breaker.record_fail`. A broken proxy (build err) → short `mark_cooling(10)` (local).
  - `2xx/4xx` → upstream healthy → `breaker.record_ok` (resets the failure window).
- **Circuit breaker (`breaker.rs`):** opens when ≥ N DISTINCT subscriptions
  failed in the window (distinct email, not a raw count) — this signals an api.anthropic.com outage, whereas one
  flaky proxy/poison yields failures of a single email and the breaker is NOT touched. While open — the entrance
  rejects with `503 + Retry-After` (no fan-out across the pool). `record_fail(now, email)`; on 2xx/4xx `record_ok` resets.
- **Rotation budget:** 429 does not spend the backend-failure budget and may rotate across available
  capacity. A live 401/403 gets at most one alternate subscription as described above; it never fans
  out across the fleet. Only backend failures spend the budget (5xx/network — an outage). The upper
  iteration bound remains "whole fleet + margin" for the failure classes that can legitimately rotate.
- **Outcome when all attempts fail:** hit the backend budget → return the last upstream error
  (an outage; the breaker is about to open); all subscriptions over the limit → `429 + Retry-After = soonest_ready`
  (the client will back off on its own — exactly this, not the error of one banned subscription); pool empty → `503`.
  Every `mark_used` is paired with `InflightGuard`/`end_stream`/`mark_done`.
- **ClaudeStore-compatible Claude emergency fallback:** compile-fixed to
  `https://api.llmsrelay.com`, configured for metered `POST /v1/messages` only, without an operator
  calibration target and with an already-created durable reserve. One external attempt is allowed only after the
  terminal state of the entire local pre-byte rotation/smooth-wait. The body is cloned after the namespace strip and before
  identity/persona/billing mutation; outbound goes only `x-api-key`, Anthropic version, client
  beta and the JSON body with the balance cap. Local OAuth/proxy/subscription/persona are never sent. Success
  uses the same `BillCtx`/`TeeMeter`, but `MeterCtx.subscription=None`, so the customer's exact
  settlement is preserved while pool spend/quota/calibration/affinity are unchanged. A non-2xx/network
  fallback is never retried and never disclosed to the client; once the external send has started, the
  `not_started` proof is stripped as execution-ambiguous. Post-byte replay is forbidden.
- **ClaudeStore GPT emergency fallback:** compile-fixed to
  `https://api3.claudestore.store`, with a separate default-off Codex-tier credential, not a Claude
  key. After the terminal state of the normal Codex home rotation/retry, at most one external
  `POST /v1/responses` is allowed, only before the first model delta and only for the compile-fixed `gpt-5.5`/`gpt-5.4`.
  The body restores the public model id; `chatgpt-account-id`, originator, client metadata,
  OAuth/proxy/private slug are never sent. The shared Responses decoder must receive nonzero,
  consistent terminal usage; local quota/health/affinity/calibration are unchanged. Non-2xx,
  network or missing usage are never retried, keep the original local public status and strip
  `not_started`. This transport never allows startup with an empty sealed Codex roster.

**Fleet anti-fingerprinting (`persona_ua`):** a fleet of 100 byte-for-byte identical UAs — is itself a
fingerprint. `persona_ua(cfg, email)` yields a UA that is **stable over time** for a subscription, but **distinct between
subscriptions**: the pool is given as a list (`user_agents` len>1) → pin by hash(email); otherwise we vary the
patch version of the base UA across `ua_spread`. The client `user-agent` is NOT passed through (in `skip_req_header`)
— the fingerprint is ours. The same UA also goes to `poll_sub`/`detect_plan` (persona health = the same fingerprint
as live traffic). Identity/beta/anthropic-version are NOT varied — they are correctness-critical (no ground-truth on
plausible alternatives). Env: `CLAUDE_API_UA` (one or a list), `CLAUDE_API_UA_SPREAD`.

**Unlimited client dispatch:** Claude, Codex and Gemini have no process/per-account/per-profile
request semaphore, local concurrency queue or concurrency rejection. Every request that passed auth/money
admission immediately picks a profile and starts an upstream attempt. In-flight counters live for the whole
life of the stream, but are used only for balancing and observability; the soft Claude threshold
spills to a less-loaded subscription and fails open by picking an available subscription if the whole fleet is above the
threshold. The unlimited RAII task tracker exists only for graceful shutdown: it instantly registers
any number of already-started tasks, closes the entrance only at process retirement and waits for them to drain.
Provider quota/cooling still honestly yield the native `429 + Retry-After`; retry/rotation is allowed
only before the first public byte — after it a repeated upstream launch is forbidden.

**Producer contract of model metadata:** native Anthropic `/v1/models` remains the authority for its
`max_input_tokens`/`max_tokens`/`capabilities`. The owned OpenAI and Gemini catalog builders additionally
publish expand-only `apitoken.limits` and `apitoken.capabilities`: exact input/output modalities,
tool calling, structured outputs, streaming, reasoning efforts and service tiers. Codex takes input
and display name from the last-good authenticated `/codex/models`, output/efforts/Fast and adapter
capabilities — from the reviewed admission config; with multiple profiles only the
minimal proven input is guaranteed, a conflicting display name is omitted, and missing input metadata
on any serving profile drops the input/context fields. Gemini publishes exact configured token
limits and model-specific capabilities; the image-generation model explicitly does not advertise tools/structured
outputs, and exact PCM WAV audio is advertised only by Flash Preview. An unknown value is omitted —
model id, pricing threshold, owner or family default are never a basis for guessing it.
The consumer contract and normalization of the three native shapes are described in
`docs/engine/UNIFIED_ROUTER.md`.

**RAII guards on request cancellation (critical):** the client drops the connection → the handler future is dropped at
`await`; without guards `mark_used(+1)` and `reserve(hold)` would NOT roll back (leak of persona capacity +
client money forever). `InflightGuard` (Drop → `mark_done`) closes the slot on any non-streaming
outcome AND on cancellation; disarmed on success (the slot is held by the stream → `end_stream`). `HoldGuard` (Drop →
`settle(hold,0)`) returns the reserve on any unsuccessful outcome AND on cancellation; disarmed on success
(the hold is closed by tee-metering with actuals). That is why `mark_cooling`/`mark_healthy` never touch in-flight —
the slot has a single owner. The breaker is fed at most once per request (anti-DoS from a poison request).

**Transparency invariants (critical — do not break):**
1. The upstream response is delivered to the client **byte-for-byte** (including the SSE stream). Never buffer,
   never rewrite the body.
2. Under the hood: Claude Code identity injection as the FIRST system block + `anthropic-beta: oauth-…` +
   the subscription's `Bearer`. The client's `system` is preserved as the second block. Without identity Anthropic does
   not accept subscription OAuth tokens — but the client must never know this. The namespaced catalog id
   (`anthropic/<native id>`) is stripped by admission before reserve and upstream (`strip_own_namespace`):
   the router's universal dispatch proxies the body byte-identically, and the prefix would reach upstream
   as-is (404); mirrors the strip of the `anthropic.rs` chat adapter. The native id is unchanged.
3. **A broken SSE is closed with an `event: error` frame** (`SseErrorTail`), not silent truncation:
   for an SDK a stalled stream is indistinguishable from a finished one, and many clients hang on it. The frame
   is part of the Anthropic protocol, so this is NOT a departure from byte-for-byte transparency, but its
   restoration — a real upstream would have sent exactly it. The wrapper is the outermost one so that metering
   never sees synthetic bytes (a failure tail is not usage), and the text is depersonalized, as in local_err.
4. **Rotation only BEFORE the stream starts:** the decision by status (429/401/403/5xx → cooling + next
   subscription) is made before the body is delivered. Once streaming has started — no switching.
4. Client request errors (400/404/422 …) are passed through as-is, WITHOUT rotation.
5. The client's authorization headers (`x-api-key`/`authorization`) NEVER go upstream — they are replaced
   with the subscription's Bearer. Never log tokens.
6. **The synthetic-error sanitizer (`LocalErr`/`local_err` in `proxy.rs`) — the SINGLE point
   where OUR responses to the client are born.** The client believes it is talking to api.anthropic.com, so
   `error.type`/`message` must NOT contain our internals (`subscription/pool/upstream/authority/
   cooling/persona/fleet/oauth`). The internal reason lives only in metrics and the `elog`
   log (`crates/elog`), NOT in
   the body. Public triplets are authentic Anthropic: `overloaded_error`=**529**, `api_error`=500,
   `rate_limit_error`=429, `authentication_error`=401, `not_found_error`=404, `request_too_large`=413.
   Capacity shortage/empty pool/breaker/authority/upstream connection failure → depersonalized retryable
   `Overloaded`/`RateLimited`. Legitimate account-state errors the client MUST know and they stay:
   `InvalidKey` (401), `LowBalance` (**402**, the docs-portal contract). Add a new error ONLY as a
   `LocalErr` variant (not a raw `err_response`); the regression test `local_err_never_leaks_*` gates this.

**Native Codex gateway invariants (the same bar as Gemini):**
0. **Profile pool = sealed roster, no child processes.** Each home is an AEAD envelope
   (`codex-credential`, XChaCha20Poly1305, profile id as AAD) with ChatGPT
   OAuth material (access/refresh token, account_id, plan, proxy). The roster is `profiles.json` +
   `credentials/<id>.json`; symlink/different path/duplicate id are forbidden. Native HTTPS to
   `chatgpt.com/backend-api/codex` via a per-profile wreq client with its own proxy: no
   supervised children, pinned binaries, ownership locks or ownership transitions — blue-green
   generations freely overlap, because state lives in the roster, not in processes.
   The service floor is one working profile: a single subscription never becomes a 503 for lack of a
   spare. **Parallelism per home is NOT limited** (like the Claude fleet): the atomic
   in-flight counter (`TurnSlot` RAII) is only a load signal for selection, not a ceiling.
   **Selection is cache-first (like `affinity.rs`):** the conversation's preferred home → warm-up of the shared
   cache-root on two homes → least loaded; equal candidates alternate via an atomic
   cursor. After success the home is written back into affinity. Selection: quota-snapshot freshness →
   in-flight → remaining window (bucketed steering ≥50%) → cursor; hard exclusion ONLY on an
   explicit provider verdict (`limit_reached` / `allowed:false` / 429) — verified live:
   `usedPercent=100` with `allowed:true` still serves; steering away from nearly-full windows is the job of
   reserve caps. All homes over the limit → one OpenAI-shaped 429+Retry-After to the nearest reset.
   **Soft window reserve (like `pool::Reserve`):** never route above the `1−base` of the window (5h default 10%,
   weekly default 3%) with deterministic jitter by profile id; under peak the fail-open filter
   relaxes to the provider wall. Cache stickiness: tenant-scoped affinity derives stable
   opaque `prompt_cache_key` and session/thread/window UUIDs, so a conversation looks like one
   continuous session and never reveals the raw customer key.
1. **Only AEAD envelopes and the pinned official client identity.** `originator: codex_cli_rs`,
   UA `codex_cli_rs/<CODEX_CLI_VERSION> (…)`, `version`, `ChatGPT-Account-ID` from the envelope;
   a turn also carries first-party-shaped session/thread/window/turn metadata in headers and body.
   Tokens/account_id/proxy and the full email are decrypted only into memory and never reach a
   log/metric/response. Control-authenticated `/codex-subs` may receive only a bounded email hint
   (the first four characters of the local-part without the domain), reviewed paid-plan identity and nullable
   lifecycle fields derived from immutable credential `issued_at` with a fixed 30-day horizon; homes are still
   addressed by opaque id (no paths or identity in logs/metrics). The client version moves only by a
   reviewed commit after a live probe
   (`research/CODEX_NATIVE_WIRE.md`).
2. **Refresh — single-flight with durable rotation (critical, a difference from Gemini).** OpenAI rotates the
   refresh_token on EVERY refresh with strict reuse detection: the credential mutex serializes the
   expiry check and refresh (a 401 burst reuses the winner), the rotated envelope
   is atomically resealed (tmp+rename) BEFORE the lock is released. On `invalid_grant` —
   exactly one envelope reload from disk (a blue-green peer may have rotated earlier) and one retry.
   The first 401 on a turn → one force-refresh+retry of the same home before the first byte; a repeated 401 →
   auth quarantine per the health policy.
3. **Model-visible context = explicit client base/developer instructions + replayed Responses
   items + client tools.** The body is assembled by construction (`build_responses_body`): persona,
   environment/project/plugin/skill/permission context and built-in tools simply do not exist
   — the app-server patch boundary is now structural. `store:false`, stateless full input per
   turn; tenant continuity — the `prompt_cache_key` digest plus opaque
   session/thread/window UUIDs derived from it (never the raw customer key).
   Current Codex top-level `tools` and legacy `additional_tools` accept client-executed function,
   Lark custom, `namespace` and `tool_search` forms through one bounded parser; a namespace groups
   function AND custom children (Codex CLI 0.147 moved its Lark `exec` inside the `functions`
   namespace) and is rebuilt into the upstream body as a group — dropping it would leave the model
   with no callable tool at all. A custom/tool-search call is executed by the
   client, the gateway returns the raw call item and never executes it. Hosted `web_search` never
   becomes a free client tool: in a tool list it is accepted as an undeliverable declaration and
   dropped (Codex ships mode `cached` by default, so rejecting it made stock configs unusable),
   inside a namespace it still fails closed. It is never forwarded upstream in either case.
4. **Provider windows — from `/wham/usage` and live headers/SSE `codex.rate_limits`.**
   A snapshot is accepted only with real duration+reset; a stale one never rejects and never wins a
   tie-break; one that never arrived equals a fresh one. The `/wham/usage` schema and header names
   are pinned from a live probe (research/CODEX_NATIVE_WIRE.md, 2026-07-31). The sweep is selective:
   busy homes are fed by
   live traffic, healthy idle ones — at a slow floor cadence, stale/suspect/unprobed — every
   tick, all with bounded concurrency (the sweep itself must not become upstream load on the fleet).
   A failed turn wakes the sweep immediately (`probe_poke`, like Claude's `request_probe`).
   Calibration is fed ONLY by wire events (probe/turn headers): reads never write, routing never
   costs DB work.
5. **Retry only BEFORE the first byte:** the `emitted` flag — once a delta has gone to the client, a second
   attempt is forbidden. Fault classification: 429/usage-limit → the ACCOUNT's fault (cooling until reset,
   retry budget unspent), 401/403 → auth (refresh+retry once, then a soft backoff — see 5a),
   timeout/5xx/EOF → the TRANSPORT's fault (streak → degraded → wedged → client rebuild),
   400/context → the CLIENT's fault (no chilling, no retry). All homes over the limit → one
   OpenAI-shaped 429 with the nearest reset. Health — a pure policy in `health.rs` along two axes
   (account healthy→suspect→dead; transport responsive→degraded→wedged), durable account axis in the
   authority.
5a. **A home rests on provider verdicts, never on our own inference.** Cooling has two axes.
   `cooling_until` is hard and durable, and only a provider statement writes it: `UsageLimited` (the
   window is full) or an auth rejection that is `permanent` or corroborated by `AUTH_DEAD_STREAK`
   over `AUTH_DEAD_MIN_SECS`. `soft_cooling_until` is in-memory and holds an uncorroborated auth
   rejection, escalating from `AUTH_SOFT_COOL_SECS` up to the `AUTH_QUARANTINE_SECS` ceiling.
   `select_home` has two escape hatches in order: relax the soft window reserve, then relax soft
   cooling (`admission_ignoring_soft_cooling`). So a `Suspect` home is de-prioritised while healthier
   capacity exists and is still reachable when it is the last capacity left, which is what the
   `Suspect`-stays-routable design always intended — a blanket 900 s quarantine on every 401/403 had
   silently defeated it, and one rejection wave could rest the whole plane for a quarter hour. The
   Gemini plane learned this the hard way in August 2026; the Claude pool never had the defect and
   remains the strictest of the three (a live 401/403 there only calls `request_probe`).
6. **Window capacity calibration — native credits separately from API USD.** The decimal `used_percent` from
   `/wham/usage`/headers is parsed without `f64` into `10^-8` fraction units. Every successful turn before ingesting the
   quota snapshot builds one immutable dual-ledger event: effective Standard/Fast, model,
   provider tier, fresh/cache-read/cache-write/output/reasoning counters, exact API nanoUSD and exact
   ChatGPT nanocredits. Reasoning is already included in output, cached input is a subset of total input; they are never
   added twice. The stable internal `cal_*` request ID is created before home selection and lives
   through transport/home retries, but never goes upstream. Registry atomically advances both
   cumulative ledgers; exact retry is idempotent. For new unpinned conversations normal selection
   first seeds every healthy home without a single immutable turn; this is only a tie-break after
   Fast/freshness/in-flight and never overrides an already-resolved affinity.

   Failed events stay in a bounded FIFO (4096), are retried before new ones and are drained independently by
   every health sweep even without a new customer turn; retire performs a final flush after the entrance
   closes to new turns. After writer recovery, the exact events and both cumulative
   ledgers become durable first, and only then is the cached post-turn quota snapshot retried — the reverse order
   falsely turned real gateway spend into external spend. Pending/drop are visible in `/codex-subs`; overflow never stays
   silent. A permanent immutable replay conflict quarantines only one row and never blocks
   subsequent ones. Estimator v10 after the credit cutover starts a shared anchor for
   both units: `native cap = 100_000_000*ΣΔnanocredits/ΣΔfraction`, API cap stays the realized
   workload equivalent by `ΣΔnanoUSD`. Old API evidence is carried into `last_*` rather than being counted as
   zero credit spend. The first quota-only movement waits for ledger catch-up; a repeated movement without
   both ledgers is marked `possibly unattributed`, but never declared external usage.
   On a one-time replay of a new estimator version, a legacy API-only snapshot that erroneously
   appeared after the credit cutover stays in the raw authority but is skipped as incomplete: the next tracked
   cumulative snapshot safely covers that interval. A live tracked→untracked regression
   stays fail-closed.
   `10^-8` storage is not treated as the provider's measurement precision: the trailing-zero resolution of each
   endpoint (a whole percent = `1_000_000` units) feeds into low/high, and an interval no larger than the rounding
   uncertainty gets `high:null`. `/codex-subs.plan_cohorts` groups exact paid plan +
   duration and divides pooled native-credit evidence by pooled fraction movement, publishing one
   capacity per home and fleet remaining; individual estimates/evidence are never overwritten, API USD
   is never pooled. Low/high, samples, confidence and the missing-data reason are published explicitly. No
   prior/EMA/WLS/float money. Raw observations survive restart/blue-green/reset and recognize a rolling reset;
   each provider-reported duration is calibrated independently. Usage translation accepts both
   actually occurring aliases (`cache_write_tokens` and legacy `cache_creation_tokens`), preferring the
   current spelling and never adding them twice.
7. **Prices — only from `metering::codex`** (audited, effective-dated). For a successful ChatGPT-auth
   turn the effective tier is determined by the accepted request: `priority` = Fast, absence of a tier =
   Standard. The completed `response.service_tier` is kept only as provider-reported diagnostics:
   the official backend usually returns `default` even on a measurably faster Fast. Reserve holds the
   conservative Fast reserve; settle/ledger/capacity/public response use the effective tier.
   Public
   synthetic errors are only
   OpenAI-shaped and without pool/profile/upstream internals — gated by
   `api::tests::public_errors_never_leak_internal_architecture`.
8. **Shutdown:** detached streaming tasks join the shutdown barrier before history+settlement;
   the `TurnEvents` Drop aborts the upstream read, settling the last snapshot — before releasing the
   background permit. Full contract/provisioning/runbook — `docs/engine/CODEX_PROVIDER.md`.

**Native Gemini gateway invariants:**
1. Only AEAD envelopes of verified paid Code Assist OAuth identities. The normal sealed roster contains
   opaque ids and strictly `<roster>/credentials/<id>.json`; symlink/different path/duplicate Google
   subject are forbidden. The explicit calibration-only `systemd-flat` layout preserves the same closed
   path check as `<roster-parent>/gemini-credential_<id>.json`, matching systemd's recursive
   `LoadCredential` flattening; there is no fallback between layouts and no installed production
   unit selects it. `/gemini-subs` publishes only an
   opaque, path-independent BLAKE3 identity of the exact encrypted credential generation already loaded
   in memory, so a reviewed operator calibration can compare its read-only snapshot with a stable runtime
   without exposing credential material.
   The runtime re-verifies the official OAuth client/token endpoint, the exact plan↔tier-label mapping,
   the paid-plan allowlist and canonical proxy uniqueness (including equivalent percent encoding).
   Tokens/full email/domain/project/tier/proxy are decrypted only into memory and never reach a
   log/metric/response; the protected `/gemini-subs` receives only a pre-derived bounded hint of
   four characters of the local-part plus nullable lifecycle fields from immutable `issued_at`.
   `google_ai_pro` uses 18 UTC calendar months with month-end clamp; other canonical plans use 30 days.
1a. **Only Google declares a credential dead, and only with the word `invalid_grant`.** A refresh
   failure is classified by the response body, not the code: `400 invalid_grant` → `TokenError::Invalid`
   (the profile is removed from rotation), `401`/`403` → `TokenError::Blocked` (the grant is intact, the
   environment rejected it — proxy IP reputation or a client block; the profile stays authenticated and merely
   cools), anything else → `Temporary`. Previously all three collapsed into
   `Invalid`, and a live paid subscription permanently left capacity with a red "auth error" for a
   reason the token does not contain. The failure is logged as a bounded line `profile/http/error/verdict` —
   without token, proxy or Google's text.
1b. **The same rule binds the generation and probe planes, and cooling has two axes.** A `401`/`403`
   from generation or from the liveness probe arrives *after* a successful refresh, so it cannot mean
   the credential died: it is `mark_auth_blocked` — an exponential backoff from
   `auth_blocked_cool_secs` capped at `auth_quarantine_secs` that never clears `authenticated`.
   `mark_auth_failed` stays reserved for `TokenError::Invalid`. Cooling is therefore split:
   `cooling_until` is the **soft, environment-derived** axis (auth 401/403, transport, blocked probes)
   and `quota_cooling_until` plus the per-model axis are the **hard, quota-derived** ones. Only a hard
   axis may deny a request outright — when the normal selection is empty, the data path re-selects with
   `select_routed_ignoring_env_cooling`, so a subscription rests on real provider limits and never on
   our inference about the environment. An exhausted selection also calls
   `request_probe_rate_limited`, bounding recovery by the probe rather than the background cadence.
   This is not theory: for two days in August 2026 the whole pool went to zero capacity nine times for
   105–300 s each, because one 401/403 wave walked the roster inside the retry loop and only the next
   scheduled sweep resurrected the — entirely healthy — profiles.
2. `GeminiGateway` serves only the startup-fixed `ProviderMode::Gemini`. Native allowlist:
   models get/list, generateContent, streamGenerateContent, countTokens. The client's `x-goog-api-key`
   (like x-api-key/Bearer) authorizes our key, but never goes to Google; the query `key`/`api_key`,
   including percent encoding, is forbidden.
3. Production HTTPS belongs to a persistent per-profile Node helper: the exact pinned
   `/usr/bin/node` v24.18.0 Linux/x64 + SHA-256, native OpenSSL, HTTP/1.1 and authenticated CONNECT.
   New profiles usually use the live-verified Antigravity 2.2.1 UA,
   `Go-http-client/2.0` refresh and reviewed bounded Antigravity
   `Client-Metadata`/`x-goog-api-client`; caller values are stripped. The published route
   `gemini-3-flash-preview` uses the same 2.2.1 UA, but without the old IDE
   `Client-Metadata`/`x-goog-api-client`: exactly this minimal tuple yielded owned generation 2xx on the
   private wire `gemini-3-flash`. The fresh exact-implementation gate of 2026-08-03 completed all 22
   Pro+Ultra turns: minimal/low/medium/high, incremental SSE, profile-local cache
   `write → prime → read`, fresh/replayed exact PCM WAV and forced tools. The PCM fallback applies the
   official exact rate of 32 tokens/second only at durations divisible by 1/32 of a second, and an
   ambiguous partial-cache split stays fail-closed. The model is in the production/public
   allowlist; the remaining models and background quota/health keep the full live-verified tuple.
   Old Gemini CLI credentials keep the prior wire until migration.
   OAuth userinfo uses a separate global-fetch/Undici profile of the same SHA-pinned Node. No
   approximate BoringSSL impersonation or ambient proxy/env.
   Antigravity text, including the published `gemini-3-flash-preview`, keeps the live-verified configured
   endpoint; the owned private-wire probe passed on the production-configured sandbox daily origin. Image
   generation always goes to the production daily origin `daily-cloudcode-pa.googleapis.com`, matching
   current first-party Antigravity CLI 1.1.10's separate `generate_image` call. Legacy Gemini CLI OAuth stays
   on `cloudcode-pa.googleapis.com`; literal loopback mocks are never redirected.
   The helper receives the proxy secret only in the first IPC frame, multiplexes bounded NDJSON, reaps the process
   group and may restart only before upstream headers. Outbound frames, inbound NDJSON/base64
   staging, OAuth response collections and short-lived header/form strings are zeroized. Loopback mocks
   stay on `wreq`. The helper separately classifies target `timeout`/`tls`/`network` and the bounded
   CONNECT reasons `proxy_timeout`, `proxy_auth`, `proxy_throttle`, `proxy_rejected`,
   `proxy_upstream`, `proxy_connect`, `proxy_eof`, `proxy_protocol`; the runtime folds all proxy/TLS
   classes into the existing network policy, never treating them as IPC protocol corruption and never
   exposing status/header/credentials.
4. A profile owns its separate transport/proxy/inflight/cooling/auth and single-flight token refresh.
   First 401 → one refresh+retry of the same profile; a repeated 401/403 → soft auth backoff per 1b,
    never de-authentication. 429 →
    model-specific profile cooling by Retry-After/RetryInfo/quota reset and rotation without
    transport budget. When a generation 429 carries NO authoritative retry hint and the profile's
    own fresh quota catalogue still reports a positive remainder for the model
    (`quota_reports_remaining`), the verdict is an RPM/concurrency stall, not exhaustion: the model
    is parked only for the short `rate_limit_rpm_cool_secs`, never the full
    `default_rate_limit_cool_secs` window — a momentary throttle must not freeze the model across
    the whole fleet for a minute. When a generation 429 DOES carry a retry hint, the hint is only
    honoured in full for a genuine `QUOTA_EXHAUSTED` error reason; any other hinted 429 (observed
    on Antigravity image generation, where a long `RetryInfo` accompanied a near-full catalogue and
    the account stayed usable via the official client) is a transient profile stall, so its hint is
    capped at `rate_limit_unknown_cool_secs` and the profile is re-probed instead of parked for
    many minutes. Fail-closed: a missing/stale catalogue, no matching bucket or any
    non-positive/unknown remainder keeps the long exhaustion cool. Generation cooling deadlines
    are also persisted to a small JSON state file (`CLAUDE_API_GEMINI_COOLDOWN_STATE_FILE`) in the writable data directory and restored on startup, so a
    blue-green slot swap does not forget a genuine exhaustion cooldown and burn a live customer
    request rediscovering it; only still-active deadlines are restored and a write failure is
    logged, never routed on. A health probe never erases
    generation cooling. Every generation/probe 429
   emits only bounded machine evidence under `gemini-rate-limit`: one request id joins pre-byte
   rotation attempts and a terminal summary, while process-keyed fingerprints correlate an
   identical provider shape within one process lifetime without logging Google prose, metadata
   values, project/email, token, proxy or customer content. Diagnostics never alter classification,
   retry or cooling. Antigravity
   `fetchAvailableModels` publishes a sanitized model catalogue: explicit zero blocks a model until
   reset, a stale/missing bucket fails open. Legacy profiles continue `retrieveUserQuota`.
   Network/token refresh/408/409/425 → short global-profile cooling. Generation 5xx/malformed
   response → exponential model-specific cooling and bounded retry, so one model never disables
   the profile's other models; other 4xx never rotate.
   If there were quota failures — the outcome is 429; only auth/transport failures — 503; an already-cooling pool — 429.
5. The Code Assist request wrapper is built by the server; the caller cannot inject project/session identity.
   For Antigravity text generation `request.sessionId` is a UUID from the keyed tenant-scoped affinity
   lineage, and the top-level `requestId=agent-<uuid>` is created once before rotation; the wrapper also
   pins `userAgent=antigravity` and `requestType=agent`. Image generation keeps only
   public affinity, but the private wire must be stateless: no `request.sessionId`, with
   `requestType=image_gen`, `requestId=image_gen/<unix-ms>/<uuid>/12`, `candidateCount=1` and
   `responseModalities=[TEXT,IMAGE]`. The resolution allowlist of the private subscription surface — only
   the live-verified `1K`/`2K`/`4K`; the Developer API-only `0.5K` fails closed until separate live evidence.
   Legacy profiles keep `request.session_id` and
   `user_prompt_id=<session UUID>########<human-turn ordinal>`.
   Public Gemini allows an empty/omitted `contents[].role`; for the strict private Antigravity
   wire the wrapper derives only such roles by alternating `user`/`model`, never rewriting explicit values.
   Native tool replay preserves any client-supplied opaque `thoughtSignature`; if the client drops
   it before sending the matching `functionResponse` (Kimi Code 0.33 does this), the private wrapper
   injects the same accepted stateless context-engineering marker used by the universal adapters.
   The compatibility marker is private-wire-only and never creates server-side signature state.
   The public model ceiling 65,536 is preserved, but Antigravity wire `maxOutputTokens` is limited to 65,535.
   The canonical Gemini 3 model id is separate from the private effort/quota id: published support for
   3 Flash Preview maps public `gemini-3-flash-preview` to the live-verified private wire
   `gemini-3-flash`, and quota admission conservatively links both observed strings
   `gemini-3-flash`/`gemini-3-flash-agent` until exact debit attribution. The production allowlist publishes the
   route after a full GREEN Pro+Ultra gate. The fallback accepts only inline PCM WAV with an
   integral `duration × 32`, preserves any
   provider `promptTokensDetails[AUDIO]` as authority and reconstructs a missing split only under
   provable cache separation (`cached=0`, `cached=prompt` or an explicit cached AUDIO). 3.6 Flash selects
   `gemini-3.6-flash-{low,medium,high}`, 3.1 Pro Preview —
   `gemini-3.1-pro-low`/`gemini-pro-agent`.
    Gemini 3.7 Flash is published under the sole client identity `gemini-3.7-flash`, mapped privately
    to the live-admitted `gemini-3.7-flash-tiered` row. The 2026-08-15 exact-SHA gate proved real
    output, terminal usage, incremental SSE and authoritative settlement; ordinary JSON/SSE always
    rewrites the private alias to the public id. Discovery advertises the text+image+audio+video+pdf
    surface with
    explicit thinking levels `low`/`medium`/`high` (live-admitted per level on `916dee0d…`,
    `$0.00546` aggregate; `minimal` is rejected locally), function calling, JSON structured output
    and implicit prompt caching (capability matrix on `35153abe…`, `$0.0190` aggregate, 8,170
    authoritative cached tokens), plus Google Search grounding (one-query admission on
    `ed63dc0f…` under the explicit 10-query reserve contract, `$0.014921` reconciled), and inline
    audio (WAV)/video (MP4)/PDF inputs (media matrix on `fc556402…`, `$0.00119025` aggregate,
    mandatory content-perception markers). The
   retained generic evidence producer admits only an admin exact
   profile plus canonical UUIDv4 `x-apitoken-calibration-request-id` and positive Unix-seconds
   `x-apitoken-calibration-not-after`; the half-open fence is `now < not_after`, so equality expires.
   Rust rechecks after token acquisition and Node carries the deadline only in IPC, checks every
   pre-connect/TLS boundary, and emits `x-apitoken-calibration-dispatch-ms` only from the final
   pinned ClientRequest `socket` event. SHA-pinned Node v24.18.0 `_http_client` synchronously emits
   that event before `_flush`; a listener-side destroy therefore writes no HTTP bytes. Exact
   `countTokens` may refresh once with `NeverReplay`; paid generation requires an already-fresh
   cached bearer. Every deadline-bound upstream POST is one-shot, with no helper restart, 401
   resend, profile rotation or smooth retry. The retired root trigger/consumer remains absent and
   no permanent canary/helper is installed; ordinary customer traffic does not require calibration
   headers and uses the normal retry/reserve/settlement path.
   Its thinking allowlist is exactly
   `low|medium|high` (omission
   means medium); `minimal`, `thinkingBudget`, `candidateCount`, deprecated sampling controls and
   any final turn without non-empty user text fail locally. A final user turn whose parts are
   exclusively `functionResponse` objects — the exact transcript every portable tool-calling
   client (OpenCode and other AI SDKs) produces after executing a call — is admitted on every
   lane: the closed `--gemini-37-tool-result-final-turn` live gate proved its wire contract
   (run gemini-cal-1787152582-af5e9cfb, request fa042530-3c7d-4aff-b795-ea1c3b2b0122: upstream
   `gemini-3.7-flash-tiered` returned incremental SSE with visible text, terminal `STOP` and
   authoritative terminal usage). Image-only final turns and prefilled model turns remain
   fail-closed on every lane. After the withdrawn 256-token
   canary spent 241 of 252 output tokens on thinking and terminated at `MAX_TOKENS`; the successful
   successor evidence used `maxOutputTokens=512`. Public requests use the standard bounded output
   control. Any different wire/quota identity
   requires fresh authenticated discovery plus exact-SHA generation evidence, never a guess from
   neighboring model families.
   The thinking level is selected before admission; quota/cooling are keyed by the private bucket, while affinity,
   billing and the client catalog use the canonical public id. Ordinary and established exact
   response/SSE paths rewrite private `modelVersion` back to the public id; the deadline-bound
   exact 3.7 evidence lane alone preserves raw upstream `modelVersion` as admission evidence. All paths
   return only `.response` (+ responseId), never the wrapper/credits/private upstream headers.
   An official CountTokensRequest `generateContentRequest` is expanded into a private request, the body model
   is replaced by the route model; ambiguous top-level contents + nested request is rejected. Unsupported
   `serviceTier`/`store` fail closed instead of a silent drop.
   Retry is allowed only before the first translated native SSE event. Stream startup is bounded by
   time/bytes/chunks, and after the first public event the number of consecutive private/accounting
   events is limited. After the Response is returned, a client disconnect detaches downstream delivery, but the task
   continues draining until the final usageMetadata. The shutdown deadline must abort the upstream read,
   settle the last snapshot and only then release the background task guard for the
   subsequent billing flush.
   Per-profile in-flight has no ceiling and serves only as a balancing signal. A resolved
   conversation affinity is a hard first choice under any local load; unbound fan-out immediately
   spreads across the least-loaded eligible profiles. A new shared
   system/tools cache-root first warms two competing profiles, then prefers the warm
   copy. Unbound routing puts fresh quota evidence ahead of stale, then inflight, coarse quota
   steering only above 50% used and a rotating cursor: exact fractions never herd a burst onto one
   account. The separate scheduler-facing batch selector layers the same hard model/profile eligibility
   with a fresh exact `gemini-5h` summary whose fixed-point remaining fraction must be strictly above
   `batch_5h_headroom_percent`; missing, stale or exact-threshold evidence returns the bounded
   `batch_5h_headroom_stop` reason without customer identity. Interactive selectors remain unchanged.
   The deterministic soft reserve/jitter is preserved; if all eligible profiles are below the reserve,
   the service floor fails open to explicit zero. Local saturation never becomes a public
   error; native RetryInfo stays only for real provider quota/cooling.
   `/gemini-subs` separates quota presence from generation health via the failure streak and last
   success/failure evidence, returns reviewed paid-plan identity and a bounded email hint (the first four
   characters of the local-part without the domain), but never Google subject/full email/project or private tier.
6. Reserve/mark-delivering/settle are durable; before upstream, `maxOutputTokens` is clamped to the full
   conservative hold of the available balance. Price only from `metering::gemini`, ledger provider only
   `registry::PROVIDER_GOOGLE`. Search is metered separately. Google Maps/File Search and unknown future
   server tools fail closed until authoritative ledger dimensions appear; never proxy a paid SKU
   for free. On published models inline audio fails closed to upstream for both generation and
   `countTokens`: the current Antigravity terminal usage folds it into generic prompt tokens
   without an authoritative `promptTokensDetails[AUDIO]`, and the aggregate result of the free token counter never
   allows honestly reconstructing the more expensive audio split. An image response with explicit
   `candidatesTokensDetails[IMAGE]` uses the provider split;
   if private Antigravity returns only aggregate candidates, the actually delivered `inlineData`
   is allocated the official fixed token SKU of the requested size, and the remainder stays text/thinking. A refusal
   without an image gets no media charge. The published Flash Preview audio fallback never guesses
   fractional duration, compressed/file audio or partial cache: such requests/usage are rejected,
   and the reconstructed exact AUDIO row lands both in the public usageMetadata and in the same Rust metering.
   A metered non-stream without authoritative usage is never delivered and is refunded; a stream after the first
   byte without final usage debits the conservative hold without a fake usage event. Public synthetic errors
   are only native Google-shaped and without profile/project/key/upstream.
7. Gemini capacity is never derived from the subscription's price or a daily request count. Antigravity
   `retrieveUserQuotaSummary` is accepted only for exact `gemini-5h`/`gemini-weekly`; `3p-*`
   are excluded. Every successful generation with terminal usage (billed or admin) builds an immutable
   provider event: internal request id, opaque profile, exact paid plan/model/tariff, all token/tool/
   search facts and disjoint official API nanoUSD legs. The event and cumulative subject spend are written
   atomically; missing usage creates no evidence. Delivery — a separate bounded FIFO 4096 with a retained
   transient head, immutable replay, one-row conflict quarantine, poll-before-observation flush,
   pending/drop/persistence diagnostics and shutdown drain. After a successful immutable
   event enqueue for an admin-only exact-target turn, the gateway immediately wakes the
   free quota/health probe with a coalesced `Notify` signal; ordinary customer traffic never changes the
   background cadence and never creates additional provider probes.
   The exact window authority is keyed by `profile + plan + bucket + duration`; legacy rows without a plan are never
   migrated. Windows are independent, the provider fraction is stored fixed-point `10^-8` together with the real
   lexical decimal resolution. A cold snapshot is an anchor, and the first complete positive-spend interval
   immediately publishes the realized blend `SCALE*ΣΔspend/ΣΔused`. Low/high account for the resolution of both
   endpoints; high stays `null` if the movement does not exceed the uncertainty. Quota may wait one
   snapshot for settlement lag; a repeated quota-only movement becomes unattributed. Reset/rolling
   rollover/jitter, overflow and estimator rebuild from immutable history fail closed. No prior/EMA/WLS/
   nominal/float money. Admin-only exact targeting accepts the full opaque profile id and an
   optional canonical `x-apitoken-calibration-request-id`; metered traffic can set neither the
   profile nor the immutable-event identity, and a target never spills/rebinds. The additional canonical
   `x-apitoken-calibration-not-after` input and `x-apitoken-calibration-dispatch-ms` response are
   confined to the exact 3.7 evidence producer described above.
8. Full contract/provisioning/runbook — `docs/engine/GEMINI_PROVIDER.md`. Verification includes the mock upstream:
   rotation fault matrix, credential stripping, RetryInfo, chunk-split SSE, no post-byte retry,
   disconnect drain+settlement and the shutdown deadline barrier.


**KIMI backend preview runtime — default-off, no public catalog:**
`kimi_calibration.rs` contains a pure estimator of one Kimi Code subscription window, and `kimi/` —
a strict loader of the encrypted roster, last-good atomic reload, refresh/quota client,
unlimited-parallel selector, provider fault classification, one-byte attempt policy, bounded turn
FIFO and an exact Anthropic Messages gateway. Contract and facts — `docs/engine/KIMI_PROVIDER.md`, schema — migration
`0027_kimi_window_calibration.sql`, types and PostgreSQL authority —
`registry::kimi_calibration`/`PgStore::{record_kimi_turn,save_kimi_calibration}`.

Two differences from Claude/Codex that must not be "unified":

1. **Quota arrives as whole `used`/`limit`, not a fraction.** Both raw numbers are the authority; the fraction and
   `measurement_resolution` are derived from them (`registry::kimi_fraction_from_native`), so
   the real `limit` sets the resolution: with `limit=1000` it is 0.1 % versus Claude's whole percent.
   A narrow envelope allows proving a finite high where a whole percent yields only `null`.
2. **The native window capacity needs no estimation — it is published.** `limit` is the window in native
   units, `limit − used` — the exact native remainder. There is NO per-turn native ledger: the provider
   returns native spend only as a window aggregate, and synthesizing it by dividing API dollars by a token
   price is forbidden. Exactly one thing is estimated — how much official API replacement cost fits in the
   window: `capacity_nano = 100_000_000 × ΣΔspend_nano / ΣΔused_fraction_units`.

Row identity — `subject + exact paid plan + exact native duration in seconds`. The 5h and 7d windows are
independent; an observation of a foreign duration is rejected, not folded into a neighbor. The duration
arrives from the provider dynamically (the 5-hour window is `duration:300, TIME_UNIT_MINUTE`), so an
unknown time unit fails closed: a wrong duration would merge two independent windows into one.
The first snapshot is an anchor; a quota-only movement waits once for settlement, a repeat becomes
`unattributed`; a rollback to an old high-water is not new spend; an estimator version change
rebuilds state from immutable history. No prior/EMA/WLS/float money.

The server composes the gateway only under a strict default-off enable. Exact KIMI aliases inside
Anthropic `/v1/messages` go through `/me` readiness, sticky unlimited-parallel selection,
pre-byte-only rotation, transparent non-stream/SSE, disconnect drain and
reserve→delivering→settlement→turn-FIFO. A cold/broken roster stays a separate KIMI fail-closed
path and never falls through to Claude.

`state.rs` also carries a dedicated `ProviderMode::Kimi` for the production delivery plane:
`serves_kimi()` is true for `Combined|Anthropic|Kimi`, so gateway composition in
`server::config` covers both the embedded dev/test backend and a separate process. In `Kimi` mode the
shared `/v1/messages` path dispatches exact aliases through the same `KimiGateway::handle`, and right after
that block stands a fail-closed gate: any non-KIMI model gets a bounded static 404 and never
reaches the Claude pool (the plane never brings it up). Gateway readiness
(`live>=1 && persistence_ok`) is published as `provider_unavailable` on `/ready` only when the
gateway is composed; without it the slot serves the disabled envelope and stays ready.

Pool parity with the other planes: every upstream client carries the pinned
`kimi_credential::KIMI_CODE_CLI_USER_AGENT` (the endpoint identifies the official CLI by this
string). A 2xx that stalled without a first byte (stalled stream start) and a failed non-stream
body read are pre-byte transport faults with rotation through the same `decide(...)`, not a terminal response
with an instantly re-picked profile. A `Retry-After` on 403/429 (bounded to one hour) cools the
quota/transport axis exactly until the hinted moment; a missing or garbage hint leaves the
default cooldown.

Roster discovery runs every 15 seconds. An unchanged profile must reuse the same
runtime `Arc`; a new/changed credential passes `/me` before the whole-generation swap. Any
read/decrypt/client/probe error and a vanished file preserve last-good. Intentional removal — only
a valid empty roster; an old in-flight lease lives out on its `Arc`. Before publication, the affected
refresh locks and a re-read prevent a snapshot that went stale during a rotating reseal from replacing
the new credential family.

The `/usages` poll runs only for an idle profile without a customer semaphore. The monotonic turn epoch
invalidates the whole HTTP snapshot if a generation started during the GET; an already-received snapshot
is protected by `turn_drain`, so a newer finalizer cannot add spend before observation. Before
HTTP and again before the writer command, the bounded turn FIFO must drain completely. The serial
PostgreSQL writer itself reads cumulative subject spend, writes immutable observations of independent
windows and applies the estimator CAS; runtime quota/tightest-window steering changes only after the
success of all windows. A transient head/DB/CAS/parser failure preserves last-good quota, exact replay is a
no-op, a poisoned request-id quarantines only one turn. Shutdown cancels the steady poll, repeats
the same turn-before-quota order after the stream barrier and bounds the final provider read by the
shared deadline. The final pass never starts a rotating OAuth refresh: the indivisible refresh/reseal
remains a steady-state-only operation. There is no public catalog and no router namespace.

`KimiGateway::operational_status` publishes an extended `KimiOperationalStatus` for readiness,
`/metrics` and admin-only `GET /kimi-subs`: fleet counts (total/live/available, three cooling axes,
total inflight), per-profile `KimiProfileStatus` and the bounded FIFO `DeliveryHealth`. Selection
availability reuses `selection::ineligible_ids` over `kimi-for-coding` (every plan serves
it, so the capability gap never lands here). `publish_quota` keeps the last
full `/usages` snapshot per profile (`KimiQuotaWindowStatus`: exact used/limit, fraction and
the real measurement resolution, resets_at/observed_at); the unknown is absent, not
filled with zero. The plan label is bounded by `bounded_plan_label`: exact static name only for
`KIMI_REVIEWED_PLANS`, otherwise the placeholder `"unreviewed"` — a raw provider string never
gets out. Privacy by construction: the subject stays private in `RuntimeProfile`; durable
calibration rows (`AsyncBilling::kimi_calibration_report` over the PostgreSQL-only
`PgStore::list_kimi_calibrations`, an empty report on the SQLite authority) are joined to the opaque id
only through `profile_id_for_subject`, and a foreign subject is never serialized outward.

Exact live-runner attribution: dispatch accepts the admin-only pair
`x-apitoken-calibration-{profile,request-id}` (validation `kimi_credential::validate_profile_id` +
UUIDv4), never proxied outward. The pinned turn works exactly on the given profile
(cooling/wall is a wall, not a reason to rebind) and writes the durable turn event under the passed immutable
request id; `AsyncBilling::kimi_recent_turns` provides a bounded newest-first read for attribution
(PostgreSQL-only, empty on the SQLite authority).

Affinity at the level of the other planes: a new conversation gets a soft warm-home preference at
attempt 0 (a warm cache root matters only among equal candidates), the home is registered by an early claim
BEFORE the first attempt (two concurrent first turns of one conversation cannot double-home), and the cooling
deadline is published to siblings via `publish_cooling_hint`. A per-model failure axis: two consecutive
failures of one model (stalled stream start, broken 2xx body) cool exactly it for 60 seconds on this
profile (`Ineligible::ModelCooling`), the profile's other models stay eligible; a model's success
clears exactly its own axis. A barriered burst test proves the absence of an admission semaphore: all N
simultaneous requests start upstream attempts before the first response.

**GLM backend preview runtime — default-off, no public catalog:**
`glm_calibration.rs` contains a pure dual-ledger estimator of one GLM Coding
Plan subscription window (Zhipu AI / Z.ai), and `glm/` — a strict loader of the encrypted roster,
last-good atomic reload, quota client, unlimited-parallel selector, two-layer (HTTP + business code) provider fault
classification, one-byte attempt policy without auth-retry, bounded turn FIFO and an exact Anthropic
Messages gateway. Contract and facts — `docs/engine/GLM_PROVIDER.md`, schema — migration
`0029_glm_window_calibration.sql`, types and PostgreSQL authority —
`registry::glm_calibration`/`PgStore::{record_glm_turn,save_glm_calibration}`.

Three differences from KIMI that must not be "unified":

1. **The credential is a static API key; there is no refresh family at all.** Neither single-flight refresh,
   nor reseal-on-rotation, nor same-profile retry after a 401: a business-code 401 (including the
   quota endpoint's trap — HTTP 200 with `code: 401` in the body) or an expired plan (1309) means
   durable `account_dead` until an atomic key republication by the Auth Bot. Reload therefore never
   holds refresh locks; the snapshot race with a peer blue-green generation is closed by a final
   roster re-read before publishing the generation.
2. **Every turn is metered twice and independently.** Official API replacement cost (nanoUSD,
   the Open Platform rate card) and native credits (microcredits from the published multiplier
   formula with off-peak ×0.5 on the UTC+8 schedule at completion time) — two disjoint
   ledgers on one immutable event; one is never reconstructed from the other, and both
   schedule ids live in the event. The provider has no cache-write money leg ("Limited-time
   Free"): a write is paid at the miss rate and folds into the fresh-input leg in the event, so
   the three disjoint legs reconcile into the total.
3. **Errors are two-layered: the business code in the body beats the HTTP class.** Classification reads the
   bounded error body before `decide` (a 429 with code 1308 is a quota wall, not a rate limit);
   the exact reset for cooling is parsed from the body of 1308/1310, otherwise a bounded fallback until the next
   idle poll. The quota probe authenticates with the raw key WITHOUT a Bearer prefix; generation —
   Bearer + the full Claude Code fingerprint from shared fleet env (Z.ai risk control bans
   SDK-like traffic): UA (per-profile pin from the `|` pool, like `persona_ua`), anthropic-version,
   the full 10-beta anthropic-beta, x-app + the entire x-stainless-* set, accept,
   `anthropic-dangerous-direct-browser-access`, identity as the first system block (no double
   injection, the semantics of `inject_identity`) and the per-profile billing block `x-anthropic-billing-header:
   cc_version=<base>.dNN; cc_entrypoint=…; cch=<hex>` (cch and .dNN are deterministic by roster id —
   `persona_cch`/`persona_ccbuild` are reused). Client identity headers never reach the gateway
   at all (structurally: `GlmRequest` does not carry them) — only the reviewed persona is synthesized.
   The units of the quota endpoint's counters are unproven live, so
   raw counters and the derived fraction are stored optional: unknown — `None`, never `0`.

Row identity — `subject (keyed-BLAKE3 digest of the key) + declared paid plan (Lite/Pro/Max,
the capitalized cohort form; the lowercase serde spelling of the envelope is mapped at adopt) + exact
native duration (5h rolling/7d weekly)`. The served model decides the money: the requested one is stored
separately, and a served model outside the allowed set (no dollar card OR no published credit
multipliers, like the echo ids glm-5/glm-5.1) — billing fails closed: conservative hold,
typed operational counter, no immutable event created. Missing terminal usage after delivery —
the same documented hold with its own counter; synthetic usage is never created. Tools/web
search/MCP/vision/highspeed are `unavailable` in v1 and fail closed before reserve.

Dispatch of exact reviewed aliases (`glm-5.2`, `glm-5.2[1m]`, `glm-5-turbo`, `glm-4.7`) inside
Anthropic `/v1/messages` — after shared authorization and the bounded body, before the Claude-specific
identity/pricing/pool (`AppState.glm`); a disabled plane, cold or broken roster yields the
fail-closed `glm_gateway_unavailable`/`glm_capacity_exhausted`, never a fallback to Claude.
Quota preflight quarantines only the profile with a dead key, not the whole gateway. Roster
discovery runs every 15 seconds: an unchanged profile reuses the same `Arc`, a new/
changed credential passes a free quota probe BEFORE the whole-generation swap, any
read/decrypt/client/probe error and a vanished file preserve last-good, a valid empty roster is the
only way to bring down the fleet; an old in-flight lease lives out on its `Arc`. The quota poll —
the same turn-before-quota order as KIMI: drain FIFO → GET → epoch re-check → a second drain
under the FIFO barrier → the serial writer itself reads the cumulative DUAL spend → immutable observation +
estimator CAS per independent window → steering published only after the durable success of all
windows; a transient failure preserves last-good quota.

`GlmGateway::operational_status` publishes a `GlmOperationalStatus` for readiness, `/metrics` and
admin-only `GET /glm-subs`: fleet counts (total/live/available, the durable
account-dead/account-suspect axes, two timed cooling axes, total inflight), the operational counters
missing-terminal-usage/served-model-rejected, per-profile `GlmProfileStatus` and the bounded FIFO
`DeliveryHealth`. Selection availability reuses `selection::ineligible_ids` over
`glm-5.2` (every reviewed plan serves it, so the capability gap never lands here).
`publish_quota` keeps the last full quota snapshot per profile (`GlmQuotaWindowStatus`:
raw used/limit/remaining optional while unit semantics are unproven, fraction and measurement
resolution alongside, resets_at/observed_at); the unknown is absent, not filled with zero. The plan
label is bounded by `bounded_plan_label` (the roster holds only three reviewed individual
plans anyway). Privacy by construction: the subject (keyed digest of the key) stays private in
`RuntimeProfile`; durable calibration rows (`AsyncBilling::glm_calibration_report` over the
PostgreSQL-only `PgStore::list_glm_calibrations`, an empty report on the SQLite authority) are joined
to the opaque id only through `profile_id_for_subject`, and a foreign subject is never serialized outward.

**Tripo3D backend preview runtime — default-off, dedicated plane, no public catalog:**
`tripo3d_calibration.rs` holds the windowless balance-track estimator (migration 0049), and
`tripo3d/` — a strict loader of the encrypted roster, last-good atomic reload, the balance/task
wire, hard/soft selection, a create-bounded attempt loop, the bounded paired-turn FIFO, the
artifact store, the upload transport and the task-lifecycle gateway. Contract and facts —
`docs/engine/TRIPO3D_PROVIDER.md`, schema — migration `0049_tripo3d_calibration.sql`, types and
PostgreSQL authority — `registry::tripo3d_calibration`/`PgStore::{record_tripo3d_turn,save_tripo3d_calibration}`.

The differences from KIMI/GLM that must not be "unified":

1. **Task-based media API, not a chat wire.** The lifecycle is create task → detached poll (2 s
   cadence, bounded deadline) → immediate artifact download into `ARTIFACT_DIR` (result URLs
   live ≤60 s) → exact settle. The money boundary is a successful upstream create
   (`code: 0 + task_id`), not a first public byte; rotation is expressible only before it, and
   per-key task isolation pins the whole drain to the creating profile. A client disconnect
   never cancels the drain. There is no streaming, no Anthropic envelope and no fleet persona:
   requests authenticate with `Bearer tsk_…` only.
2. **Calibration pairs like Codex/Gemini, not KIMI/GLM.** The FIFO head is the immutable turn
   event PLUS the post-turn balance read taken in the task's wake; one writer command records
   the turn, reads the cumulative dual ledgers already including it, and persists the
   observation (`response` source naming the request id) + estimator CAS. The free periodic
   balance poll still runs the turn-before-quota gate and never reads with a pending head.
3. **Hard/soft cooling per manifest §4.1.** Hard axis = provider verdicts only: 429+`2000` with
   `Retry-After` (exact timed cool), 403+`2010` insufficient balance (rest until a balance probe
   shows funds), a proven balance shortfall against the reserve. Soft axis = 401 (static key,
   no refresh — soft quarantine with exponential backoff from 15 s, reset on proven success),
   transport faults, failed probes. The strict pass honors both; `selection::select_ignoring_soft`
   is the second pass bounded by the tried set; a full-hard fleet answers an honest 429 with
   `Retry-After`. Unknown business codes fail closed onto the HTTP class; a 2xx without
   `code: 0` is a protocol anomaly that never rotates (a possible double-create).
4. **Money: reserve = exact published price or the documented family-max conservative reserve**
   (`metering::tripo3d::tripo3d_reserve_credits`); settlement is exactly the authoritative
   `consumed_credit` (millicredits, float-free parse, ≤3 decimals). Above the reserve bound →
   typed anomaly, conservative hold, no event. Missing `consumed_credit` on success → hold +
   counter. Documented `failed`/`expired` → zero settle + refund + legal zero-pair event.
   Undocumented `banned`/`cancelled`/`unknown` finals (manifest §6.5) → conservative hold +
   counter, no fabricated consumption.
5. **Attachments are account-scoped.** The upload endpoints (`upload/sts` multipart images
   ≤20 MB → `image_token`; `upload/sts/token` + SigV4 S3 PUT model files ≤64 MiB → plane
   `t3m-` token) pin the token to the uploading profile; task creation with a pinned token is
   sticky to that profile because another key cannot see the upload. The pin map is bounded,
   TTL'd, in-memory and never money state.

`Tripo3dGateway::operational_status` publishes a `Tripo3dOperationalStatus` for readiness,
`/metrics` and admin-only `GET /tripo3d-subs`: fleet counts, per-profile hard/soft cooling with
resets, raw balance halves (parsed halves `None` while the unit is unproven), the typed anomaly
counters, tracked-task count and the FIFO `DeliveryHealth`. Privacy by construction: the
subject (keyed digest) stays private in `RuntimeProfile`; durable calibration rows
(`AsyncBilling::tripo3d_calibration_report`, PostgreSQL-only) join to the opaque id only
through `profile_id_for_subject`, and the read model (`Tripo3dTaskView`) never carries the
upstream task id or signed URLs.

**Suno backend preview runtime — default-off, dedicated plane, no public catalog:**
`suno_calibration.rs` holds the monthly-window estimator (migration 0050), and `suno/` — a
strict loader of the encrypted roster (the Clerk session id is the dedup identity), last-good
atomic reload, the Clerk/billing/generation wire, hard/soft selection, a create-bounded attempt
loop, the bounded paired-turn FIFO, the artifact store, the customer upload intake and the
generation-lifecycle gateway. Contract and facts — `docs/engine/SUNO_PROVIDER.md`, schema —
migration `0050_suno_window_calibration.sql`, types and PostgreSQL authority —
`registry::suno_calibration`/`PgStore::{record_suno_turn,save_suno_calibration}`.

The differences from Tripo3D that must not be "unified":

1. **The credential is a session, not a static key.** Short-lived JWTs are minted on demand
   through the profile's pinned egress and never persisted (30 s in-memory reuse, the TTL is an
   open unknown). A mint — and any business-host call — may rotate the Clerk `__client`
   material via `set-cookie`, so `session.rs` holds a per-profile single-flight from mint/merge
   through envelope re-seal: the winner merges the rotation and atomically re-seals the roster
   credential file (tmp → fsync → 0600 → rename → dir fsync) BEFORE releasing the lock; the
   loser re-reads. A failed re-seal keeps serving the last durably sealed material.
2. **No documented business codes and no per-turn consumption field.** Classification is
   conservative HTTP-only (manifest §4.1): 401/403 post-mint is SOFT auth (streak+elapsed
   corroboration before removal from routable), 429 is the HARD rate wall, 5xx/timeouts are
   bounded transport. `QuotaExhausted` and `CaptchaRequired` are never status-classified: only
   the billing probe zeroing (hard) and the `/api/c/check` pre-check (soft, never solved)
   construct them.
3. **Settlement is the attributed credit delta, not a `consumed_credit` field.** The drain
   reads the free `/api/billing/info/` immediately before creation (baseline, valid only while
   the profile has no other in-flight lease) and again in the generation's wake; the delta
   (`total_credits_left` drawdown, else `monthly_usage` advance) settles the customer exactly
   when the turn epoch proves the window exclusive. Ambiguous attribution settles the reserve
   (published 5 credits for a song; the documented conservative 50-credit bound for
   extend/lyrics/stems) with `native_schedule_derived = true` and an unattributed-movement
   counter; a delta above the reserve bound is a typed tariff anomaly; a finalized-but-failed
   generation with zero attributed movement refunds the hold.
4. **Operations: the whole §4 wire table, fail-closed elsewhere.** `song` (custom mode when
   tags/title are given, else the description form), `extend` (concat wire: clip id + position
   only), `lyrics`, `stems` (no documented split-kind selector — conservative reserve). MIDI is
   tariff-priced but has no wire-table entry and stays unadmitted; unknown operations get a 400
   naming the admitted set. Attachments: the blueprint documents no upstream upload endpoint,
   so the plane persists customer audio itself (≤96 MiB, magic-sniffed, tmp→fsync→rename) and
   any attachment-carrying generation fails closed with `suno_attachment_upstream_unknown`.
5. **Window identity is evidence-derived.** Quota observations are keyed by the exact month the
   raw `period` field names (strict `YYYY-MM` → the month's own reset anchor and duration); an
   absent or unparseable `period` writes NO observation (turn events still persist) — an
   unknown duration fails closed, and no synthetic 30/31-day constant exists anywhere.

`SunoGateway::operational_status` publishes a `SunoOperationalStatus` for readiness, `/metrics`
and admin-only `GET /suno-subs`: fleet counts, per-profile hard/soft cooling with resets, raw
quota counters (unknown stays `None`; the bonus drip is not modelled), the unattributed/tariff
anomaly counters, tracked-generation count and the FIFO `DeliveryHealth`. Privacy by
construction: subject/cookie/session id/proxy stay private in `RuntimeProfile`; durable
calibration rows (`AsyncBilling::suno_calibration_report`, PostgreSQL-only) join to the opaque
id only through `profile_id_for_subject`, and the read model (`SunoGenerationView`) never
carries upstream clip ids or media URLs.

**GPT Image 2 native wire and producer-first Images API:** `codex::images` owns the strict native
Codex OAuth generation/edit wire. It posts JSON to `{CodexConfig.base_url}/images/generations|edits`
through one existing sealed Codex home, with the existing bearer/account/originator/UA/version headers
plus a fresh `x-codex-image-turn-id`. The native library keeps typed controls for private evidence, but
the customer producer deliberately accepts only the live-proven contract:
`model=gpt-image-2|gpt-image-2-2026-04-21`, `n=1`, `background=opaque`, `quality=low`, `size=auto`,
`output_format=png`, and base64 JSON output. `POST /v1/images/generations` accepts bounded JSON;
`POST /v1/images/edits` accepts multipart with one to five strict PNG `image` fields. Authentication precedes
body buffering. Masks, multiple references/outputs, exact sizes, transparent background, medium/high,
JPEG/WebP/compression, partial-image streaming, and Responses multi-turn image state fail closed.
The response contains exactly one bounded PNG and reconstructed allow-listed terminal usage. A metered
success returns the engine reservation/money identity in `x-request-id`; an admin-only unmetered success
keeps a generated opaque request id. Upstream request metadata is never substituted for this identity.

Each customer operation freezes one admitted home, performs the existing free `/wham/usage` preflight,
reserves a typed immutable OpenAI image snapshot, and dispatches only to that exact home. Generation
holds the conservative prompt+low-output ceiling; edit additionally holds the official Tier-5 whole-minute
8M-token image-input envelope because OpenAI publishes no normative high-fidelity input formula. A
successful result must return internally consistent text/image input and image-output token details;
cached input is accepted only when its modality split is authoritative, so the current aggregate-only
nonzero cache counter is rejected rather than guessed. Exact official replacement cost is settled across
fresh/cached text, fresh/cached image, and image output legs. A successful provider turn with malformed
usage or controls keeps its full hold for recovery instead of becoming free; ambiguous post-dispatch
errors never advertise `not_started` and are never replayed. The authenticated production
generation+edit smoke is watchdog-GREEN (delivery `d172c6fd0116ba73b051fc5aa02193a4885de5da`), so the
model is published: `/v1/models` deliberately does not list it, the unified router proxies both
image routes as a native OpenAI lane and its preset lists the snapshot, and the generation-6
product catalogs, site and public docs include it. There is no image API key, reseller origin, or
environment variable.

The private canary still freezes an explicit or admitted profile and publishes only mode-`0600` evidence.
Generation and one-reference edit are watchdog-GREEN under the exact procedure in
`docs/ops/GPT_IMAGE_2_CANARY.md`; unsupported controls remain blockers described in
`research/GPT_IMAGE_2_EVIDENCE.md`.

**Tuning for live Anthropic** (identity/beta/UA/version) — via `ProxyConfig` fields, which
`server` takes from env. Default values — in `config.rs`.

**Verification:** `cargo build -p forward`; full smoke — through the binary against the mock upstream
(`tests/rotation_fanout_smoke.sh`; universal chat lane end-to-end — `tests/universal_chat_smoke.sh`).
