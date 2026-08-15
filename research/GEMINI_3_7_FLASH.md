# Gemini 3.7 Flash — Stage 1 admission dossier

## Review metadata

| Field | Value |
|---|---|
| Review date | 2026-08-14 (Asia/Shanghai) |
| Repository base inspected | `c371685d5cad8a184c577799f934b72a975f916b` |
| Delivery stage | Stage 1: research, tariff, dormant implementation, and controlled canary only |
| Exact official Developer API model ID | `gemini-3.7-flash` |
| Official release status | Generally available (GA), stable, released 2026-08-13 |
| Publication decision | **READY FOR A SEPARATE PUBLICATION COMMIT** after GREEN live on exact SHA `c4f0773a…` |
| Evidence policy | Official Google sources plus sanitized owned-catalogue and exact-SHA live evidence; only the confirmed private alias is accepted |

This document is the append-only admission record for Gemini 3.7 Flash. It is not a public
catalogue entry and it does not claim that any existing Gemini subscription credential, Code Assist
plan, router preset, customer plan, or storefront can generate with this model.

## Executive verdict

Gemini 3.7 Flash is a real, stable Gemini Developer API model. Google announced its general
availability on 2026-08-13 and documents the exact public ID `gemini-3.7-flash`.

The product decision for Stage 1 is deliberately narrower:

1. Keep `gemini-3.7-flash` as the sole public/dormant product identity. The exact private
   `gemini-3.7-flash-tiered` candidate is permitted only because a later owned
   `fetchAvailableModels` snapshot exposed that row; do not infer any other private, effort or quota
   alias.
2. Record the official Standard tariff with its 2026 promotional and 2027 rates, but do not expose
   the model publicly or make it a default.
3. Treat official Developer API and Antigravity availability as separate from availability through
   the product's subscription-backed Gemini credential path.
4. Exact runtime `c4f0773a…` passed the one-shot live under the product identity contract approved
   by the person: `gemini-3.7-flash-tiered` is a private upstream alias, while
   `gemini-3.7-flash` is the only customer-visible identity.
5. Proceed only through a separate publication commit. The two earlier failed candidates remain
   withdrawn and blocked from retry; the GREEN request is also terminal and must not be replayed.

## Official evidence

All external sources below are first-party Google sources and were checked on 2026-08-14.

| Source | What it establishes | What it does not establish |
|---|---|---|
| [Gemini API release notes](https://ai.google.dev/gemini-api/docs/changelog) | GA announcement on 2026-08-13 and the exact public ID | Subscription quota, Code Assist support, or the product's private wire name |
| [Gemini 3.7 Flash model page](https://ai.google.dev/gemini-api/docs/models/gemini-3.7-flash) | Stable status, token limits, modalities, and official capabilities | Availability to every credential type or customer plan |
| [Latest models guide — Interactions API](https://ai.google.dev/gemini-api/docs/latest-model) | Production readiness, migration notes, thinking levels, and Antigravity positioning | A private Code Assist transport contract |
| [Latest models guide — `generateContent`](https://ai.google.dev/gemini-api/docs/generate-content/latest-model) | Exact `generateContent` request form and migration guidance | Successful generation through this repository's implementation |
| [Gemini Developer API pricing](https://ai.google.dev/gemini-api/docs/pricing) | Promotional and post-promotion rates, grounding allowance, and query charging | The product's customer multiplier or subscription quota |
| [Thinking guide](https://ai.google.dev/gemini-api/docs/thinking) | `low`/`medium`/`high`, default `medium`, and streamed thinking behavior | Acceptance by an untested private upstream route |
| [Context caching guide](https://ai.google.dev/gemini-api/docs/caching) | Implicit caching behavior and the 4,096-token minimum for 3.7 Flash | A cache hit for the owned credential |
| [`countTokens` REST reference](https://ai.google.dev/api/tokens#method:-models.counttokens) | Public token-counting contract | Generation availability or billable quota |
| [`streamGenerateContent` REST reference](https://ai.google.dev/api/generate-content#method:-models.streamgeneratecontent) | Public SSE streaming contract | Incremental streaming through the product |
| [Google DeepMind model card](https://deepmind.google/models/model-cards/gemini-3-7-flash/) | Model lineage, 1M/64K limits, publication date, and official distribution surfaces | A mapping from those surfaces to the product's subscription wire |
| [Gemini Code Assist release notes](https://docs.cloud.google.com/gemini/docs/codeassist/release-notes) | No Gemini 3.7 Flash announcement was present as of the review | Permanent absence from Code Assist |
| [Gemini 3 in Code Assist](https://docs.cloud.google.com/gemini/docs/codeassist/gemini-3) | The documented Code Assist model set observed on the review date | Permission to substitute a Developer API ID into a Code Assist route |

## Official model contract

| Property | Official contract |
|---|---|
| Public model ID | `gemini-3.7-flash` |
| Lifecycle | Stable / GA |
| Input limit | 1,048,576 tokens |
| Output limit | 65,536 tokens |
| Input modalities | Text, image, video, audio, and PDF |
| Output modality | Text |
| Thinking | Supported; `low`, `medium`, and `high`; default `medium` |
| Structured output | Supported |
| Function calling | Supported |
| Context caching | Supported |
| Code execution | Supported |
| File Search | Supported |
| Google Search grounding | Supported, with separately metered executed queries |
| Google Maps grounding | Supported, with separately metered search queries |
| URL context | Supported |
| Computer Use | Preview capability |
| Image generation | Not supported |
| Audio generation / TTS | Not supported |
| Live API | Not supported |

These are upstream capabilities, not Stage 1 product promises. A later publication may advertise only
the subset that the exact implementation SHA and every claimed credential plan prove live.

### Request and migration rules

- The exact unary REST route is
  `POST /v1beta/models/gemini-3.7-flash:generateContent`.
- The exact token preflight route is
  `POST /v1beta/models/gemini-3.7-flash:countTokens`.
- SSE uses `streamGenerateContent?alt=sse`; a buffered terminal response is not proof of incremental
  streaming.
- `thinkingLevel` replaces `thinkingBudget`. Only `low`, `medium`, and `high` are valid for this
  model. `minimal` is not supported, and omission means `medium`.
- Google migration guidance says to remove `temperature`, `topP`/`top_p`, and `topK`/`top_k` rather
  than promise sampling behavior. They may be ignored or rejected depending on the surface. The
  product must reject them locally for this model instead of silently claiming that they work.
- `candidateCount`/`candidate_count` is unsupported for Gemini 3.x and must not be forwarded as a
  supported control.
- Prefilled model turns are unsupported, and the final user turn must contain non-empty text.
  Stage 1 therefore also rejects image-only and tool-result-only final turns; those paths remain
  unclaimed until an exact-SHA live gate proves their complete wire contract.
- Thought signatures must survive multi-turn and tool-call round trips.
- A `FunctionResponse` must preserve both the function `name` and its `call_id`.
- Output/thinking token accounting uses the output rate; thinking tokens are not free metadata.

## Distribution boundaries

The word “available” is surface-specific. The following boundaries prevent an official public release
from being misread as proof for a private subscription transport.

| Surface | Evidence through 2026-08-15 | Stage 1 product conclusion |
|---|---|---|
| Gemini Developer API | Officially GA under `gemini-3.7-flash` | The public ID remains the product identity; the canary may map it only to an exact owned private row |
| Google AI Studio and documented Google product surfaces | Listed by first-party documentation/model card | Confirms Google distribution only |
| Gemini Managed Agents / Antigravity agent | Official latest-model guidance makes 3.7 Flash the default underlying model for that hosted agent | This is a different surface from the product's OAuth-backed Antigravity/Code Assist transport and does not reveal a private quota or wire alias |
| Gemini Code Assist | The checked Code Assist model page and release notes did not document 3.7 Flash | Do not claim Code Assist support; absence is a dated observation, not a permanent verdict |
| Owned subscription catalogue | No 3.7 row on 2026-08-14; exact positive `gemini-3.7-flash-tiered` row on Pro and Ultra on 2026-08-15 | The exact row may be a dormant private-wire candidate; no plan may be advertised before live acceptance |
| This product | No live acceptance exists for the exact implementation SHA | Dormant only; no public catalogue/default/router/web exposure |

### Owned catalogue observations

The owned catalogue observation was sanitized before being recorded here. On 2026-08-14 it contained
neither a Gemini 3.7 quota row nor a Gemini 3.7 private wire row. Consequently, the original dormant
candidate kept `gemini-3.7-flash` on both public and private sides.

This absence does not prove that the model will never appear in the owned catalogue, and a future
catalogue appearance would still not prove generation. Any later private name must come from fresh,
credential-bound authoritative evidence and must be recorded as a new dated finding; it must not be
derived from neighboring Gemini aliases.

At `2026-08-15T01:31:54+08:00`, the production read-only status exposed a materially new owned
catalogue snapshot. The exact private row `gemini-3.7-flash-tiered` was present with positive
remaining quota and `antigravity_model` type on six Google AI Pro profiles and one Google AI Ultra
profile; one additional Pro profile did not contain the row. The public `conversion_models` list
still omitted 3.7, so ordinary discovery remained closed. No profile identity, token, project or
proxy was retained in this observation.

The same recheck found no 3.7 entry in the current Code Assist release notes, the Gemini 3 Code
Assist page, the Antigravity model page or official `google-gemini/gemini-cli` HEAD
`c0d192452b4e2df7efb6d62a60385f475bfd6779`. Those absences do not override the credential-bound
catalogue row, but they prevent a broader Code Assist availability claim. The exact row authorized
one new dormant private-wire candidate only; the later exact-SHA live proved that it serves the
public 3.7 contract while returning the same confirmed private alias.

## Official rate card

The repository's money invariant is integer nanoUSD. The Standard tariff converts exactly without
floating-point arithmetic:

| Standard consumption | Through 2026-12-31 | Starting 2027-01-01 |
|---|---:|---:|
| Input | `750 nanoUSD / token` | `1,500 nanoUSD / token` |
| Output, including thinking | `3,750 nanoUSD / token` | `7,500 nanoUSD / token` |
| Cached input | `75 nanoUSD / token` | `150 nanoUSD / token` |
| Cache storage | `500 nanoUSD / token-hour` | `1,000 nanoUSD / token-hour` |

The official pricing page publishes no separate long-context surcharge for Gemini 3.7 Flash. The
Stage 1 engine tariff is the Standard tariff only; it does not imply Batch, Flex, or Priority routing.

For completeness, Google also publishes these service-class rates. They are evidence, not Stage 1
product routes:

| Service class | Consumption | Through 2026-12-31 | Starting 2027-01-01 |
|---|---|---:|---:|
| Batch / Flex | Input | `375 nanoUSD / token` | `750 nanoUSD / token` |
| Batch / Flex | Output, including thinking | `1,875 nanoUSD / token` | `3,750 nanoUSD / token` |
| Batch / Flex | Cached input | `75 nanoUSD / 2 tokens` | `75 nanoUSD / token` |
| Batch / Flex | Cache storage | `500 nanoUSD / token-hour` | `1,000 nanoUSD / token-hour` |
| Priority | Input | `1,350 nanoUSD / token` | `2,700 nanoUSD / token` |
| Priority | Output, including thinking | `6,750 nanoUSD / token` | `13,500 nanoUSD / token` |
| Priority | Cached input | `135 nanoUSD / token` | `270 nanoUSD / token` |
| Priority | Cache storage | `500 nanoUSD / token-hour` | `1,000 nanoUSD / token-hour` |

The promotional Batch/Flex cached-input rate is an exact rational rate of 75 nanoUSD per two tokens.
It must never be represented as a floating-point `37.5` nanoUSD/token. If the product later supports
that class, accounting must preserve the exact ratio at an aggregate integer boundary.

### Grounding charges

Google Search grounding has a shared allowance of 5,000 free search requests per month across Gemini
3.x models on a paid account. After the allowance, each executed search query costs
`14,000,000 nanoUSD` (`$14 / 1,000 queries`). One customer request can execute multiple queries, and
each executed query is charged. The pricing page does not publish a different 2027 grounding rate.

Google Maps grounding likewise lists a shared 5,000-free-prompt monthly allowance across Gemini 3
models, followed by `14,000,000 nanoUSD` per executed search query. Neither free allowance is a reason
to treat grounding as unmetered.

A single paid grounding query costs 140 times the default `$0.0001` admission budget. Therefore the
admission micro-smoke must not invoke Search or Maps unless an authoritative remaining free allowance
is known before dispatch or the person explicitly authorizes a larger bounded budget.

## Admission and live-acceptance matrix

The first two one-shot lives on 2026-08-15 are **WITHDRAWN**. The later 512-token successor
`c4f0773a…` is **GREEN** under the explicitly approved public/private identity contract. Every
request remains terminal; no failed or successful paid transport may be replayed.

| Check | Exact evidence required | Budget rule | Publication meaning |
|---|---|---|---|
| Owned catalogue discovery | Sanitized 3.7 quota and wire rows bound to the credential/plan | Free | **CANDIDATE OBSERVED** on 2026-08-15: `gemini-3.7-flash-tiered` was positive on six Pro and one Ultra profile; never generation proof |
| `countTokens` preflight | 2xx for exact `gemini-3.7-flash` and a positive authoritative token count | Free; always first | **GREEN 2026-08-15** on `c4f0773a…`: one attested `totalTokens=19`; historical partial rows remain non-generation evidence |
| Minimal generation | One 2xx incremental SSE request with byte-exact real text, terminal authoritative usage, and the confirmed public/private upstream identity from the exact candidate SHA | Default `$0.0001`; the person authorized one no-retry generation for exact SHA `c4f0773a…` at the conservative `$0.788352` ceiling | **GREEN 2026-08-15**: exact `1 … 64`, terminal `STOP`, authoritative 20 input / 478 output tokens, raw `gemini-3.7-flash-tiered`, and reconciled `$0.0018075`; the authorization is consumed |
| Incremental SSE | The same paid generation must contain at least two visible non-thinking text frames, including one before the terminal event, then finish cleanly with authoritative usage; candidate-only frames are insufficient | Included in the single admission request; no second stream request | **GREEN**: eight SSE/candidate frames and seven visible non-thinking text frames in the same admitted generation |
| Thinking `low` | Real output and authoritative usage with `thinkingLevel=low` | Separately bounded | **GREEN 2026-08-15** on `916dee0d…`: 470 output incl. 288 thinking, terminal `STOP`, exact `1 … 64` over 7 visible SSE frames, reconciled `$0.001778` |
| Thinking default | Omitted `thinkingLevel` returns the admitted exact output and usage | Included in the single admission request | **GREEN** for the omitted/default path only; 296 thinking tokens were authoritative |
| Thinking explicit `medium` | Explicit `thinkingLevel=medium` follows documented semantics | Separately bounded | **GREEN 2026-08-15** on `916dee0d…`: 473 output incl. 291 thinking, terminal `STOP`, exact output over 7 visible frames, reconciled `$0.001789` |
| Thinking `high` | Real output and authoritative usage with `thinkingLevel=high` | Separately bounded | **GREEN 2026-08-15** on `916dee0d…`: 501 output incl. 319 thinking, terminal `STOP`, exact output over 6 visible frames, reconciled `$0.001894` |
| Unsupported controls | Product rejects sampling controls and `candidateCount`; `minimal` is rejected locally or never offered | Unit/contract test; no paid request required | Prevents false controls from entering the public schema |
| Structured output | Valid response matching the requested schema plus terminal usage | Separately bounded | Required only if published for this model |
| Function calling | Forced tool call with preserved `name`, `call_id`, and thought signature; successful follow-up | Separately bounded | Required only if published for this model |
| Caching | Authoritative cache-token usage on a request meeting the 4,096-token minimum | Separately bounded | Required before a cache/billing claim |
| Image/video/audio/PDF input | One successful bounded request per modality with real text output and usage | Separately bounded | Test every modality the product intends to advertise |
| Search/Maps grounding | Grounding metadata, executed-query count, and authoritative cost evidence | Not permitted under the default cap unless free allowance is proven; otherwise explicit larger budget | Official capability remains unclaimed in Stage 1 |
| Code execution, File Search, URL context, Computer Use Preview | Surface-specific real result and usage for every claimed control | Separate risk and budget approval where applicable | Not claimed in Stage 1 |
| Each subscription plan | Repeat required generation/stream/control checks with a credential bound to that exact plan | Per-plan bounded budget | One plan's success proves only that plan |

### One-shot admission calculation

The default aggregate admission ceiling remains `100,000 nanoUSD`. First call `countTokens`, which
is free. The private Code Assist route can prepend provider-owned instructions that are absent from
that count, so the proved input ceiling is the complete 1,048,576-token context, not the observed
short prompt. With `maxOutputTokens=256`, the person explicitly authorized the more conservative
post-promo Standard worst-case ceiling for one request and no retry, even though the one-shot may be
dispatched and accepted only inside the promotional epoch:

```text
1,048,576 * 1,500 + 256 * 7,500 = 1,574,784,000 nanoUSD = $1.574784
```

The larger bound is authorization, not intended spend and not permission to cross the immutable
`2027-01-01T00:00:00Z` dispatch cutoff. The exact prompt remains `Output the integers 1 through 64,
separated by single spaces, and nothing else.`; the request has a 256-token maximum, omits
`thinkingLevel`, and uses no grounding, paid tool, redirect, reconnect, rotation, resend, or
automatic retry. Success requires the exact concatenated integers `1` through `64`, single-space
separated, and at least two visible non-thinking SSE text frames including one preterminal frame.
Actual promotional-epoch terminal usage—not the ceiling—is charged and reconciled; the
ceiling is a fail-closed reserve, not an expected spend or acceptance evidence by itself.
The private canary pins `CLAUDE_API_TARIFF_OVERRIDES=0` in its fixed `ExecStart`, binding both
preflight and immutable settlement to the compiled official schedule; a mutable hot override can
neither authorize the request nor raise its event above this ceiling.

### Narrowed execution budget — 2026-08-15

The person subsequently authorized exceeding the default `$0.0001` only when required and only to
the minimum ceiling proved immediately before dispatch. The reviewed generic-runner admission mode
therefore does not consume the older post-promotion `$1.574784` envelope while the request is fenced
inside the current promotional epoch. It recomputes and requires exactly:

```text
1,048,576 * 750 + 256 * 3,750 = 787,392,000 nanoUSD = $0.787392
```

This is still a worst-case reserve for one no-retry request, not expected spend. The count is free,
actual settlement remains authoritative, and any tariff change or dispatch beyond the current epoch
stops before generation and requires a fresh explicit numeric contract. The older larger
authorization above remains historical context; it is not the active execution budget.

### Authorized 512-token successor budget — 2026-08-15

The tiered-wire attempt showed that 256 tokens were not a valid acceptance bound: 241 of 252 output
tokens were thinking tokens and the response terminated at `MAX_TOKENS`. The smallest reviewed
successor doubles only the output allowance to an explicit `maxOutputTokens=512`; the dormant
producer rejects the withdrawn 256-token payload and every other explicit bound before dispatch.
The current promotional worst-case ceiling is therefore:

```text
1,048,576 * 750 + 512 * 3,750 = 788,352,000 nanoUSD = $0.788352
```

On 2026-08-15 the person explicitly authorized exactly one paid generation at that ceiling for exact
SHA `c4f0773a…`, after production `deploy/watchdog` was GREEN and the free `countTokens` preflight
succeeded. The authorization was consumed by the GREEN run and does not apply to either withdrawn
SHA, any later implementation SHA, a retry/replay, a later tariff epoch, grounding, tools, or another
paid control test. It was a conservative reserve; authoritative terminal usage fixed the actual
charge at `$0.0018075`.

## Two-stage delivery decision

### Stage 1 — current change

Allowed:

- this official research record;
- exact Standard nanoUSD tariff epochs;
- a dormant internal candidate keyed only by `gemini-3.7-flash`;
- local unit/contract tests that do not claim live availability;
- a controlled canary after an exact implementation SHA and budget bound exist.

Forbidden in Stage 1:

- production defaults or systemd defaults;
- the public model catalogue or public aliases;
- unified-router presets or public `/v1/models` exposure;
- web, OpenKeys, admin, commerce, sales, or public documentation claims;
- an inferred Code Assist, Antigravity, effort-suffixed, quota, or private wire alias;
- claims for Search, Maps, tools, modalities, streaming, or subscription plans without their matching
  live acceptance rows.

The dedicated Stage 1 root bridge, repository trigger, admission state machine and private canary
unit were retired. They were delivery automation only: the exact tariff, hidden model definition,
dormant Rust producer, response evidence checks and no-replay transport safeguards remain. The
successful controlled live used `tools/gemini_calibration/run_live.py` against an explicitly
operator-launched exact-SHA non-public canary without installing another permanent root helper or
making the model reachable by ordinary traffic.

### Withdrawn firing attempt — 2026-08-14

The first exact trigger SHA, `f2ced4f9edfb4d42ad5bb1d6ef9f0bc7c7593044`, was rejected by the
installed production-head guard before trigger discovery, any protected authority read,
`countTokens`, generation, or spend. It is not a model acceptance attempt and must not be retried.
Its descendant removed the trigger and restored a GREEN production head. The later cleanup retired
the unused helper assets and the paid branch-protection dependency. At that point the live matrix
remained **PENDING**, spend was `0 nanoUSD`, and all public surfaces
remained forbidden until a distinct controlled attempt succeeded.

### Withdrawn controlled live — 2026-08-15

The controlled live used the production-GREEN runtime
`20d945ce59e9dea749ec7c74b7d322525bc29a05` and generic-runner commit
`19516258a948f91bc0c13365ad9f24c65489530c`. A root-launched transient systemd unit ran the exact
binary as `deploy` on loopback port `18895`, with the production PostgreSQL authority, sealed roster,
pinned Antigravity/Node identity, `CLAUDE_API_TARIFF_OVERRIDES=0`, no Caddy route, and 30-minute
automatic lifetime. Ordinary discovery remained closed. The selected opaque profile reported the
authoritative `google_ai_ultra` plan and healthy persistence; delivery began and ended with zero
pending or dropped events.

The runner calculated and required exactly `787392000 nanoUSD` (`$0.787392`). It invoked the free
count once under request `d685cabe-d878-4940-a8d6-09870d5378f5`; the response had a valid positive
dispatch timestamp before the shared deadline. That report version did not retain the returned
`totalTokens`, so this historical row proves the response/dispatch boundary but is not claimed as a
positive-count acceptance row. The runner now requires and records a positive count for any future
candidate. It then invoked the paid SSE transport once under a
different UUID, `6d9e4bb1-b7ce-4020-a091-d9835f567cf8`. Google returned HTTP 404 with status
`NOT_FOUND` and message `The requested model resource was not found.` No output, raw model version,
terminal usage, incremental frame, or immutable billing event was produced.

The sanitized report records `spent_nanousd_total=0`, but also
`admission_spend_reconciled=false`; without terminal authoritative usage/event, zero is a local
ledger observation rather than provider-reconciled spend. The command is terminal and cannot be
resumed. The canary was stopped and collected, port `18895` was confirmed closed, and the normal
Gemini production plane remained healthy and continued to omit 3.7 from discovery.

This exact implementation candidate is withdrawn. The runner rejects its SHA before network I/O,
all Stage 2 surfaces remain forbidden, and no second request may be sent for it. A future attempt is
permitted only after materially new implementation evidence produces a different runtime SHA and a
new explicit numeric budget contract.

### Withdrawn tiered-wire controlled live — 2026-08-15

The second controlled live used production-GREEN runtime
`2c8aca0d1230bbf774b7e82ef11d651c4b705864`, whose dormant public identity mapped generation and
quota admission to the owned `gemini-3.7-flash-tiered` row. A root-launched transient systemd unit
ran that exact release binary as `deploy` on loopback port `18896`, with production PostgreSQL
billing/evidence, the sealed roster, pinned transport identity, tariff overrides disabled, transport
retries disabled, no Caddy route, and a 30-minute maximum lifetime. Native model discovery returned
zero rows and ordinary exact-model lookup returned 404. The selected opaque profile was a healthy
`google_ai_ultra` subscription with positive tiered quota; authority delivery began and ended with
zero pending or dropped events.

The runner recomputed and required the authorized `787392000 nanoUSD` (`$0.787392`) ceiling. It sent
one free count transport and received an attested `totalTokens=19`. It then sent exactly one paid SSE
transport under a distinct UUID. The response exhausted `maxOutputTokens=256` with
`finishReason=MAX_TOKENS`, not the required `STOP`: immutable usage contained 20 input tokens and
252 output tokens, of which 241 were thinking tokens. The incomplete response did not expose the
required visible non-thinking frame sequence, raw canonical `modelVersion`, terminal response usage,
or response/event usage parity.

The immutable event nevertheless reconciled the one billable transport exactly: `15000` input
nanoUSD plus `945000` output nanoUSD, for `960000 nanoUSD` (`$0.000960`) total on the authoritative
Ultra plan and compiled tariff schedule. The report therefore records
`admission_spend_reconciled=true`, but `complete=false` and `resume_safe=false`. Reconciled billing is
not publication evidence when response identity, output, terminal usage and incremental SSE fail.

The transient canary was stopped and collected, port `18896` was confirmed closed, and the stable
Gemini production plane remained ready. This exact tiered-wire SHA is withdrawn and the runner now
rejects it before network I/O. It must not be retried or published. Any later admission requires a
materially new implementation SHA, a separately reviewed change that addresses the terminal
output-bound failure, and a fresh explicit numeric budget contract.

### GREEN 512-token controlled live — 2026-08-15

The third and terminal controlled live used production-GREEN runtime
`c4f0773a6a250fc48d8d8df05d5e14b7f7eeb8cb`. A transient loopback-only canary ran its exact release
binary on port `18897` with production PostgreSQL billing/evidence, the sealed roster, tariff
overrides and transport retries disabled, no Caddy route, and no public discovery. The selected
opaque profile was `google_ai_ultra` with positive `gemini-3.7-flash-tiered` quota; authority
delivery was healthy with zero pending or dropped events.

One free count transport returned an attested `totalTokens=19`. One distinct paid SSE transport
then returned 2xx and the byte-exact `1 … 64` response across eight candidate frames, seven of them
with visible non-thinking text. It ended in terminal `STOP` with usage. The immutable event bound the
same request to 20 input tokens and 478 output tokens, including 296 thinking tokens, and reconciled
`15000 + 1792500 = 1807500 nanoUSD` (`$0.0018075`) on the compiled promotional schedule. No retry,
rotation, replay, grounding, tool, or second paid control was used.

The upstream response named itself `gemini-3.7-flash-tiered`. The report version active during the
run returned at its older exact-public-ID comparison before assigning the later terminal/SSE/parity
booleans, even though the response had already passed parsing, exact-output and frame checks and the
immutable event was reconciled. The person then explicitly approved the product contract that the
confirmed tiered spelling is a private upstream alias for Gemini 3.7 Flash. The runner regression now
accepts only public `gemini-3.7-flash` or confirmed `gemini-3.7-flash-tiered` in this closed admission
leg and rejects the alias on ordinary legs. Ordinary runtime translation always rewrites upstream
identity to the sole customer-visible `gemini-3.7-flash`.

This makes the exact SHA GREEN without another network call. The canary was stopped and collected,
port `18897` was confirmed closed, and the stable Gemini production plane stayed ready. The live
authorization is consumed and cannot be inherited or replayed.

### Gate to Stage 2 — separate follow-up commit

Publication may begin only after all of the following are true:

1. The exact dormant implementation commit SHA is recorded.
2. A reviewed extension of the generic runner calculates the exact effective tariff bound before
   transport, then makes one free exact-ID `countTokens` call. The request sends the exact profile,
   canonical UUIDv4, and bounded `not_after`; the response is bound to its UUID, profile, model, HTTP
   status and execution state. Success additionally requires exactly one canonical positive producer
   `x-apitoken-calibration-dispatch-ms` strictly below `not_after * 1000`. The producer may perform
   at most one non-replayable pre-POST token refresh. A failed or ambiguous count aborts before paid
   generation.
3. The runner makes at most one paid incremental SSE generation. It uses an already-fresh cached
   bearer and the same exact profile/UUIDv4/not-after contract, with no post-selection refresh,
   helper restart, OAuth resend, profile rotation, redirect, reconnect or replay. The pinned producer
   records the authoritative dispatch timestamp after target TLS in Node's synchronous pre-`_flush()`
   socket listener, before any upstream HTTP bytes; missing, duplicate, malformed, equal-cutoff, or
   later evidence is terminal. It returns 2xx with real output and terminal authoritative usage
   within the explicitly authorized aggregate cap. This single request is the incremental SSE proof:
   it must produce at least two visible non-thinking text frames, including a preterminal frame. The
   immutable `priced_ts` must reproduce the official rate effective at dispatch; an earlier plan or
   count is not permission to dispatch after its cutoff. The
   response must preserve one raw upstream `modelVersion`: public `gemini-3.7-flash` or the confirmed
   private `gemini-3.7-flash-tiered` alias. The terminal authority snapshot and outcome/event binding
   must reproduce the exact admitted profile and paid plan; missing or changed plan proof is not
   acceptance. Customer responses still expose only the public spelling.
4. Only the exact thinking behavior exercised by the paid request is proved; explicit `low`,
   `medium`, and `high` remain unpublished until separately tested.
5. Every control, modality, and credential plan proposed for publication passes its matching matrix
   row; untested features remain absent from the claim.
6. Metering reconciles input, output/thinking, cached-input reads, and any grounding query counts
   using integer or exact-rational nanoUSD arithmetic. Explicit `cachedContent` and its token-hour
   storage SKU remain rejected and outside publication scope until the provider supplies an
   authoritative storage-duration signal and the ledger gains that dimension.
7. The controlled production live is GREEN on the exact SHA.
8. Publication surfaces are changed together under the new-model checklist in a separate commit.

The conditions above are satisfied by exact SHA `c4f0773a…` for the deliberately narrow default
text/SSE surface. Untested controls and explicit thinking levels remain unclaimed. Any failed
generation remains withdrawn; a red SHA is never published or retried as if it were green.

### Explicit thinking levels controlled live — 2026-08-15

After the default-surface GREEN, a follow-up exact-SHA matrix on production-GREEN runtime
`916dee0df5a9b78b0e4f2a00632100d59d4adfbc` admitted all three documented explicit levels on the
same healthy `google_ai_ultra` profile with positive `gemini-3.7-flash-tiered` quota. The runner's
`--gemini-37-thinking-levels` mode sent one free attested `countTokens` and exactly one paid
4096-token incremental SSE generation per level (no retry, replay or rotation), each with the
byte-exact `1 … 64` output contract, raw `gemini-3.7-flash-tiered` modelVersion proof, terminal
`STOP`, terminal usage equal to the immutable event, and a positive thinking token class:

| Level | Output tokens | Thinking tokens | Visible SSE text frames | Reconciled spend |
|---|---|---|---|---|
| `low` | 470 | 288 | 7 | `$0.001778` |
| `medium` | 473 | 291 | 7 | `$0.001789` |
| `high` | 501 | 319 | 6 | `$0.001894` |

Aggregate spend was `5,460,000 nanoUSD` (`$0.00546`) against an authorized `2,405,376,000`
nanoUSD worst-case ceiling; the report is complete with zero unavailable capabilities. An earlier
512-token probe of `low` twice showed the level is input-variable (zero thinking once, 338
thinking and a `MAX_TOKENS` truncation once), so admission uses the 4096-token bound; the level's
semantics under a small client output cap remain as variable as the provider makes them, which the
product surface does not hide. `minimal` was never dispatched: the model's own rules reject it and
the wire-mapping refuses it locally, so it is not and cannot become an advertised effort. The
transient canary on port `18898` was stopped and collected after the matrix, and the stable Gemini
production plane stayed ready throughout.

Publication of `reasoning_efforts: ["low", "medium", "high"]` in the native and unified catalogues
follows in the same change as this evidence, together with the site capability note.

### Full-capability controlled live — 2026-08-15

A second follow-up matrix on the same production-GREEN runtime `35153abe5f5157e6044016a8c3b49625de017ed3`
and the same healthy Ultra profile admitted every remaining text-surface control with one free
attested `countTokens` plus exactly one paid generation per leg (no retry/replay/rotation,
per-leg coverage misses recorded, aggregate `$0.0190` against a `$5.518464` worst-case ceiling):

| Capability leg | Evidence | Reconciled spend |
|---|---|---|
| `sse` | Terminal `STOP`, terminal usage, response/event parity, incremental multi-frame SSE with visible text | `$0.000557` |
| `structured` (JSON `responseMimeType` + `responseSchema`) | Schema-valid JSON `{"marker":"CALIBRATION_OK","answer":42}`, terminal STOP/usage/parity | `$0.000337` |
| `tool-prompt` (forced `functionDeclarations` call) | Invoked `calibration_probe` functionCall, terminal STOP/usage/parity | `$0.000393` |
| `cache-write` | 12,337 input tokens written, terminal STOP/usage/parity | `$0.010622` |
| `cache-read` | Same payload replayed with authoritative **8,170 cached input tokens**, terminal STOP/usage/parity | `$0.006101` |
| `image-input` (inline PNG) | 1,137 input tokens, real one-word color answer, terminal STOP/usage/parity | `$0.000991` |

The `long-context` leg was skipped before dispatch with no spend: the model's flat tariff has no
long-context tier, so there is no threshold crossing to prove. Search remains undispatched and
unadvertised for every Gemini 3 model: it is billed per query with no provider-documented fanout
ceiling, so a paid request cannot be hard-bounded in advance. Audio input stays rejected by the
native gate for this route. Two earlier same-day probes informed the final bodies: cache legs now
pin `thinkingLevel=low` because the default level can spend a 1,024-token output entirely on
thoughts for long contexts, and the brief SSE admission accepts a single visible frame because a
one-word answer legitimately arrives in one text frame. Both adjustments live in the runner with
regression tests; no admission rule was weakened for the thinking-level matrix.

Publication of `tool_calling`, `structured_outputs`, image input and implicit caching in the
native and unified catalogues follows in the same change as this evidence.

## Checklist disposition

The new-model checklist in
[`docs/CHANGE_CHECKLISTS.md`](../docs/CHANGE_CHECKLISTS.md) applies. This document covers the Stage 1
research, official tariff, evidence boundary, dormant-name decision, and canary contract.

| Checklist area | Stage 1 disposition |
|---|---|
| Official identity and behavior research | Applicable; recorded here |
| Official tariff and effective epochs | Applicable; recorded here in exact nanoUSD units |
| Dormant implementation and tests | Applicable; owned by the implementation change, not this research file |
| Exact-SHA controlled live | Applicable; `c4f0773a…` is GREEN for default text/SSE with reconciled `$0.0018075`. Historical `20d945ce…` and `2c8aca0d…` remain withdrawn; no request may be replayed |
| Public catalogue, defaults, router, site, and public docs | Completed in the separate Stage 2 publication change after the GREEN live |
| New provider checklist | Not applicable; Gemini is an existing provider |
| Customer price or multiplier change | Not applicable; this records upstream cost only and changes no customer multiplier |
| Database migration | Not applicable; no schema change is introduced by this document |
| Cross-context contract / `docs/DEPENDENCIES.md` | Applicable; both retained exact-evidence and public discovery/router consumers are recorded there |
| Commerce, sales, OpenKeys, and admin publication | Admin consumes the engine conversion-model feed automatically; OpenKeys execution has no model allowlist, while its issuance display remains scoped to its Anthropic/OpenAI product types; Commerce and sales have no per-model catalog mirror |

## Stage 2 publication — 2026-08-15

The separate publication change exposes `gemini-3.7-flash` in native and unified discovery,
production Gemini defaults, the router's reviewed model manifest, the customer website and the docs
builder. The private `gemini-3.7-flash-tiered` identity remains confined to Antigravity dispatch and
quota matching; ordinary JSON/SSE responses, usage attribution and billing retain only the public
id. No root helper, trigger, private unit or permanent canary is restored.

The public capability claim is deliberately narrower than the official Developer API page: default
text generation, free `countTokens`, incremental SSE and terminal authoritative usage. Explicit
thinking levels, tools, structured output and non-text inputs remain unadvertised until separate
controlled evidence exists. The 2026-08-15 paid admission authorization is consumed and is not
replayed by publication.

## Secret hygiene

No raw access token, refresh token, OAuth envelope, API key, authorization header, cookie, account
email, subject ID, project/organization ID, proxy endpoint, customer prompt, response body, or raw
catalogue dump is stored in this document. The owned-catalogue findings are limited to dated sanitized
absence/presence, the exact non-secret model row, plan classes and aggregate profile counts. Future
live evidence must record only the exact commit SHA, plan label, HTTP/result class, sanitized model
identity, token/query usage, cost reconciliation, and pass/fail outcome; credentials and payloads
remain outside version control.
