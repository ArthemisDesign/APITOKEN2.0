# Gemini pool live calibration

`tools/gemini_calibration/run_live.py` performs a bounded load run of native Gemini through
real paid Code Assist subscriptions. The script does not derive capacity from prompt size and
does not trust client-side charging: the only source of cost is immutable Google turn events in
the `/gemini-subs` backend, and the only source of window movement is exact provider quota
snapshots.

## What the run covers

For every model in the backend `conversion_models`, the runner checks native non-stream and
SSE, all published thinking levels, fresh text, a repeatable text-cache payload, audio and a
repeatable audio payload, a function-declaration tool prompt, and Google Search. For models
with tiered pricing, a dedicated payload must cross the published long-context threshold by
actual `countTokens`; for the image model, 1K, 2K, and 4K are executed separately. The free
`countTokens` is the preflight of every paid request. HTTP 400/403/404 and a successful turn
without the expected separately metered token class are recorded in
`unavailable_capabilities` rather than counted as zero spend. `toolUsePromptTokenCount` is
only an optional subset of regular input: a missing subset is acceptable when the forced
`functionCall`, terminal usage, and response/event parity are proven, because the entire
`promptTokenCount` is metered exactly once.
After the already-paid generation, the runner also verifies the native response body itself:
the public `modelVersion`, visible non-thought text (or the mandatory
`functionCall`/`inlineData` for the corresponding control), `finishReason`, terminal
`usageMetadata`, and an exact match of the response token vector with the immutable event.
SSE counts as incremental only with at least two frames containing visible non-thinking text,
including one before the terminal frame; candidate-only or thought-only frames and a single
buffered visible frame do not pass the gate.

The backend estimator remains workload-dependent: it estimates the API-dollar equivalent of
the actually observed mixture. Plan is part of identity; the 5-hour and weekly windows are
independent. Bounds account for the decimal resolution of both provider snapshots.
Indistinguishable movement does not turn into `$0`, and finite high is absent when movement
does not exceed measurement uncertainty.

## Safeguards

- Without `--execute`, the runner prints a deterministic dry-run plan and does not open the
  backend/API.
- `--budget-usd` is parsed into integer nanoUSD and cannot exceed a total of `$40` per run.
- Before the first turn, `calibration_authority_available=true`, an empty queue, zero
  `dropped_events`, healthy persistence, and a known paid plan for every selected profile
  are mandatory.
- The admin-only `x-apitoken-calibration-profile` header contains the full opaque Gemini
  profile ID. The backend selects exactly it, does not spill/rebind, and does not bypass
  auth death, cooling, or provider zero. The header is never forwarded to Google.
- Before dispatch, `countTokens` and the official effective-dated rate card build the
  worst-case bound. Code Assist adds provider-owned instructions that are absent from the
  `countTokens` result; the live cache leg has already proven such hidden input. Therefore
  every generation leg reserves the model's full official input-context limit. This is the
  only hard ceiling for both ordinary hidden prompt and the separate
  `toolUsePromptTokenCount` absent from `countTokens` — not an assumption about the size of
  the client payload or JSON declaration.
  Search, metered once per grounded prompt, reserves one official SKU. For Gemini 3,
  Google meters every internal query but does not publish a hard fanout ceiling: the runner
  records such Search as unavailable and does not send the paid request. Image uses the
  forcibly bounded 1K/2K/4K SKUs (1,120/1,680/2,520 tokens) plus the requested text-output
  ceiling.
- A paid request after transport ambiguity is never retried. The general matrix runner may safely
  retry GET and a pre-claim `countTokens`; generation gets exactly one attempt. A future dormant
  Gemini 3.7 plan must narrow this further to one free exact-ID count followed by at most one paid
  generation, without adding a second transport implementation.
- `429`/`503` stops only the target profile, not the remaining matrix of healthy profiles,
  exclusively when the stable Gemini plane returned the authoritative
  `x-apitoken-execution-state: not_started`. `RetryInfo` and a sanitized body are not
  evidence on their own. Such a partial report can be continued via
  `--resume-report`: the runner preserves the same `run_id`, the overall budget, cache
  lineage, and exact spend, skips already-completed or proven-unavailable legs, and does not
  add new profiles or models.
  Generic 5xx without a not-started proof, SSH/HTTP transport ambiguity, and any other
  unverified failure are marked
  `resume_safe=false`; repeating such a paid leg is forbidden.
- For every paid turn, the runner creates a canonical UUIDv4 in advance and passes it via
  the admin-only `x-apitoken-calibration-request-id` header together with the exact profile
  target. The immutable backend evidence must show exactly this request id with the expected
  profile/model and the full token/API-cost vector. Rebind, a broken sum of cost legs,
  pending FIFO, or actual above the preflight bound stop the run fail closed; parallel
  customer traffic does not participate in attribution.
- A successful HTTP code on its own is not evidence. The response is decoded in memory
  without persisting the generated text in the report. A private/missing `modelVersion`,
  thoughts-only output, malformed JSON/SSE, missing terminal usage/finish, single-frame SSE,
  an uninvoked forced tool, or a discrepancy between response usage and the immutable turn
  is a terminal coverage failure. The runner already accounts for the confirmed spend, does
  not repeat the request, and halts the remaining paid matrix.
  For explicit `low`/`medium`/`high` thinking levels, the immutable usage must contain
  non-zero thinking output tokens. `minimal` permits a zero counter: this level uses dynamic
  thinking, and a successful response with full public identity/output/terminal usage proof
  does not become a coverage miss merely because the separate thinking token class is
  absent.
  Similarly, `tool_prompt_tokens` is a diagnostic subset of `input_tokens`, not a separate
  price bucket. Google may fold the declaration into the regular `promptTokenCount`; the
  runner requires an actual forced `functionCall` and full terminal response/event usage
  equality, but does not invent the missing subset and does not charge it a second time.
  HTTP 400/403/404 of a required capability leads to the same fail-closed outcome. The only
  non-blocking `unavailable_capabilities` entry is a Search skipped in advance without
  generation due to a documented unlimited per-query fanout (`blocking=false`,
  `skipped_before_dispatch=true`).
- Published Gemini subscription routes other than Flash Preview intentionally reject inline audio
  before provider dispatch: live Antigravity usage collapses it into generic prompt tokens, while
  free `countTokens` returns only a total. Published Flash Preview is the sole bounded exception:
  strict integral-duration PCM WAV generation may use its reviewed 32-token/second fallback, but
  every ambiguous format/cache split remains blocking rather than pricing the higher audio SKU as
  text or guessing.
- The cache payload contains a unique `run_id` and a stable ordinal profile scope; write/read
  pairs of one profile are byte-for-byte identical, but another profile or run cannot mistake
  someone else's cache warmth for its own. The scope contains no raw profile id or provider
  identity. Each replay group executes consecutively within a single profile before moving on
  to the next one. For Flash Preview this is the fixed matrix `write → prime → read`: one
  adjacent replay already produced a cache hit on Pro but remained fully fresh on Ultra, so
  the second successful generation is a pre-planned prime, and only the third must show the
  cache token class. This is not a retry after transport ambiguity or failed generation;
  every turn has its own request id, immutable evidence, and charge. The absence of the cache
  class on the final read is still terminal. Flash Preview cache/audio legs use a bounded
  `maxOutputTokens=512`: the model has already exhausted the 128-token dynamic-thinking
  budget without a visible response, and the earlier audio turn used 119/128 tokens.
  The full two-plan worst-case matrix equals `23,099,392,000 nanoUSD` and requires a separate
  explicit aggregate cap of `$24`; the previous `$21` authorization is insufficient, even
  though the actual spend is usually measured in cents.
- After durable settlement, an exact-target turn immediately wakes the free provider
  quota/health probe; regular customer traffic keeps the background cadence. The runner
  still waits a minimum of 16 seconds as an independent guard on provider snapshot
  propagation and then polls the backend until a quota snapshot with
  `quota_updated_at >= immutable completed_at` appears. Before such a post-turn snapshot,
  fraction delta is not counted as model/token-class evidence and does not enter
  profitability. Quota-only movement that repeats without spend goes to
  `unattributed_fraction_units`.
- API/control keys are loaded only locally or inside the production shell and never enter
  the report.

## Production command

Running is permitted only after a green production deploy of the exact runtime SHA:

```bash
python3 tools/gemini_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --report /tmp/gemini-calibration-report.json
```

After an explicit transient provider stop, the same run continues without repeated spend:

```bash
python3 tools/gemini_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --budget-usd 40 \
  --resume-report /tmp/gemini-calibration-report.json \
  --report /tmp/gemini-calibration-report.json
```

On resume, `--budget-usd` is the original aggregate cap, not additional budget; the value
must exactly match the checkpoint. `complete=false`, `resume_safe=true`,
`resume_proof=x-apitoken-execution-state:not_started`, and `pending_legs` explicitly show
what remains after cooling. `resume_safe=false` means a terminal manual investigation with
no repetition of the paid request. A narrow versioned exception exists only for an
already-completed `minimal` turn that the old runner wrongly stopped solely because of a
zero thinking token count: the new runner preserves the exact spend and the proven record,
clears only that obsolete coverage miss, and continues the pending legs without replay. For
this, the public `modelVersion`, the real visible output, terminal finish/usage,
response/event usage parity, non-stream identity, and the single blocking miss must match
exactly; any difference leaves the report terminal. A change of paid plan or effective
tariff schedule between attempts also terminates the resume: evidence from different money
identities is never merged into one run.

The production SSH path reads `/gemini-subs` through the stable Gemini plane at
`127.0.0.1:8794` and sends generation there with a remote-only forwarding-admin key. The
secret is never returned over SSH. To verify the full public data path through the unified
router, keep control/evidence on the Gemini plane but direct the paid requests to the stable
router origin `127.0.0.1:8802`:

```bash
python3 tools/gemini_calibration/run_live.py \
  --execute \
  --production-capacity-over-ssh \
  --production-api-over-ssh \
  --production-capacity-port 8794 \
  --production-api-port 8802 \
  --budget-usd 30 \
  --report /tmp/gemini-router-calibration-report.json
```

The ports are deliberately independent: the router does not publish `/gemini-subs`, and the
direct control plane remains the sole authority for immutable turn evidence,
settlement/FIFO, and quota observations. Meanwhile, `countTokens` and every billable
generation pass through the same active router slot as customer traffic. Both chosen ports
and the access method are recorded in the report.
For a controlled gate of a dormant model, both reads can be directed to a pre-launched
non-public exact-SHA canary on the same host via `--production-ssh-target <user@host>` and
`--production-capacity-port <loopback-port> --production-api-port <loopback-port>`. Target and ports
are validated before SSH; the default remains `apitokensale:8794`. The canary must use the production PostgreSQL authority, billing, and
immutable calibration evidence: an isolated process without them is not publication
evidence.

## Gemini 3.7 Flash controlled live

The former model-specific state machine, repository trigger, root gate, transport wrapper, sealed
producer copy and private systemd unit are retired. They supplied no live
evidence: the only trigger attempt was rejected before `countTokens`, generation, or spend. The
model's exact tariff, dormant Rust producer and generic response/evidence safeguards remain.

`tools/gemini_calibration/run_live.py` is the only retained live-runner base. It does not install a
service, select production credentials by itself, or make a dormant model public. Before any future
Gemini 3.7 live, a separate reviewed change must add only the smallest model-specific plan needed by
that generic runner and document the exact implementation SHA, request, controls, tariff epoch and
aggregate budget. The operator then launches an exact-SHA non-public canary through the established
server access path, using the production PostgreSQL billing/evidence authority and the normal sealed
credential roster. No permanent root helper or repository trigger is installed.

The controlled sequence remains fail closed:

1. prove the exact runtime SHA is production `deploy/watchdog` GREEN and that ordinary discovery and
   traffic still cannot reach `gemini-3.7-flash`;
2. validate the compiled official effective-dated tariff and calculate the conservative aggregate
   ceiling before transport. The default admission cap is `100000 nanoUSD` (`$0.0001`); a larger
   ceiling requires explicit person authorization for that single attempt;
3. make one free exact-ID `countTokens` preflight, then at most one paid incremental SSE generation
   on the same exact profile. The producer's deadline, dispatch attestation, cached-bearer and
   `NeverReplay` rules remain mandatory, so transport ambiguity is terminal and cannot rotate,
   reconnect, resend, or retry the paid request;
4. require real visible non-thought output in multiple incremental frames, raw upstream
   `modelVersion=gemini-3.7-flash`, terminal `STOP`, authoritative terminal usage, exact response
   versus immutable-event token parity, plan/profile attribution, and integer nanoUSD reconciliation;
5. publish in a separate commit only after that evidence is GREEN. Any failed or ambiguous
   generation leaves the model dormant and unpublished.

The previously designed 64-integer prompt and `maxOutputTokens=256` are research inputs, not an
active authorization or installed admission contract. They may be reused only if a fresh reviewed
plan proves they fit the then-current tariff epoch and user-approved budget.
## Offline verification

```bash
python3 -m unittest tools.gemini_calibration.test_run_live
```

The tests cover the dry-run, the hard `$40` guard, the authority/FIFO baseline, exact
attribution against the background of several new events, cost-vector integrity,
long-context/search/image bounds, the full capabilities matrix, byte-identical cache/audio
replay, the forced tool call, public model identity, real non-thought output, terminal
response/event usage parity, incremental SSE, and fail-closed resume with exact spend
restoration, including the non-replay reclassification of a proven `minimal` turn with a
zero thinking token count and the fail-closed rejection of substituted evidence. Decoder coverage
also rejects malformed or buffered-only SSE, duplicate JSON keys, inconsistent response identity,
non-terminal usage, non-STOP completion and response/event token mismatches without opening a
network connection. Any future model-specific Gemini 3.7 plan must add its own exact tariff/budget
tests before a live is authorized.

## Result

The `gemini-live-calibration/v2` report preserves the exact total and the spend per opaque
profile in nanoUSD, the full token/API-cost vector of every turn, the 5h/7d fraction delta,
unavailable capabilities, profile quota walls, the before/after identity of each window, the
final backend snapshot, and `model_profitability` sorted by API nanoUSD per 1% of the
corresponding 5h/7d window, separately for each paid plan, model, and token class. A change
of reset identity is never counted as model-specific fraction delta. Every successful record
additionally contains only sanitized `response_evidence` (frame/output/control counters, the
public model id, and terminal/incremental/usage parity booleans), but not the response text.
`blocking_unavailable_capabilities` makes a terminal publication miss explicit; the report
may have `complete=true` only without such misses and without unfinished legs.

The ranking may be used for commercial selection only for rows with a positive
distinguishable quota delta, provided no foreign immutable turn appeared between
before/after on the same profile. The runner marks such an interval
`profitability_eligible=false` and excludes it from the ranking. A missing row means
insufficient provider resolution/isolation, not a zero value of the model.
