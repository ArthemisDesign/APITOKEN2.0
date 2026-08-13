# Gemini 3.7 Flash — Stage 1 admission dossier

## Review metadata

| Field | Value |
|---|---|
| Review date | 2026-08-14 (Asia/Shanghai) |
| Repository base inspected | `c371685d5cad8a184c577799f934b72a975f916b` |
| Delivery stage | Stage 1: research, tariff, dormant implementation, and controlled canary only |
| Exact official Developer API model ID | `gemini-3.7-flash` |
| Official release status | Generally available (GA), stable, released 2026-08-13 |
| Publication decision | **DO NOT PUBLISH** until the exact implementation SHA passes controlled live acceptance |
| Evidence policy | Official Google sources plus a sanitized observation of the owned catalogue; no inferred private aliases |

This document is the append-only admission record for Gemini 3.7 Flash. It is not a public
catalogue entry and it does not claim that any existing Gemini subscription credential, Code Assist
plan, router preset, customer plan, or storefront can generate with this model.

## Executive verdict

Gemini 3.7 Flash is a real, stable Gemini Developer API model. Google announced its general
availability on 2026-08-13 and documents the exact public ID `gemini-3.7-flash`.

The product decision for Stage 1 is deliberately narrower:

1. Use only `gemini-3.7-flash` as the dormant candidate. Do not invent a private wire name,
   effort-suffixed alias, Code Assist alias, or quota alias.
2. Record the official Standard tariff with its 2026 promotional and 2027 rates, but do not expose
   the model publicly or make it a default.
3. Treat official Developer API and Antigravity availability as separate from availability through
   the product's subscription-backed Gemini credential path.
4. Require a GREEN live run on the exact implementation SHA before a separate publication change.
   A catalogue row or successful `countTokens` call is discovery evidence, not generation proof.
5. Withdraw the candidate if a minimal-size generation fails. A failed request is not permission to publish
   the model “for checking.”

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

| Surface | Evidence on 2026-08-14 | Stage 1 product conclusion |
|---|---|---|
| Gemini Developer API | Officially GA under `gemini-3.7-flash` | Exact public ID may be used for the dormant canary |
| Google AI Studio and documented Google product surfaces | Listed by first-party documentation/model card | Confirms Google distribution only |
| Gemini Managed Agents / Antigravity agent | Official latest-model guidance makes 3.7 Flash the default underlying model for that hosted agent | This is a different surface from the product's OAuth-backed Antigravity/Code Assist transport and does not reveal a private quota or wire alias |
| Gemini Code Assist | The checked Code Assist model page and release notes did not document 3.7 Flash | Do not claim Code Assist support; absence is a dated observation, not a permanent verdict |
| Owned subscription catalogue | Sanitized inspection found no 3.7 quota row and no 3.7 private wire row | No private alias may be invented; no subscription plan may be advertised |
| This product | No live acceptance exists for the exact implementation SHA | Dormant only; no public catalogue/default/router/web exposure |

### Owned catalogue observation

The owned catalogue observation was sanitized before being recorded here. On 2026-08-14 it contained
neither a Gemini 3.7 quota row nor a Gemini 3.7 private wire row. Consequently,
`gemini-3.7-flash` is the only non-invented dormant candidate.

This absence does not prove that the model will never appear in the owned catalogue, and a future
catalogue appearance would still not prove generation. Any later private name must come from fresh,
credential-bound authoritative evidence and must be recorded as a new dated finding; it must not be
derived from neighboring Gemini aliases.

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

No live row below was executed as part of this research document. Status is **PENDING** unless marked
as an observation. The exact implementation commit SHA must be recorded before any request is treated
as acceptance evidence.

| Check | Exact evidence required | Budget rule | Publication meaning |
|---|---|---|---|
| Owned catalogue discovery | Sanitized 3.7 quota and wire rows bound to the credential/plan | Free | **OBSERVED ABSENT** on 2026-08-14; never generation proof |
| `countTokens` preflight | 2xx for exact `gemini-3.7-flash` and a positive authoritative token count | Free; always first | Reachability/tokenization only; never generation proof |
| Minimal unary generation | 2xx, non-empty real text output, terminal authoritative usage, and terminal model identity from the exact candidate SHA | Default `$0.0001`; the person explicitly authorized the exact promo worst-case ceiling `$0.786492` for one no-retry attempt | Mandatory; failure means withdrawal |
| Incremental SSE | More than a buffered terminal-only event, visible incremental output, clean terminal completion, and authoritative terminal usage | Separately bounded after admission | Mandatory before any streaming claim |
| Thinking `low` | Real output and authoritative usage with `thinkingLevel=low` | Separately bounded | Required before advertising `low` |
| Thinking default/`medium` | Omitted level and explicit `medium` both follow documented semantics | Separately bounded | Required before advertising default `medium` |
| Thinking `high` | Real output and authoritative usage with `thinkingLevel=high` | Separately bounded | Required before advertising `high` |
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
short prompt. With `maxOutputTokens=16`, the person explicitly authorized exactly this current-promo
worst-case ceiling for one request and no retry:

```text
1,048,576 * 750 + 16 * 3,750 = 786,492,000 nanoUSD = $0.786492
```

This authorization expires with the promotional epoch at `2027-01-01T00:00:00Z`; it is not a general
calibration budget. The request must use a short prompt, explicit 16-token maximum, no grounding, no
paid tool, and no automatic retry. Actual terminal usage must be reconciled against the bound; the
ceiling is a fail-closed reserve, not an expected spend or acceptance evidence by itself.

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

### Gate to Stage 2 — separate follow-up commit

Publication may begin only after all of the following are true:

1. The exact dormant implementation commit SHA is recorded.
2. A free exact-ID `countTokens` call is atomically claimed once immediately before the final cutoff
   check and connection open, then passes with a response bound to the immutable canonical request
   UUID, digest, profile, model, HTTP status and execution state. The exact-profile engine path and
   controller both forbid helper restart, OAuth resend, profile rotation, redirect and reconnect;
   any failure or crash after the claim permanently withdraws this admission candidate.
3. Minimal generation is armed, then atomically claimed exactly once immediately before the final
   cutoff check and network open. It is dispatched before the immutable promo cutoff
   `2027-01-01T00:00:00Z`, then returns 2xx with real output and terminal authoritative usage within
   the aggregate admission cap. The immutable `priced_ts` must remain in that promo epoch and
   reproduce the exact official rate; arming before the cutoff is not permission to dispatch after
   it, and a crash after the single-use claim is permanently ambiguous rather than retryable. The
   terminal authority snapshot and outcome/event binding must reproduce the exact admitted profile
   and paid plan; missing or changed plan proof is not acceptance.
4. Incremental SSE passes with terminal authoritative usage.
5. Every control, modality, and credential plan proposed for publication passes its matching matrix
   row; untested features remain absent from the claim.
6. Metering reconciles input, output/thinking, cached-input reads, and any grounding query counts
   using integer or exact-rational nanoUSD arithmetic. Explicit `cachedContent` and its token-hour
   storage SKU remain rejected and outside publication scope until the provider supplies an
   authoritative storage-duration signal and the ledger gains that dimension.
7. The controlled production live is GREEN on the exact SHA.
8. Publication surfaces are changed together under the new-model checklist in a separate commit.

Any failed generation withdraws this admission candidate. Recovery requires a new implementation
commit and a fresh controlled run; a red SHA is not published or retried as if it were green.

## Checklist disposition

The new-model checklist in
[`docs/CHANGE_CHECKLISTS.md`](../docs/CHANGE_CHECKLISTS.md) applies. This document covers the Stage 1
research, official tariff, evidence boundary, dormant-name decision, and canary contract.

| Checklist area | Stage 1 disposition |
|---|---|
| Official identity and behavior research | Applicable; recorded here |
| Official tariff and effective epochs | Applicable; recorded here in exact nanoUSD units |
| Dormant implementation and tests | Applicable; owned by the implementation change, not this research file |
| Exact-SHA controlled live | Applicable but pending; no success is claimed here |
| Public catalogue, defaults, router, site, and public docs | Not applicable to Stage 1; explicitly deferred and forbidden |
| New provider checklist | Not applicable; Gemini is an existing provider |
| Customer price or multiplier change | Not applicable; this records upstream cost only and changes no customer multiplier |
| Database migration | Not applicable; no schema change is introduced by this document |
| Cross-context contract / `docs/DEPENDENCIES.md` | Existing model/price map was corrected in the same commit because it still named deleted release-generation and OpenKeys-policy surfaces; no new public link is added |
| Commerce, sales, OpenKeys, and admin publication | Not applicable to Stage 1; deferred until proven Stage 2 publication scope exists |

## Secret hygiene

No raw access token, refresh token, OAuth envelope, API key, authorization header, cookie, account
email, subject ID, project/organization ID, proxy endpoint, customer prompt, response body, or raw
catalogue dump is stored in this document. The owned-catalogue finding is limited to the sanitized
absence of a 3.7 quota/wire row. Future live evidence must record only the exact commit SHA, plan label,
HTTP/result class, sanitized model identity, token/query usage, cost reconciliation, and pass/fail
outcome; credentials and payloads remain outside version control.
