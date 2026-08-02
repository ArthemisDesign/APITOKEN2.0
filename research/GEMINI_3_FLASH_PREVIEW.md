# Gemini 3 Flash Preview subscription evidence — 2026-08-02

## Review metadata

- Provider / slug: Google Gemini subscription pool / `gemini-3-flash-preview`.
- Review date and timezone: 2026-08-02, Europe/Moscow.
- Product base SHA: `926b0b0b5dd7174e749446cf6a42372ced6164a2`; candidate SHA is assigned at commit.
- Intended public surface: native Gemini `models`, `generateContent`, `streamGenerateContent` and
  `countTokens`, plus the existing OpenAI/Anthropic compatibility skins.
- Credential source: existing Auth Bot-provisioned, AEAD-sealed paid Code Assist / Antigravity
  OAuth profiles; this change introduces no credential field, grant or scope.
- Owned live plan available for the final test: one authorized Google AI Pro profile, addressed by
  opaque profile id through the existing admin-only calibration route.

## Executive verdict

- Implementation readiness: `preview-ready`; generation publication remains blocked on the
  controlled post-deploy live matrix below.
- Terms/compliance: no new OAuth identity or scope; the existing Gemini subscription review still
  governs. Undocumented subscription transport evidence is not treated as vendor permission.
- Credential verdict: unchanged and within the existing encrypted roster threat model.
- Usage/settlement verdict: official paid rates and all existing token/Search dimensions are exact;
  successful generation must still prove terminal non-zero `usageMetadata` on the subscription wire.
- Streaming verdict: implementation reuses the proven incremental SSE translator, but this exact
  model still needs a live incremental frame and terminal usage.
- Quota verdict: owned discovery proves both bounded quota rows exist, not that generation works.
- Main unresolved risk: the current Antigravity backend may retain quota for an older preview ID
  while rejecting generation. Such a result requires withdrawing the model from the allowlist.

## Official sources

| Claim | URL / revision | Accessed | Exact proof | Does not prove |
|---|---|---|---|---|
| Public model id, lifecycle, modalities and token limits | <https://ai.google.dev/gemini-api/docs/models/gemini-3-flash-preview> | 2026-08-02 | `gemini-3-flash-preview`; 1,048,576 input; 65,536 output; text output | Code Assist / Antigravity subscription availability |
| Standard paid rates | <https://ai.google.dev/gemini-api/docs/pricing#gemini-3-flash-preview> | 2026-08-02 | exact text/audio/cache/output and Search prices recorded below | Native subscription credits or quota size |
| Thinking controls | <https://ai.google.dev/gemini-api/docs/thinking#thinking-levels> | 2026-08-02 | `minimal`, `low`, `medium`, `high` are the reviewed public levels | Private effort model aliases |
| Official Code Assist client identity | Google Gemini CLI commit [`f47d6c6f7a1308d81f9f57acf7d279f0928c5249`](https://github.com/google-gemini/gemini-cli/commit/f47d6c6f7a1308d81f9f57acf7d279f0928c5249) | 2026-08-02 | defines the preview constant and sends the public model id unchanged | Current Antigravity backend generation success |

## Subscription plan and model matrix

| Exact plan / surface | Official status | Owned live status | Native route | Usage authority | GA decision |
|---|---|---|---|---|---|
| Gemini Developer API paid standard | public preview model and rates documented | not required for subscription transport | public Developer API | terminal Developer API usage | pricing capability only |
| Gemini CLI Code Assist | official CLI exposes and sends the public id | owned catalogue has `gemini-3-flash` and `gemini-3-flash-agent` | legacy Code Assist wrapper | terminal subscription usage | live generation pending |
| Google AI Pro via Antigravity | no public normative private-wire contract | owned quota rows present on 2026-08-02 | Antigravity agent wrapper | terminal subscription usage | candidate until controlled smoke |
| Google AI Ultra / Code Assist Standard / Enterprise / Workspace AI Ultra | provider accepts these paid-plan classes generally | no model-specific owned run | existing plan-specific transport | unknown for this model | no availability claim |

A quota row is not generation evidence. A successful Google AI Pro run would still not prove every
other accepted paid plan.

## Model translation and controls

| Public id | Generation wire id | Quota identity | Controls | Price schedule | Evidence | Decision |
|---|---|---|---|---|---|---|
| `gemini-3-flash-preview` | unchanged public id | Antigravity agent: `gemini-3-flash-agent`; visible non-agent row: `gemini-3-flash`; legacy CLI: public id | `thinkingConfig` keeps `minimal\|low\|medium\|high`; unknown levels fail locally | `google/gemini-developer-api/2026-08-02` | official CLI + signed Antigravity inspection + owned quota | implemented; live gate pending |

The installed, signed Antigravity 2.4.3 language server contains the public model ID and no
`gemini-3-flash-agent` generation string. Combined with the existing pinned `requestType=agent`
wrapper and owned discovery rows, this supports separating wire generation from quota accounting;
it does not by itself prove generation.

## Wire matrix

| Operation | URL / wrapper | Required controls | Framing / success authority | Errors and retry |
|---|---|---|---|---|
| model list | existing public `/v1beta/models` catalogue | public id only | native JSON catalogue | local allowlist stays fail closed |
| `generateContent` | existing Code Assist wrapper, public wire id, `requestType=agent` | route-owned project/session/request identity | 2xx candidate, canonical public `modelVersion`, terminal non-zero usage | existing 4xx/429/5xx policy; retry only before public bytes |
| `streamGenerateContent?alt=sse` | same model and wrapper | same controls | at least one incremental public SSE event plus terminal usage | no retry after first translated event |
| `countTokens` | nested native request with route-owned model | no customer project injection | 2xx and positive `totalTokens`; quota-free and unbilled | deterministic client errors do not rotate |
| quota | Antigravity `fetchAvailableModels` | authenticated profile transport | sanitized agent/non-agent rows | explicit zero blocks only the mapped agent generation route |
| refresh | existing official token endpoint and client identity | existing single-flight policy | successful token result only | only exact `invalid_grant` kills the credential |

## Official rate card

| Effective epoch | Usage leg | Official paid-standard rate | Engine integer rate | Source |
|---|---|---|---|---|
| model lifetime, reviewed 2026-08-02 | text/image/video input | $0.50 / 1M tokens | 500 nanoUSD/token | official pricing table |
| same | audio input | $1.00 / 1M tokens | 1,000 nanoUSD/token | official pricing table |
| same | cached text input | $0.05 / 1M tokens | 50 nanoUSD/token | official pricing table |
| same | cached audio input | $0.10 / 1M tokens | 100 nanoUSD/token | official pricing table |
| same | output including thinking | $3.00 / 1M tokens | 3,000 nanoUSD/token | official pricing table |
| same | Google Search | $14 / 1,000 queries | 14,000,000 nanoUSD/query | official pricing table |

No long-context surcharge is published for this model. Search is disjoint from token legs and is
charged only from provider-reported query counts.

## Native quota contract

| Bucket | Duration / reset | Unit | Hard stop | Evidence | Product use |
|---|---|---|---|---|---|
| `gemini-3-flash-agent` | provider row may carry reset; fixed duration is not asserted | provider `remainingAmount` and/or decimal fraction | explicit zero on a fresh catalogue | owned Antigravity discovery | generation admission, steering and retry time |
| `gemini-3-flash` | provider row may carry reset; fixed duration is not asserted | same provider fields | not used to invent a generation alias | owned Antigravity discovery | operator visibility only |
| public id on legacy CLI | existing legacy quota response | existing provider fields | existing explicit-zero policy | official CLI identity; model-specific live row pending | legacy admission only |

Quota amount is never converted into API dollars or inferred from plan price. The existing 5h and
weekly calibration authority remains independent of these per-model catalogue rows.

## Authentication threat model

- OAuth grant / client identity: unchanged official Gemini CLI or Antigravity client selected from
  the sealed credential; caller-supplied identity is stripped.
- Issuer, token endpoint, scopes, redirect, state and PKCE: unchanged from
  `docs/engine/GEMINI_PROVIDER.md`; this model adds none.
- Refresh race: existing per-profile single-flight refresh; only Google's exact `invalid_grant`
  marks the credential dead.
- Stable duplicate identity: roster continues rejecting duplicate Google subject and profile id.
- Proxy/geography: the credential's unique authenticated proxy remains pinned per profile; no
  ambient proxy or caller override is introduced.
- Secret storage: OAuth tokens, full email, subject, project and proxy remain inside the AEAD
  envelope and process memory; profile id remains opaque in live targeting.
- Revocation/removal and Auth Bot rollback: unchanged atomic roster publication/removal flow.

## GitHub and artifact evidence

| Source | Revision / artifact | License / authority | Relevant paths / hypothesis | Independent? | Executed? |
|---|---|---|---|---|---|
| Google Gemini CLI | `f47d6c6f7a1308d81f9f57acf7d279f0928c5249` | Apache-2.0; official Google client | `packages/core/src/config/models.ts`; public ID is a real Code Assist generation identity | official, not independent | no; source inspection only |
| Installed Antigravity | signed macOS app 2.4.3 | official distributed artifact; non-normative | language server strings; agent quota id is not a generation alias | separate official artifact | no; static inspection only |

Public searches did not find two unrelated maintained implementations of
`gemini-3-flash-agent`. No community source is used as authority, and the missing corroboration is
why the gateway does not guess a private generation alias.

## Controlled live results

| Date | Plan / client | Public / native model | Operation | Status | Incremental? | Usage non-zero? | Quota evidence | Sanitized result |
|---|---|---|---|---|---|---|---|---|
| 2026-08-02 | owned Google AI Pro / Antigravity | public preview / quota rows | model quota discovery | success | n/a | n/a | both agent and non-agent rows present | quota presence only; generation unknown |
| post-deploy pending | same owned opaque profile | public id unchanged | countTokens + default and four thinking levels, non-stream and SSE | pending | pending | required | exact profile attribution required | publication gate |

The live runner must use `--profiles gemini_oauth_000001 --models gemini-3-flash-preview`, require
its canonical calibration request attribution, and retain only the sanitized report. No paid leg is
retried without authoritative `not_started` proof.

## Conflict log

| Topic | Official | Owned live | Chosen behavior | Risk / next experiment |
|---|---|---|---|---|
| generation ID | official CLI sends public preview id | generation pending; signed Antigravity has the same id | send public id unchanged | run all live generation legs after exact SHA is green |
| quota identity | no normative private row contract | agent and non-agent rows exist | Antigravity agent admission uses only `gemini-3-flash-agent` | explicit zero may reveal a changed row; withdraw/fix on evidence |
| subscription availability | public Developer API model exists | only quota presence today | candidate, not GA evidence | model-not-found/unsupported requires allowlist withdrawal |
| thinking controls | public levels documented | subscription execution pending | preserve public level in `thinkingConfig` | test default plus all four levels |

## Unsupported surfaces

- No private `gemini-3-flash-agent` generation alias: there is no official or live proof it is a
  generation ID.
- No cross-plan promise beyond the owned Google AI Pro result.
- No new image-generation, TTS, realtime/live or Developer-API-only route.
- No nominal quota-to-dollar conversion and no plan-price capacity estimate.
- No unknown future thinking level; the matrix remains closed.

## Implementation decisions and rollback

| Decision | Evidence | Product paths / tests | Rollback |
|---|---|---|---|
| add exact paid tariff and limits | official model and pricing pages | `crates/metering/src/gemini.rs`, exact-rate tests | remove model in a new tariff epoch only if official facts are withdrawn |
| keep public wire id, map Antigravity quota separately | official CLI, signed artifact, owned rows | `crates/forward/src/gemini/{config,pool,api}.rs` | remove from runtime allowlist or amend mapping from new live evidence |
| publish dormant pricing generation 4 without mutating generation 3 | immutable pricing contract | `packages/contracts`, `crates/forward/src/pricing.rs`, digest tests | leave frozen generations; publish a later additive generation |
| expose in web/catalog and systemd allowlist | model contract plus required post-deploy smoke | web model tests, server conversion tests, watchdog regression | new fix SHA removes model from public allowlist/catalog if smoke fails |

## Secret-hygiene confirmation

- [x] no bearer/refresh token, cookie or credential file captured;
- [x] no raw email/account/subject/project/organization id retained;
- [x] no authenticated proxy URL retained;
- [x] no private error body or prompt content retained;
- [x] no untrusted repository code, install script or binary executed;
- [x] all saved evidence uses bounded classes, opaque profile identity and synthetic prompts;
- [x] no temporary research clone or raw capture remains in the product worktree.
